//! 频谱 tap（docs/04 §5.1，ADR-0006）。
//!
//! 订阅制"必要采样复制"：渲染线程把**未乘音量**的插值样本混成单声道
//! f32 推入 SPSC 无锁环（环满丢新样本并计数，不影响音频）；独立 FFT
//! 线程凑 2048 样本 → Hann 窗 → radix-2 FFT → 48 对数分带 → u8[48]，
//! ≥30ms 间隔发 `EngineEvent::Spectrum`。关闭（SetSpectrum{false}）时
//! 生产端只剩一个布尔检查，消费者线程闲置，零拷贝零分配。
//!
//! UI 侧契约：`RemoteSpectrumSource` 监听 `spectrum:frame`（宿主把
//! EngineEvent::Spectrum 原样 emit），分带值 0–255、无副载波数据。

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::SyncSender;
use std::sync::Arc;

/// 环容量（样本数，2 的幂）。48kHz 混单声道后 ≈ 170ms 余量（96kHz ≈ 85ms）。
const RING_CAP: usize = 8192;
const RING_MASK: usize = RING_CAP - 1;
/// FFT 窗长。48kHz 下 2048 = 43ms，频率分辨率 ~23Hz。
const FFT_SIZE: usize = 2048;
/// 契约分带数（docs/04 §5：48 对数分带）。
pub const BANDS: usize = 48;
/// 帧最小间隔（毫秒）≈ 30fps 上限。
const FRAME_MIN_INTERVAL_MS: u64 = 30;
/// 显示动态范围：-80dB..0dB → 0..255。
const DB_FLOOR: f32 = -80.0;

/// 单声道样本 SPSC 环。单生产者（渲染线程）单消费者（FFT 线程）。
///
/// 安全性：UnsafeCell 槽位由 head/tail 分区——生产者只写 [tail, head+cap)
/// 内的空槽（未发布给读者），消费者只读 [tail, head) 已发布槽；两区间
/// 永不相交（环满时写者丢弃不越界），故无数据竞争。
struct MonoRing {
    buf: std::cell::UnsafeCell<Box<[f32; RING_CAP]>>,
    head: AtomicUsize, // 写位置（只由生产者改）
    tail: AtomicUsize, // 读位置（只由消费者改）
}

unsafe impl Sync for MonoRing {}

impl MonoRing {
    fn new() -> Self {
        Self {
            buf: std::cell::UnsafeCell::new(Box::new([0.0; RING_CAP])),
            head: AtomicUsize::new(0),
            tail: AtomicUsize::new(0),
        }
    }

    /// 生产者推一个样本。满则丢弃（丢新保旧，SPSC 不变量安全）并计数。
    fn push(&self, s: f32, dropped: &AtomicU64) {
        let head = self.head.load(Ordering::Relaxed);
        let tail = self.tail.load(Ordering::Acquire);
        if head.wrapping_sub(tail) >= RING_CAP {
            dropped.fetch_add(1, Ordering::Relaxed);
            return;
        }
        unsafe { (*self.buf.get())[head & RING_MASK] = s };
        self.head.store(head.wrapping_add(1), Ordering::Release);
    }

    /// 消费者取最多 `out.len()` 个样本，返回实际读取数。
    fn pop(&self, out: &mut [f32]) -> usize {
        let head = self.head.load(Ordering::Acquire);
        let tail = self.tail.load(Ordering::Relaxed);
        let avail = head.wrapping_sub(tail);
        let n = avail.min(out.len());
        for i in 0..n {
            out[i] = unsafe { (*self.buf.get())[(tail + i) & RING_MASK] };
        }
        self.tail.store(tail.wrapping_add(n), Ordering::Release);
        n
    }
}

/// tap 句柄：渲染线程持有（生产端），引擎线程共享订阅开关。
#[derive(Clone)]
pub struct SpectrumTap {
    ring: Arc<MonoRing>,
    enabled: Arc<AtomicBool>,
    dropped: Arc<AtomicU64>,
    /// 实际采样率（Hz），由引擎在 Load 后填充分带标定；0 = 按 48kHz 兜底
    sample_rate: Arc<AtomicU32>,
}

impl SpectrumTap {
    pub fn new() -> Self {
        Self {
            ring: Arc::new(MonoRing::new()),
            enabled: Arc::new(AtomicBool::new(false)),
            dropped: Arc::new(AtomicU64::new(0)),
            sample_rate: Arc::new(AtomicU32::new(48_000)),
        }
    }

    /// 订阅开关。false 时生产端只剩一次原子读。
    /// 清环动作在消费者线程做（tail 的单写者不变量，P2：生产侧曾在这里写
    /// tail 与消费者竞争，重开后可能重复读一帧旧样本）。
    pub fn set_enabled(&self, enabled: bool) {
        self.enabled.store(enabled, Ordering::Release);
    }

    /// 分带频率标定用实际采样率（Load 成功后由引擎线程调用）。
    pub fn set_sample_rate(&self, sample_rate: u32) {
        if sample_rate >= 8_000 {
            self.sample_rate.store(sample_rate, Ordering::Relaxed);
        }
    }

    /// 渲染线程：一帧（所有声道平均，未乘音量）入环。
    #[inline]
    pub fn push_frame(&self, samples: &[f32], channels: usize) {
        if !self.enabled.load(Ordering::Relaxed) || channels == 0 {
            return;
        }
        if samples.len() % channels != 0 {
            return;
        }
        let inv = 1.0 / channels as f32;
        for frame in samples.chunks_exact(channels) {
            let mono = frame.iter().sum::<f32>() * inv;
            self.ring.push(mono, &self.dropped);
        }
    }

    /// 渲染线程：推**已混好的单声道输出帧**（重采样之后、乘音量之前）。
    ///
    /// 为什么不用 push_frame 推源帧：源帧速率 = 输出帧 × rate，倍速下入环
    /// 速率跟着变（2× 溢出丢帧、0.5× 帧率腰斩），而频谱应当反映实际出声。
    #[inline]
    pub fn push_mono(&self, mono: &[f32]) {
        if !self.enabled.load(Ordering::Relaxed) {
            return;
        }
        for &s in mono {
            self.ring.push(s, &self.dropped);
        }
    }

    pub fn dropped_count(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }

    /// 启动 FFT 消费线程。事件经 ev_tx 出（EngineEvent::Spectrum）。
    /// 通道有界（引擎事件通道 256 容量），满时丢新帧——下一帧 30ms 后就来。
    pub fn spawn_consumer(&self, ev_tx: SyncSender<crate::EngineEvent>) {
        let ring = Arc::clone(&self.ring);
        let enabled = Arc::clone(&self.enabled);
        let sample_rate = Arc::clone(&self.sample_rate);
        let spawned = std::thread::Builder::new()
            .name("spectrum-fft".into())
            .spawn(move || consume_loop(&ring, &enabled, &sample_rate, ev_tx));
        if let Err(e) = spawned {
            tracing::warn!("频谱 FFT 线程创建失败（本会话无可视化）: {e}");
        }
    }
}

/// 消费者主循环：凑 FFT_SIZE 新样本 → 窗 → FFT → 分带 → 节流发帧。
fn consume_loop(
    ring: &MonoRing,
    enabled: &AtomicBool,
    sample_rate: &AtomicU32,
    ev_tx: SyncSender<crate::EngineEvent>,
) {
    let mut staging = [0.0f32; FFT_SIZE];
    let mut filled = 0usize;
    let mut last_frame = std::time::Instant::now() - std::time::Duration::from_millis(1_000);

    loop {
        if !enabled.load(Ordering::Relaxed) {
            // 清环（tail 追平 head）：只能在消费者线程做——tail 的单写者
            // 不变量（set_enabled 不再碰 tail）；禁用期间生产端不推新样本，
            // 这里把禁用前在途的残留排掉，避免重开后出旧帧。
            let head = ring.head.load(Ordering::Acquire);
            ring.tail.store(head, Ordering::Release);
            std::thread::sleep(std::time::Duration::from_millis(50));
            continue;
        }
        if filled < FFT_SIZE {
            let n = ring.pop(&mut staging[filled..]);
            if n == 0 {
                // 数据不足：短暂等待（音频实时性不受影响，本线程是旁路）
                std::thread::sleep(std::time::Duration::from_millis(5));
                continue;
            }
            filled += n;
        }
        if filled < FFT_SIZE {
            continue;
        }
        // 节流：间隔不足时等待（下一轮窗口数据会在环里排队）
        let elapsed = last_frame.elapsed();
        if elapsed.as_millis() < FRAME_MIN_INTERVAL_MS as u128 {
            std::thread::sleep(
                std::time::Duration::from_millis(FRAME_MIN_INTERVAL_MS) - elapsed,
            );
        }
        let bands = compute_bands(&staging, sample_rate.load(Ordering::Relaxed) as f32);
        last_frame = std::time::Instant::now();
        match ev_tx.try_send(crate::EngineEvent::Spectrum { bands }) {
            Ok(()) => {}
            // 通道满：丢新保旧（帧是瞬态可视化数据）；对端关闭：引擎退出
            Err(std::sync::mpsc::TrySendError::Full(_)) => {}
            Err(std::sync::mpsc::TrySendError::Disconnected(_)) => return,
        }
        filled = 0;
    }
}

/// Hann 窗 + radix-2 FFT + 48 对数分带 → u8。
/// `sample_rate`：实际流采样率（Hz），分带频率轴按它标定
/// （P2：此前恒按 48kHz，44.1k 源频带偏移约 8.8%）。
pub fn compute_bands(samples: &[f32; FFT_SIZE], sample_rate: f32) -> Vec<u8> {
    // 1) Hann 窗
    let mut re: Vec<f32> = samples
        .iter()
        .enumerate()
        .map(|(i, &s)| {
            let w = 0.5 * (1.0 - (2.0 * std::f32::consts::PI * i as f32 / FFT_SIZE as f32).cos());
            s * w
        })
        .collect();
    let mut im = vec![0.0f32; FFT_SIZE];

    // 2) radix-2 迭代 FFT（位反转 + 蝶形）
    fft_inplace(&mut re, &mut im);

    // 3) 对数分带：20Hz–20kHz 均匀 48 带（每带取最大幅值，动画更有起伏）
    // 带边界：f_i = 20 * 1000^(i/BANDS)；bin = f * FFT_SIZE / sample_rate
    let sr = if sample_rate >= 8_000.0 { sample_rate } else { 48_000.0 };
    let half = FFT_SIZE / 2;
    let mut bands = vec![0u8; BANDS];
    let mut bin_lo = 1usize; // 跳过 DC
    for b in 0..BANDS {
        let f_hi = 20.0f32 * 1000f32.powf((b + 1) as f32 / BANDS as f32);
        let bin_hi = ((f_hi * FFT_SIZE as f32 / sr).ceil() as usize).min(half);
        let bin_hi = bin_hi.max(bin_lo + 1); // 每带至少 1 bin
        let mut peak = 0.0f32;
        for k in bin_lo..bin_hi.min(half) {
            let mag = (re[k] * re[k] + im[k] * im[k]).sqrt();
            peak = peak.max(mag);
        }
        bin_lo = bin_hi;
        // 幅值 → dB → 归一化（窗增益补偿 +6dB，忽略）
        let db = 20.0 * (peak / FFT_SIZE as f32 * 2.0).max(1e-9).log10();
        let norm = ((db - DB_FLOOR) / -DB_FLOOR).clamp(0.0, 1.0);
        bands[b] = (norm * 255.0) as u8;
    }
    bands
}

/// 就地 radix-2 FFT（输入长度须为 2 的幂）。
fn fft_inplace(re: &mut [f32], im: &mut [f32]) {
    let n = re.len();
    debug_assert!(n.is_power_of_two());
    // 位反转重排
    let mut j = 0usize;
    for i in 0..n {
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
        let mut m = n >> 1;
        while m >= 1 && j & m != 0 {
            j ^= m;
            m >>= 1;
        }
        j |= m;
    }
    // 蝶形
    let mut len = 2;
    while len <= n {
        let ang = -2.0 * std::f32::consts::PI / len as f32;
        let (w_re, w_im) = (ang.cos(), ang.sin());
        let mut base = 0;
        while base < n {
            let (mut cur_re, mut cur_im) = (1.0f32, 0.0f32);
            for k in 0..len / 2 {
                let a = base + k;
                let b = a + len / 2;
                let t_re = re[b] * cur_re - im[b] * cur_im;
                let t_im = re[b] * cur_im + im[b] * cur_re;
                re[b] = re[a] - t_re;
                im[b] = im[a] - t_im;
                re[a] += t_re;
                im[a] += t_im;
                let next = cur_re * w_re - cur_im * w_im;
                cur_im = cur_re * w_im + cur_im * w_re;
                cur_re = next;
            }
            base += len;
        }
        len <<= 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ring_basic_push_pop() {
        let ring = MonoRing::new();
        let dropped = AtomicU64::new(0);
        for i in 0..100 {
            ring.push(i as f32, &dropped);
        }
        let mut out = [0f32; 100];
        assert_eq!(ring.pop(&mut out), 100);
        assert_eq!(out[0], 0.0);
        assert_eq!(out[99], 99.0);
        assert_eq!(ring.pop(&mut out), 0);
    }

    #[test]
    fn ring_overflow_drops_new_and_keeps_invariant() {
        let ring = MonoRing::new();
        let dropped = AtomicU64::new(0);
        for i in 0..(RING_CAP as u64 + 100) {
            ring.push(i as f32, &dropped);
        }
        assert_eq!(dropped.load(Ordering::Relaxed), 100); // 最后 100 个新样本被丢
        let mut out = vec![0f32; RING_CAP];
        assert_eq!(ring.pop(&mut out), RING_CAP);
        assert_eq!(out[0], 0.0); // 最旧的样本保留
    }

    #[test]
    fn fft_sine_peaks_at_expected_bin() {
        // 48kHz、FFT_SIZE 点、1000Hz 正弦 → 峰值 bin ≈ 1000*2048/48000 ≈ 43
        let sr = 48_000.0f32;
        let freq = 1_000.0f32;
        let samples: Vec<f32> = (0..FFT_SIZE)
            .map(|i| (2.0 * std::f32::consts::PI * freq * i as f32 / sr).sin())
            .collect();
        let mut arr = [0f32; FFT_SIZE];
        arr.copy_from_slice(&samples);
        let bands = compute_bands(&arr, 48_000.0);
        // 1000Hz 应落在第 ~30 带（f_30 = 20*1000^(30/48) ≈ 940Hz）
        let peak_band = bands
            .iter()
            .enumerate()
            .max_by_key(|(_, v)| **v)
            .map(|(i, _)| i)
            .unwrap();
        assert!((27..=33).contains(&peak_band), "peak band = {peak_band}");
        assert!(bands[peak_band] > 150, "峰值带应有足够强度: {}", bands[peak_band]);
    }

    #[test]
    fn silence_is_zero() {
        let bands = compute_bands(&[0.0; FFT_SIZE], 48_000.0);
        assert!(bands.iter().all(|&b| b == 0));
    }

    #[test]
    fn sample_rate_shifts_bin_mapping() {
        // 分带边界按 Hz 定义、bin 换算按采样率——低频带够宽，错标定不显。
        // 用 96kHz 源 + 15kHz 正弦放大差异：正确标定 bin=15000*2048/96000=320
        // → 落第 45 带；若误用 48k 标定，45 带去看 bin 581-671，能量在
        // bin 320（对应误标 7500Hz）→ 落第 41 带。
        let sr = 96_000.0f32;
        let freq = 15_000.0f32;
        let samples: Vec<f32> = (0..FFT_SIZE)
            .map(|i| (2.0 * std::f32::consts::PI * freq * i as f32 / sr).sin())
            .collect();
        let mut arr = [0f32; FFT_SIZE];
        arr.copy_from_slice(&samples);
        let correct = compute_bands(&arr, 96_000.0);
        let wrong = compute_bands(&arr, 48_000.0);
        let peak = |b: &[u8]| {
            b.iter().enumerate().max_by_key(|(_, v)| **v).map(|(i, _)| i).unwrap()
        };
        assert!(
            (45..=47).contains(&peak(&correct)),
            "正确采样率标定应落 45-47 带，实际 {}",
            peak(&correct)
        );
        assert_ne!(peak(&wrong), peak(&correct), "误用 48k 标定应偏移到别的带");
    }

    #[test]
    fn tap_disabled_pushes_nothing() {
        let tap = SpectrumTap::new();
        tap.set_enabled(false);
        tap.push_frame(&[0.5, 0.5, 0.5, 0.5], 2);
        tap.push_mono(&[0.1, 0.2]);
        // 真实断言环内容：禁用时生产端必须一次都不写
        let mut out = [0f32; 8];
        assert_eq!(tap.ring.pop(&mut out), 0, "禁用时环内必须为空");
        assert_eq!(tap.dropped_count(), 0);
    }

    #[test]
    fn tap_pushes_mono_average() {
        let tap = SpectrumTap::new();
        tap.set_enabled(true);
        tap.push_frame(&[1.0, 0.0, 1.0, 0.0], 2); // 两帧 L=1 R=0 → mono=0.5
        let mut out = [0f32; 2];
        assert_eq!(tap.ring.pop(&mut out), 2, "两帧应入环两个单声道样本");
        assert!((out[0] - 0.5).abs() < 1e-6, "mono = (1+0)/2，实际 {}", out[0]);
        assert!((out[1] - 0.5).abs() < 1e-6);
    }

    /// 变速不改变入环速率：推 N 个输出帧就得到 N 个样本（此前推源帧，
    /// 2× 速每秒入环 2× 样本，环长期溢出）。
    #[test]
    fn push_mono_is_rate_independent() {
        let tap = SpectrumTap::new();
        tap.set_enabled(true);
        tap.push_mono(&[0.3; 32]);
        let mut out = [0f32; 64];
        assert_eq!(tap.ring.pop(&mut out), 32);
        assert_eq!(tap.dropped_count(), 0, "32 样本远小于环容量，不该丢");
    }
}
