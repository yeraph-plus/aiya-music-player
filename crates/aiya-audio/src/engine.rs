//! WASAPI 共享模式输出（事件驱动）。
//!
//! 关键决策（docs/04 §2）：`EventsShared { autoconvert: true }` ——
//! 引擎自己完成 源格式→mix format 的转换与 SRC（SRC_DEFAULT_QUALITY），
//! 我们只交源采样率的 f32 交错样本，v1 不自建重采样器。
//! 独占模式（bit-perfect + 链内 DSP）走同一 render 循环的 EventsExclusive 分支，P2 后补。

use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::sync::Arc;
use std::time::Duration;

use wasapi::DeviceEnumerator;
use wasapi::{Direction, SampleType, StreamMode, WaveFormat};

use crate::decoder::Decoder;
use crate::AudioError;

/// 引擎线程收到的命令。
///
/// wire 侧（ADR-0002 sidecar）用相邻 tag：`{"t":"load","v":"D:/…"}`。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "t", content = "v", rename_all = "camelCase")]
pub enum EngineCommand {
    /// 装载并播放
    Load(String),
    Pause,
    Resume,
    /// 跳到指定毫秒（若未装载则忽略）
    Seek(u64),
    /// 0.0–1.0
    SetVolume(f32),
    /// 变速：0.5–2.0，线性插值重采样（v1 变速变调；WSOLA 不变调后补）
    SetRate(f32),
    Stop,
    /// 频谱 tap 订阅开关（订阅制必要采样复制，关闭零开销）
    SetSpectrum(bool),
    /// 退出线程
    Shutdown,
}

/// 引擎线程发出的回报。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "t", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum EngineEvent {
    /// 装载成功（时长毫秒）
    Loaded { duration_ms: u64 },
    /// 播放位置推进（渲染循环节拍发出，~每 10ms）
    Position { position_ms: u64 },
    /// 曲目自然播完
    Eof,
    /// 状态变化（供 PlayerActor 对齐）
    StateChanged { playing: bool },
    /// 频谱帧（tap FFT 线程 ~30fps；宿主原样 emit `spectrum:frame`）
    Spectrum { bands: Vec<u8> },
    Error { message: String },
}

/// 引擎事件通道容量。Position ~10ms/帧 + Spectrum ~30fps，正常消费下远用不满；
/// 消费端卡死时关键事件（Loaded/Eof/Error）有 ~100ms 的等待窗口，之后放弃。
const EVENT_CHANNEL_CAP: usize = 256;

/// 有界事件通道的发送端封装。背压策略：
/// * 可丢类（Position/Spectrum）——满了直接丢新保旧，生产端永不被拖住；
/// * 关键类（Loaded/Eof/StateChanged/Error）——最多等 100ms 腾位，超时丢弃
///   并记日志（消费端正常在微秒级取走，此路径只在宿主泵死掉时触发）。
#[derive(Clone)]
struct EventTx(SyncSender<EngineEvent>);

impl EventTx {
    fn send(&self, e: EngineEvent) {
        match e {
            EngineEvent::Position { .. } | EngineEvent::Spectrum { .. } => {
                let _ = self.0.try_send(e);
            }
            mut e => {
                for _ in 0..50 {
                    match self.0.try_send(e) {
                        Ok(()) => return,
                        Err(TrySendError::Full(e2)) => {
                            e = e2;
                            std::thread::sleep(Duration::from_millis(2));
                        }
                        Err(TrySendError::Disconnected(_)) => return,
                    }
                }
                tracing::warn!(event = ?e, "engine event dropped: consumer stalled >100ms");
            }
        }
    }
}

/// 引擎句柄：命令进 / 事件出。Drop 不强制停线程（Shutdown 命令负责）。
pub struct Engine {
    tx: mpsc::Sender<EngineCommand>,
    rx: Receiver<EngineEvent>,
    worker: Option<std::thread::JoinHandle<()>>,
    /// 装载后由 Load 事件填充
    pub duration_ms: Arc<AtomicU64>,
    pub playing: Arc<AtomicBool>,
}

impl Engine {
    pub fn spawn() -> Result<Self, AudioError> {
        let (cmd_tx, cmd_rx) = mpsc::channel::<EngineCommand>();
        let (ev_tx, ev_rx) = mpsc::sync_channel::<EngineEvent>(EVENT_CHANNEL_CAP);
        let worker = std::thread::Builder::new()
            .name("aiya-audio-engine".into())
            .spawn(move || render_loop(cmd_rx, ev_tx))
            .map_err(|e| AudioError::Custom(e.to_string()))?;
        Ok(Self {
            tx: cmd_tx,
            rx: ev_rx,
            worker: Some(worker),
            duration_ms: Arc::new(AtomicU64::new(0)),
            playing: Arc::new(AtomicBool::new(false)),
        })
    }

    pub fn send(&self, cmd: EngineCommand) {
        let _ = self.tx.send(cmd);
    }

    /// 非阻塞取事件（PlayerActor 轮询）。
    pub fn try_recv(&self) -> Option<EngineEvent> {
        self.rx.try_recv().ok()
    }

    pub fn shutdown(&mut self) {
        let _ = self.tx.send(EngineCommand::Shutdown);
        if let Some(w) = self.worker.take() {
            let _ = w.join();
        }
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// 渲染循环：COM init → 设备 → mix format → 事件驱动写流。
fn render_loop(cmd_rx: Receiver<EngineCommand>, ev_tx: SyncSender<EngineEvent>) {
    let ev = EventTx(ev_tx);
    // tap 线程拿裸 SyncSender（自走 try_send 丢帧策略，见 consume_loop）
    let tap = crate::spectrum::SpectrumTap::new();
    tap.spawn_consumer(ev.0.clone());
    let send = |e: EngineEvent| ev.send(e);

    { let _ = wasapi::initialize_mta(); }

    // 变速跨曲目保持：在顶层循环外声明，EOF/Stop/错误回到顶层后不重置
    // （P1：此前每轮 Load 周期重置 1.0，自然播完切下一曲后倍速静默丢失
    // 且 UI 仍显示旧值；SetRate 只应被显式命令修改）
    let mut pending_rate: f32 = 1.0;
    'outer: loop {
        // 命令循环：等 Load 或 Shutdown（Load 前到的 SetRate/SetSpectrum 暂存生效）
        let mut path = loop {
            match cmd_rx.recv() {
                Ok(EngineCommand::Load(p)) => break p,
                Ok(EngineCommand::SetRate(r)) => pending_rate = r.clamp(0.5, 2.0),
                Ok(EngineCommand::SetSpectrum(on)) => tap.set_enabled(on),
                Ok(EngineCommand::Shutdown) | Err(_) => break 'outer,
                Ok(other) => {
                    // 未装载时只响应 Stop 类命令的回报
                    handle_preload(&other, &send);
                }
            }
        };

        // play_file 内部收到的新 Load 在这里续播（path 从 play_file 带回，
        // 不能在内部丢弃 —— 否则那条 Load 凭空消失，外层 recv() 永远等不到）。
        loop {
            match play_file(&cmd_rx, &send, &path, pending_rate, &tap) {
                Ok(LoadExit::Finished) => break, // EOF/Stop/错误恢复：回顶层等下一条 Load
                Ok(LoadExit::NewLoad { path: p, rate }) => {
                    path = p;
                    pending_rate = rate;
                }
                Ok(LoadExit::Shutdown) => break 'outer,
                Err(e) => {
                    tracing::error!(%path, error = %e, "engine stream failed");
                    send(EngineEvent::Error { message: e.to_string() });
                    // 回到命令循环等下一条 Load（设备恢复）
                    break;
                }
            }
        }
    }

    { let _ = wasapi::deinitialize(); }
}

/// play_file 的退出原因。NewLoad 必须把 path/rate 带回外层续播。
enum LoadExit {
    /// EOF / Stop / 设备错误：回命令循环等下一条 Load
    Finished,
    /// 播放中收到新 Load：直接换曲续播
    NewLoad { path: String, rate: f32 },
    /// 收到 Shutdown：终止渲染线程
    Shutdown,
}

fn handle_preload(cmd: &EngineCommand, send: &impl Fn(EngineEvent)) {
    match cmd {
        EngineCommand::Stop => send(EngineEvent::StateChanged { playing: false }),
        _ => {}
    }
}

/// 设备失效恢复的最大重试次数（指数退避 200/400/800/1600/1600ms，约 4.6s 总窗口）。
/// 覆盖常见场景：默认设备切换（USB 声卡插拔）、蓝牙重连、驱动瞬时失效。
const DEVICE_RECOVERY_ATTEMPTS: u32 = 5;

/// 打开好的输出流状态。设备失效时整体重建（换默认设备后 mix format 可能变，
/// autoconvert 需要重新协商）。
struct Output {
    client: wasapi::AudioClient,
    event: wasapi::Handle,
    render: wasapi::AudioRenderClient,
    buffer_frames: u32,
    /// 一帧原始字节缓冲（f32 交错），容量 = buffer_frames * channels * 4
    sample_bytes: Vec<u8>,
}

/// 枚举默认渲染设备并启动事件驱动共享流。
fn open_output(decoder: &Decoder) -> Result<Output, AudioError> {
    let enumerator = DeviceEnumerator::new().map_err(wasapi_err)?;
    let device = enumerator.get_default_device(&Direction::Render).map_err(wasapi_err)?;
    let mut client = device.get_iaudioclient().map_err(wasapi_err)?;

    // 源格式：f32 交错，让引擎做转换（autoconvert: true）
    let src_fmt = WaveFormat::new(
        32,
        32,
        &SampleType::Float,
        decoder.sample_rate as usize,
        decoder.channels,
        None,
    );
    client
        .initialize_client(
            &src_fmt,
            &Direction::Render,
            &StreamMode::EventsShared { autoconvert: true, buffer_duration_hns: 2_000_000 }, // 200ms
        )
        .map_err(wasapi_err)?;
    let event = client.set_get_eventhandle().map_err(wasapi_err)?;
    let render = client.get_audiorenderclient().map_err(wasapi_err)?;
    let buffer_frames = client.get_buffer_size().map_err(wasapi_err)?;
    let frame_bytes = decoder.channels * 4usize;
    let sample_bytes = vec![0u8; buffer_frames as usize * frame_bytes];

    client.start_stream().map_err(wasapi_err)?;
    Ok(Output { client, event, render, buffer_frames, sample_bytes })
}

/// 重建输出流（退避后重枚举默认设备）。位置无需显式恢复——
/// 池内已解码样本和 decoder 位置都在调用方手里，天然连续。
fn reopen_output(out: &mut Output, decoder: &Decoder, attempt: u32) -> Result<(), AudioError> {
    let _ = out.client.stop_stream(); // 旧流尽力停，失败无所谓（马上整个丢弃）
    let backoff_ms = 200u64 << attempt.min(3);
    std::thread::sleep(Duration::from_millis(backoff_ms));
    *out = open_output(decoder)?;
    Ok(())
}

/// 打开文件并进入渲染循环直到 Stop/Shutdown/新 Load/EOF。
/// `initial_rate`：Load 前收到的变速指令（如启动恢复/恢复会话）。
/// `tap`：频谱采样点（跨曲目常驻；drain 前取已消费源帧，未乘音量）。
fn play_file(
    cmd_rx: &Receiver<EngineCommand>,
    send: &impl Fn(EngineEvent),
    path: &str,
    initial_rate: f32,
    tap: &crate::spectrum::SpectrumTap,
) -> Result<LoadExit, AudioError> {
    let mut decoder = Decoder::open(Path::new(path))?;
    tap.set_sample_rate(decoder.sample_rate); // 频谱分带按实际采样率标定
    send(EngineEvent::Loaded { duration_ms: decoder.duration_ms });

    let mut out = open_output(&decoder)?;

    // 已解码未播的交错样本池：变速插值的数据源（rate=1 时等价于直通）
    let mut pool: Vec<f32> = Vec::new();
    // 池内的源帧读取位置（帧单位，f64）；drain 后减去已消费帧数保持相位连续
    let mut pool_pos: f64 = 0.0;
    let mut decoder_eof = false;

    let mut volume: f32 = 1.0;
    let mut rate: f32 = initial_rate;
    let mut paused = false;
    let mut user_stopped = false;
    send(EngineEvent::StateChanged { playing: true });

    let mut result: Result<(), AudioError> = Ok(());
    // 流式阶段（padding/write/start）捕获的设备错误；恢复见循环尾部
    let mut stream_err: Option<AudioError> = None;
    let mut device_failures: u32 = 0;

    loop {
        // 等引擎缓冲区消费事件。超时/错误都视为一次正常唤醒——绝不能把
        // 超时当 Err 拆流（P0：暂停后 stop_stream 令事件停发，2s 超时曾走
        // `?` 把流销毁，暂停期间所有命令石沉大海）。
        // 新暂停模型下流保持 running，正常播放时事件每 ~10ms 触发一次，
        // 超时不会命中；暂停期间引擎可能因缓冲排空停发事件，用 100ms 短
        // 超时兜底驱动命令循环（Resume/Seek 响应延迟 ≤100ms）。
        let wait_ms = if paused { 100 } else { 2000 };
        let _ = out.event.wait_for_event(wait_ms);

        // 非阻塞处理命令
        loop {
            match cmd_rx.try_recv() {
                Ok(EngineCommand::Pause) if !paused => {
                    // 暂停 = 停止喂数据，流保持 running（事件照常触发、命令
                    // 循环存活）。不 stop_stream：停流后 WASAPI 引擎不再为
                    // 该流处理传递，事件停发。缓冲内残余（≤200ms）播完自然
                    // 静音，Resume 直接续喂数据，无 start/stop 状态机风险。
                    paused = true;
                    send(EngineEvent::StateChanged { playing: false });
                }
                Ok(EngineCommand::Resume) if paused => {
                    paused = false;
                    // 流从未停止（见 Pause），无需 start_stream，直接恢复喂数据
                    send(EngineEvent::StateChanged { playing: true });
                }
                Ok(EngineCommand::Seek(ms)) => {
                    // stop（reset 要求 stopped 态）→ seek → 清池（旧样本作废）→ reset → 重启
                    let _ = out.client.stop_stream();
                    if let Err(e) = decoder.seek_ms(ms) {
                        result = Err(e);
                        break;
                    }
                    pool.clear();
                    pool_pos = 0.0;
                    decoder_eof = false;
                    let _ = out.client.reset_stream();
                    // 新暂停模型：装载期间流始终 running（暂停只是不喂数据），
                    // seek 后无条件重启；暂停态缓冲为空继续静音。
                    // start 失败按设备错误走恢复路径（不走 ? 拆流）
                    if let Err(e) = out.client.start_stream() {
                        stream_err = Some(wasapi_err(e));
                        break;
                    }
                    send(EngineEvent::Position { position_ms: decoder.position_ms() });
                }
                Ok(EngineCommand::SetVolume(v)) => volume = v.clamp(0.0, 1.0),
                Ok(EngineCommand::SetRate(r)) => rate = r.clamp(0.5, 2.0),
                Ok(EngineCommand::SetSpectrum(on)) => tap.set_enabled(on),
                Ok(EngineCommand::Stop) => {
                    user_stopped = true;
                    break;
                }
                Ok(EngineCommand::Load(p)) => {
                    // 换曲：停流后把新 path 带回外层续播（绝不能在此丢弃）
                    let _ = out.client.stop_stream();
                    return Ok(LoadExit::NewLoad { path: p, rate });
                }
                Ok(EngineCommand::Shutdown) => {
                    let _ = out.client.stop_stream();
                    return Ok(LoadExit::Shutdown);
                }
                Ok(_) => {}
                Err(_) => break,
            }
        }
        if user_stopped {
            send(EngineEvent::StateChanged { playing: false });
            send(EngineEvent::Eof); // Stop 视为曲目结束（PlayerActor 决定下一动作）
            let _ = out.client.stop_stream();
            return Ok(LoadExit::Finished);
        }
        if result.is_err() {
            let _ = out.client.stop_stream();
            return Err(result.unwrap_err());
        }
        if paused {
            continue;
        }

        // 设备错误恢复（唯一出口）：重建输出流后继续当前曲目。
        // 位置天然连续（池与 decoder 状态都在本地）；已写入旧设备缓冲的
        // 样本随重建丢失，最多回跳 200ms，可接受。
        if let Some(e) = stream_err.take() {
            if device_failures >= DEVICE_RECOVERY_ATTEMPTS {
                tracing::error!(%e, attempts = device_failures, "设备恢复重试耗尽，放弃当前曲目");
                let _ = out.client.stop_stream();
                return Err(AudioError::Wasapi(format!("设备恢复失败（{device_failures} 次重试）：{e}")));
            }
            match reopen_output(&mut out, &decoder, device_failures) {
                Ok(()) => {
                    device_failures += 1; // 写成功后归零；连续失败则退避递增
                    tracing::warn!(error = %e, attempt = device_failures, "输出设备失效，已重建（退避重试中）");
                }
                Err(re) => {
                    device_failures += 1;
                    tracing::warn!(error = %e, retry_error = %re, attempt = device_failures, "设备重建失败，将继续退避");
                }
            }
            continue;
        }

        // 写满可用空间
        let padding = match out.client.get_current_padding() {
            Ok(p) => p,
            Err(e) => {
                stream_err = Some(wasapi_err(e));
                continue;
            }
        };
        let available = out.buffer_frames - padding;
        if available == 0 {
            send(EngineEvent::Position { position_ms: play_position(&decoder, &pool) });
            continue;
        }
        // 每轮渲染心跳都上报位置（泵侧再做 500ms 节流；写满瞬间 available 不会为 0）
        send(EngineEvent::Position { position_ms: play_position(&decoder, &pool) });

        let mut frames_to_write = available as usize;
        let mut eof_now = false;
        let channels = decoder.channels.max(1);

        'fill: while frames_to_write > 0 {
            // 1) 补池：至少够本轮输出所需源帧（frames_to_write * rate + 1），除非解码到头
            let need_frames =
                (frames_to_write as f64 * rate as f64 + 2.0).ceil() as usize * channels;
            while !decoder_eof && pool.len() < need_frames {
                match decoder.next_packet() {
                    Ok(Some(samples)) => pool.extend_from_slice(samples),
                    Ok(None) => decoder_eof = true,
                    Err(e) => {
                        result = Err(e);
                        break 'fill;
                    }
                }
            }

            let pool_frames = pool.len() / channels;
            // 2) 本轮可输出帧数：池内 [pool_pos, pool_frames) 按 rate 换算
            let max_out =
                (((pool_frames as f64) - pool_pos) / rate as f64).floor().max(0.0) as usize;
            let out_frames = max_out.min(frames_to_write);
            if out_frames == 0 {
                if decoder_eof {
                    // 池耗尽（残余 < 1 输出帧，≈ 数十微秒）→ 曲目结束
                    eof_now = true;
                }
                break 'fill;
            }

            // 3) 线性插值输出（索引 clamp 保证尾部不越界）
            let frame_bytes_all = channels * 4;
            let chunk = &mut out.sample_bytes[..out_frames * frame_bytes_all];
            resample_take(&pool, pool_pos, rate as f64, channels, out_frames, volume, chunk);
            match out.render.write_to_device(
                out_frames,
                &out.sample_bytes[..out_frames * frame_bytes_all],
                None,
            ) {
                Ok(()) => device_failures = 0, // 写成功：恢复计数归零
                Err(e) => {
                    // 设备失效：走循环尾部的统一恢复出口
                    stream_err = Some(wasapi_err(e));
                    break 'fill;
                }
            }
            frames_to_write -= out_frames;

            // 4) 消费池：前移读取位置，drain 已消费帧。
            //    tap 点：已消费的源帧未乘音量（低音量频谱仍可视），单声道入环。
            pool_pos += out_frames as f64 * rate as f64;
            let consumed = pool_pos.floor() as usize;
            if consumed > 0 {
                let n = consumed.min(pool_frames) * channels;
                tap.push_frame(&pool[..n], channels);
                pool.drain(..n);
                pool_pos -= consumed as f64;
            }
            if frames_to_write == 0 {
                break 'fill;
            }
        }

        if eof_now {
            // 等缓冲播完再报 EOF：padding 归零或超时（间隙响应 Shutdown，
            // 不让退出命令被最长 500ms 地卡住）。padding 此刻报错说明设备
            // 恰好失效——直接按 EOF 收尾，下一曲 Load 会重建设备。
            let deadline = std::time::Instant::now() + Duration::from_millis(500);
            loop {
                if let Ok(EngineCommand::Shutdown) = cmd_rx.try_recv() {
                    let _ = out.client.stop_stream();
                    return Ok(LoadExit::Shutdown);
                }
                let pad = out.client.get_current_padding().unwrap_or(0);
                if pad == 0 || std::time::Instant::now() > deadline {
                    break;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            let _ = out.client.stop_stream();
            send(EngineEvent::Position { position_ms: decoder.duration_ms });
            send(EngineEvent::StateChanged { playing: false });
            send(EngineEvent::Eof);
            // EOF 后回命令循环等下一条 Load
            return Ok(LoadExit::Finished);
        }
    }
}

/// 播放位置（源时间轴毫秒）：解码位置减去池内未播样本的源时长。
/// decoder.position_ms() 是最后解码包的起点，池中的样本还没进设备 ——
/// 变速时池可能积压（rate<1），不修正的话进度条会领先于耳朵。
fn play_position(decoder: &Decoder, pool: &[f32]) -> u64 {
    let pool_frames = (pool.len() / decoder.channels.max(1)) as u64;
    let pool_ms = (pool_frames * 1000) / (decoder.sample_rate as u64).max(1);
    decoder.position_ms().saturating_sub(pool_ms)
}

/// 从 pool 的 pool_pos 起按 rate 线性插值取 out_frames 帧，交错 f32 LE 字节写入 out。
/// 输出帧 i 对应源位置 pool_pos + i*rate；越界索引 clamp 到池尾（仅 EOF 尾部触发）。
fn resample_take(
    pool: &[f32],
    pool_pos: f64,
    rate: f64,
    channels: usize,
    out_frames: usize,
    volume: f32,
    out: &mut [u8],
) {
    let pool_frames = pool.len() / channels.max(1);
    let mut bi = 0usize;
    for i in 0..out_frames {
        let src_f = pool_pos + i as f64 * rate;
        let i0 = (src_f.floor() as usize).min(pool_frames.saturating_sub(1));
        let i1 = (i0 + 1).min(pool_frames.saturating_sub(1));
        // i0 被 clamp 到池尾时 frac 可能 >1（EOF 残余防御路径），钳到 [0,1]
        // 复制末帧而非线性外插
        let frac = ((src_f - i0 as f64) as f32).clamp(0.0, 1.0);
        for ch in 0..channels {
            let a = pool[i0 * channels + ch];
            let b = pool[i1 * channels + ch];
            let s = (a + (b - a) * frac) * volume;
            out[bi..bi + 4].copy_from_slice(&s.to_le_bytes());
            bi += 4;
        }
    }
}

fn wasapi_err(e: wasapi::WasapiError) -> AudioError {
    AudioError::Wasapi(e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 读出 resample_take 输出的 f32 帧。
    fn read_f32(bytes: &[u8]) -> Vec<f32> {
        bytes
            .chunks_exact(4)
            .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect()
    }

    #[test]
    fn rate_one_is_passthrough() {
        // 1 通道 4 帧：0,1,2,3 → 原样
        let pool: Vec<f32> = vec![0.0, 1.0, 2.0, 3.0];
        let mut out = vec![0u8; 4 * 4];
        resample_take(&pool, 0.0, 1.0, 1, 4, 1.0, &mut out);
        let s = read_f32(&out);
        for (i, v) in s.iter().enumerate() {
            assert!((v - i as f32).abs() < 1e-6, "frame {i} = {v}");
        }
    }

    #[test]
    fn rate_two_consumes_every_other_frame() {
        // 2× 速：4 源帧出 2 输出帧（位置 0 和 2）
        let pool: Vec<f32> = vec![0.0, 1.0, 2.0, 3.0];
        let mut out = vec![0u8; 2 * 4];
        resample_take(&pool, 0.0, 2.0, 1, 2, 1.0, &mut out);
        let s = read_f32(&out);
        assert!((s[0] - 0.0).abs() < 1e-6);
        assert!((s[1] - 2.0).abs() < 1e-6);
    }

    #[test]
    fn rate_half_interpolates_between_frames() {
        // 0.5× 速：4 源帧出 7 输出帧（位置 0, 0.5, 1, ..., 3）
        let pool: Vec<f32> = vec![0.0, 1.0, 2.0, 3.0];
        let mut out = vec![0u8; 7 * 4];
        resample_take(&pool, 0.0, 0.5, 1, 7, 1.0, &mut out);
        let s = read_f32(&out);
        let expect = [0.0, 0.5, 1.0, 1.5, 2.0, 2.5, 3.0];
        for (i, (v, e)) in s.iter().zip(expect).enumerate() {
            assert!((v - e).abs() < 1e-6, "frame {i}: {v} != {e}");
        }
    }

    #[test]
    fn stereo_channels_stay_interleaved() {
        // 2 通道：L=0,1,2,3；R=10,11,12,13（交错）
        let mut pool = Vec::new();
        for f in 0..4i32 {
            pool.push(f as f32);
            pool.push(f as f32 + 10.0);
        }
        let mut out = vec![0u8; 4 * 2 * 4]; // rate=1 → 4 帧输出（2ch×4B）
        resample_take(&pool, 0.0, 1.0, 2, 4, 1.0, &mut out);
        let s = read_f32(&out);
        // 每帧 [L, R]
        for f in 0..4usize {
            assert!((s[f * 2] - f as f32).abs() < 1e-6, "L frame {f}");
            assert!((s[f * 2 + 1] - (f as f32 + 10.0)).abs() < 1e-6, "R frame {f}");
        }
    }

    #[test]
    fn volume_multiplies_after_resample() {
        let pool: Vec<f32> = vec![1.0, 1.0, 1.0, 1.0];
        let mut out = vec![0u8; 4];
        resample_take(&pool, 0.0, 1.0, 1, 1, 0.5, &mut out);
        let s = read_f32(&out);
        assert!((s[0] - 0.5).abs() < 1e-6);
    }

    #[test]
    fn pool_offset_continues_phase() {
        // pool_pos=1.5（上次消费残留的相位）：rate=1 → 从 1.5 插值起
        let pool: Vec<f32> = vec![0.0, 1.0, 2.0, 3.0];
        let mut out = vec![0u8; 4];
        resample_take(&pool, 1.5, 1.0, 1, 1, 1.0, &mut out);
        let s = read_f32(&out);
        assert!((s[0] - 1.5).abs() < 1e-6);
    }

    #[test]
    fn out_of_range_pos_clamps_to_tail() {
        // 越界 pool_pos：clamp 尾帧，不 panic（EOF 残余防御）
        let pool: Vec<f32> = vec![1.0, 2.0];
        let mut out = vec![0u8; 4];
        resample_take(&pool, 5.0, 1.0, 1, 1, 1.0, &mut out);
        let s = read_f32(&out);
        assert!((s[0] - 2.0).abs() < 1e-6);
    }
}
