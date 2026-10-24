//! 音频 sidecar supervisor（ADR-0002）。
//!
//! 职责：启动 `aiya-audio-host.exe` 子进程、命名管道 JSON 行帧通信、
//! 心跳看门狗（sidecar 每 1s 心跳，>5s 无消息 kill+重启+恢复）、崩溃隔离。
//! 对外接口与 `aiya_audio::Engine` 对齐（send/try_recv），lib.rs 改动最小。
//!
//! 线程模型：
//! - writer 线程（唯一）：消费命令队列 → 写入 writer_slot 里的当前管道
//! - reader 线程（每代一个）：读当前管道事件 → ev_tx；EOF 即退出
//! - watchdog 线程：alive 超时 → kill + 换代重连 + 通过命令队列恢复现场
//!
//! 换代（generation）：管道名带代号，旧 reader 线程不会污染新代的存活时钟。

use std::fs::File;
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// 看门狗超时：>5s 无任何 sidecar 消息（心跳/事件）判定死亡（ADR-0002）。
const WATCHDOG_TIMEOUT_MS: u64 = 5_000;
/// 重启时连接子进程的重试窗口。
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// 恢复现场所需的最小状态（由命令/事件转发时顺手记录）。
#[derive(Default, Clone)]
struct TrackState {
    last_path: Option<String>,
    /// 最近一次 Load 的代数：恢复时必须沿用，否则宿主会把这些事件
    /// 当成过期回报丢掉（UI 停在旧状态）。
    last_token: u64,
    volume: f32,
    rate: f32,
    position_ms: u64,
    playing: bool,
    /// 独占偏好。sidecar 崩溃重启后是新进程，引擎内部的 pending_exclusive
    /// 归零；不回放这条，用户开着独占会静默退回共享（设置页却显示已开）。
    exclusive: bool,
}

/// 跨线程共享的 supervisor 状态。
struct Shared {
    child: Mutex<Option<Child>>,
    /// 当前代管道写端；None = 断线中（命令静默丢弃，等看门狗重启）
    writer_slot: Mutex<Option<BufWriter<File>>>,
    /// 当前代最后消息时间（单调时钟 Instant）。此前用系统时钟毫秒：
    /// 休眠唤醒/改系统时间后 now-last 必然 >5s，健康 sidecar 会被误杀重启。
    alive: Mutex<Instant>,
    generation: AtomicU64,
    shutting_down: AtomicBool,
}

pub struct SidecarHandle {
    cmd_tx: Sender<aiya_audio::EngineCommand>,
    ev_rx: Receiver<aiya_audio::EngineEvent>,
    shared: Arc<Shared>,
    track: Arc<Mutex<TrackState>>,
}

impl SidecarHandle {
    /// 定位 sidecar 可执行文件：环境变量优先，其次主程序同目录（dev/release 共用 target 目录）。
    fn locate_host_exe() -> Result<PathBuf, String> {
        if let Ok(p) = std::env::var("AIYA_AUDIO_HOST") {
            let p = PathBuf::from(p);
            if p.exists() {
                return Ok(p);
            }
        }
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let sibling = exe.parent().unwrap().join("aiya-audio-host.exe");
        if sibling.exists() {
            return Ok(sibling);
        }
        Err(format!(
            "找不到 sidecar：{} 不存在（dev 下先 cargo build -p aiya-audio-host）",
            sibling.display()
        ))
    }

    pub fn launch() -> Result<Self, String> {
        let host_exe = Self::locate_host_exe()?;
        let pipe_base = format!("aiya-audio-{}", std::process::id());
        let (cmd_tx, cmd_rx) = mpsc::channel::<aiya_audio::EngineCommand>();
        // 事件有界队列：Position 可丢旧帧（ADR-0002），其余不丢
        let (ev_tx, ev_rx) = mpsc::sync_channel::<aiya_audio::EngineEvent>(512);
        let track = Arc::new(Mutex::new(TrackState {
            volume: 1.0,
            rate: 1.0,
            ..Default::default()
        }));
        let shared = Arc::new(Shared {
            child: Mutex::new(None),
            writer_slot: Mutex::new(None),
            alive: Mutex::new(Instant::now()),
            generation: AtomicU64::new(0),
            shutting_down: AtomicBool::new(false),
        });

        // 首连失败直接报错（setup 阶段就该失败得明确）
        let gen = 0;
        let conn = connect_once(&host_exe, &pipe_base, gen, &shared)?;
        *shared.writer_slot.lock().unwrap() = Some(BufWriter::new(conn.writer));
        spawn_reader(conn.reader, ev_tx.clone(), shared.clone(), gen, track.clone());
        spawn_writer(cmd_rx, shared.clone());
        spawn_watchdog(shared.clone(), ev_tx, pipe_base.clone(), host_exe.clone(), track.clone());

        Ok(Self {
            cmd_tx,
            ev_rx,
            shared,
            track,
        })
    }

    pub fn send(&self, cmd: aiya_audio::EngineCommand) {
        // 记录恢复现场所需的命令侧状态
        {
            let mut t = self.track.lock().unwrap();
            match &cmd {
                aiya_audio::EngineCommand::Load { path, token, .. } => {
                    t.last_path = Some(path.clone());
                    t.last_token = *token;
                }
                aiya_audio::EngineCommand::SetVolume(v) => t.volume = *v,
                aiya_audio::EngineCommand::SetRate(r) => t.rate = *r,
                aiya_audio::EngineCommand::SetExclusive(on) => t.exclusive = *on,
                _ => {}
            }
        }
        let _ = self.cmd_tx.send(cmd);
    }

    /// 非阻塞取事件（事件泵轮询）。
    pub fn try_recv(&self) -> Option<aiya_audio::EngineEvent> {
        self.ev_rx.try_recv().ok()
    }

    pub fn shutdown(&self) {
        self.shared.shutting_down.store(true, Ordering::SeqCst);
        let _ = self.cmd_tx.send(aiya_audio::EngineCommand::Shutdown);
        // 给 sidecar 一点时间自行退出，超时强杀
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            let mut guard = self.shared.child.lock().unwrap();
            if let Some(c) = guard.as_mut() {
                if matches!(c.try_wait(), Ok(Some(_))) {
                    return;
                }
            }
            drop(guard);
            std::thread::sleep(Duration::from_millis(50));
        }
        if let Some(c) = self.shared.child.lock().unwrap().as_mut() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

impl Drop for SidecarHandle {
    fn drop(&mut self) {
        self.shutdown();
    }
}

struct Connection {
    reader: File,
    writer: File,
}

/// 起子进程 + 连接其命名管道 + 握手。管道名带代号，换代不冲突。
fn connect_once(
    host_exe: &std::path::Path,
    pipe_base: &str,
    generation: u64,
    shared: &Arc<Shared>,
) -> Result<Connection, String> {
    let pipe = format!(r"\\.\pipe\{pipe_base}-{generation}");
    let child = Command::new(host_exe)
        .args(["--pipe", &pipe])
        .stdout(Stdio::null())
        // stderr 落 {app_data}/logs/sidecar.log：引擎进程自己的 tracing
        // 此前被 Stdio::null() 丢掉，崩溃/设备失败完全没有痕迹
        .stderr(crate::logging::sidecar_stderr())
        .spawn()
        .map_err(|e| format!("启动 sidecar 失败：{e}"))?;
    *shared.child.lock().unwrap() = Some(child);

    // 连接 + 握手；任何失败路径都收尸（P2：spawn 后连接/握手失败曾把
    // 孤儿 sidecar 进程留在后台——无看门狗线程接管它）
    let result = connect_and_handshake(&pipe);
    if result.is_err() {
        if let Some(mut c) = shared.child.lock().unwrap().take() {
            let _ = c.kill();
            let _ = c.wait();
        }
        return result;
    }
    let conn = result.unwrap();

    *shared.alive.lock().unwrap() = Instant::now();
    Ok(conn)
}

fn connect_and_handshake(pipe: &str) -> Result<Connection, String> {
    // 连接重试（子进程创建管道需要一点时间）
    let client = {
        let deadline = Instant::now() + CONNECT_TIMEOUT;
        loop {
            match miow::pipe::connect(pipe) {
                Ok(c) => break c,
                Err(_) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(100));
                }
                Err(e) => return Err(format!("连接 sidecar 管道超时 {pipe}：{e}")),
            }
        }
    };

    // 握手：Hello → Welcome（版本不匹配由 sidecar 侧 exit(2)，这里同样校验）
    let mut writer = BufWriter::new(client.try_clone().map_err(|e| e.to_string())?);
    let mut reader = BufReader::new(client);
    let bytes = aiya_audio::encode_line(&aiya_audio::HostToSidecar::Hello {
        protocol: aiya_audio::PROTOCOL_VERSION,
    })
    .map_err(|e| e.to_string())?;
    writer
        .write_all(&bytes)
        .and_then(|_| writer.flush())
        .map_err(|e| format!("握手发送失败：{e}"))?;

    let deadline = Instant::now() + Duration::from_secs(3);
    let mut line = String::new();
    loop {
        line.clear();
        let n = reader
            .read_line(&mut line)
            .map_err(|e| format!("握手读取失败：{e}"))?;
        if n == 0 {
            return Err("sidecar 在握手中断开（可能协议不兼容或管道冲突）".into());
        }
        if let Ok(aiya_audio::SidecarToHost::Welcome { protocol }) = serde_json::from_str(line.trim()) {
            if protocol != aiya_audio::PROTOCOL_VERSION {
                return Err(format!("协议不兼容：sidecar v{protocol}"));
            }
            break;
        }
        if Instant::now() > deadline {
            return Err("握手超时（未收到 Welcome）".into());
        }
    }

    let reader_file = reader.into_inner();
    let writer_file = writer.into_inner().map_err(|e| e.to_string())?;
    Ok(Connection { reader: reader_file, writer: writer_file })
}

/// 事件接收线程（每代一个）：管道 → ev_tx。EOF/错误即退出，不碰 alive（看门狗按超时判）。
fn spawn_reader(
    reader: File,
    ev_tx: SyncSender<aiya_audio::EngineEvent>,
    shared: Arc<Shared>,
    generation: u64,
    track: Arc<Mutex<TrackState>>,
) {
    std::thread::Builder::new()
        .name("sidecar-evt-recv".into())
        .spawn(move || {
            let mut reader = BufReader::new(reader);
            let mut line = String::new();
            loop {
                line.clear();
                match reader.read_line(&mut line) {
                    Ok(0) | Err(_) => break, // 管道断开：本代终结
                    Ok(_) => {}
                }
                // 只有当前代的线程可以刷新存活时钟
                if shared.generation.load(Ordering::SeqCst) == generation {
                    *shared.alive.lock().unwrap() = Instant::now();
                }
                let Ok(msg) =
                    serde_json::from_str::<aiya_audio::SidecarToHost>(line.trim())
                else {
                    continue;
                };
                match msg {
                    aiya_audio::SidecarToHost::Evt(evt) => {
                        // 顺手记录恢复现场状态
                        {
                            let mut t = track.lock().unwrap();
                            match &evt {
                                aiya_audio::EngineEvent::Position { position_ms, .. } => {
                                    t.position_ms = *position_ms;
                                }
                                aiya_audio::EngineEvent::StateChanged { playing, .. } => {
                                    t.playing = *playing;
                                }
                                _ => {}
                            }
                        }
                        // 有界队列：满了丢 Position（ADR 允许丢位置帧）；其余事件
                        // （Eof/StateChanged/Error）阻塞等空位——P2：此前 Full
                        // 静默丢任意事件，丢 Eof 会让播放器卡在播放态不切曲。
                        // 泵 25ms 一轮 drain，阻塞窗口极短。
                        match ev_tx.try_send(evt) {
                            Ok(()) => {}
                            Err(mpsc::TrySendError::Full(restage)) => {
                                if !matches!(restage, aiya_audio::EngineEvent::Position { .. }) {
                                    if ev_tx.send(restage).is_err() {
                                        break;
                                    }
                                }
                            }
                            Err(mpsc::TrySendError::Disconnected(_)) => break,
                        }
                    }
                    aiya_audio::SidecarToHost::Hb
                    | aiya_audio::SidecarToHost::Pong
                    | aiya_audio::SidecarToHost::Welcome { .. } => {}
                }
            }
        })
        .expect("spawn sidecar evt reader");
}

/// 命令转发线程（唯一，终身）：命令队列 → writer_slot 当前管道。
fn spawn_writer(cmd_rx: Receiver<aiya_audio::EngineCommand>, shared: Arc<Shared>) {
    std::thread::Builder::new()
        .name("sidecar-cmd-fwd".into())
        .spawn(move || loop {
            let Ok(cmd) = cmd_rx.recv() else { return };
            let bytes = match aiya_audio::encode_line(&aiya_audio::HostToSidecar::Cmd(cmd)) {
                Ok(b) => b,
                Err(_) => continue,
            };
            let mut guard = shared.writer_slot.lock().unwrap();
            match guard.as_mut() {
                Some(w) => {
                    if w.write_all(&bytes).and_then(|_| w.flush()).is_err() {
                        // 管道断开：清槽，看门狗接管
                        *guard = None;
                    }
                }
                // None = 断线中：命令丢弃（恢复由看门狗负责）
                None => {}
            }
        })
        .expect("spawn sidecar cmd writer");
}

/// 看门狗线程：alive 超时 → kill + 换代重连 + 恢复现场。
fn spawn_watchdog(
    shared: Arc<Shared>,
    ev_tx: SyncSender<aiya_audio::EngineEvent>,
    pipe_base: String,
    host_exe: PathBuf,
    track: Arc<Mutex<TrackState>>,
) {
    std::thread::Builder::new()
        .name("sidecar-watchdog".into())
        .spawn(move || loop {
            std::thread::sleep(Duration::from_secs(1));
            if shared.shutting_down.load(Ordering::SeqCst) {
                return;
            }
            let idle = shared.alive.lock().unwrap().elapsed();
            if idle.as_millis() <= WATCHDOG_TIMEOUT_MS as u128 {
                continue;
            }
            tracing::error!("sidecar 心跳超时（>{}ms），kill 并重启", WATCHDOG_TIMEOUT_MS);

            // kill 旧代
            if let Some(mut c) = shared.child.lock().unwrap().take() {
                let _ = c.kill();
                let _ = c.wait();
            }
            *shared.writer_slot.lock().unwrap() = None;
            let gen = shared.generation.fetch_add(1, Ordering::SeqCst) + 1;

            // 重连（失败则下一轮看门狗再试）
            let conn = match connect_once(&host_exe, &pipe_base, gen, &shared) {
                Ok(c) => c,
                Err(e) => {
                    tracing::error!(%e, generation = gen, "sidecar 重启失败，下轮重试");
                    continue;
                }
            };

            // 恢复现场：音量/倍速/当前曲（带起点与代数）/播放态。
            // 恢复字节必须在**发布 writer_slot 之前**写好：此前先发布槽再拼恢复
            // 序列，窗口内 writer 线程会把用户命令（如 Load/Play）先送进新
            // sidecar，随后被恢复序列的旧 Load 覆盖 —— 表现为"重启后跳回上一首"。
            let t = track.lock().unwrap().clone();
            let mut writer = BufWriter::new(conn.writer);
            let mut restore: Vec<u8> = Vec::new();
            // 独占偏好无条件回放（与是否装载无关）：引擎未装载时只暂存，
            // 首曲 Load 才真正开流。漏了这条，崩溃重启后会静默退回共享。
            if let Ok(b) = aiya_audio::encode_line(&aiya_audio::HostToSidecar::Cmd(
                aiya_audio::EngineCommand::SetExclusive(t.exclusive),
            )) {
                restore.extend_from_slice(&b);
            }
            if let Some(path) = t.last_path {
                for c in [
                    aiya_audio::EngineCommand::SetVolume(t.volume),
                    aiya_audio::EngineCommand::SetRate(t.rate),
                    // 起点直接进 Load：不再先 Load 再 Seek（那会先出声再从 0 跳）
                    aiya_audio::EngineCommand::Load {
                        path,
                        start_ms: t.position_ms,
                        token: t.last_token,
                    },
                ] {
                    if let Ok(b) = aiya_audio::encode_line(&aiya_audio::HostToSidecar::Cmd(c)) {
                        restore.extend_from_slice(&b);
                    }
                }
                if !t.playing {
                    if let Ok(b) = aiya_audio::encode_line(&aiya_audio::HostToSidecar::Cmd(
                        aiya_audio::EngineCommand::Pause,
                    )) {
                        restore.extend_from_slice(&b);
                    }
                }
            }
            // 无论有没有当前曲都要把恢复序列写进去（独占偏好可能就在里面）
            let _ = writer.write_all(&restore).and_then(|_| writer.flush());
            tracing::info!(
                position_ms = t.position_ms,
                playing = t.playing,
                exclusive = t.exclusive,
                "sidecar 恢复现场完成"
            );
            *shared.writer_slot.lock().unwrap() = Some(writer);
            spawn_reader(conn.reader, ev_tx.clone(), shared.clone(), gen, track.clone());
            tracing::info!(generation = gen, "sidecar 已重启");
        })
        .expect("spawn sidecar watchdog");
}

