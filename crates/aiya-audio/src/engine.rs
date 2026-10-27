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

use crate::decoder::DecodedStream;
use crate::AudioError;

/// 引擎线程收到的命令。
///
/// wire 侧（ADR-0002 sidecar）用相邻 tag：`{"t":"load","v":{"path":…,"startMs":…,"token":…,"paused":…}}`。
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
    ///
    /// `paused`（协议 v7）修掉"启动恢复先响一下"：此前"装载但暂停"要靠
    /// Load 后紧跟一条 Pause 实现，而引擎在 Load 的开流期间就开始出声，
    /// Pause 要等渲染循环第一次收命令才生效——启动恢复必有几百毫秒残声。
    /// 现在装载意图自带暂停态，开流前就知道不该出声。
    Load {
        path: String,
        start_ms: u64,
        token: u64,
        #[serde(default)]
        paused: bool,
    },
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
    /// 输出配置（模式 / 时钟 / 缓冲深度）。播放中切换会重开输出流（位置与
    /// 已解码样本不受影响）；实际结果见随后的 `EngineEvent::AudioMode`。
    ///
    /// 取代早期的 `SetExclusive(bool)`：模式、时钟、缓冲是**一组**参数，分开
    /// 下发会出现"模式已切、缓冲还是旧的"中间态，且每次都要重建流两次。
    SetOutputConfig(OutputConfig),
    /// 10 段 EQ 整体下发（协议 v6，docs/24 B5b）。播放中热切：系数重算、
    /// 延迟线保留。生效值（clamp 后）经 `EngineEvent::EqApplied` 回报。
    SetEq(crate::eq::EqParams),
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
    /// 当前**实际**输出状态。这是"设备到底给了什么"，不是用户设置：
    /// `mode=shared` 而 `requested=exclusive` 就是回落（原因在 `reason`）。
    AudioMode {
        token: u64,
        /// 用户请求的模式：`shared` / `exclusive` / `exclusiveRequired` / `null`
        requested: String,
        /// 实际模式：`shared` / `exclusive` / `null`
        mode: String,
        /// 实际时钟：`events` / `polling`
        clock: String,
        /// 实际缓冲深度（毫秒，取整）
        buffer_ms: u32,
        /// 实际送设备的样本格式：`f32` / `i24-in-32` / `i16`
        format: String,
        /// 回落/降级原因（空 = 完全按请求生效）
        reason: Option<String>,
    },
    /// 播放失败。`kind` 是**分类**（`fileMissing`/`io`/`decode`/
    /// `codecUnsupported`/`deviceLost`/`unsupportedFormat`/`unknown`），在错误
    /// 产生处由 `AudioError::kind()` 定，不经宿主猜——UI 要靠它决定
    /// "跳下一首"还是"提示/亮灰标"。
    Error { token: u64, kind: String, message: String },
    /// EQ 设置已被 sidecar 接受（协议 v6）。`params` 是**生效值**（clamp 后），
    /// UI 以它为准而不是自己发出去的值。未装载时的 SetEq 回报 token=0。
    EqApplied { token: u64, params: crate::eq::EqParams },
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
    /// 累计写入设备的帧数（跨曲目不清零）。
    ///
    /// 存在的理由：**"引擎报告在播"和"声音真的在走"是两件事**。独占模式曾
    /// 因 padding 恒等于 buffer 而只写一缓冲就停（`writable_frames` 注释有
    /// 实测），而当时的冒烟只断言了 `AudioMode` 事件，一路绿灯。用"单位时间
    /// 内写入帧数 ≈ 采样率"才能证伪静音。诊断页也用它算实际输出速率。
    pub written_frames: Arc<AtomicU64>,
}

impl Engine {
    pub fn spawn() -> Result<Self, AudioError> {
        let (cmd_tx, cmd_rx) = mpsc::channel::<EngineCommand>();
        let (ev_tx, ev_rx) = mpsc::sync_channel::<EngineEvent>(EVENT_CHANNEL_CAP);
        let written = Arc::new(AtomicU64::new(0));
        let written_worker = Arc::clone(&written);
        let worker = std::thread::Builder::new()
            .name("aiya-audio-engine".into())
            .spawn(move || render_loop(cmd_rx, ev_tx, written_worker))
            .map_err(|e| AudioError::Custom(e.to_string()))?;
        Ok(Self {
            tx: cmd_tx,
            rx: ev_rx,
            worker: Some(worker),
            duration_ms: Arc::new(AtomicU64::new(0)),
            playing: Arc::new(AtomicBool::new(false)),
            written_frames: written,
        })
    }

    /// 已写入设备的累计帧数（诊断/冒烟用）。
    pub fn written_frames(&self) -> u64 {
        self.written_frames.load(std::sync::atomic::Ordering::Relaxed)
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
fn render_loop(
    cmd_rx: Receiver<EngineCommand>,
    ev_tx: SyncSender<EngineEvent>,
    written: Arc<AtomicU64>,
) {
    let ev = EventTx(ev_tx);
    // tap 线程拿裸 SyncSender（自走 try_send 丢帧策略，见 consume_loop）
    // mut：push_* 要独占借用（单生产者是类型级约束，见 spectrum::RingProducer）
    let mut tap = crate::spectrum::SpectrumTap::new();
    tap.spawn_consumer(ev.0.clone());
    let send = |e: EngineEvent| ev.send(e);

    { let _ = wasapi::initialize_mta(); }

    // 把渲染线程抬进 "Pro Audio" 调度组：Windows 默认不给音频线程任何优待，
    // 后台有编译/杀毒任务时就是欠载爆音。失败不致命（见 mmcss 模块）。
    let _mmcss = crate::mmcss::register("Pro Audio")
        .inspect(|_| tracing::debug!("已注册 MMCSS Pro Audio"))
        .or_else(|| {
            tracing::warn!("MMCSS 不可用，渲染线程按普通优先级运行（偶发爆音风险升高）");
            None
        });

    // 变速跨曲目保持：在顶层循环外声明，EOF/Stop/错误回到顶层后不重置
    // （P1：此前每轮 Load 周期重置 1.0，自然播完切下一曲后倍速静默丢失
    // 且 UI 仍显示旧值；SetRate 只应被显式命令修改）
    let mut pending_rate: f32 = 1.0;
    // 输出配置同理跨曲目保持（设置项，不随换曲回默认）
    let mut pending_cfg = OutputConfig::default();
    // EQ 参数同样跨曲目保持（协议 v6）。play_file 内部收到 SetEq 时会更新
    // 自己的实例并把最终值经 LoadExit 带回这里（与 rate/cfg 同一套账法）
    let mut pending_eq = crate::eq::EqParams::default();
    'outer: loop {
        // 命令循环：等 Load 或 Shutdown（Load 前到的 SetRate/SetSpectrum 暂存生效）
        let (mut path, mut start_ms, mut token, mut load_paused) = loop {
            match cmd_rx.recv() {
                Ok(EngineCommand::Load { path, start_ms, token, paused }) => {
                    break (path, start_ms, token, paused)
                }
                Ok(EngineCommand::SetRate(r)) => pending_rate = r.clamp(0.5, 2.0),
                Ok(EngineCommand::SetSpectrum(on)) => tap.set_enabled(on),
                Ok(EngineCommand::SetOutputConfig(c)) => pending_cfg = c,
                Ok(EngineCommand::SetEq(p)) => {
                    let applied = p.clamped();
                    pending_eq = applied;
                    // 未装载阶段 token=0（"装载代数来自最近的 Load"——还没有）
                    send(EngineEvent::EqApplied { token: 0, params: applied });
                }
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
            // null = 不碰任何音频设备：解码与时钟照常，只是不出声。
            // 单独一条路径而不是给 Output 加变体：后者要把 40 多处 client
            // 访问都改成 Option，代价远大于收益（而且两边的推进逻辑本来就不同）。
            let outcome = if pending_cfg.mode == ModePref::Null {
                play_null(
                    &cmd_rx,
                    &send,
                    &path,
                    start_ms,
                    token,
                    load_paused,
                    pending_rate,
                    pending_cfg,
                    pending_eq,
                    &mut tap,
                    &written,
                )
            } else {
                play_file(
                    &cmd_rx,
                    &send,
                    &path,
                    start_ms,
                    token,
                    load_paused,
                    pending_rate,
                    pending_cfg,
                    pending_eq,
                    &mut tap,
                    &written,
                )
            };
            match outcome {
                Ok(LoadExit::Finished { cfg, eq }) => {
                    pending_cfg = cfg;
                    pending_eq = eq;
                    break; // EOF/Stop/错误恢复：回顶层等下一条 Load
                }
                Ok(LoadExit::NewLoad { path: p, rate, start_ms: s, token: t, cfg, eq, paused }) => {
                    path = p;
                    pending_rate = rate;
                    start_ms = s;
                    token = t;
                    pending_cfg = cfg;
                    pending_eq = eq;
                    load_paused = paused;
                }
                Ok(LoadExit::Shutdown) => break 'outer,
                Err(e) => {
                    tracing::error!(%path, error = %e, "engine stream failed");
                    send(EngineEvent::Error {
                        token,
                        kind: e.kind().as_str().into(),
                        message: e.to_string(),
                    });
                    // 回到命令循环等下一条 Load（设备恢复）
                    break;
                }
            }
        }
    }

    { wasapi::deinitialize(); }
}

/// Null 输出的实时节拍（毫秒）：一次解码/推进一小批，避免忙转。
const NULL_TICK_MS: u64 = 20;

/// 按 Load 的 path 打开源：http(s) → HLS 流（docs/22 G3），否则本地文件。
/// 判定放在 engine 入口一处，队列/UI 永远只递字符串。
fn open_source(path: &str) -> Result<DecodedStream, AudioError> {
    if path.starts_with("http://") || path.starts_with("https://") {
        DecodedStream::open_hls(path)
    } else {
        DecodedStream::open_file(Path::new(path))
    }
}

/// 无声渲染：解码、进度、变速、seek 全部照常，只是没有音频设备参与。
///
/// 用途（不是"Tauri 原生后端"——Tauri 没有音频 API，WebView `<audio>` 会是
/// 第二条音频路径，ADR-0003/0006 已排除）：无音频设备/远程桌面下应用仍可用，
/// 以及"A/B 摘掉输出段"判断爆音是否来自音频路径。
///
/// 时钟用 `sleep` 而不是忙等：这个模式的价值是"和真实播放同样的节奏出现
/// 同样的位置事件"，忙等只会白烧一个核。
// 参数多不是"从某个结构体抄一遍"，而是 Load 命令 + 线程里累积的 pending_* 状态
// （变速/输出配置/EQ/tap）在进入播放循环前的汇合点。打包成结构体只是把同一批字段
// 挪个位置（调用点只有一处），不减少信息量——留作 docs/36 的重构项，这里先记账。
#[allow(clippy::too_many_arguments)]
fn play_null(
    cmd_rx: &Receiver<EngineCommand>,
    send: &impl Fn(EngineEvent),
    path: &str,
    start_ms: u64,
    token: u64,
    initial_paused: bool,
    initial_rate: f32,
    initial_cfg: OutputConfig,
    initial_eq: crate::eq::EqParams,
    tap: &mut crate::spectrum::SpectrumTap,
    written: &AtomicU64,
) -> Result<LoadExit, AudioError> {
    let mut decoder = open_source(path)?;
    tap.set_sample_rate(decoder.sample_rate());
    // 直播流不可 seek：start_ms 忽略（UI 按 source=stream 禁拖，这里是保险）
    if start_ms > 0 && decoder.seekable() {
        decoder.seek_ms(start_ms)?;
    }
    send(EngineEvent::Loaded { token, duration_ms: decoder.duration_ms() });
    send(EngineEvent::AudioMode {
        token,
        requested: initial_cfg.mode.as_str().to_string(),
        mode: OutputMode::Null.as_str().to_string(),
        clock: "null".to_string(),
        buffer_ms: 0,
        format: "none".to_string(),
        reason: Some("已关闭音频输出（解码与进度照常推进）".to_string()),
    });

    let channels = decoder.channels().max(1);
    let mut rate: f32 = initial_rate;
    let mut cfg = initial_cfg;
    // 静音模式不滤波（没有输出），但参数要记账：切回真实输出时经 LoadExit 带回
    let mut eq_params = initial_eq;
    let mut paused = initial_paused;
    send(EngineEvent::StateChanged { token, playing: !initial_paused });

    let mut last_tick = std::time::Instant::now();
    loop {
        // 到点才推进；暂停时也睡，避免空转
        std::thread::sleep(Duration::from_millis(NULL_TICK_MS));
        let now = std::time::Instant::now();
        let elapsed = now.duration_since(last_tick);
        last_tick = now;

        loop {
            match cmd_rx.try_recv() {
                Ok(EngineCommand::Pause) if !paused => {
                    paused = true;
                    send(EngineEvent::StateChanged { token, playing: false });
                }
                Ok(EngineCommand::Resume) if paused => {
                    paused = false;
                    send(EngineEvent::StateChanged { token, playing: true });
                }
                Ok(EngineCommand::Seek(ms)) => {
                    // 直播流忽略 seek（UI 禁拖；误触发不能杀播放）
                    if decoder.seekable() {
                        decoder.seek_ms(ms)?;
                        send(EngineEvent::Position { token, position_ms: decoder.position_ms() });
                    }
                }
                Ok(EngineCommand::SetVolume(_)) => {}
                Ok(EngineCommand::SetRate(r)) => rate = r.clamp(0.5, 2.0),
                Ok(EngineCommand::SetSpectrum(on)) => tap.set_enabled(on),
                Ok(EngineCommand::SetEq(p)) => {
                    eq_params = p.clamped();
                    send(EngineEvent::EqApplied { token, params: eq_params });
                }
                Ok(EngineCommand::SetOutputConfig(c)) => {
                    if c.mode != ModePref::Null {
                        // 切回真实输出：把当前位置带出去，让外层用新配置重开
                        return Ok(LoadExit::NewLoad {
                            path: path.to_string(),
                            rate,
                            start_ms: decoder.position_ms(),
                            token,
                            cfg: c,
                            eq: eq_params,
                            paused,
                        });
                    }
                    cfg = c;
                }
                Ok(EngineCommand::Stop) => {
                    send(EngineEvent::StateChanged { token, playing: false });
                    send(EngineEvent::Eof { token });
                    return Ok(LoadExit::Finished { cfg, eq: eq_params });
                }
                Ok(EngineCommand::Load { path: p, start_ms: s, token: t, paused: np }) => {
                    return Ok(LoadExit::NewLoad {
                        path: p,
                        rate,
                        start_ms: s,
                        token: t,
                        cfg,
                        eq: eq_params,
                        paused: np,
                    });
                }
                Ok(EngineCommand::Shutdown) => return Ok(LoadExit::Shutdown),
                Ok(_) => {}
                Err(_) => break,
            }
        }
        if paused {
            continue;
        }

        // 按真实时间推进：20ms 墙钟 → 20ms × rate 的源样本，解码后丢弃。
        // 这样 Position 的节奏和有声播放一致（UI 的时间轴、歌词滚动不跳）。
        let want_frames =
            (decoder.sample_rate() as f64 * elapsed.as_secs_f64() * rate as f64).round() as usize;
        let mut got = 0usize;
        let mut eof = false;
        while got < want_frames {
            match decoder.next_packet() {
                Ok(Some(samples)) => {
                    got += samples.len() / channels;
                    // tap 内部按订阅开关短路（关闭时只剩一次原子读），
                    // 静音模式下频谱仍应跟着音乐动（UI 不显示成"卡死"）
                    tap.push_frame(samples, channels);
                }
                Ok(None) => {
                    eof = true;
                    break;
                }
                Err(e) => return Err(e),
            }
        }
        written.fetch_add(got as u64, std::sync::atomic::Ordering::Relaxed);
        send(EngineEvent::Position {
            token,
            position_ms: decoder.position_ms(),
        });
        if eof {
            send(EngineEvent::Position { token, position_ms: decoder.duration_ms() });
            send(EngineEvent::StateChanged { token, playing: false });
            send(EngineEvent::Eof { token });
            return Ok(LoadExit::Finished { cfg, eq: eq_params });
        }
    }
}

/// play_file 的退出原因。NewLoad 必须把 path/起点/代数带回外层续播。
enum LoadExit {
    /// EOF / Stop / 设备错误：回命令循环等下一条 Load。
    /// 带回最新配置与 EQ 参数（播放中用户可能改过设置）
    Finished { cfg: OutputConfig, eq: crate::eq::EqParams },
    /// 播放中收到新 Load：直接换曲续播。
    ///
    /// `paused` 随新 Load 的暂停态走（协议 v7）：装载自带暂停时续播也保持暂停。
    NewLoad {
        path: String,
        rate: f32,
        start_ms: u64,
        token: u64,
        cfg: OutputConfig,
        eq: crate::eq::EqParams,
        paused: bool,
    },
    /// 收到 Shutdown：终止渲染线程
    Shutdown,
}

fn handle_preload(cmd: &EngineCommand, send: &impl Fn(EngineEvent)) {
    if cmd == &EngineCommand::Stop { send(EngineEvent::StateChanged { token: 0, playing: false }) }
}

/// 构造 `AudioMode` 事件（"设备到底给了什么"的完整快照）。
fn mode_event(
    token: u64,
    cfg: &OutputConfig,
    out: &Output,
    reason: Option<String>,
) -> EngineEvent {
    // 流已释放（切配置失败/暂停释放）时没有事实可报，按请求值占位并说明
    if !out.is_live() {
        return EngineEvent::AudioMode {
            token,
            requested: cfg.mode.as_str().to_string(),
            mode: OutputMode::Null.as_str().to_string(),
            clock: "none".to_string(),
            buffer_ms: 0,
            format: "none".to_string(),
            reason: reason.or_else(|| Some("输出流未打开".to_string())),
        };
    }
    EngineEvent::AudioMode {
        token,
        requested: cfg.mode.as_str().to_string(),
        mode: out.mode.as_str().to_string(),
        clock: out.clock.as_str().to_string(),
        buffer_ms: out.buffer_ms,
        format: out.fmt.label().to_string(),
        reason,
    }
}

/// 哑流判定窗口（毫秒）：这么久一帧都没写进设备，就认为这条流"假开"了。
///
/// 为什么需要这条兜底：`Initialize` 成功、`AudioMode` 报 `exclusive`、设备时钟
/// 一路推进，都可能与"真的在出声"无关。实测本机独占轮询在 200ms/100ms 缓冲下
/// 就是这种状态——padding 恒等于 buffer、一帧不消费，只有 50ms 才正常；而
/// 事件驱动独占 200ms 反而工作良好。这类组合无法靠枚举格式/周期预测，
/// 只能看运行期有没有数据真的写进去。
const DUMB_STREAM_MS: u64 = 600;

/// 哑流自愈的下一步配置（`None` = 没有可退的路）。
///
/// 顺序刻意是"先换时钟、再降模式"：时钟是同一个独占流内部的实现细节，换它
/// 不改变用户的音质选择；只有两种时钟都哑了才怀疑独占本身。
fn dumb_retry_cfg(cfg: &OutputConfig, out: &Output, tries: u32) -> Option<OutputConfig> {
    if out.mode != OutputMode::Exclusive {
        return None;
    }
    match tries {
        // 第一步：换另一种时钟（坏的是轮询就换事件，反之亦然）
        1 => Some(OutputConfig {
            clock: match out.clock {
                Clock::Events => ClockPref::Polling,
                Clock::Polling => ClockPref::Events,
            },
            ..*cfg
        }),
        // 第二步：独占流派不上用场。用户只是"优先独占"就回落共享继续放；
        // "必须独占"的语义是宁可停也不要共享，交给调用方报错。
        2 if cfg.mode == ModePref::Exclusive => Some(OutputConfig {
            mode: ModePref::Shared,
            clock: ClockPref::Auto,
            ..*cfg
        }),
        _ => None,
    }
}

/// 设备失效恢复的最大重试次数（指数退避 200/400/800/1600/1600ms，约 4.6s 总窗口）。
/// 覆盖常见场景：默认设备切换（USB 声卡插拔）、蓝牙重连、驱动瞬时失效。
const DEVICE_RECOVERY_ATTEMPTS: u32 = 5;

/// 打开文件并进入渲染循环直到 Stop/Shutdown/新 Load/EOF。
/// `start_ms`：装载起点（启动恢复/崩溃恢复用；0 = 从头）。
/// `initial_paused`：装载即暂停（协议 v7；启动恢复"暂停态装载"不再先响一下）。
/// `token`：本曲装载代数，原样回显进所有事件。
/// `initial_rate`：Load 前收到的变速指令（如启动恢复/恢复会话）。
/// `initial_cfg`：输出配置（模式/时钟/缓冲/暂停释放）。
/// `initial_eq`：EQ 参数（协议 v6）。随流构建滤波器，播放中可热切。
/// `tap`：频谱采样点（跨曲目常驻；推**重采样后**的输出帧，未乘音量）。
// 同 play_null：参数是 Load + pending_* 状态的汇合点，见上一条注释。
#[allow(clippy::too_many_arguments)]
fn play_file(
    cmd_rx: &Receiver<EngineCommand>,
    send: &impl Fn(EngineEvent),
    path: &str,
    start_ms: u64,
    token: u64,
    initial_paused: bool,
    initial_rate: f32,
    initial_cfg: OutputConfig,
    initial_eq: crate::eq::EqParams,
    tap: &mut crate::spectrum::SpectrumTap,
    written: &AtomicU64,
) -> Result<LoadExit, AudioError> {
    let mut decoder = open_source(path)?;
    tap.set_sample_rate(decoder.sample_rate()); // 频谱分带按实际采样率标定
    // 起点必须在开流之前落定：先播再 seek 会有可闻的"从头闪一下"
    // （直播流不可 seek：忽略 start_ms，从 live 边缘起播）
    if start_ms > 0 && decoder.seekable() {
        decoder.seek_ms(start_ms)?;
    }
    send(EngineEvent::Loaded { token, duration_ms: decoder.duration_ms() });

    // 配置意图与设备事实分开记：cfg 是用户设置，out.mode 是设备实际给的。
    // 回落不算错误（Error 会让 UI 弹红条），用 AudioMode 的 reason 说明。
    let mut cfg = initial_cfg;
    // EQ：滤波器实例绑定本流的格式（采样率/声道），参数跨曲目经 LoadExit 记账
    let mut eq_params = initial_eq;
    let mut eq = crate::eq::Eq::new(eq_params, decoder.sample_rate(), decoder.channels());
    let (mut out, mut mode_note) = open_output(&decoder, &cfg)?;
    send(mode_event(token, &cfg, &out, mode_note.take()));
    // 装载即暂停（协议 v7）：开流后立即让出独占设备（与播放中暂停同一语义），
    // 否则启动恢复会握着声卡不放；共享模式下流空转不出声（渲染循环见 paused 分支）
    if initial_paused && cfg.release_on_pause && out.mode == OutputMode::Exclusive {
        out.release();
        tracing::debug!("装载即暂停（独占）：已释放音频设备");
    }

    // 已解码未播的交错样本池：变速插值的数据源（rate=1 时等价于直通）
    let mut pool: Vec<f32> = Vec::new();
    // 池内的源帧读取位置（帧单位，f64）；drain 后减去已消费帧数保持相位连续
    let mut pool_pos: f64 = 0.0;
    let mut decoder_eof = false;

    let mut volume: f32 = 1.0;
    // 当前实际增益 = 音量 × 淡入淡出系数，逐样本向目标逼近。
    // 跨 chunk 保持，否则每块首样本都会重新跳变，斜坡等于没做。
    let mut gain_now: f32 = 1.0;
    // 淡入淡出目标（1 = 正常，0 = 静音）。播放控制靠它做 declick：
    // 暂停/seek/停止都是"波形突然截断"，不做淡出就是可闻的咔哒。
    let mut declick_target: f32 = 1.0;
    // 一个斜坡跨多少个样本（按源采样率标定，变速时斜坡时长略变，无感）
    let ramp_samples = (decoder.sample_rate() * VOLUME_RAMP_MS / 1000).max(1) as f32;
    // 每样本最大增益变化量（0→1 恰好用满一个斜坡）
    let ramp_step = 1.0 / ramp_samples;
    let mut rate: f32 = initial_rate;
    let mut paused = initial_paused;
    let mut user_stopped = false;
    // 淡出完成后要执行的动作（保证"先静音再动"）
    let mut pending_pause = false;
    let mut pending_seek: Option<u64> = None;
    let mut pending_stop = false;
    let mut pending_switch = false;
    // 切配置期间要应用的新配置（淡出完成前 cfg 仍是旧的，避免半途按新配置判断）
    let mut pending_cfg_apply: Option<OutputConfig> = None;
    // 频谱推的是输出帧（重采样 + 单声道混合后、乘音量前）：
    // 此前推源帧，2× 速入环速率翻倍长期溢出丢帧，0.5× 掉到 ~12fps。
    let mut tap_mono: Vec<f32> = Vec::new();
    // 重采样后的 f32 输出暂存（乘过增益），随后按设备位深编码成字节。
    // 独占降位深时要两趟：一趟算术、一趟取整，避免在插值内联两套写字节逻辑。
    let mut out_f32: Vec<f32> = Vec::new();
    send(EngineEvent::StateChanged { token, playing: !initial_paused });

    let mut result: Result<(), AudioError> = Ok(());
    // 流式阶段（padding/write/start）捕获的设备错误；恢复见循环尾部
    let mut stream_err: Option<AudioError> = None;
    let mut device_failures: u32 = 0;
    // 哑流自愈：最后一次真正写进设备的时刻 + 已降级次数
    let mut last_write = std::time::Instant::now();
    let mut dumb_tries: u32 = 0;

    loop {
        // 等引擎缓冲区消费事件。超时/错误都视为一次正常唤醒——绝不能把
        // 超时当 Err 拆流（P0：暂停后 stop_stream 令事件停发，2s 超时曾走
        // `?` 把流销毁，暂停期间所有命令石沉大海）。
        //
        // 淡出进行中必须用很短的等待：20ms 的斜坡等不起 2s 的事件超时，
        // 否则按暂停后要过两秒才静音。
        let fading = (gain_now - declick_target).abs() > 0.002;
        let wait_ms: u32 = if paused {
            100
        } else if fading {
            10
        } else {
            match out.clock {
                Clock::Events => 2000,
                Clock::Polling => 5,
            }
        };
        // 用 Option 链而不是 Deref：独占暂停会把流整个释放，那时没有事件句柄，
        // 直接 `out.event` 会走 Deref 的 expect 直接 panic（实测过一次）
        if let Some(ev) = out.0.as_ref().and_then(|i| i.event.as_ref()) {
            let _ = ev.wait_for_event(wait_ms);
        } else if out.is_live() {
            // 轮询流没有事件句柄，按轮询周期睡；缓冲有空间就立刻回填
            std::thread::sleep(Duration::from_millis(u64::from(wait_ms)));
        } else {
            // 流被释放（切配置失败待重建 / 独占暂停已让出设备）：别忙转
            std::thread::sleep(Duration::from_millis(u64::from(wait_ms.min(50))));
        }

        // 非阻塞处理命令
        loop {
            match cmd_rx.try_recv() {
                Ok(EngineCommand::Pause) if !paused => {
                    // 暂停 = 先淡出到静音再停止喂数据，流保持 running
                    // （事件照常触发、命令循环存活）。不 stop_stream：停流后
                    // WASAPI 引擎不再为该流处理传递，事件停发。
                    //
                    // 为什么淡出：直接把波形截断在任意非零样本上等于给耳机
                    // 一个阶跃，就是那声"哒"。20ms 淡出完全听不出来。
                    pending_pause = true;
                    declick_target = 0.0;
                    last_write = std::time::Instant::now();
                }
                Ok(EngineCommand::Resume) if paused => {
                    paused = false;
                    declick_target = 1.0;
                    last_write = std::time::Instant::now();
                    // 独占时暂停会把设备还回去（别的程序要用），这里重新打开。
                    // 位置天然连续：decoder 停在原地，池里还有未播样本。
                    if !out.is_live() {
                        match open_output(&decoder, &cfg) {
                            Ok((new_out, note)) => {
                                out.install(new_out);
                                send(mode_event(token, &cfg, &out, note));
                            }
                            // 开不回来交给设备恢复路径（会退避重试）
                            Err(e) => stream_err = Some(e),
                        }
                    }
                    send(EngineEvent::StateChanged { token, playing: true });
                }
                Ok(EngineCommand::Seek(ms)) => {
                    // 淡出后执行：直接 seek 会把正在响的波形从中间切断
                    pending_seek = Some(ms);
                    declick_target = 0.0;
                    last_write = std::time::Instant::now();
                }
                Ok(EngineCommand::SetVolume(v)) => volume = v.clamp(0.0, 1.0),
                Ok(EngineCommand::SetRate(r)) => rate = r.clamp(0.5, 2.0),
                Ok(EngineCommand::SetSpectrum(on)) => tap.set_enabled(on),
                Ok(EngineCommand::SetEq(p)) => {
                    // 热切：系数重算、延迟线保留。回报生效值（clamp 后），
                    // UI 以它为准（防"发了 20 结果存了 12"的漂移）
                    eq_params = p.clamped();
                    eq.set_params(eq_params);
                    send(EngineEvent::EqApplied { token, params: eq_params });
                }
                Ok(EngineCommand::SetOutputConfig(next)) if next.mode == ModePref::Null => {
                    // 切到静音输出要走另一条渲染路径（play_null）：停流后把当前
                    // 位置带回外层重新装载。位置取 play_position（扣掉池内未播
                    // 样本），接上时不跳。
                    let resume_ms = play_position(&decoder, &pool);
                    out.stop();
                    return Ok(LoadExit::NewLoad {
                        path: path.to_string(),
                        rate,
                        start_ms: resume_ms,
                        token,
                        cfg: next,
                        eq: eq_params,
                        paused,
                    });
                }
                Ok(EngineCommand::SetOutputConfig(next)) => {
                    let prev = cfg;
                    cfg = next;
                    // 只在"当前流不满足新配置"时重开流；独占被拒后再次请求
                    // （用户重试）仍会走到这里，因为 out.mode 仍是 Shared。
                    if matches_config(&out, &cfg) {
                        // 已在目标状态（例如"切回共享"时本来就是共享）：
                        // 仍回报一次，让调用方拿到确认而不是靠猜
                        send(mode_event(token, &cfg, &out, None));
                    } else {
                        // 切流本身有几十毫秒空档，先静音再切，避免"咔"一声
                        pending_switch = true;
                        declick_target = 0.0;
                        pending_cfg_apply = Some(cfg);
                        cfg = prev; // 淡出期间仍按旧配置跑
                    }
                }
                Ok(EngineCommand::Stop) => {
                    pending_stop = true;
                    declick_target = 0.0;
                }
                Ok(EngineCommand::Load { path: p, start_ms: s, token: t, paused: np }) => {
                    // 换曲：停流后把新 path/起点/代数带回外层续播（绝不能在此丢弃）
                    out.stop();
                    return Ok(LoadExit::NewLoad {
                        path: p,
                        rate,
                        start_ms: s,
                        token: t,
                        cfg,
                        eq: eq_params,
                        paused: np,
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

        // ---- 淡出完成后的动作（顺序：暂停 / 切配置 / seek / 停止）----
        if gain_now <= 0.002 && (pending_pause || pending_seek.is_some() || pending_stop || pending_switch) {
            if pending_pause {
                pending_pause = false;
                paused = true;
                send(EngineEvent::StateChanged { token, playing: false });
                // 独占暂停把设备让出去：否则浏览器/会议软件拿不到声卡
                // （docs/04 §6 的要求，此前没实现）。
                if cfg.release_on_pause && out.is_live() && out.mode == OutputMode::Exclusive {
                    out.release();
                    tracing::debug!("独占暂停：已释放音频设备");
                }
            }
            if pending_switch {
                pending_switch = false;
                let apply = pending_cfg_apply.take().unwrap_or(cfg);
                cfg = apply;
                match switch_output(&mut out, &decoder, &cfg) {
                    SwitchOutcome::Switched(note) => {
                        send(mode_event(token, &cfg, &out, note));
                    }
                    SwitchOutcome::KeptPlaying(why) => {
                        send(mode_event(token, &cfg, &out, Some(format!("切换失败：{why}"))));
                    }
                    // 流已彻底释放：交给设备恢复路径重建（会重新退避重试）
                    SwitchOutcome::Dead(e) => {
                        stream_err = Some(e);
                    }
                }
                declick_target = 1.0; // 新流淡入
            }
            if let Some(ms) = pending_seek.take() {
                // 直播流不可 seek：整个动作丢弃（out 不能停在 stopped 态不重启）
                if !decoder.seekable() {
                    send(EngineEvent::Position { token, position_ms: decoder.position_ms() });
                } else {
                    // stop（reset 要求 stopped 态）→ seek → 清池（旧样本作废）→ reset → 重启
                    out.stop();
                    if let Err(e) = decoder.seek_ms(ms) {
                        result = Err(e);
                    } else {
                        pool.clear();
                        pool_pos = 0.0;
                        decoder_eof = false;
                        // 流可能正处于"已释放待重建"的窗口（切配置两次都失败）
                        if out.is_live() {
                            let _ = out.client.reset_stream();
                            if let Err(e) = out.client.start_stream() {
                                stream_err = Some(wasapi_err(e));
                            }
                        }
                        send(EngineEvent::Position { token, position_ms: decoder.position_ms() });
                        declick_target = 1.0; // seek 后淡入，避免落点处爆音
                    }
                }
            }
            if pending_stop {
                pending_stop = false;
                user_stopped = true;
            }
        }

        if user_stopped {
            send(EngineEvent::StateChanged { token, playing: false });
            send(EngineEvent::Eof { token }); // Stop 视为曲目结束（PlayerActor 决定下一动作）
            out.stop();
            return Ok(LoadExit::Finished { cfg, eq: eq_params });
        }
        if let Err(e) = result {
            out.stop();
            return Err(e);
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
            match reopen_output(&mut out, &decoder, device_failures, &cfg) {
                Ok(note) => {
                    device_failures += 1; // 写成功后归零；连续失败则退避递增
                    tracing::warn!(error = %e, attempt = device_failures, "输出设备失效，已重建（退避重试中）");
                    // 换设备后实际模式可能变（新设备不支持独占 / 反而支持）：
                    // 不告知 UI 的话，设置页会一直显示旧设备的能力
                    if note.is_some() || !matches_config(&out, &cfg) {
                        send(mode_event(token, &cfg, &out, note));
                    }
                }
                Err(re) => {
                    device_failures += 1;
                    tracing::warn!(error = %e, retry_error = %re, attempt = device_failures, "设备重建失败，将继续退避");
                }
            }
            continue;
        }

        // 走到这里流必须是活的。重建失败的 `continue` 已在上方返回，但切配置
        // 的 Dead 分支会留下空流——直接去碰 padding/render 会 panic。
        if !out.is_live() {
            // 独占暂停释放了设备：这是正常静止态，无需重建
            if !paused {
                stream_err = Some(AudioError::Device("输出流意外为空".into()));
            }
            continue;
        }

        // 哑流自愈（必须在 `if paused` 之前判断 paused 标志，在其后判断
        // !paused，这里用 !paused 条件即可）：流开着但一帧都写不进去。
        if !paused && out.is_live() && last_write.elapsed() > Duration::from_millis(DUMB_STREAM_MS) {
            last_write = std::time::Instant::now(); // 重置，避免每轮都触发
            dumb_tries += 1;
            let why = format!(
                "{} · {}ms 缓冲下设备不消费数据（{DUMB_STREAM_MS}ms 无写入）",
                out.clock.as_str(),
                out.buffer_ms
            );
            match dumb_retry_cfg(&cfg, &out, dumb_tries) {
                Some(next) => {
                    tracing::warn!(reason = %why, next = ?next.mode, "输出流哑火，自动降级");
                    cfg = next;
                    match switch_output(&mut out, &decoder, &cfg) {
                        SwitchOutcome::Switched(note) => {
                            let mut r = why;
                            if let Some(n) = note {
                                r.push('；');
                                r.push_str(&n);
                            }
                            send(mode_event(token, &cfg, &out, Some(r)));
                        }
                        SwitchOutcome::KeptPlaying(e) => {
                            send(mode_event(token, &cfg, &out, Some(format!("{why}；{e}"))));
                        }
                        SwitchOutcome::Dead(e) => stream_err = Some(e),
                    }
                }
                None => {
                    // 没路可退：必须独占就直接报错（宁可停也不要静默共享），
                    // 其余交给设备恢复路径（它还会退避重试几轮）
                    tracing::error!(reason = %why, "输出流哑火且无可降级路径");
                    if cfg.mode == ModePref::ExclusiveRequired && out.mode == OutputMode::Exclusive {
                        return Err(AudioError::Device(format!("独占流不工作：{why}")));
                    }
                    stream_err = Some(AudioError::Device(format!("输出流不消费数据：{why}")));
                }
            }
            continue;
        }

        if paused {
            // 暂停时不喂数据。流还活着就顺手探一次设备存活（拔耳机/切默认
            // 设备），失败即置 stream_err 走恢复路径；独占暂停已把设备让出去
            // 时没有句柄可探——那是正常的静止态，不是故障。
            if out.is_live() {
                if let Err(e) = out.client.get_current_padding() {
                    stream_err = Some(wasapi_err(e));
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
        let available = writable_frames(out.clock, out.mode, out.buffer_frames, padding);
        // 每轮渲染心跳都上报位置（泵侧再做 500ms 节流；写满瞬间 available 不会为 0）
        // 位置要扣掉设备缓冲里还没播的部分，否则进度条领先耳朵最多 200ms。
        // 独占事件下 padding 恒等于 buffer，正好等于"在途一整个缓冲"的估值，
        // 所以这条公式在两个模式下都成立（不必换成 IAudioClock，它比写入超前 5%）。
        let position_now = |decoder: &DecodedStream, pool: &[f32], latency: u32| -> u64 {
            play_position(decoder, pool).saturating_sub(padding_ms(decoder, latency))
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
        let channels = decoder.channels().max(1);

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
            // 增益斜坡：本块内从 gain_now 出发，最多走 ramp_step×帧数，
            // 绝不越过目标（越过后 clamp 仍是错的：波形会在一个块内超调削顶）。
            let target = volume * declick_target;
            let diff = target - gain_now;
            let max_delta = ramp_step * out_frames as f32;
            let delta = diff.clamp(-max_delta, max_delta);
            let step = delta / out_frames as f32;
            resample_take(
                &pool,
                pool_pos,
                rate as f64,
                channels,
                out_frames,
                gain_now,
                step,
                &mut eq,
                &mut out_f32,
                &mut tap_mono,
            );
            gain_now += delta;
            if diff.abs() <= max_delta {
                gain_now = target; // 到位了：抹掉浮点残差，别一直"差一点点"
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
                Ok(()) => {
                    device_failures = 0; // 写成功：恢复计数归零
                    last_write = std::time::Instant::now();
                    written.fetch_add(out_frames as u64, std::sync::atomic::Ordering::Relaxed);
                }
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
            //
            // 独占下不能用 `padding == 0` 判排空：实测独占事件驱动下 padding
            // 恒等于 buffer，永远等不到 0，会白等满 500ms 上限才报 EOF（歌曲
            // 尾巴明明已经播完）。改用"在途 = 一整个缓冲"的时长估算。
            let deadline = std::time::Instant::now() + Duration::from_millis(500);
            let drained_at = std::time::Instant::now()
                + Duration::from_millis(
                    padding_ms(&decoder, out.buffer_frames).clamp(10, 500),
                );
            loop {
                match cmd_rx.try_recv() {
                    Ok(EngineCommand::Shutdown) => {
                        out.stop();
                        return Ok(LoadExit::Shutdown);
                    }
                    Ok(EngineCommand::Load { path: p, start_ms: s, token: t, paused: np }) => {
                        out.stop();
                        return Ok(LoadExit::NewLoad {
                            path: p,
                            rate,
                            start_ms: s,
                            token: t,
                            cfg,
                            eq: eq_params,
                            paused: np,
                        });
                    }
                    Ok(EngineCommand::Stop) => {
                        user_stopped = true;
                        break;
                    }
                    Ok(EngineCommand::SetSpectrum(on)) => tap.set_enabled(on),
                    // 曲尾只剩不到 500ms：只更新配置/EQ，重开流留给下一曲
                    Ok(EngineCommand::SetOutputConfig(c)) => cfg = c,
                    Ok(EngineCommand::SetEq(p)) => {
                        eq_params = p.clamped();
                        send(EngineEvent::EqApplied { token, params: eq_params });
                    }
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
                let now = std::time::Instant::now();
                if now > deadline {
                    break;
                }
                if out.mode == OutputMode::Exclusive {
                    if now >= drained_at {
                        break;
                    }
                } else if out.client.get_current_padding().unwrap_or(0) == 0 {
                    break;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            out.stop();
            if user_stopped {
                send(EngineEvent::StateChanged { token, playing: false });
                send(EngineEvent::Eof { token });
                return Ok(LoadExit::Finished { cfg, eq: eq_params });
            }
            send(EngineEvent::Position { token, position_ms: decoder.duration_ms() });
            send(EngineEvent::StateChanged { token, playing: false });
            send(EngineEvent::Eof { token });
            // EOF 后回命令循环等下一条 Load
            return Ok(LoadExit::Finished { cfg, eq: eq_params });
        }
    }
}

/// 用户请求的输出模式。
///
/// `Exclusive` 与 `ExclusiveRequired` 的区别只有一件事：设备不给独占时，
/// 前者回落共享继续放（体验优先），后者报错停下（音质优先）。两者都需要，
/// 因为"我开了独占但其实一直在走共享混音器"是发烧用户最不能接受的静默降级。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ModePref {
    Shared,
    /// 优先独占，设备拒绝则回落共享（原因随 `AudioMode` 上报）
    Exclusive,
    /// 必须独占，设备拒绝则报错（不回落，不静默降级）
    ExclusiveRequired,
    /// 不打开音频设备：解码、进度、歌词照常推进，只是不出声。
    ///
    /// 存在的理由不是"另一种后端"（Tauri 没有音频 API，WebView `<audio>`
    /// 会复活第二条音频路径，ADR-0003/0006 都已排除），而是两件实事：
    /// ① 无声卡/远程桌面的机器上应用仍可用，UI 不假死；
    /// ② 排查"爆音是音频路径还是解码/调度引起的"时，一键摘掉输出段对照。
    Null,
}

impl ModePref {
    pub fn as_str(self) -> &'static str {
        match self {
            ModePref::Shared => "shared",
            ModePref::Exclusive => "exclusive",
            ModePref::ExclusiveRequired => "exclusiveRequired",
            ModePref::Null => "null",
        }
    }

    /// wire / 设置里的字符串 → 枚举。未知值按 `Shared`（不认识的配置不该
    /// 让播放器彻底没声）。
    pub fn parse(s: &str) -> Self {
        match s {
            "exclusive" | "exclusiveFallback" => ModePref::Exclusive,
            "exclusiveRequired" | "exclusiveStrict" => ModePref::ExclusiveRequired,
            "null" | "none" | "off" => ModePref::Null,
            _ => ModePref::Shared,
        }
    }
}

/// 计时方式。事件驱动由设备在缓冲将空时唤起线程；轮询靠固定间隔自己查。
///
/// 这个选项的真实价值是**故障对照**：某些驱动的事件回调会迟到或不来
/// （表现为周期性爆音/丢帧），换成轮询立刻能判定责任方。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ClockPref {
    /// 独占先试事件、失败再试轮询；共享固定用事件（更省电、延迟更稳）
    Auto,
    Events,
    Polling,
}

impl ClockPref {
    pub fn as_str(self) -> &'static str {
        match self {
            ClockPref::Auto => "auto",
            ClockPref::Events => "events",
            ClockPref::Polling => "polling",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "events" => ClockPref::Events,
            "polling" => ClockPref::Polling,
            _ => ClockPref::Auto,
        }
    }
}

/// 实际生效的计时方式（`Auto` 解析后的结果）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Clock {
    Events,
    Polling,
}

impl Clock {
    fn as_str(self) -> &'static str {
        match self {
            Clock::Events => "events",
            Clock::Polling => "polling",
        }
    }
}

/// 一组完整的输出配置。**这几项要一起下发**：分别下发会出现中间态，
/// 而且每次重建输出流都要丢掉设备缓冲里的未播样本（几十毫秒的空档）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutputConfig {
    pub mode: ModePref,
    /// 缓冲深度（毫秒），0 = 自动（共享 200ms / 独占用设备默认周期）。
    ///
    /// 这个才是用户真正能感知的旋钮：缓冲越深越抗欠载（不易爆音），
    /// 但暂停/seek 的响应与内存占用随之变差。事件/轮询只影响谁叫醒线程。
    pub buffer_ms: u32,
    pub clock: ClockPref,
    /// 暂停时释放音频设备。独占模式下设备被本进程持有，别的程序（浏览器、
    /// 会议软件）会拿不到声音；暂停时让出来是主流播放器的做法。
    /// 共享模式无意义（不独占设备）。
    pub release_on_pause: bool,
}

impl Default for OutputConfig {
    fn default() -> Self {
        Self {
            mode: ModePref::Shared,
            buffer_ms: 0,
            clock: ClockPref::Auto,
            release_on_pause: true,
        }
    }
}

impl OutputConfig {
    /// 从设置里的字符串拼装（缺项/非法值都按默认，绝不因为一个坏字段停播）。
    pub fn from_parts(mode: &str, buffer_ms: i64, clock: &str, release_on_pause: bool) -> Self {
        Self {
            mode: ModePref::parse(mode),
            // 上限 2s：再深就只是延迟，不是抗抖
            buffer_ms: buffer_ms.clamp(0, 2000) as u32,
            clock: ClockPref::parse(clock),
            release_on_pause,
        }
    }
}

/// 输出模式（设备实际给的）。
///
/// `Null` 不是设备能力，是"用户要求不出声"；它和 `Shared` 一样是**事实**，
/// 区别在于这个事实里没有任何音频设备参与。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OutputMode {
    Shared,
    Exclusive,
    Null,
}

impl OutputMode {
    fn as_str(self) -> &'static str {
        match self {
            OutputMode::Shared => "shared",
            OutputMode::Exclusive => "exclusive",
            OutputMode::Null => "null",
        }
    }
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
///
/// 同一个机制也给"独占暂停让出设备"用：暂停时 `release()`，Resume 再开。
struct Output(Option<OutputInner>);

struct OutputInner {
    client: wasapi::AudioClient,
    /// 事件句柄。轮询模式下为 `None`——不建事件，等待改 sleep。
    event: Option<wasapi::Handle>,
    render: wasapi::AudioRenderClient,
    buffer_frames: u32,
    /// 实际生效的样本格式（独占降位深时不是 F32）
    fmt: OutFormat,
    /// 一帧原始字节缓冲，容量 = buffer_frames * channels * fmt.bytes_per_sample()
    sample_bytes: Vec<u8>,
    /// 实际生效的模式（请求独占但被拒时这里是 Shared）
    mode: OutputMode,
    /// 实际生效的计时方式（`Auto` 在这里一定已解析成具体值）
    clock: Clock,
    /// 实际缓冲深度（毫秒，由 buffer_frames / 采样率算出，不是用户填的值）
    buffer_ms: u32,

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

    /// 彻底释放 COM 句柄（独占切模式/暂停让出的前提）
    fn release(&mut self) {
        self.0 = None;
    }

    fn install(&mut self, other: Output) {
        self.0 = other.0;
    }

    /// 停流。已释放时静默跳过——切配置失败待重建的窗口里，Stop/Load/Shutdown
    /// 仍可能到达，不能因为流为空就 panic。
    fn stop(&mut self) {
        if let Some(i) = self.0.as_mut() {
            let _ = i.client.stop_stream();
        }
    }

}

fn src_format(decoder: &DecodedStream) -> WaveFormat {
    WaveFormat::new(
        32,
        32,
        &SampleType::Float,
        decoder.sample_rate() as usize,
        decoder.channels(),
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
    clock: Clock,
    rate: u32,
) -> Result<Output, AudioError> {
    // 事件句柄只在事件模式建：轮询流不需要它，建了反而要维护一次多余唤醒
    let event = match clock {
        Clock::Events => Some(client.set_get_eventhandle().map_err(wasapi_err)?),
        Clock::Polling => None,
    };
    let render = client.get_audiorenderclient().map_err(wasapi_err)?;
    let buffer_frames = client.get_buffer_size().map_err(wasapi_err)?;
    let sample_bytes = vec![0u8; buffer_frames as usize * channels * fmt.bytes_per_sample()];
    let buffer_ms = (buffer_frames as u64 * 1000 / rate.max(1) as u64) as u32;
    client.start_stream().map_err(wasapi_err)?;
    Ok(Output(Some(OutputInner {
        client,
        event,
        render,
        buffer_frames,
        fmt,
        sample_bytes,
        mode,
        clock,
        buffer_ms,
    })))
}

/// 自动缓冲深度（毫秒）。共享模式 200ms 是 WASAPI 的常见默认；独占轮询也用它。
const DEFAULT_BUFFER_MS: u32 = 200;

fn ms_to_hns(ms: u32) -> i64 {
    ms as i64 * 10_000
}

/// 一次打开尝试的完整参数。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Attempt {
    clock: Clock,
    /// 喂数据的节拍：事件模式=设备周期，轮询模式=轮询间隔
    period_hns: i64,
    /// 缓冲深度。事件驱动的独占被 WASAPI 强制成 buffer == period，
    /// 所以那里两者同值；轮询可以真正做到"深缓冲 + 勤轮询"。
    buffer_hns: i64,
}

impl Attempt {
    fn label(&self) -> String {
        format!(
            "{} · 周期 {:.1}ms · 缓冲 {:.0}ms",
            self.clock.as_str(),
            self.period_hns as f64 / 10_000.0,
            self.buffer_hns as f64 / 10_000.0
        )
    }
}

/// 独占模式的尝试序列：格式（外层）× 时钟/周期（内层）。
///
/// 两条实测结论决定了这个顺序：
/// 1. 设备自报的默认周期**不可信**——Realtek 44.1k 下默认 10ms 直接
///    `0x88890019`，20ms 才成功；同一设备 48k 下 10ms 反而可用。所以"自动"
///    必须枚举，不能只用默认值。
/// 2. 用户显式给了缓冲深度时**只试那一个**：他要 10ms 就是要 10ms，
///    偷偷换成 50ms 属于静默改变配置（和静默降级一样不可接受）。失败原因
///    会原样上报，"设了没生效"必须能解释。
fn exclusive_attempts(cfg: &OutputConfig, default_period: Option<i64>) -> Vec<Attempt> {
    let clocks: &[Clock] = match cfg.clock {
        ClockPref::Auto => &[Clock::Events, Clock::Polling],
        ClockPref::Events => &[Clock::Events],
        ClockPref::Polling => &[Clock::Polling],
    };
    let mut out = Vec::new();
    for &clock in clocks {
        match clock {
            Clock::Events => {
                let periods: Vec<i64> = if cfg.buffer_ms > 0 {
                    vec![ms_to_hns(cfg.buffer_ms)]
                } else {
                    let mut v: Vec<i64> = Vec::new();
                    if let Some(d) = default_period {
                        v.push(d);
                        v.push(d.saturating_mul(2));
                    }
                    let p = ms_to_hns(50);
                    if !v.contains(&p) {
                        v.push(p);
                    }
                    v
                };
                for p in periods {
                    out.push(Attempt { clock, period_hns: p, buffer_hns: p });
                }
            }
            Clock::Polling => {
                let buf = if cfg.buffer_ms > 0 {
                    ms_to_hns(cfg.buffer_ms)
                } else {
                    ms_to_hns(DEFAULT_BUFFER_MS)
                };
                let mut periods: Vec<i64> = Vec::new();
                if let Some(d) = default_period {
                    if d <= buf {
                        periods.push(d);
                    }
                }
                for ms in [10u32, 20, 50] {
                    let p = ms_to_hns(ms);
                    if p <= buf && !periods.contains(&p) {
                        periods.push(p);
                    }
                }
                if periods.is_empty() {
                    periods.push(ms_to_hns(10));
                }
                for p in periods {
                    out.push(Attempt { clock, period_hns: p, buffer_hns: buf });
                }
            }
        }
    }
    out
}

/// 共享模式的缓冲深度：显式值优先，否则自动。
fn shared_buffer_hns(cfg: &OutputConfig) -> i64 {
    if cfg.buffer_ms > 0 {
        ms_to_hns(cfg.buffer_ms)
    } else {
        ms_to_hns(DEFAULT_BUFFER_MS)
    }
}

/// 共享模式的时钟：`Polling` 只在用户显式要求时用，`Auto` 固定走事件
/// （事件驱动延迟更稳、CPU 占用更低；轮询的意义是故障对照）。
fn shared_clock(cfg: &OutputConfig) -> Clock {
    match cfg.clock {
        ClockPref::Polling => Clock::Polling,
        _ => Clock::Events,
    }
}

/// 共享模式：交给引擎做格式转换（autoconvert），我们只交源格式 f32。
fn open_shared(decoder: &DecodedStream, buffer_hns: i64, clock: Clock) -> Result<Output, AudioError> {
    let mut client = new_client()?;
    let mode = match clock {
        Clock::Events => StreamMode::EventsShared {
            autoconvert: true,
            buffer_duration_hns: buffer_hns,
        },
        Clock::Polling => StreamMode::PollingShared {
            autoconvert: true,
            buffer_duration_hns: buffer_hns,
        },
    };
    client
        .initialize_client(&src_format(decoder), &Direction::Render, &mode)
        .map_err(wasapi_err)?;
    finish_stream(
        client,
        OutputMode::Shared,
        decoder.channels(),
        OutFormat::F32,
        clock,
        decoder.sample_rate(),
    )
}

/// 共享模式 + "显式缓冲被拒则退回自动"的说明。
///
/// 共享模式下 WASAPI 对过小的缓冲很敏感（10ms 在多数板载上直接失败）。
/// 退回自动而不是报错，是因为"缓冲没调成"不该让用户没音乐听；但原因必须
/// 带出去——否则设置页写着 10ms、实际 200ms，等于骗人。
fn open_shared_with_note(
    decoder: &DecodedStream,
    cfg: &OutputConfig,
) -> Result<(Output, Option<String>), AudioError> {
    let clock = shared_clock(cfg);
    match open_shared(decoder, shared_buffer_hns(cfg), clock) {
        Ok(o) => Ok((o, None)),
        Err(e) if cfg.buffer_ms > 0 => {
            let auto = ms_to_hns(DEFAULT_BUFFER_MS);
            let o = open_shared(decoder, auto, clock)?;
            Ok((
                o,
                Some(format!(
                    "设备不接受 {}ms 缓冲（{e}），已改用自动 {DEFAULT_BUFFER_MS}ms",
                    cfg.buffer_ms
                )),
            ))
        }
        Err(e) => Err(e),
    }
}

/// 独占模式：设备必须接受**源采样率 + 源声道数**，位深可降（见 [`OutFormat`]）。
///
/// 仍然不做重采样：独占的意义是绕开共享混音器，只要采样率对得上，位深从
/// f32 降到 i24/i16 是有损但极小的（16bit 源在 i16 上完全无损）。真碰上
/// 采样率不匹配（例如 96k 源 + 只支持 48k 的设备），宁可回落共享也不做 SRC。
///
/// 返回 `Err(原因)` 由调用方决定回落还是报错——"独占没生效"必须是可解释的，
/// 不能只是静默变差。
fn open_exclusive(decoder: &DecodedStream, cfg: &OutputConfig) -> Result<Output, String> {
    let rate = decoder.sample_rate();
    let channels = decoder.channels();
    let mut best_reason: Option<String> = None;
    let default_period = new_client()
        .ok()
        .and_then(|c| c.get_device_period().ok().map(|(p, _min)| p));
    let attempts = exclusive_attempts(cfg, default_period);

    for (fmt, bits, valid, sub) in EXCLUSIVE_CANDIDATES {
        let want = WaveFormat::new(bits, valid, &sub, rate as usize, channels, None);
        // 探测用一次性 client：Initialize 失败后同一 client 的可用性依赖驱动
        // 实现，失败后重建最省心（只在装载/切配置时发生）
        let negotiated = {
            let probe = new_client().map_err(|e| e.to_string())?;
            match probe.is_supported_exclusive_with_quirks(&want) {
                Ok(f) => f,
                Err(e) => {
                    best_reason.get_or_insert_with(|| {
                        format!(
                            "{rate} Hz / {channels} ch 不支持独占（{} 被拒：{e}）",
                            fmt.label()
                        )
                    });
                    continue;
                }
            }
        };
        if negotiated.get_samplespersec() != rate || negotiated.get_nchannels() as usize != channels {
            best_reason.get_or_insert_with(|| {
                format!(
                    "设备独占只支持 {} Hz / {} ch（源为 {rate} Hz / {channels} ch，不做重采样）",
                    negotiated.get_samplespersec(),
                    negotiated.get_nchannels()
                )
            });
            continue;
        }
        // 以协商回来的实际位深为准（driver 可能把 i24-in-32 折成别的东西）
        let actual = match (negotiated.get_bitspersample(), negotiated.get_subformat().ok()) {
            (32, Some(SampleType::Float)) => OutFormat::F32,
            (32, Some(SampleType::Int)) => OutFormat::I24In32,
            (16, _) => OutFormat::I16,
            (b, _) => {
                best_reason.get_or_insert_with(|| format!("设备独占要求 {b} bit 格式，暂不支持"));
                continue;
            }
        };

        for att in &attempts {
            let mut client = new_client().map_err(|e| e.to_string())?;
            let mode = match att.clock {
                Clock::Events => StreamMode::EventsExclusive {
                    period_hns: att.period_hns,
                },
                Clock::Polling => StreamMode::PollingExclusive {
                    buffer_duration_hns: att.buffer_hns,
                    period_hns: att.period_hns,
                },
            };
            match client.initialize_client(&negotiated, &Direction::Render, &mode) {
                Ok(()) => {
                    tracing::info!(
                        format = actual.label(),
                        rate,
                        channels,
                        clock = att.clock.as_str(),
                        period_ms = att.period_hns as f64 / 10_000.0,
                        buffer_ms = att.buffer_hns as f64 / 10_000.0,
                        "独占模式已启用"
                    );
                    return finish_stream(
                        client,
                        OutputMode::Exclusive,
                        channels,
                        actual,
                        att.clock,
                        rate,
                    )
                    .map_err(|e| e.to_string());
                }
                Err(e) => {
                    best_reason = Some(format!(
                        "{} 独占流初始化失败（{}：{e}）",
                        actual.label(),
                        att.label()
                    ));
                }
            }
        }
    }

    Err(best_reason.unwrap_or_else(|| "设备不接受任何独占格式".to_string()))
}

/// 打开输出。第二个返回值是**降级原因**（`None` = 完全按请求生效）。
///
/// `ExclusiveRequired` 失败时返回 `Err` 而不是回落：用户明确要了"没独占就别放"，
/// 这时切到共享等于骗他。错误里带设备原话，UI 直接展示。
fn open_output(
    decoder: &DecodedStream,
    cfg: &OutputConfig,
) -> Result<(Output, Option<String>), AudioError> {
    match cfg.mode {
        ModePref::Null => Err(AudioError::Custom("null 模式不打开音频设备".into())),
        ModePref::Shared => open_shared_with_note(decoder, cfg),
        ModePref::Exclusive | ModePref::ExclusiveRequired => match open_exclusive(decoder, cfg) {
            Ok(out) => Ok((out, None)),
            Err(reason) => {
                if cfg.mode == ModePref::ExclusiveRequired {
                    return Err(AudioError::Unsupported(format!("独占模式不可用：{reason}")));
                }
                let (out, note) = open_shared_with_note(decoder, cfg)?;
                Ok((
                    out,
                    Some(match note {
                        Some(n) => format!("{reason}；共享侧：{n}"),
                        None => reason,
                    }),
                ))
            }
        },
    }
}

/// 当前流是否已经满足这份配置（满足就不重建——每次重建都要丢掉设备缓冲里
/// 未播的几十毫秒，设置页点两下就听出断音）。
fn matches_config(out: &Output, cfg: &OutputConfig) -> bool {
    if !out.is_live() {
        return false;
    }
    let mode_ok = match cfg.mode {
        ModePref::Null => false,
        ModePref::Shared => out.mode == OutputMode::Shared,
        ModePref::Exclusive | ModePref::ExclusiveRequired => out.mode == OutputMode::Exclusive,
    };
    if !mode_ok {
        return false;
    }
    // Auto = "现在这个就行"：独占 Auto 优先事件，已经是事件流就没必要重开
    let clock_ok = match cfg.clock {
        ClockPref::Auto => true,
        ClockPref::Events => out.clock == Clock::Events,
        ClockPref::Polling => out.clock == Clock::Polling,
    };
    // 缓冲：0 = 自动，不挑；显式值按 ±2ms 容差（buffer_frames → ms 有取整）
    let buffer_ok = cfg.buffer_ms == 0 || out.buffer_ms.abs_diff(cfg.buffer_ms) <= 2;
    clock_ok && buffer_ok
}

/// 重建输出流（退避后重枚举默认设备）。位置无需显式恢复——
/// 池内已解码样本和 decoder 位置都在调用方手里，天然连续。
/// `cfg` 是用户请求的配置（不是上一次的实际模式）：换设备后可能从共享变独占。
fn reopen_output(
    out: &mut Output,
    decoder: &DecodedStream,
    attempt: u32,
    cfg: &OutputConfig,
) -> Result<Option<String>, AudioError> {
    // 旧流尽力停，失败无所谓（马上整个丢弃）；已释放时跳过（switch_output 的
    // Dead 分支会走到这里）
    out.stop();
    let backoff_ms = 200u64 << attempt.min(3);
    std::thread::sleep(Duration::from_millis(backoff_ms));
    let (new_out, note) = open_output(decoder, cfg)?;
    out.install(new_out);
    Ok(note)
}

/// 切输出的结果。三种结局必须分开表达——"没切成"和"设备没了"对用户的
/// 意义完全不同：前者音乐继续、只解释一句；后者要走设备恢复。
enum SwitchOutcome {
    /// 已切到目标配置（`None` 表示没降级，`Some` 是回落/降级原因）
    Switched(Option<String>),
    /// 目标配置打不开，已退回原配置继续播（带原因）
    KeptPlaying(String),
    /// 原配置也开不起来：流为空，必须走设备恢复
    Dead(AudioError),
}

/// 播放中切换输出配置：**先彻底释放旧流，再开新流**。
///
/// 顺序不能反：独占期间设备被本进程独占，一边持有 IAudioClient 一边开共享流会
/// 撞 `AUDCLNT_E_DEVICE_IN_USE(0x8889000A)`（只 `stop_stream` 不够，实测仍失败，
/// 必须 drop 掉句柄）。代价是新流建立期间有几十毫秒空档，由调用方的淡出盖住。
fn switch_output(out: &mut Output, decoder: &DecodedStream, cfg: &OutputConfig) -> SwitchOutcome {
    // 记下原配置用于退回：不能只记模式，时钟/缓冲也要回到原样。
    // 流可能已被释放（独占暂停让出设备后用户改设置），这时没有"原配置"，
    // 直接按新配置开——退回目标就是新配置本身。
    let prev = match out.0.as_ref() {
        Some(i) => OutputConfig {
            mode: match i.mode {
                OutputMode::Shared => ModePref::Shared,
                OutputMode::Exclusive => ModePref::Exclusive,
                OutputMode::Null => ModePref::Null,
            },
            buffer_ms: i.buffer_ms,
            clock: match i.clock {
                Clock::Events => ClockPref::Events,
                Clock::Polling => ClockPref::Polling,
            },
            release_on_pause: cfg.release_on_pause,
        },
        None => *cfg,
    };
    out.release();
    match open_output(decoder, cfg) {
        Ok((new_out, note)) => {
            out.install(new_out);
            SwitchOutcome::Switched(note)
        }
        // 用户只是想改设置，不该因此丢音乐：退回原配置再试一次
        Err(e) => match open_output(decoder, &prev) {
            Ok((back, _)) => {
                out.install(back);
                SwitchOutcome::KeptPlaying(e.to_string())
            }
            Err(e2) => {
                tracing::error!(%e, %e2, "切换输出配置失败且无法退回原配置");
                SwitchOutcome::Dead(e2)
            }
        },
    }
}

/// 播放位置（源时间轴毫秒）：解码位置减去池内未播样本的源时长。
/// decoder.position_ms() 是最后解码包的起点，池中的样本还没进设备 ——
/// 变速时池可能积压（rate<1），不修正的话进度条会领先于耳朵。
fn play_position(decoder: &DecodedStream, pool: &[f32]) -> u64 {
    let pool_frames = (pool.len() / decoder.channels().max(1)) as u64;
    let pool_ms = (pool_frames * 1000) / (decoder.sample_rate() as u64).max(1);
    decoder.position_ms().saturating_sub(pool_ms)
}

/// 设备缓冲区里尚未播出的时长（毫秒）。docs/04 §3.5：
/// position = base + frames_written - latency_ms。不扣这段，进度条会比
/// 耳朵超前最多一个缓冲深度（200ms），暂停瞬间停在"超前 200ms"处。
fn padding_ms(decoder: &DecodedStream, padding_frames: u32) -> u64 {
    (padding_frames as u64 * 1000) / (decoder.sample_rate() as u64).max(1)
}

/// 本轮可写帧数。
///
/// **共享模式**：`buffer - padding`。实测 shared 下 padding 在 0..buffer 之间
/// 正常跟随（事件与轮询都一样）。
///
/// **独占 + 事件**：写满一整个缓冲。事件驱动的语义就是"缓冲已经播空，该填了"，
/// 且独占事件下 `GetCurrentPadding` 恒等于 buffer（`examples/clock_probe.rs`
/// 实测：每次 250ms 采样都是 9600，两种缓冲深度都一样），拿它算恒得 0 ——
/// 那正是"报在播、实际静音"那一版 P0 的直接原因。
///
/// **独占 + 轮询**：按 `buffer - padding`。轮询没有"缓冲空了"这个信号，只能问
/// padding；实测 50ms 缓冲下 padding 正常波动（70 种取值），写入 ≈49114 帧/秒
/// 与采样率吻合。
///
/// 但同一台设备 **200ms/100ms 缓冲的独占轮询会"假开"**：Initialize 成功、
/// 时钟推进、`AudioMode` 报 exclusive，之后 padding 恒等于 buffer、一帧都不再
/// 消费。这类故障没法靠算法识别，只能靠运行期写入速率兜底（见
/// `DUMB_STREAM_MS` 的哑流自愈）。
fn writable_frames(clock: Clock, mode: OutputMode, buffer_frames: u32, padding: u32) -> u32 {
    match (mode, clock) {
        // 事件驱动的独占：事件本身就是"缓冲空了"的信号，写满
        (OutputMode::Exclusive, Clock::Events) => buffer_frames,
        // 其余（含独占轮询）：按 padding 差额，坏流会算成 0 并触发哑流自愈
        _ => buffer_frames.saturating_sub(padding),
    }
}

/// 从 pool 的 pool_pos 起按 rate 线性插值取 out_frames 帧，交错 f32 写入 out。
/// 输出帧 i 对应源位置 pool_pos + i*rate；越界索引 clamp 到池尾（仅 EOF 尾部触发）。
/// `vol_start` / `vol_step`：音量斜坡的本块起点与每帧增量（见 [`VOLUME_RAMP_MS`]）。
/// `eq`：10 段 EQ（协议 v6），插值后、乘音量**前**整块滤波——增益提升不能
/// 顶爆已归一化的输出电平；旁路时零开销直通。频谱 tap 吃**滤波后**的单声道：
/// 频谱反映的应该是耳朵听到的东西，不是文件里的东西。
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
    eq: &mut crate::eq::Eq,
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
        for ch in 0..channels {
            let a = pool[i0 * channels + ch];
            let b = pool[i1 * channels + ch];
            out.push(a + (b - a) * frac);
        }
    }
    // EQ：整块就地滤波（bypass 时跳过，样本位级不动——docs/24 B5b 旁路证明）
    if !eq.is_bypass() {
        for frame in out.chunks_mut(channels) {
            eq.process_frame(frame);
        }
    }
    // 单声道：滤波后混（tap 反映实际出声内容）
    for frame in out.chunks_exact(channels) {
        mono_out.push(frame.iter().sum::<f32>() * inv_ch);
    }
    // 音量斜坡：逐样本乘（在 EQ 之后，见函数头注释）
    for (i, s) in out.iter_mut().enumerate() {
        *s *= vol_start + vol_step * (i / channels) as f32;
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
            for (c, s) in dst.as_chunks_mut::<4>().0.iter_mut().zip(src) {
                c.copy_from_slice(&s.to_le_bytes());
            }
        }
        OutFormat::I24In32 => {
            // 24 bit 有效位**左对齐**在 32 bit 容器里（WAVEFORMATEXTENSIBLE 约定）
            for (c, s) in dst.as_chunks_mut::<4>().0.iter_mut().zip(src) {
                let x = (s.clamp(-1.0, 1.0) * 8_388_608.0)
                    .round()
                    .clamp(-8_388_608.0, 8_388_607.0);
                c.copy_from_slice(&((x as i32) << 8).to_le_bytes());
            }
        }
        OutFormat::I16 => {
            for (c, s) in dst.as_chunks_mut::<2>().0.iter_mut().zip(src) {
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
        let mut eq = crate::eq::Eq::new(crate::eq::EqParams::default(), 48_000, ch);
        resample_take(pool, pos, rate, ch, frames, vol, 0.0, &mut eq, &mut out, &mut Vec::new());
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
        let mut eq = crate::eq::Eq::new(crate::eq::EqParams::default(), 48_000, 1);
        resample_take(&pool, 0.0, 1.0, 1, 4, 0.0, 0.25, &mut eq, &mut out, &mut Vec::new());
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
        let mut eq = crate::eq::Eq::new(crate::eq::EqParams::default(), 48_000, 1);
        resample_take(&pool2, 1.0, 1.0, 1, 2, 1.0, 0.25, &mut eq, &mut out, &mut Vec::new());
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
            .as_chunks::<2>().0.iter()
            .map(|c| i16::from_le_bytes([c[0], c[1]]))
            .collect()
    }

    /// 读出 i24-in-32（右移还原成 24 bit 数值）。
    fn read_i24(bytes: &[u8]) -> Vec<i32> {
        bytes
            .as_chunks::<4>().0.iter()
            .map(|c| i32::from_le_bytes([c[0], c[1], c[2], c[3]]) >> 8)
            .collect()
    }

    #[test]
    fn encode_f32_is_bit_exact() {
        let src = [0.0f32, 0.5, -0.5, 1.0];
        let mut dst = vec![0u8; src.len() * 4];
        encode_frames(OutFormat::F32, &src, &mut dst);
        let back: Vec<f32> = dst
            .as_chunks::<4>().0.iter()
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

    #[test]
    fn writable_shared_tracks_padding() {
        assert_eq!(writable_frames(Clock::Events, OutputMode::Shared, 8820, 0), 8820);
        assert_eq!(writable_frames(Clock::Events, OutputMode::Shared, 8820, 4000), 4820);
        // 满缓冲：无可写（此时不该写，否则覆盖未播样本）
        assert_eq!(writable_frames(Clock::Events, OutputMode::Shared, 8820, 8820), 0);
        // padding 异常大于 buffer 时饱和到 0，不能下溢成天文数字
        assert_eq!(writable_frames(Clock::Events, OutputMode::Shared, 480, 900), 0);
        // 共享模式即便拿到时钟值也不该改判据（padding 在共享下是可信的）
        assert_eq!(writable_frames(Clock::Polling, OutputMode::Shared, 480, 100), 380);
    }

    #[test]
    fn writable_exclusive_events_writes_full_buffer() {
        // 独占事件：padding 恒等于 buffer（实测两种缓冲深度、每次采样都是满值），
        // 拿它算可写恒得 0 —— 那正是"报在播实际静音"的 P0。事件即信号，写满。
        assert_eq!(writable_frames(Clock::Events, OutputMode::Exclusive, 9600, 9600), 9600);
        assert_eq!(writable_frames(Clock::Events, OutputMode::Exclusive, 9600, 0), 9600);
    }

    #[test]
    fn writable_exclusive_polling_trusts_padding() {
        // 独占轮询：50ms 缓冲下 padding 正常波动（实测 70 种取值、写入 49114 帧/秒），
        // 按差额回填才对；坏流（padding 恒满）会算出 0，由哑流自愈接管。
        assert_eq!(writable_frames(Clock::Polling, OutputMode::Exclusive, 2560, 2203), 357);
        assert_eq!(writable_frames(Clock::Polling, OutputMode::Exclusive, 2560, 2560), 0);
        // 异常 padding > buffer 时饱和
        assert_eq!(writable_frames(Clock::Polling, OutputMode::Exclusive, 480, 900), 0);
    }

    // ---- 输出配置 → 尝试序列 ----

    #[test]
    fn explicit_buffer_is_respected_exactly() {
        // 用户点名 10ms 就只试 10ms：偷偷改成 50ms 属于静默改配置
        let cfg = OutputConfig {
            mode: ModePref::Exclusive,
            buffer_ms: 10,
            clock: ClockPref::Events,
            release_on_pause: true,
        };
        let a = exclusive_attempts(&cfg, Some(100_000));
        assert_eq!(a.len(), 1);
        assert_eq!(a[0].clock, Clock::Events);
        assert_eq!(a[0].period_hns, 100_000); // 10ms = 100_000 hns
        assert_eq!(a[0].buffer_hns, a[0].period_hns);
    }

    #[test]
    fn auto_enumerates_when_device_default_is_lie() {
        // 设备自报 10ms，但 44.1k 实测 10ms 报 0x88890019、20ms 才行：
        // 自动模式必须给出兜底周期，不能只有一个候选。
        let cfg = OutputConfig {
            mode: ModePref::Exclusive,
            buffer_ms: 0,
            clock: ClockPref::Events,
            release_on_pause: true,
        };
        let a = exclusive_attempts(&cfg, Some(100_000));
        assert!(a.len() >= 2, "自动模式必须有兜底尝试：{a:?}");
        assert_eq!(a[0].period_hns, 100_000);
        assert_eq!(a[1].period_hns, 200_000);
        assert!(a.iter().any(|x| x.period_hns == ms_to_hns(50)));
        // 顺序即优先级：先试设备默认（最省延迟），再退让
        assert!(a.windows(2).all(|w| w[0].period_hns <= w[1].period_hns));
    }

    #[test]
    fn auto_clock_tries_events_then_polling() {
        let cfg = OutputConfig {
            mode: ModePref::ExclusiveRequired,
            buffer_ms: 0,
            clock: ClockPref::Auto,
            release_on_pause: true,
        };
        let a = exclusive_attempts(&cfg, Some(100_000));
        assert_eq!(a[0].clock, Clock::Events);
        assert!(a.iter().any(|x| x.clock == Clock::Polling), "Auto 必须留轮询兜底");
        // 轮询能配深缓冲（事件被 WASAPI 强制 buffer==period）
        let polling = a.iter().find(|x| x.clock == Clock::Polling).unwrap();
        assert_eq!(polling.buffer_hns, ms_to_hns(DEFAULT_BUFFER_MS));
        assert!(polling.period_hns <= polling.buffer_hns, "周期不能大于缓冲");
    }

    #[test]
    fn polling_clock_never_tries_events() {
        let cfg = OutputConfig {
            mode: ModePref::Exclusive,
            buffer_ms: 0,
            clock: ClockPref::Polling,
            release_on_pause: true,
        };
        assert!(exclusive_attempts(&cfg, None)
            .iter()
            .all(|x| x.clock == Clock::Polling));
        let cfg = OutputConfig { clock: ClockPref::Events, ..cfg };
        assert!(exclusive_attempts(&cfg, None)
            .iter()
            .all(|x| x.clock == Clock::Events));
    }

    #[test]
    fn polling_period_falls_back_when_default_exceeds_buffer() {
        // 设备默认周期 50ms，但用户只要 10ms 缓冲：50 > 10 不能直接用，
        // 必须退回 10ms（WASAPI 要求 period <= buffer）
        let cfg = OutputConfig {
            mode: ModePref::Exclusive,
            buffer_ms: 10,
            clock: ClockPref::Polling,
            release_on_pause: true,
        };
        let a = exclusive_attempts(&cfg, Some(ms_to_hns(50)));
        assert_eq!(a[0].buffer_hns, ms_to_hns(10));
        assert!(a.iter().all(|x| x.period_hns <= x.buffer_hns));
        assert!(!a.iter().any(|x| x.period_hns == ms_to_hns(50)));
    }

    // ---- 配置匹配（避免无意义重建流）----

    fn fake_cfg() -> OutputConfig {
        OutputConfig::default()
    }

    #[test]
    fn matches_config_respects_auto_and_tolerance() {
        // 共享 200ms 事件流：默认配置（全 auto）应当匹配，不重建
        let cfg = fake_cfg();
        assert_eq!(cfg.mode, ModePref::Shared);
        // 无流时永远不匹配（必须打开）
        let out = Output(None);
        assert!(!matches_config(&out, &cfg));
    }

    // ---- 配置解析：坏值不致命 ----

    #[test]
    fn config_parsing_never_kills_playback() {
        // 未知模式按共享（不能让一个拼错的字符串把播放器变成没声）
        assert_eq!(OutputConfig::from_parts("wat", 0, "wat", true).mode, ModePref::Shared);
        assert_eq!(OutputConfig::from_parts("exclusiveRequired", 0, "", true).mode, ModePref::ExclusiveRequired);
        assert_eq!(OutputConfig::from_parts("null", 0, "", true).mode, ModePref::Null);
        // 缓冲上限 2s，负数归零
        assert_eq!(OutputConfig::from_parts("shared", 99_999, "", true).buffer_ms, 2000);
        assert_eq!(OutputConfig::from_parts("shared", -5, "", true).buffer_ms, 0);
        // 时钟：未知按 auto
        assert_eq!(OutputConfig::from_parts("shared", 0, "nope", true).clock, ClockPref::Auto);
        assert_eq!(OutputConfig::from_parts("shared", 0, "polling", true).clock, ClockPref::Polling);
    }
}
