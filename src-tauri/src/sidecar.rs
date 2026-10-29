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
    /// 输出配置（模式/时钟/缓冲/暂停释放）。sidecar 崩溃重启后是新进程，
    /// 引擎内部的 pending_cfg 归零；不回放这条，用户开着独占会静默退回共享
    /// （设置页却显示已开）。
    output: aiya_audio::OutputConfig,
    /// EQ 参数（协议 v6）。与输出配置同性质：设置项而非曲目属性，崩溃重启
    /// 后必须无条件回放（引擎未装载时只暂存，首曲 Load 即生效）。
    eq: aiya_audio::EqParams,
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

/// 取互斥锁，**忽略毒化**（持锁线程 panic 后返回 panic 前一刻的值）。
///
/// 为什么不用 `.lock().unwrap()`：这些锁护的都是"最近一次写入的值"——子进程
/// 句柄、管道写端、存活时钟、恢复现场。持锁线程 panic 一次（`BufWriter` 写
/// 管道时的 IO panic、`Instant` 运算等）就会把锁永久标成 poisoned，之后
/// **每一个** `.lock().unwrap()` 都 panic，症状是播放器从此彻底不能播——而
/// 根因只是一次本来可恢复的写失败。这些数据没有跨字段不变量需要保护
/// （每个 Mutex 只管自己那一份），读到"panic 前的值"是安全的：最坏情况是
/// 拿着已经死掉的子进程句柄再杀一次，`kill` 会返回错误，无害。
fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

pub struct SidecarHandle {
    cmd_tx: Sender<aiya_audio::EngineCommand>,
    ev_rx: Receiver<aiya_audio::EngineEvent>,
    shared: Arc<Shared>,
    track: Arc<Mutex<TrackState>>,
}

impl SidecarHandle {
    /// 定位 sidecar 可执行文件：环境变量优先，其次主程序同目录（dev/release 共用 target 目录）。
    /// 即使定位失败也返回候选路径（看门狗按它重试，spawn 时自然报"文件不存在"）；
    /// 第二个值是失败原因，None = 定位成功。
    fn locate_host_exe() -> (PathBuf, Option<String>) {
        if let Ok(p) = std::env::var("AIYA_AUDIO_HOST") {
            let p = PathBuf::from(p);
            if p.exists() {
                return (p, None);
            }
        }
        let sibling = match std::env::current_exe() {
            Ok(exe) => exe
                .parent()
                .unwrap_or_else(|| std::path::Path::new("."))
                .join("aiya-audio-host.exe"),
            Err(e) => return (PathBuf::from("aiya-audio-host.exe"), Some(e.to_string())),
        };
        if sibling.exists() {
            return (sibling, None);
        }
        (
            sibling.clone(),
            Some(format!(
                "找不到 sidecar：{} 不存在（dev 下先 cargo build -p aiya-audio-host）",
                sibling.display()
            )),
        )
    }

    pub fn launch() -> Result<Self, String> {
        let (host_exe, locate_err) = Self::locate_host_exe();
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

        // 首连失败走启动降级而不是 Err：此前 setup 直接失败 = release 下
        // （windows_subsystem，无控制台无窗口）panic 静默消失，用户双击后
        // "什么都不发生"。现在应用照常起，先给 UI 一条音频错误事件，再交给
        // 看门狗按代重试——杀软扫未签名 exe 拖慢冷启动的场景几十秒内自愈，
        // sidecar 文件缺失时每轮重试留痕，文件就位后自动恢复。
        let gen = 0;
        match locate_err {
            Some(e) => degrade_launch(&ev_tx, e),
            None => {
                let t_connect = Instant::now();
                match connect_once(&host_exe, &pipe_base, gen, &shared) {
                    Ok(conn) => {
                        // spawn → 握手完成。冷启动慢（杀软扫未签名 exe）时这个数
                        // 会到十几秒，而看门狗从握手那一刻开始计时，两者叠加才会
                        // 出现"刚起来就被杀"。
                        tracing::info!(
                            elapsed_ms = t_connect.elapsed().as_millis() as u64,
                            "sidecar 首次握手完成"
                        );
                        *lock(&shared.writer_slot) = Some(BufWriter::new(conn.writer));
                        spawn_reader(
                            conn.reader,
                            ev_tx.clone(),
                            shared.clone(),
                            gen,
                            track.clone(),
                        );
                    }
                    Err(e) => degrade_launch(&ev_tx, e),
                }
            }
        }
        spawn_writer(cmd_rx, shared.clone());
        spawn_watchdog(shared.clone(), ev_tx, pipe_base.clone(), host_exe, track.clone());

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
            let mut t = lock(&self.track);
            match &cmd {
                aiya_audio::EngineCommand::Load { path, token, .. } => {
                    t.last_path = Some(path.clone());
                    t.last_token = *token;
                }
                aiya_audio::EngineCommand::SetVolume(v) => t.volume = *v,
                aiya_audio::EngineCommand::SetRate(r) => t.rate = *r,
                aiya_audio::EngineCommand::SetOutputConfig(c) => t.output = *c,
                aiya_audio::EngineCommand::SetEq(p) => t.eq = *p,
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
            let mut guard = lock(&self.shared.child);
            if let Some(c) = guard.as_mut() {
                if matches!(c.try_wait(), Ok(Some(_))) {
                    return;
                }
            }
            drop(guard);
            std::thread::sleep(Duration::from_millis(50));
        }
        if let Some(c) = lock(&self.shared.child).as_mut() {
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

/// 首连失败的降级出口：发一条装载前错误（token=0，pump 按"无曲目错误"上报
/// 给 UI），写端留空、reader 不起——命令静默丢弃，等看门狗 ~5s 后按代重试。
fn degrade_launch(ev_tx: &SyncSender<aiya_audio::EngineEvent>, reason: String) {
    tracing::error!(%reason, "sidecar 首连失败：以降级模式启动，由看门狗重试");
    let _ = ev_tx.send(aiya_audio::EngineEvent::Error {
        token: 0,
        kind: aiya_audio::AudioErrorKind::Internal.as_str().into(),
        message: format!("音频引擎暂时不可用，正在后台重试：{reason}"),
    });
}

/// 起子进程 + 连接其命名管道 + 握手。管道名带代号，换代不冲突。
fn connect_once(
    host_exe: &std::path::Path,
    pipe_base: &str,
    generation: u64,
    shared: &Arc<Shared>,
) -> Result<Connection, String> {
    // spawn 前查：关闭流程开始后看门狗不该再起新进程。
    // 看门狗循环头查过一次 shutting_down，但从查到 spawn 之间隔了
    // 判死/kill/换代几步 —— 关闭恰好插进来时，这里不拦就会留下无人收尸的
    // 孤儿 sidecar（shutdown 已经跑完、看门狗线程随后退出）。
    if shared.shutting_down.load(Ordering::SeqCst) {
        return Err("正在关闭：跳过 sidecar 重连".into());
    }
    let pipe = format!(r"\\.\pipe\{pipe_base}-{generation}");
    let mut cmd = Command::new(host_exe);
    cmd.args(["--pipe", &pipe])
        .stdout(Stdio::null())
        // stderr 落 {app_data}/logs/sidecar.log：引擎进程自己的 tracing
        // 此前被 Stdio::null() 丢掉，崩溃/设备失败完全没有痕迹
        .stderr(crate::logging::sidecar_stderr());

    // 不给子进程分配控制台：audio-host 是控制台子系统程序（为了 --selftest
    // 能正常输出，见那个 crate 顶部注释），被 GUI 主进程拉起时 Windows 会给它
    // 新建一个黑窗。这个标志是父进程侧的闸门，子进程自己改 subsystem 管不到。
    crate::proc::no_window(&mut cmd);

    let child = cmd
        .spawn()
        .map_err(|e| format!("启动 sidecar 失败：{e}"))?;
    *lock(&shared.child) = Some(child);

    // spawn 后再查：与 shutdown() 的窄竞态（检查在前、spawn 在后）兜底。
    // 命中就立即收尸返回 —— shutdown 的等待循环/最终 kill 只认 shared.child
    // 槽里的进程，槽被这里 take 走之后没人管它。
    if shared.shutting_down.load(Ordering::SeqCst) {
        if let Some(mut c) = lock(&shared.child).take() {
            let _ = c.kill();
            let _ = c.wait();
        }
        return Err("正在关闭：已回收刚启动的 sidecar".into());
    }

    // 连接 + 握手；任何失败路径都收尸（P2：spawn 后连接/握手失败曾把
    // 孤儿 sidecar 进程留在后台——无看门狗线程接管它）
    let result = connect_and_handshake(&pipe);
    if result.is_err() {
        if let Some(mut c) = lock(&shared.child).take() {
            let _ = c.kill();
            let _ = c.wait();
        }
        return result;
    }
    let conn = result.unwrap();

    *lock(&shared.alive) = Instant::now();
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
                    // 管道断开：本代终结。
                    //
                    // 这里必须留一行日志：读线程退出后**不再有人刷新 alive**，
                    // 看门狗会在 5s 后把"读线程偷偷退了"报成"心跳超时"并 kill。
                    // 此前这条 break 是静默的，一旦发生就只能看着一句假的超时日志猜。
                    Ok(0) => {
                        tracing::warn!(generation, "sidecar 管道 EOF，本代读取结束");
                        break;
                    }
                    Err(e) => {
                        tracing::warn!(generation, %e, "sidecar 管道读错误，本代读取结束");
                        break;
                    }
                    Ok(_) => {}
                }
                // 只有当前代的线程可以刷新存活时钟
                if shared.generation.load(Ordering::SeqCst) == generation {
                    *lock(&shared.alive) = Instant::now();
                }
                let Ok(msg) =
                    serde_json::from_str::<aiya_audio::SidecarToHost>(line.trim())
                else {
                    continue;
                };
                match msg {
                    aiya_audio::SidecarToHost::Evt(evt) => {
                        // 顺手记录恢复现场状态。按 token 归属（与命令侧记录的
                        // last_token 比对）：切歌瞬间旧曲的迟到回报不能写进
                        // 恢复现场，否则崩溃重启会从旧曲位置/播放态续播新曲。
                        {
                            let mut t = lock(&track);
                            match &evt {
                                aiya_audio::EngineEvent::Position { token, position_ms }
                                    if *token == t.last_token =>
                                {
                                    t.position_ms = *position_ms;
                                }
                                aiya_audio::EngineEvent::StateChanged { token, playing }
                                    if *token == t.last_token =>
                                {
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
                                if !matches!(restage, aiya_audio::EngineEvent::Position { .. })
                                    && ev_tx.send(restage).is_err() {
                                        break;
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
            let mut guard = lock(&shared.writer_slot);
            // None = 断线中：命令丢弃（恢复由看门狗负责）
            if let Some(w) = guard.as_mut() {
                if w.write_all(&bytes).and_then(|_| w.flush()).is_err() {
                    // 管道断开：清槽，看门狗接管
                    *guard = None;
                }
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
            let idle = lock(&shared.alive).elapsed();
            if idle.as_millis() <= WATCHDOG_TIMEOUT_MS as u128 {
                continue;
            }
            // 带上实际静默长度：超时值只是阈值，"静默 5.1s"和"静默 11s"
            // 指向完全不同的原因（前者是边界抖动，后者是对方真的停了）。
            tracing::error!(
                idle_ms = idle.as_millis() as u64,
                generation = shared.generation.load(Ordering::SeqCst),
                "sidecar 心跳超时（>{}ms），kill 并重启",
                WATCHDOG_TIMEOUT_MS
            );

            // 先分清"死了"还是"卡住"——两者的处置一样（只能重启），但归因完全不同：
            // 已退出 = 崩溃/自杀，去查 panic 与 sidecar.log 末尾；
            // 仍存活 = 循环卡住（管道写阻塞、线程被系统饿死），去查"心跳迟到"。
            // 没有这一位，事后只能看到一句"心跳超时"，猜不出是哪种。
            {
                let mut child = lock(&shared.child);
                let state = match child.as_mut() {
                    Some(c) => match c.try_wait() {
                        Ok(Some(status)) => format!("已退出 {status}"),
                        Ok(None) => "仍存活但无消息，按僵死处理".to_string(),
                        Err(e) => format!("try_wait 失败：{e}"),
                    },
                    None => "无子进程句柄".to_string(),
                };
                tracing::error!(state = %state, "sidecar 判死");
            }

            // kill 旧代
            if let Some(mut c) = lock(&shared.child).take() {
                let _ = c.kill();
                let _ = c.wait();
            }
            *lock(&shared.writer_slot) = None;
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
            let t = lock(&track).clone();
            let mut writer = BufWriter::new(conn.writer);
            let mut restore: Vec<u8> = Vec::new();
            // 独占偏好无条件回放（与是否装载无关）：引擎未装载时只暂存，
            // 首曲 Load 才真正开流。漏了这条，崩溃重启后会静默退回共享。
            if let Ok(b) = aiya_audio::encode_line(&aiya_audio::HostToSidecar::Cmd(
                aiya_audio::EngineCommand::SetOutputConfig(t.output),
            )) {
                restore.extend_from_slice(&b);
            }
            // EQ 偏好无条件回放（v6），理由同上：设置项不随装载与否
            if let Ok(b) = aiya_audio::encode_line(&aiya_audio::HostToSidecar::Cmd(
                aiya_audio::EngineCommand::SetEq(t.eq),
            )) {
                restore.extend_from_slice(&b);
            }
            if let Some(path) = t.last_path {
                for c in [
                    aiya_audio::EngineCommand::SetVolume(t.volume),
                    aiya_audio::EngineCommand::SetRate(t.rate),
                    // 起点与暂停态直接进 Load（协议 v7）：不再先 Load 再 Seek/Pause
                    // （那会先出声再从 0 跳/先响一下再停——恢复现场同样受
                    // "装载即出声"之害，只是发生在 sidecar 崩溃重启时）
                    aiya_audio::EngineCommand::Load {
                        path,
                        start_ms: t.position_ms,
                        token: t.last_token,
                        paused: !t.playing,
                    },
                ] {
                    if let Ok(b) = aiya_audio::encode_line(&aiya_audio::HostToSidecar::Cmd(c)) {
                        restore.extend_from_slice(&b);
                    }
                }
            }
            // 无论有没有当前曲都要把恢复序列写进去（独占偏好可能就在里面）
            let _ = writer.write_all(&restore).and_then(|_| writer.flush());
            tracing::info!(
                position_ms = t.position_ms,
                playing = t.playing,
                output_mode = t.output.mode.as_str(),
                output_buffer_ms = t.output.buffer_ms,
                "sidecar 恢复现场完成"
            );
            *lock(&shared.writer_slot) = Some(writer);
            spawn_reader(conn.reader, ev_tx.clone(), shared.clone(), gen, track.clone());
            tracing::info!(generation = gen, "sidecar 已重启");
        })
        .expect("spawn sidecar watchdog");
}

