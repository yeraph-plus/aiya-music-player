//! 均衡器（协议 v6 起，docs/24 B5b；docs/39 加音色双旋钮）。
//!
//! 结构：pregain → 10 × RBJ biquad（低架 / 8 × 峰值 / 高架）
//!       → 低音低架（90 Hz）+ 高音高架（7.5 kHz）→（音量在后面）。
//! EQ 必须在音量**之前**：提升增益时才不会顶爆已归一化的输出电平。
//!
//! 设计取舍：
//! - **无外部依赖**：RBJ cookbook 公式手写（~40 行），不引 biquad crate——
//!   公式是教科书内容，引依赖换不来正确性。
//! - **全 0 dB = 硬旁路**：`enabled && 所有增益为 0` 时直接跳过滤波。
//!   数学上 RBJ 公式在 A=1 时退化为恒等（见 `tests::bypass_all_zero`），但
//!   浮点等式依赖"逐项相等"，硬旁路是零风险承诺：旁路证明（docs/24 B5b
//!   验收）不靠浮点巧合成立。
//! - **系数热切不重建状态**：切预设时只重算系数，延迟线保留。系数连续变化
//!   （±12 dB 内）不会产生可闻爆音；重建状态反而会在每个预设切换点引入
//!   直流瞬态。
//! - **超 Nyquist 频段自动跳过**：32 kHz 采样率下 16 kHz 频段的公式会算出
//!   NaN——跳过该段（不是 clamp），其余段照常工作。
//! - **音色与图示正交**（docs/39）：低音/高音是独立的 shelf 滤波器，串在
//!   图示 10 段之后，与任何段的增益互不覆盖——图示 EQ 管"修曲线"，
//!   音色双旋钮管"一听就有"的整体明暗。

/// EQ 频段中心频率（Hz）。标准十段：31/62/125/250/500/1k/2k/4k/8k/16k。
pub const EQ_BAND_HZ: [f32; 10] = [
    31.25, 62.5, 125.0, 250.0, 500.0, 1000.0, 2000.0, 4000.0, 8000.0, 16000.0,
];

/// 低音旋钮的低架转折频率（Hz）。90 Hz：外放（<100 Hz 滚降）与耳机上
/// 都听得见；再低（如 31 Hz）在小喇叭上等于没做（docs/39 方案 A 否决理由）。
pub const EQ_BASS_SHELF_HZ: f32 = 90.0;
/// 高音旋钮的高架转折频率（Hz）。7.5 kHz：管"亮度/齿音"，低于 32 kHz
/// 采样率的 0.45×Nyquist（14.4 kHz），任何输出格式下都安全。
pub const EQ_TREBLE_SHELF_HZ: f32 = 7500.0;

/// 单段增益上限（dB）。±12 dB 足够修音色，再大就是拿 EQ 当音量踏板用。
pub const EQ_GAIN_LIMIT_DB: f32 = 12.0;
/// pregain 上限（dB）。允许正值的理由：用户可能整体拉低后想补回来；
/// 上限挡住"把 pregain 当主音量"的用法（那会绕过音量归一化）。
pub const EQ_PREGAIN_LIMIT_DB: f32 = 12.0;

/// 一份完整的 EQ 设置（wire 形状，camelCase）。
///
/// **整体下发**（与 `OutputConfig` 同理）：分字段下发会出现"已启用但频段
/// 还是旧的"中间态，且每次下发都要重算全部系数。
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct EqParams {
    /// 10 段增益（dB，-12..+12），下标对应 [`EQ_BAND_HZ`]
    pub bands: [f32; 10],
    /// 低音旋钮（dB，-12..+12）：90 Hz 低架，独立于图示段
    pub bass_db: f32,
    /// 高音旋钮（dB，-12..+12）：7.5 kHz 高架，独立于图示段
    pub treble_db: f32,
    /// 前置增益（dB，-12..+12）：抗削顶的预留旋钮
    pub pregain: f32,
    /// 总开关。关 = 硬旁路（逐样本直通，零计算）
    pub enabled: bool,
}

impl Default for EqParams {
    fn default() -> Self {
        Self {
            bands: [0.0; 10],
            bass_db: 0.0,
            treble_db: 0.0,
            pregain: 0.0,
            enabled: false,
        }
    }
}

impl EqParams {
    /// 增益 clamp（命令入口与引擎入口各跑一次：脏数据不能变成 NaN 系数）。
    pub fn clamped(mut self) -> Self {
        for b in &mut self.bands {
            *b = b.clamp(-EQ_GAIN_LIMIT_DB, EQ_GAIN_LIMIT_DB);
        }
        self.bass_db = self.bass_db.clamp(-EQ_GAIN_LIMIT_DB, EQ_GAIN_LIMIT_DB);
        self.treble_db = self.treble_db.clamp(-EQ_GAIN_LIMIT_DB, EQ_GAIN_LIMIT_DB);
        self.pregain = self
            .pregain
            .clamp(-EQ_PREGAIN_LIMIT_DB, EQ_PREGAIN_LIMIT_DB);
        self
    }

    /// 是否等价于旁路（关，或开但全部增益为 0）。
    fn is_bypass(&self) -> bool {
        !self.enabled
            || (self.pregain == 0.0
                && self.bass_db == 0.0
                && self.treble_db == 0.0
                && self.bands.iter().all(|g| *g == 0.0))
    }
}

/// 滤波器形状（RBJ cookbook 分类）。
#[derive(Debug, Clone, Copy)]
enum Shape {
    /// 低架（lowShelf）：转折频率以下整体抬/降
    LowShelf,
    /// 高架（highShelf）：转折频率以上整体抬/降
    HighShelf,
    /// 峰值（peaking EQ）：以 f0 为中心钟形抬/降
    Peak,
}

/// 单个 biquad 的系数（RBJ cookbook，直 I 型）。
#[derive(Debug, Clone, Copy, Default)]
struct Coeffs {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
}

impl Coeffs {
    /// 按形状算系数。`LowShelf/HighShelf` 用斜率 S=1 → alpha = sin/2·√2；
    /// `Peak` 用 Q=1.1（约 ±1 段的过渡带，消费级 EQ 常规取值）。
    // `!(f0 > 0.0)` 是刻意的：写成 `f0 <= 0.0` 时 NaN 会让比较返回 false，NaN 被放过后
    // 面的 powf/cos 会把整条 EQ 系数污染成 NaN（静音或爆音）。取反写法对 NaN 为 true。
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    fn design(shape: Shape, gain_db: f32, f0: f32, sample_rate: f32) -> Option<Coeffs> {
        if !(f0 > 0.0) || f0 >= 0.45 * sample_rate {
            return None; // 超 Nyquist：公式会产生 NaN，跳过该段
        }
        let a = 10f32.powf(gain_db / 40.0); // A = 10^(dB/40)
        let w0 = std::f32::consts::TAU * f0 / sample_rate;
        let (cos, sin) = (w0.cos(), w0.sin());
        let alpha = match shape {
            Shape::LowShelf | Shape::HighShelf => sin / 2.0 * std::f32::consts::SQRT_2,
            Shape::Peak => sin / (2.0 * 1.1),
        };
        let (b0, b1, b2, a0, a1, a2);
        if matches!(shape, Shape::LowShelf) {
            // 低架（lowShelf）
            let sq = a.sqrt();
            let two_sq_alpha = 2.0 * sq * alpha;
            b0 = a * ((a + 1.0) - (a - 1.0) * cos + two_sq_alpha);
            b1 = 2.0 * a * ((a - 1.0) - (a + 1.0) * cos);
            b2 = a * ((a + 1.0) - (a - 1.0) * cos - two_sq_alpha);
            a0 = (a + 1.0) + (a - 1.0) * cos + two_sq_alpha;
            a1 = -2.0 * ((a - 1.0) + (a + 1.0) * cos);
            a2 = (a + 1.0) + (a - 1.0) * cos - two_sq_alpha;
        } else if matches!(shape, Shape::HighShelf) {
            // 高架（highShelf）
            let sq = a.sqrt();
            let two_sq_alpha = 2.0 * sq * alpha;
            b0 = a * ((a + 1.0) + (a - 1.0) * cos + two_sq_alpha);
            b1 = -2.0 * a * ((a - 1.0) + (a + 1.0) * cos);
            b2 = a * ((a + 1.0) + (a - 1.0) * cos - two_sq_alpha);
            a0 = (a + 1.0) - (a - 1.0) * cos + two_sq_alpha;
            a1 = 2.0 * ((a - 1.0) - (a + 1.0) * cos);
            a2 = (a + 1.0) - (a - 1.0) * cos - two_sq_alpha;
        } else {
            // 峰值（peaking EQ）
            b0 = 1.0 + alpha * a;
            b1 = -2.0 * cos;
            b2 = 1.0 - alpha * a;
            a0 = 1.0 + alpha / a;
            a1 = -2.0 * cos;
            a2 = 1.0 - alpha / a;
        }
        Some(Coeffs {
            b0: b0 / a0,
            b1: b1 / a0,
            b2: b2 / a0,
            a1: a1 / a0,
            a2: a2 / a0,
        })
    }
}

/// 单段单声道的直 I 型状态：[x1, x2, y1, y2]。
type BandState = [f32; 4];

/// 滤波器槽位总数：图示 10 段 + 低音低架 + 高音高架（docs/39）。
const SLOTS: usize = 12;
/// 音色双旋钮在槽位数组里的下标（图示段占 0..10）。
const SLOT_BASS: usize = 10;
const SLOT_TREBLE: usize = 11;

/// 12 槽 EQ 处理器（绑定一个输出流的采样率/声道数）。
///
/// 生命周期与 `play_file` 一致：装载时按解码器格式构建，换曲重建（新进程
/// 或新流本来就要重置状态）。`set_params` 只换系数不动状态。
pub struct Eq {
    params: EqParams,
    sample_rate: f32,
    channels: usize,
    /// 每槽系数（超 Nyquist 槽为 None = 直通）
    coeffs: [Option<Coeffs>; SLOTS],
    /// state[slot][channel]
    state: Vec<[BandState; SLOTS]>,
}

impl Eq {
    pub fn new(params: EqParams, sample_rate: u32, channels: usize) -> Self {
        let mut eq = Self {
            params: params.clamped(),
            sample_rate: sample_rate as f32,
            channels: channels.max(1),
            coeffs: [None; SLOTS],
            state: Vec::new(),
        };
        eq.redesign();
        eq.state = vec![[[0.0; 4]; SLOTS]; eq.channels];
        eq
    }

    /// 当前生效参数（clamp 后）。
    pub fn params(&self) -> &EqParams {
        &self.params
    }

    /// 热切参数：系数重算，延迟线保留（切预设无爆音的前提）。
    pub fn set_params(&mut self, params: EqParams) {
        self.params = params.clamped();
        self.redesign();
    }

    fn redesign(&mut self) {
        let bypass = self.params.is_bypass();
        for (i, &f0) in EQ_BAND_HZ.iter().enumerate() {
            let gain = if bypass { 0.0 } else { self.params.bands[i] };
            let shape = if i == 0 {
                Shape::LowShelf
            } else if i == 9 {
                Shape::HighShelf
            } else {
                Shape::Peak
            };
            self.coeffs[i] = Coeffs::design(shape, gain, f0, self.sample_rate);
        }
        let bass = if bypass { 0.0 } else { self.params.bass_db };
        self.coeffs[SLOT_BASS] =
            Coeffs::design(Shape::LowShelf, bass, EQ_BASS_SHELF_HZ, self.sample_rate);
        let treble = if bypass { 0.0 } else { self.params.treble_db };
        self.coeffs[SLOT_TREBLE] = Coeffs::design(
            Shape::HighShelf,
            treble,
            EQ_TREBLE_SHELF_HZ,
            self.sample_rate,
        );
    }

    /// 是否在做实际滤波（UI/测试用：旁路时调用方可以直接跳过拷贝）。
    pub fn is_bypass(&self) -> bool {
        self.params.is_bypass()
    }

    /// 处理一帧交错样本（就地修改，frame.len() == channels）。
    /// 返回是否真的滤波了（旁路时 false，样本未动）。
    pub fn process_frame(&mut self, frame: &mut [f32]) -> bool {
        if self.params.is_bypass() || frame.len() < self.channels {
            return false;
        }
        let pg = 10f32.powf(self.params.pregain / 20.0);
        for (ch, s) in frame.iter_mut().take(self.channels).enumerate() {
            let mut x = *s * pg;
            for (band, c) in self.coeffs.iter().enumerate() {
                let Some(c) = c else { continue };
                let st = &mut self.state[ch][band];
                let y = c.b0 * x + c.b1 * st[0] + c.b2 * st[1] - c.a1 * st[2] - c.a2 * st[3];
                st[1] = st[0];
                st[0] = x;
                st[3] = st[2];
                st[2] = y;
                x = y;
            }
            *s = x;
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 生成 N 帧正弦波（交错 ch 声道）。
    fn sine(freq: f32, sample_rate: u32, ch: usize, frames: usize) -> Vec<f32> {
        (0..frames * ch)
            .map(|i| {
                let t = (i / ch) as f32 / sample_rate as f32;
                (std::f32::consts::TAU * freq * t).sin() * 0.5
            })
            .collect()
    }

    #[test]
    fn params_clamp() {
        let mut p = EqParams::default();
        p.bands[0] = 99.0;
        p.bands[9] = -99.0;
        p.bass_db = 50.0;
        p.treble_db = -50.0;
        p.pregain = 50.0;
        p.enabled = true;
        let c = p.clamped();
        assert_eq!(c.bands[0], EQ_GAIN_LIMIT_DB);
        assert_eq!(c.bands[9], -EQ_GAIN_LIMIT_DB);
        assert_eq!(c.bass_db, EQ_GAIN_LIMIT_DB);
        assert_eq!(c.treble_db, -EQ_GAIN_LIMIT_DB);
        assert_eq!(c.pregain, EQ_PREGAIN_LIMIT_DB);
    }

    #[test]
    fn bypass_all_zero_matches_disabled_bit_exact() {
        // docs/24 B5b 验收：EQ 全 0 dB 时输出与关 EQ 逐样本一致（旁路证明）
        let src = sine(1000.0, 48_000, 2, 4096);
        let mut off = Eq::new(EqParams::default(), 48_000, 2);
        let mut on = Eq::new(
            EqParams {
                enabled: true,
                ..Default::default()
            },
            48_000,
            2,
        );
        assert!(off.is_bypass() && on.is_bypass());
        let a = src.clone();
        off.process_frame(&mut []);
        let mut b = src.clone();
        for frame in b.chunks_mut(2) {
            on.process_frame(frame);
        }
        // is_bypass 短路：两路都未触碰样本 → 位级一致
        assert_eq!(a, b);
        assert_eq!(a, src);
    }

    #[test]
    fn boost_band_amplifies_near_center_frequency() {
        // 1 kHz 段 +6 dB：1 kHz 正弦增益应明显为正，10 kHz 应接近不变
        let params = EqParams {
            bands: [0.0, 0.0, 0.0, 0.0, 0.0, 6.0, 0.0, 0.0, 0.0, 0.0],
            enabled: true,
            ..Default::default()
        };
        let mut eq = Eq::new(params, 48_000, 1);
        assert!(!eq.is_bypass());
        let mut rms_at = |freq: f32| -> f32 {
            let mut v = sine(freq, 48_000, 1, 48_000);
            for frame in v.chunks_mut(1) {
                eq.process_frame(frame);
            }
            // 取后半段（稳态，丢弃暂态）
            let tail = &v[24_000..];
            (tail.iter().map(|x| x * x).sum::<f32>() / tail.len() as f32).sqrt()
        };
        let boosted = rms_at(1000.0);
        let untouched = rms_at(10_000.0);
        let baseline = 0.5 / std::f32::consts::SQRT_2; // 0.5 幅度正弦的 RMS
        assert!(
            (boosted / baseline - 2.0).abs() < 0.3,
            "1 kHz 增益应约 2×（+6dB），实测 {boosted}/{baseline}"
        );
        assert!(
            (untouched / baseline - 1.0).abs() < 0.2,
            "10 kHz 应近似直通，实测 {untouched}/{baseline}"
        );
    }

    #[test]
    fn preset_switching_stays_bounded() {
        // docs/24 B5b 验收：20 次连切预设无爆音——输出不得出现 NaN 或越界尖峰
        let presets: [EqParams; 5] = [
            EqParams {
                bands: [6.0; 10],
                pregain: -4.0,
                ..Default::default()
            },
            EqParams {
                bands: [-3.0, 2.0, 4.0, 1.0, -2.0, 0.0, 3.0, 5.0, 2.0, -1.0],
                pregain: -2.0,
                enabled: true,
                ..Default::default()
            },
            EqParams {
                bands: [0.0; 10],
                pregain: 0.0,
                enabled: true,
                ..Default::default()
            },
            EqParams {
                bands: [-8.0; 10],
                pregain: 6.0,
                enabled: true,
                ..Default::default()
            },
            EqParams::default(), // enabled=false：旁路 ↔ 滤波之间的切换也要无爆音
        ];
        let mut eq = Eq::new(
            EqParams {
                bass_db: 8.0,
                treble_db: -6.0,
                enabled: true,
                ..presets[0]
            },
            44_100,
            2,
        );
        let mut peak: f32 = 0.0;
        for round in 0..20 {
            eq.set_params(presets[round % presets.len()]);
            let mut v = sine(440.0, 44_100, 2, 512);
            for frame in v.chunks_mut(2) {
                eq.process_frame(frame);
            }
            for x in &v {
                assert!(x.is_finite(), "第 {round} 轮出现非有限样本");
                peak = peak.max(x.abs());
            }
        }
        assert!(peak < 4.0, "20 次连切出现尖峰 {peak}");
    }

    #[test]
    fn over_nyquist_band_is_skipped() {
        // 16 kHz 段在 32 kHz 采样率下超 Nyquist：跳过而非 NaN
        let params = EqParams {
            bands: [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 9.0],
            enabled: true,
            ..Default::default()
        };
        let mut eq = Eq::new(params, 32_000, 1);
        let mut v = sine(1000.0, 32_000, 1, 4800);
        for frame in v.chunks_mut(1) {
            eq.process_frame(frame);
        }
        assert!(v.iter().all(|x| x.is_finite()));
        assert!(eq.coeffs[9].is_none());
    }

    #[test]
    fn bass_knob_amplifies_low_shelf_region() {
        // docs/39：低音旋钮 +6 dB → 50 Hz 正弦应约 2×（架子在转折频率以下
        // 平台增益），1 kHz 中频应近似不动（与图示段正交）
        let params = EqParams {
            bass_db: 6.0,
            enabled: true,
            ..Default::default()
        };
        let mut eq = Eq::new(params, 48_000, 1);
        assert!(!eq.is_bypass());
        let rms_at = |eq: &mut Eq, freq: f32| -> f32 {
            let mut v = sine(freq, 48_000, 1, 48_000);
            for frame in v.chunks_mut(1) {
                eq.process_frame(frame);
            }
            let tail = &v[24_000..];
            (tail.iter().map(|x| x * x).sum::<f32>() / tail.len() as f32).sqrt()
        };
        let baseline = 0.5 / std::f32::consts::SQRT_2;
        let low = rms_at(&mut eq, 50.0);
        let mid = rms_at(&mut eq, 1000.0);
        assert!(
            (low / baseline - 2.0).abs() < 0.3,
            "50 Hz 应约 2×（+6dB 低架平台），实测 {low}/{baseline}"
        );
        assert!(
            (mid / baseline - 1.0).abs() < 0.15,
            "1 kHz 应近似直通（音色与图示正交），实测 {mid}/{baseline}"
        );
    }

    #[test]
    fn treble_knob_amplifies_high_shelf_region() {
        // docs/39：高音旋钮 +6 dB → 10 kHz 正弦应约 2×（7.5 kHz 架子以上
        // 平台增益），500 Hz 应近似不动
        let params = EqParams {
            treble_db: 6.0,
            enabled: true,
            ..Default::default()
        };
        let mut eq = Eq::new(params, 48_000, 1);
        let rms_at = |eq: &mut Eq, freq: f32| -> f32 {
            let mut v = sine(freq, 48_000, 1, 48_000);
            for frame in v.chunks_mut(1) {
                eq.process_frame(frame);
            }
            let tail = &v[24_000..];
            (tail.iter().map(|x| x * x).sum::<f32>() / tail.len() as f32).sqrt()
        };
        let baseline = 0.5 / std::f32::consts::SQRT_2;
        let high = rms_at(&mut eq, 10_000.0);
        let mid = rms_at(&mut eq, 500.0);
        assert!(
            (high / baseline - 2.0).abs() < 0.3,
            "10 kHz 应约 2×（+6dB 高架平台），实测 {high}/{baseline}"
        );
        assert!(
            (mid / baseline - 1.0).abs() < 0.15,
            "500 Hz 应近似直通，实测 {mid}/{baseline}"
        );
    }

    #[test]
    fn tone_only_params_block_bypass() {
        // 只有音色旋钮非零（图示全 0、pregain 0）时不是旁路——否则旋钮会静默失效
        let p = EqParams {
            bass_db: 3.0,
            enabled: true,
            ..Default::default()
        };
        assert!(!p.is_bypass() || !p.enabled);
        let q = EqParams {
            treble_db: -3.0,
            enabled: true,
            ..Default::default()
        };
        assert!(!q.is_bypass() || !q.enabled);
    }
}
