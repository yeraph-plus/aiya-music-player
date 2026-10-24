//! WASAPI 输出（默认共享模式，事件驱动）。
//!
//! 关键决策（docs/04 §2）：共享模式用 `EventsShared { autoconvert: true }` ——
//! 引擎自己完成 源格式→mix format 的转换与 SRC，我们只交源采样率的 f32
//! 交错样本，v1 不自建重采样器。
//!
//! 独占模式（`EventsExclusive`）是**可选**能力：设备不接受时**回落共享**，
//! 并把原因随 `EngineEvent::AudioMode` 上报（完成定义要求"独占可选且失败可解释"）。
//! 独占下**不做重采样/声道映射**（采样率与声道必须精确匹配，否则回落），但允许
//! **位深下调** f32→i24→i16——实测板载声卡的独占接口普遍只接受整数格式，只试
//! f32 会让独占在消费级设备上形同虚设。位深转换是一次乘加取整，无 SRC 复杂度。

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
/// wire 侧（ADR-0002 sidecar）用相邻 tag：`{"t":"load","v":{"path":…,"startMs":…,"token":…}}`。
/// `rename_all_fields` 与 `EngineEvent` 对齐：不加它时字段名是 Rust 原样
/// （`start_ms`），同一份协议里出现 `start_ms` / `positionMs` 两种拼法。
///
/// `PartialEq` 只为测试断言"下发了哪些指令、顺序对不对"（宿主泵的换曲/停机
/// 语义全靠它验证）；比较语义与运行路径无关。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "t", content = "v", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum EngineCommand {
    /// 装载并从 `start_ms` 起播。
    ///
    /// `token` 是 PlayerActor 的装载代数（ADR-0003 generation）：引擎把它
    /// 原样回显进每个事件，宿主据此丢弃过期回报。此前事件不带身份，宿主
    /// 只能拿"当前 token"顶替 → 曲尾按下一首会把旧曲的 Eof 记到新曲头上，
    /// 连跳两首。
    ///
    /// `start_ms` 修掉"启动恢复永远从 0 秒出声"：此前 Actor 算出起点却被
    /// 适配层 `let _ = start_ms` 丢弃，引擎侧无从得知。
    Load { path: String, start_ms: u64, token: u64 },
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
    /// 独占模式开关。播放中切换会重开输出流（位置与已解码样本不受影响）；
    /// 设备拒绝时回落共享，原因见随后的 `EngineEvent::AudioMode`。
    SetExclusive(bool),
    /// 退出线程
    Shutdown,
}

/// 引擎线程发出的回报。
///
/// 每个事件都带 `token`（装载代数，来自最近的 `Load`）：宿主拿它与
/// Actor 的 token 比对，过期回报直接丢弃（ADR-0003）。装载前的命令
/// 回应（如未 Load 时的 Stop）token 为 0。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "t", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum EngineEvent {
    /// 装载成功（时长毫秒）
    Loaded { token: u64, duration_ms: u64 },
    /// 播放位置推进（渲染循环节拍发出，~每 10ms）
    Position { token: u64, position_ms: u64 },
    /// 曲目自然播完
    Eof { token: u64 },
    /// 状态变化（供 PlayerActor 对齐）
    StateChanged { token: u64, playing: bool },
    /// 频谱帧（tap FFT 线程 ~30fps；宿主原样 emit `spectrum:frame`）
    Spectrum { bands: Vec<u8> },
    /// 当前**实际**输出模式。`exclusive=false` 且 `reason` 有值 = 请求过独占但
    /// 被设备拒绝、已回落共享（不是错误，UI 应展示为"降级"而非失败）。
    AudioMode {
        token: u64,
        exclusive: bool,
        reason: Option<String>,
    },
    Error { token: u64, message: String },
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
    // 独占偏好同理跨曲目保持（设置项，不随换曲回默认）：
    // 用户开了独占后，后续每首都要先试独占；设备拒绝才回落。
    let mut pending_exclusive = false;
    'outer: loop {
        // 命令循环：等 Load 或 Shutdown（Load 前到的 SetRate/SetSpectrum 暂存生效）
        let (mut path, mut start_ms, mut token) = loop {
            match cmd_rx.recv() {
                Ok(EngineCommand::Load { path, start_ms, token }) => {
                    break (path, start_ms, token)
                }
                Ok(EngineCommand::SetRate(r)) => pending_rate = r.clamp(0.5, 2.0),
                Ok(EngineCommand::SetSpectrum(on)) => tap.set_enabled(on),
                Ok(EngineCommand::SetExclusive(on)) => pending_exclusive = on,
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
            match play_file(
                &cmd_rx,
                &send,
                &path,
                start_ms,
                token,
                pending_rate,
                pending_exclusive,
                &tap,
            ) {
                Ok(LoadExit::Finished { exclusive }) => {
                    pending_exclusive = exclusive;
                    break; // EOF/Stop/错误恢复：回顶层等下一条 Load
                }
                Ok(LoadExit::NewLoad { path: p, rate, start_ms: s, token: t, exclusive }) => {
                    path = p;
                    pending_rate = rate;
                    start_ms = s;
                    token = t;
                    pending_exclusive = exclusive;
                }
                Ok(LoadExit::Shutdown) => break 'outer,
                Err(e) => {
                    tracing::error!(%path, error = %e, "engine stream failed");
                    send(EngineEvent::Error { token, message: e.to_string() });
                    // 回到命令循环等下一条 Load（设备恢复）
                    break;
                }
            }
        }
    }

    { let _ = wasapi::deinitialize(); }
}

/// play_file 的退出原因。NewLoad 必须把 path/起点/代数带回外层续播。
enum LoadExit {
    /// EOF / Stop / 设备错误：回命令循环等下一条 Load。
    /// 带回最新的独占偏好（播放中用户可能切过模式）
    Finished { exclusive: bool },
    /// 播放中收到新 Load：直接换曲续播。
    NewLoad {
        path: String,
        rate: f32,
        start_ms: u64,
        token: u64,
        exclusive: bool,
    },
    /// 收到 Shutdown：终止渲染线程
    Shutdown,
}

fn handle_preload(cmd: &EngineCommand, send: &impl Fn(EngineEvent)) {
    match cmd {
        EngineCommand::Stop => send(EngineEvent::StateChanged { token: 0, playing: false }),
        _ => {}
    }
}

/// 设备失效恢复的最大重试次数（指数退避 200/400/800/1600/1600ms，约 4.6s 总窗口）。
/// 覆盖常见场景：默认设备切换（USB 声卡插拔）、蓝牙重连、驱动瞬时失效。
const DEVICE_RECOVERY_ATTEMPTS: u32 = 5;

/// 输出模式。独占是可选增强：被拒就回落 `Shared`，绝不静默改变音质。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OutputMode {
    Shared,
    Exclusive,
}

/// 送进设备的样本格式（`sample_bytes` 的编码方式）。
///
/// 为什么需要它：共享模式 `autoconvert: true` 让我们永远交 f32，但**独占模式
/// 由设备说了算**。实测（`examples/exclusive_probe.rs`，Realtek 板载）设备对
/// f32 一律拒绝、只接受整数格式——若只试 f32，独占在消费级声卡上等于不存在。
/// 位深转换只是乘一个标量再取整，不需要重采样器，所以放开：**采样率与声道数
/// 仍必须精确匹配**（那才需要 SRC，v1 不做）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OutFormat {
    /// 4 字节 IEEE float（共享模式恒为此；独占下设备原生支持时也用它）
    F32,
    /// 4 字节容器、高 24 bit 有效（WAVEFORMATEXTENSIBLE wValidBitsPerSample=24）
    I24In32,
    /// 2 字节整数
    I16,
}

impl OutFormat {
    fn bytes_per_sample(self) -> usize {
        match self {
            OutFormat::F32 | OutFormat::I24In32 => 4,
            OutFormat::I16 => 2,
        }
    }

    fn label(self) -> &'static str {
        match self {
            OutFormat::F32 => "f32",
            OutFormat::I24In32 => "i24-in-32",
            OutFormat::I16 => "i16",
        }
    }
}

/// 独占候选格式，**按音质优先级**排列：能无损就不降位深。
///
/// (格式, wBitsPerSample, wValidBitsPerSample, 子格式)
const EXCLUSIVE_CANDIDATES: [(OutFormat, usize, usize, SampleType); 3] = [
    (OutFormat::F32, 32, 32, SampleType::Float),
    (OutFormat::I24In32, 32, 24, SampleType::Int),
    (OutFormat::I16, 16, 16, SampleType::Int),
];

/// 打开好的输出流状态。设备失效时整体重建（换默认设备后 mix format 可能变，
/// autoconvert 需要重新协商）。
///
/// 外层 `Option` 只为"可释放"存在：独占模式下设备被**本进程**持有，切模式时
/// 必须先 drop 掉 `IAudioClient` 才能重新打开——实测只 `stop_stream` 仍会
/// 报 `AUDCLNT_E_DEVICE_IN_USE(0x8889000A)`。用 `Deref` 转到内层，其它读取点
/// （`out.client` / `out.mode` …）保持原样。
struct Output(Option<OutputInner>);

struct OutputInner {
    client: wasapi::AudioClient,
    event: wasapi::Handle,
    render: wasapi::AudioRenderClient,
    buffer_frames: u32,
    /// 实际生效的样本格式（独占降位深时不是 F32）
    fmt: OutFormat,
    /// 一帧原始字节缓冲，容量 = buffer_frames * channels * fmt.bytes_per_sample()
    sample_bytes: Vec<u8>,
    /// 实际生效的模式（请求独占但被拒时这里是 Shared）
    mode: OutputMode,
}

impl std::ops::Deref for Output {
    type Target = OutputInner;
    fn deref(&self) -> &OutputInner {
        self.0.as_ref().expect("输出流已释放却仍被访问")
    }
}

impl std::ops::DerefMut for Output {
    fn deref_mut(&mut self) -> &mut OutputInner {
        self.0.as_mut().expect("输出流已释放却仍被访问")
    }
}

impl Output {
    fn is_live(&self) -> bool {
        self.0.is_some()
    }

    /// 彻底释放 COM 句柄（独占切模式的前提）
    fn release(&mut self) {
        self.0 = None;
    }

    fn install(&mut self, other: Output) {
        self.0 = other.0;
    }

    /// 停流。已释放时静默跳过——切模式失败待重建的窗口里，Stop/Load/Shutdown
    /// 仍可能到达，不能因为流为空就 panic。
    fn stop(&mut self) {
        if let Some(i) = self.0.as_mut() {
            let _ = i.client.stop_stream();
        }
    }
}

fn src_format(decoder: &Decoder) -> WaveFormat {
    WaveFormat::new(
        32,
        32,
        &SampleType::Float,
        decoder.sample_rate as usize,
        decoder.channels,
        None,
    )
}

fn new_client() -> Result<wasapi::AudioClient, AudioError> {
    let enumerator = DeviceEnumerator::new().map_err(wasapi_err)?;
    let device = enumerator
        .get_default_device(&Direction::Render)
        .map_err(wasapi_err)?;
    device.get_iaudioclient().map_err(wasapi_err)
}

fn finish_stream(
    client: wasapi::AudioClient,
    mode: OutputMode,
    channels: usize,
    fmt: OutFormat,
) -> Result<Output, AudioError> {
    let event = client.set_get_eventhandle().map_err(wasapi_err)?;
    let render = client.get_audiorenderclient().map_err(wasapi_err)?;
    let buffer_frames = client.get_buffer_size().map_err(wasapi_err)?;
    let sample_bytes = vec![0u8; buffer_frames as usize * channels * fmt.bytes_per_sample()];
    client.start_stream().map_err(wasapi_err)?;
    Ok(Output(Some(OutputInner {
        client,
        event,
        render,
        buffer_frames,
        fmt,
        sample_bytes,
        mode,
    })))
}

/// 共享模式：交给引擎做格式转换（autoconvert），我们只交源格式 f32。
fn open_shared(decoder: &Decoder) -> Result<Output, AudioError> {
    let mut client = new_client()?;
    client
        .initialize_client(
            &src_format(decoder),
            &Direction::Render,
            &StreamMode::EventsShared {
                autoconvert: true,
                buffer_duration_hns: 2_000_000, // 200ms
            },
        )
        .map_err(wasapi_err)?;
    finish_stream(client, OutputMode::Shared, decoder.channels, OutFormat::F32)
}

/// 独占模式：设备必须接受**源采样率 + 源声道数**，位深可降（见 [`OutFormat`]）。
///
/// 仍然不做重采样：独占的意义是绕开共享混音器，只要采样率对得上，位深从
/// f32 降到 i24/i16 是有损但极小的（16bit 源在 i16 上完全无损）。真碰上
/// 采样率不匹配（例如 96k 源 + 只支持 48k 的设备），宁可回落共享也不做 SRC。
///
/// 返回 `Err(原因)` 由调用方回落共享并且**把原因带给 UI**——"独占没生效"
/// 必须是可解释的，不能只是静默变差。
fn open_exclusive(decoder: &Decoder) -> Result<Output, String> {
    let rate = decoder.sample_rate;
    let channels = decoder.channels;
    // 记录"最接近成功"的那次失败：全部候选都挂时用它解释原因
    let mut best_reason: Option<String> = None;

    for (fmt, bits, valid, sub) in EXCLUSIVE_CANDIDATES {
        // 每个候选重建 client：Initialize 失败后同一 client 的可用性依赖驱动实现
        let mut client = new_client().map_err(|e| e.to_string())?;
        let want = WaveFormat::new(bits, valid, &sub, rate as usize, channels, None);
        let negotiated = match client.is_supported_exclusive_with_quirks(&want) {
            Ok(f) => f,
            Err(e) => {
                best_reason = Some(format!(
                    "{rate} Hz / {channels} ch 下 f32/i24/i16 均被拒绝（最后尝试 {}：{e}）",
                    fmt.label()
                ));
                continue;
            }
        };
        if negotiated.get_samplespersec() != rate || negotiated.get_nchannels() as usize != channels {
            best_reason = Some(format!(
                "设备独占只支持 {} Hz / {} ch（源为 {rate} Hz / {channels} ch，不做重采样）",
                negotiated.get_samplespersec(),
                negotiated.get_nchannels()
            ));
            continue;
        }
        // 以协商回来的实际位深为准（driver 可能把 i24-in-32 折成别的东西）
        let actual = match (negotiated.get_bitspersample(), negotiated.get_subformat().ok()) {
            (32, Some(SampleType::Float)) => OutFormat::F32,
            (32, Some(SampleType::Int)) => OutFormat::I24In32,
            (16, _) => OutFormat::I16,
            (b, _) => {
                best_reason = Some(format!("设备独占要求 {b} bit 格式，暂不支持"));
                continue;
            }
        };
        let (period, _min) = match client.get_device_period() {
            Ok(p) => p,
            Err(e) => {
                best_reason = Some(format!("读取设备周期失败（{e}）"));
                continue;
            }
        };
        if let Err(e) = client.initialize_client(
            &negotiated,
            &Direction::Render,
            &StreamMode::EventsExclusive { period_hns: period },
        ) {
            best_reason = Some(format!("{} 独占流初始化失败（{e}）", actual.label()));
            continue;
        }
        tracing::info!(format = actual.label(), rate, channels, "独占模式已启用");
        return finish_stream(client, OutputMode::Exclusive, channels, actual)
            .map_err(|e| e.to_string());
    }

    Err(best_reason.unwrap_or_else(|| "设备不接受任何独占格式".to_string()))
}

/// 打开输出：请求独占时先试独占，失败回落共享。第二个返回值是回落原因。
fn open_output(decoder: &Decoder, want: OutputMode) -> Result<(Output, Option<String>), AudioError> {
    if want == OutputMode::Exclusive {
        match open_exclusive(decoder) {
            Ok(out) => return Ok((out, None)),
            Err(reason) => {
                // 每次尝试都重建 AudioClient：Initialize 失败后同一 client 的
                // 可用性依赖驱动实现，重建一次最省心（只在装载/切换时发生）
                let out = open_shared(decoder)?;
                return Ok((out, Some(reason)));
            }
        }
    }
    Ok((open_shared(decoder)?, None))
}

/// 重建输出流（退避后重枚举默认设备）。位置无需显式恢复——
/// 池内已解码样本和 decoder 位置都在调用方手里，天然连续。
/// `want` 是用户请求的模式（不是上一次的实际模式）：换设备后可能从共享变独占。
fn reopen_output(
    out: &mut Output,
    decoder: &Decoder,
    attempt: u32,
    want: OutputMode,
) -> Result<Option<String>, AudioError> {
    // 旧流尽力停，失败无所谓（马上整个丢弃）；已释放时跳过（switch_output 的
    // Dead 分支会走到这里）
    out.stop();
    let backoff_ms = 200u64 << attempt.min(3);
    std::thread::sleep(Duration::from_millis(backoff_ms));
    let (new_out, note) = open_output(decoder, want)?;
    out.install(new_out);
    Ok(note)
}

/// 切模式的结果。三种结局必须分开表达——"没切成"和"设备没了"对用户的
/// 意义完全不同：前者音乐继续、只解释一句；后者要走设备恢复。
enum SwitchOutcome {
    /// 已切到目标模式（`None` 表示没降级，`Some` 是回落共享的原因）
    Switched(Option<String>),
    /// 目标模式打不开，已退回原模式继续播（带原因）
    KeptPlaying(String),
    /// 原模式也开不起来：流为空，必须走设备恢复
    Dead(AudioError),
}

/// 播放中切换输出模式：**先彻底释放旧流，再开新流**。
///
/// 顺序不能反：独占期间设备被本进程独占，一边持有 IAudioClient 一边开共享流会
/// 撞 `AUDCLNT_E_DEVICE_IN_USE(0x8889000A)`（只 `stop_stream` 不够，实测仍失败，
/// 必须 drop 掉句柄）。代价是新流建立期间有几十毫秒空档，这是切模式本身的语义。
fn switch_output(out: &mut Output, decoder: &Decoder, want: OutputMode) -> SwitchOutcome {
    let prev = out.mode;
    out.release();
    match open_output(decoder, want) {
        Ok((new_out, note)) => {
            out.install(new_out);
            SwitchOutcome::Switched(note)
        }
        // 用户只是想切模式，不该因此丢音乐：退回原模式再试一次
        Err(e) => match open_output(decoder, prev) {
            Ok((back, _)) => {
                out.install(back);
                SwitchOutcome::KeptPlaying(e.to_string())
            }
            Err(e2) => {
                tracing::error!(%e, %e2, "切换输出模式失败且无法退回原模式");
                SwitchOutcome::Dead(e2)
            }
        },
    }
}

/// 打开文件并进入渲染循环直到 Stop/Shutdown/新 Load/EOF。
/// `start_ms`：装载起点（启动恢复/崩溃恢复用；0 = 从头）。
/// `token`：本曲装载代数，原样回显进所有事件。
/// `initial_rate`：Load 前收到的变速指令（如启动恢复/恢复会话）。
/// `initial_exclusive`：设置项里的独占偏好（设备拒绝时回落共享并上报原因）。
/// `tap`：频谱采样点（跨曲目常驻；推**重采样后**的输出帧，未乘音量）。
fn play_file(
    cmd_rx: &Receiver<EngineCommand>,
    send: &impl Fn(EngineEvent),
    path: &str,
    start_ms: u64,
    token: u64,
    initial_rate: f32,
    initial_exclusive: bool,
    tap: &crate::spectrum::SpectrumTap,
) -> Result<LoadExit, AudioError> {
    let mut decoder = Decoder::open(Path::new(path))?;
    tap.set_sample_rate(decoder.sample_rate); // 频谱分带按实际采样率标定
    // 起点必须在开流之前落定：先播再 seek 会有可闻的"从头闪一下"
    if start_ms > 0 {
        decoder.seek_ms(start_ms)?;
    }
    send(EngineEvent::Loaded { token, duration_ms: decoder.duration_ms });

    // 模式意图与模式事实分开记：want 是用户设置，out.mode 是设备实际给的。
    // 回落不算错误（Error 会让 UI 弹红条），用 AudioMode 的 reason 说明。
    let mut want_exclusive = initial_exclusive;
    let (mut out, mut mode_note) = open_output(
        &decoder,
        if want_exclusive { OutputMode::Exclusive } else { OutputMode::Shared },
    )?;
    send(EngineEvent::AudioMode {
        token,
        exclusive: out.mode == OutputMode::Exclusive,
        reason: mode_note.take(),
    });

    // 已解码未播的交错样本池：变速插值的数据源（rate=1 时等价于直通）
    let mut pool: Vec<f32> = Vec::new();
    // 池内的源帧读取位置（帧单位，f64）；drain 后减去已消费帧数保持相位连续
    let mut pool_pos: f64 = 0.0;
    let mut decoder_eof = false;

    let mut volume: f32 = 1.0;
    // 当前实际输出音量（逐样本向 `volume` 逼近；跨 chunk 保持，否则每块首
    // 样本都会重新跳变，斜坡等于没做）
    let mut volume_now: f32 = 1.0;
    // 一个斜坡跨多少个样本（按源采样率标定，变速时斜坡时长略变，无感）
    let ramp_samples = (decoder.sample_rate * VOLUME_RAMP_MS / 1000).max(1) as f32;
    let mut rate: f32 = initial_rate;
    let mut paused = false;
    let mut user_stopped = false;
    // 频谱推的是输出帧（重采样 + 单声道混合后、乘音量前）：
    // 此前推源帧，2× 速入环速率翻倍长期溢出丢帧，0.5× 掉到 ~12fps。
    let mut tap_mono: Vec<f32> = Vec::new();
    // 重采样后的 f32 输出暂存（乘过音量），随后按设备位深编码成字节。
    // 独占降位深时要两趟：一趟算术、一趟取整，避免在插值内联两套写字节逻辑。
    let mut out_f32: Vec<f32> = Vec::new();
    send(EngineEvent::StateChanged { token, playing: true });

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
        let wait_ms: u32 = if paused { 100 } else { 2000 };
        if out.is_live() {
            let _ = out.event.wait_for_event(wait_ms);
        } else {
            // 流被释放（切模式失败待重建）：没有事件句柄可等，别忙转
            std::thread::sleep(Duration::from_millis(u64::from(wait_ms.min(50))));
        }

        // 非阻塞处理命令
        loop {
            match cmd_rx.try_recv() {
                Ok(EngineCommand::Pause) if !paused => {
                    // 暂停 = 停止喂数据，流保持 running（事件照常触发、命令
                    // 循环存活）。不 stop_stream：停流后 WASAPI 引擎不再为
                    // 该流处理传递，事件停发。缓冲内残余（≤200ms）播完自然
                    // 静音，Resume 直接续喂数据，无 start/stop 状态机风险。
                    paused = true;
                    send(EngineEvent::StateChanged { token, playing: false });
                }
                Ok(EngineCommand::Resume) if paused => {
                    paused = false;
                    // 流从未停止（见 Pause），无需 start_stream，直接恢复喂数据
                    send(EngineEvent::StateChanged { token, playing: true });
                }
                Ok(EngineCommand::Seek(ms)) => {
                    // stop（reset 要求 stopped 态）→ seek → 清池（旧样本作废）→ reset → 重启
                    out.stop();
                    if let Err(e) = decoder.seek_ms(ms) {
                        result = Err(e);
                        break;
                    }
                    pool.clear();
                    pool_pos = 0.0;
                    decoder_eof = false;
                    // 流可能正处于"已释放待重建"的窗口（切模式两次都失败）：
                    // 此时只挪 decoder 位置，重置/重启交给恢复路径
                    if out.is_live() {
                        let _ = out.client.reset_stream();
                        // 新暂停模型：装载期间流始终 running（暂停只是不喂数据），
                        // seek 后无条件重启；暂停态缓冲为空继续静音。
                        // start 失败按设备错误走恢复路径（不走 ? 拆流）
                        if let Err(e) = out.client.start_stream() {
                            stream_err = Some(wasapi_err(e));
                            break;
                        }
                    }
                    send(EngineEvent::Position { token, position_ms: decoder.position_ms() });
                }
                Ok(EngineCommand::SetVolume(v)) => volume = v.clamp(0.0, 1.0),
                Ok(EngineCommand::SetRate(r)) => rate = r.clamp(0.5, 2.0),
                Ok(EngineCommand::SetSpectrum(on)) => tap.set_enabled(on),
                Ok(EngineCommand::SetExclusive(on)) => {
                    want_exclusive = on;
                    let want = if on { OutputMode::Exclusive } else { OutputMode::Shared };
                    // 只在"实际模式和意图不一致"时重开流；独占被拒后再次请求
                    // （用户重试）仍会走到这里，因为 out.mode 仍是 Shared。
                    if out.mode != want {
                        match switch_output(&mut out, &decoder, want) {
                            SwitchOutcome::Switched(note) => send(EngineEvent::AudioMode {
                                token,
                                exclusive: out.mode == OutputMode::Exclusive,
                                reason: note,
                            }),
                            SwitchOutcome::KeptPlaying(why) => {
                                send(EngineEvent::AudioMode {
                                    token,
                                    exclusive: out.mode == OutputMode::Exclusive,
                                    reason: Some(format!("切换失败：{why}")),
                                })
                            }
                            // 流已彻底释放：交给设备恢复路径重建（会重新退避重试）
                            SwitchOutcome::Dead(e) => {
                                stream_err = Some(e);
                                break;
                            }
                        }
                    } else {
                        // 已经在目标模式（例如"切回共享"时本来就是共享）：
                        // 仍回报一次，让调用方拿到确认而不是靠猜
                        send(EngineEvent::AudioMode {
                            token,
                            exclusive: out.mode == OutputMode::Exclusive,
                            reason: None,
                        });
                    }
                }
                Ok(EngineCommand::Stop) => {
                    user_stopped = true;
                    break;
                }
                Ok(EngineCommand::Load { path: p, start_ms: s, token: t }) => {
                    // 换曲：停流后把新 path/起点/代数带回外层续播（绝不能在此丢弃）
                    out.stop();
                    return Ok(LoadExit::NewLoad {
                        path: p,
                        rate,
                        start_ms: s,
                        token: t,
                        exclusive: want_exclusive,
                    });
                }
                Ok(EngineCommand::Shutdown) => {
                    out.stop();
                    return Ok(LoadExit::Shutdown);
                }
                Ok(_) => {}
                Err(_) => break,
            }
        }
        if user_stopped {
            send(EngineEvent::StateChanged { token, playing: false });
            send(EngineEvent::Eof { token }); // Stop 视为曲目结束（PlayerActor 决定下一动作）
            out.stop();
            return Ok(LoadExit::Finished { exclusive: want_exclusive });
        }
        if result.is_err() {
            out.stop();
            return Err(result.unwrap_err());
        }

        // 设备错误恢复（唯一出口）：重建输出流后继续当前曲目。
        // 位置天然连续（池与 decoder 状态都在本地）；已写入旧设备缓冲的
        // 样本随重建丢失，最多回跳 200ms，可接受。
        //
        // 注意顺序：必须在 `if paused` 之前处理。此前 pause 分支先 continue，
        // 暂停态下 seek 的 start_stream 失败（stream_err 置位）永不处理——
        // 既不重建也不上报，UI 完全无感。
        if let Some(e) = stream_err.take() {
            if device_failures >= DEVICE_RECOVERY_ATTEMPTS {
                tracing::error!(%e, attempts = device_failures, "设备恢复重试耗尽，放弃当前曲目");
                out.stop();
                return Err(AudioError::Wasapi(format!("设备恢复失败（{device_failures} 次重试）：{e}")));
            }
            let want = if want_exclusive { OutputMode::Exclusive } else { OutputMode::Shared };
            match reopen_output(&mut out, &decoder, device_failures, want) {
                Ok(note) => {
                    device_failures += 1; // 写成功后归零；连续失败则退避递增
                    tracing::warn!(error = %e, attempt = device_failures, "输出设备失效，已重建（退避重试中）");
                    // 换设备后实际模式可能变（新设备不支持独占 / 反而支持）：
                    // 不告知 UI 的话，设置页会一直显示旧设备的能力
                    if note.is_some() || out.mode != want {
                        send(EngineEvent::AudioMode {
                            token,
                            exclusive: out.mode == OutputMode::Exclusive,
                            reason: note,
                        });
                    }
                }
                Err(re) => {
                    device_failures += 1;
                    tracing::warn!(error = %e, retry_error = %re, attempt = device_failures, "设备重建失败，将继续退避");
                }
            }
            continue;
        }

        // 走到这里流必须是活的。重建失败的 `continue` 已在上方返回，但切模式
        // 的 Dead 分支会留下空流——直接去碰 padding/render 会 panic。
        if !out.is_live() {
            std::thread::sleep(Duration::from_millis(20));
            continue;
        }

        if paused {
            // 暂停时不喂数据，但仍要探测设备存活（拔耳机/切默认设备）：
            // 只看一眼 padding，失败即置 stream_err，下轮走上面的恢复路径。
            if let Err(e) = out.client.get_current_padding() {
                stream_err = Some(wasapi_err(e));
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
        let available = out.buffer_frames.saturating_sub(padding);
        // 每轮渲染心跳都上报位置（泵侧再做 500ms 节流；写满瞬间 available 不会为 0）
        // 位置要扣掉设备缓冲里还没播的部分，否则进度条领先耳朵最多 200ms
        let position_now = |decoder: &Decoder, pool: &[f32], padding: u32| -> u64 {
            play_position(decoder, pool).saturating_sub(padding_ms(decoder, padding))
        };
        send(EngineEvent::Position {
            token,
            position_ms: position_now(&decoder, &pool, padding),
        });
        if available == 0 {
            continue;
        }

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

            // 3) 线性插值输出（索引 clamp 保证尾部不越界），再编码成设备位深
            let bps = out.fmt.bytes_per_sample();
            let frame_bytes_all = channels * bps;
            out_f32.clear();
            tap_mono.clear();
            // 音量斜坡：本块内从 volume_now 线性走到目标，块尾写回
            let diff = volume - volume_now;
            let step = if diff.abs() < 1e-5 { 0.0 } else { diff / ramp_samples };
            resample_take(
                &pool,
                pool_pos,
                rate as f64,
                channels,
                out_frames,
                volume_now,
                step,
                &mut out_f32,
                &mut tap_mono,
            );
            volume_now = (volume_now + step * out_frames as f32).clamp(0.0, 1.0);
            if step == 0.0 {
                volume_now = volume; // 避免浮点残差一直"差一点点"
            }
            encode_frames(
                out.fmt,
                &out_f32,
                &mut out.sample_bytes[..out_frames * frame_bytes_all],
            );
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
            //    tap 点：推**重采样后的输出帧**（未乘音量，低音量频谱仍可视），
            //    单声道已在 resample_take 内混好 —— 与"实际出声内容"同速。
            pool_pos += out_frames as f64 * rate as f64;
            let consumed = pool_pos.floor() as usize;
            if consumed > 0 {
                let n = consumed.min(pool_frames) * channels;
                pool.drain(..n);
                pool_pos -= consumed as f64;
            }
            tap.push_mono(&tap_mono);
            if frames_to_write == 0 {
                break 'fill;
            }
        }

        if eof_now {
            // 等缓冲播完再报 EOF：padding 归零或超时。等待期间必须继续消费
            // 命令——此前只认 Shutdown，用户在曲尾 500ms 内按"下一首"的 Load
            // 会滞留到 EOF 之后，表现是慢半拍甚至丢一次切歌。
            let deadline = std::time::Instant::now() + Duration::from_millis(500);
            loop {
                match cmd_rx.try_recv() {
                    Ok(EngineCommand::Shutdown) => {
                        out.stop();
                        return Ok(LoadExit::Shutdown);
                    }
                    Ok(EngineCommand::Load { path: p, start_ms: s, token: t }) => {
                        out.stop();
                        return Ok(LoadExit::NewLoad {
                        path: p,
                        rate,
                        start_ms: s,
                        token: t,
                        exclusive: want_exclusive,
                    });
                    }
                    Ok(EngineCommand::Stop) => {
                        user_stopped = true;
                        break;
                    }
                    Ok(EngineCommand::SetSpectrum(on)) => tap.set_enabled(on),
                    // 曲尾只剩不到 500ms：只更新偏好，重开流留给下一曲
                    Ok(EngineCommand::SetExclusive(on)) => want_exclusive = on,
                    Ok(EngineCommand::Seek(_))
                    | Ok(EngineCommand::Pause)
                    | Ok(EngineCommand::Resume)
                    | Ok(EngineCommand::SetVolume(_))
                    | Ok(EngineCommand::SetRate(_)) => {
                        // 曲尾的这些指令无意义（马上换曲/停止），丢弃即可；
                        // 换曲后宿主会重新下发音量/倍速（见 drive 序列）
                    }
                    Err(_) => {}
                }
                let pad = out.client.get_current_padding().unwrap_or(0);
                if pad == 0 || std::time::Instant::now() > deadline {
                    break;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            out.stop();
            if user_stopped {
                send(EngineEvent::StateChanged { token, playing: false });
                send(EngineEvent::Eof { token });
                return Ok(LoadExit::Finished { exclusive: want_exclusive });
            }
            send(EngineEvent::Position { token, position_ms: decoder.duration_ms });
            send(EngineEvent::StateChanged { token, playing: false });
            send(EngineEvent::Eof { token });
            // EOF 后回命令循环等下一条 Load
            return Ok(LoadExit::Finished { exclusive: want_exclusive });
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

/// 设备缓冲区里尚未播出的时长（毫秒）。docs/04 §3.5：
/// position = base + frames_written - latency_ms。不扣这段，进度条会比
/// 耳朵超前最多一个缓冲深度（200ms），暂停瞬间停在"超前 200ms"处。
fn padding_ms(decoder: &Decoder, padding_frames: u32) -> u64 {
    (padding_frames as u64 * 1000) / (decoder.sample_rate as u64).max(1)
}

/// 从 pool 的 pool_pos 起按 rate 线性插值取 out_frames 帧，交错 f32（已乘音量）写入 out。
/// 输出帧 i 对应源位置 pool_pos + i*rate；越界索引 clamp 到池尾（仅 EOF 尾部触发）。
/// `vol_start` / `vol_step`：音量斜坡的本块起点与每帧增量（见 [`VOLUME_RAMP_MS`]）。
/// `mono_out`：同步写出每帧的单声道平均（未乘音量），供频谱 tap 使用。
#[allow(clippy::too_many_arguments)]
fn resample_take(
    pool: &[f32],
    pool_pos: f64,
    rate: f64,
    channels: usize,
    out_frames: usize,
    vol_start: f32,
    vol_step: f32,
    out: &mut Vec<f32>,
    mono_out: &mut Vec<f32>,
) {
    let pool_frames = pool.len() / channels.max(1);
    let inv_ch = 1.0 / channels.max(1) as f32;
    out.reserve(out_frames * channels);
    for i in 0..out_frames {
        let src_f = pool_pos + i as f64 * rate;
        let i0 = (src_f.floor() as usize).min(pool_frames.saturating_sub(1));
        let i1 = (i0 + 1).min(pool_frames.saturating_sub(1));
        // i0 被 clamp 到池尾时 frac 可能 >1（EOF 残余防御路径），钳到 [0,1]
        // 复制末帧而非线性外插
        let frac = ((src_f - i0 as f64) as f32).clamp(0.0, 1.0);
        let vol = vol_start + vol_step * i as f32;
        let mut mono = 0.0f32;
        for ch in 0..channels {
            let a = pool[i0 * channels + ch];
            let b = pool[i1 * channels + ch];
            let s = a + (b - a) * frac;
            mono += s;
            out.push(s * vol);
        }
        mono_out.push(mono * inv_ch);
    }
}

/// f32（-1.0..1.0）→ 目标位深的交错字节。
///
/// 取整用 `round`（就近）而不是截断：截断会给所有样本引入 −0.5 LSB 的直流偏置，
/// 在 i16 上就是可测的偏置。不做抖动（dither）：f32→i24 的量化噪声在 −144 dBFS，
/// 远低于任何设备的底噪；为了它引入随机数不划算。
fn encode_frames(fmt: OutFormat, src: &[f32], dst: &mut [u8]) {
    match fmt {
        OutFormat::F32 => {
            for (c, s) in dst.chunks_exact_mut(4).zip(src) {
                c.copy_from_slice(&s.to_le_bytes());
            }
        }
        OutFormat::I24In32 => {
            // 24 bit 有效位**左对齐**在 32 bit 容器里（WAVEFORMATEXTENSIBLE 约定）
            for (c, s) in dst.chunks_exact_mut(4).zip(src) {
                let x = (s.clamp(-1.0, 1.0) * 8_388_608.0)
                    .round()
                    .clamp(-8_388_608.0, 8_388_607.0);
                c.copy_from_slice(&((x as i32) << 8).to_le_bytes());
            }
        }
        OutFormat::I16 => {
            for (c, s) in dst.chunks_exact_mut(2).zip(src) {
                let x = (s.clamp(-1.0, 1.0) * 32_768.0)
                    .round()
                    .clamp(-32_768.0, 32_767.0);
                c.copy_from_slice(&(x as i16).to_le_bytes());
            }
        }
    }
}

/// 音量斜坡时长（毫秒）。音量是逐样本乘上去的，目标值瞬间跳变等于给波形
/// 加了一个阶跃 → 宽带"咔哒"。20ms 听不出过渡，也不至于让拖音量条发钝。
const VOLUME_RAMP_MS: u32 = 20;

fn wasapi_err(e: wasapi::WasapiError) -> AudioError {
    AudioError::Wasapi(e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 跑一遍 resample_take，返回交错 f32 输出。
    fn take(pool: &[f32], pos: f64, rate: f64, ch: usize, frames: usize, vol: f32) -> Vec<f32> {
        let mut out = Vec::new();
        resample_take(pool, pos, rate, ch, frames, vol, 0.0, &mut out, &mut Vec::new());
        out
    }

    #[test]
    fn rate_one_is_passthrough() {
        // 1 通道 4 帧：0,1,2,3 → 原样
        let s = take(&[0.0, 1.0, 2.0, 3.0], 0.0, 1.0, 1, 4, 1.0);
        for (i, v) in s.iter().enumerate() {
            assert!((v - i as f32).abs() < 1e-6, "frame {i} = {v}");
        }
    }

    #[test]
    fn rate_two_consumes_every_other_frame() {
        // 2× 速：4 源帧出 2 输出帧（位置 0 和 2）
        let s = take(&[0.0, 1.0, 2.0, 3.0], 0.0, 2.0, 1, 2, 1.0);
        assert!((s[0] - 0.0).abs() < 1e-6);
        assert!((s[1] - 2.0).abs() < 1e-6);
    }

    #[test]
    fn rate_half_interpolates_between_frames() {
        // 0.5× 速：4 源帧出 7 输出帧（位置 0, 0.5, 1, ..., 3）
        let s = take(&[0.0, 1.0, 2.0, 3.0], 0.0, 0.5, 1, 7, 1.0);
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
        let s = take(&pool, 0.0, 1.0, 2, 4, 1.0); // rate=1 → 4 帧输出
        for f in 0..4usize {
            assert!((s[f * 2] - f as f32).abs() < 1e-6, "L frame {f}");
            assert!((s[f * 2 + 1] - (f as f32 + 10.0)).abs() < 1e-6, "R frame {f}");
        }
    }

    #[test]
    fn volume_multiplies_after_resample() {
        let s = take(&[1.0, 1.0, 1.0, 1.0], 0.0, 1.0, 1, 1, 0.5);
        assert!((s[0] - 0.5).abs() < 1e-6);
    }

    #[test]
    fn volume_ramp_is_monotonic_within_block() {
        // 0 → 1 在一个 4 帧块里线性上升，中间没有阶跃（每步 0.25）
        let pool = [1.0f32; 4];
        let mut out = Vec::new();
        resample_take(&pool, 0.0, 1.0, 1, 4, 0.0, 0.25, &mut out, &mut Vec::new());
        let expect = [0.0, 0.25, 0.5, 0.75];
        for (i, (v, e)) in out.iter().zip(expect).enumerate() {
            assert!((v - e).abs() < 1e-6, "frame {i}: {v} != {e}");
        }
    }

    #[test]
    fn volume_ramp_continues_across_blocks() {
        // 第二块从上一块的终点相位继续（0.0 + 4×0.25 = 1.0 → 直接满音量）
        let pool2 = [1.0f32; 2];
        let mut out = Vec::new();
        resample_take(&pool2, 1.0, 1.0, 1, 2, 1.0, 0.25, &mut out, &mut Vec::new());
        assert!((out[0] - 1.0).abs() < 1e-6);
    }

    #[test]
    fn pool_offset_continues_phase() {
        // pool_pos=1.5（上次消费残留的相位）：rate=1 → 从 1.5 插值起
        let s = take(&[0.0, 1.0, 2.0, 3.0], 1.5, 1.0, 1, 1, 1.0);
        assert!((s[0] - 1.5).abs() < 1e-6);
    }

    #[test]
    fn out_of_range_pos_clamps_to_tail() {
        // 越界 pool_pos：clamp 尾帧，不 panic（EOF 残余防御）
        let s = take(&[1.0, 2.0], 5.0, 1.0, 1, 1, 1.0);
        assert!((s[0] - 2.0).abs() < 1e-6);
    }

    // ---- encode_frames：独占降位深 ----

    /// 读出 i16 交错样本。
    fn read_i16(bytes: &[u8]) -> Vec<i16> {
        bytes
            .chunks_exact(2)
            .map(|c| i16::from_le_bytes([c[0], c[1]]))
            .collect()
    }

    /// 读出 i24-in-32（右移还原成 24 bit 数值）。
    fn read_i24(bytes: &[u8]) -> Vec<i32> {
        bytes
            .chunks_exact(4)
            .map(|c| i32::from_le_bytes([c[0], c[1], c[2], c[3]]) >> 8)
            .collect()
    }

    #[test]
    fn encode_f32_is_bit_exact() {
        let src = [0.0f32, 0.5, -0.5, 1.0];
        let mut dst = vec![0u8; src.len() * 4];
        encode_frames(OutFormat::F32, &src, &mut dst);
        let back: Vec<f32> = dst
            .chunks_exact(4)
            .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect();
        assert_eq!(back, src);
    }

    #[test]
    fn encode_i16_scales_and_clamps() {
        // 满刻度 ±1.0 要落在 ±32767/-32768，超范围要削顶而不是回绕
        let src = [0.0f32, 1.0, -1.0, 1.5, -1.5, 0.5];
        let mut dst = vec![0u8; src.len() * 2];
        encode_frames(OutFormat::I16, &src, &mut dst);
        assert_eq!(read_i16(&dst), vec![0, 32767, -32768, 32767, -32768, 16384]);
    }

    #[test]
    fn encode_i24_is_left_aligned_in_32() {
        // 24 bit 有效位左对齐：读数右移 8 后应等于 round(v * 2^23)
        let src = [0.0f32, 1.0, -1.0, 0.25, -0.25];
        let mut dst = vec![0u8; src.len() * 4];
        encode_frames(OutFormat::I24In32, &src, &mut dst);
        assert_eq!(
            read_i24(&dst),
            vec![0, 8_388_607, -8_388_608, 2_097_152, -2_097_152]
        );
        // 最高有效字节不能为 0（否则说明写成了右对齐）
        assert_eq!(dst[4 + 3], 0x7F, "左对齐时第 4 字节应是 0x7F");
    }

    #[test]
    fn encode_rejects_short_dst_silently() {
        // 缓冲区比样本多/少时只按 dst 长度写，绝不越界 panic
        let src = [1.0f32, 1.0, 1.0];
        let mut dst = vec![0u8; 4]; // 只装得下 2 个 i16
        encode_frames(OutFormat::I16, &src, &mut dst);
        assert_eq!(read_i16(&dst), vec![32767, 32767]);
    }
}
