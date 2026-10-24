//! AIYA Music 音频 sidecar 进程（ADR-0002）。
//!
//! 职责：持有一个 `aiya_audio::Engine`，通过命名管道以 JSON 行帧与 host 通信。
//! 不打开 SQLite、不依赖 Tauri —— 崩溃只带走音频，host 负责重启与恢复。
//!
//! 用法：
//! - `aiya-audio-host --pipe \\.\pipe\aiya-audio-<token>`  常规运行
//! - `aiya-audio-host --selftest`                          设备自检（枚举渲染设备后退出）
//!
//! 协议（aiya_audio::proto）：握手 Hello/Welcome 校验 `PROTOCOL_VERSION`，
//! 不兼容直接退出（exit 2），不静默降级；下行每 1s 心跳（host 侧 5s 超时 kill+restart）。

use std::io::{BufRead, BufReader, BufWriter, Write};
use std::os::windows::io::{AsRawHandle, FromRawHandle, RawHandle};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use miow::pipe::NamedPipe;

use aiya_audio::{encode_line, Engine, HostToSidecar, SidecarToHost, PROTOCOL_VERSION};

/// 会话结束原因。
enum SessionEnd {
    /// host 明确要求退出（EngineCommand::Shutdown）
    Shutdown,
}

/// 本地时区时间戳（与宿主 `logging.rs` 同格式，方便两条日志按时间对齐）。
struct LocalTime;

impl tracing_subscriber::fmt::time::FormatTime for LocalTime {
    fn format_time(
        &self,
        w: &mut tracing_subscriber::fmt::format::Writer<'_>,
    ) -> std::fmt::Result {
        let t = time::OffsetDateTime::now_local().unwrap_or_else(|_| time::OffsetDateTime::now_utc());
        write!(
            w,
            "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}",
            t.year(),
            u8::from(t.month()),
            t.day(),
            t.hour(),
            t.minute(),
            t.second(),
            t.millisecond()
        )
    }
}

fn main() {
    // 日志走 stderr：宿主把本进程 stderr 重定向到 {app_data}/logs/sidecar.log。
    // fmt() 默认写 **stdout**，而宿主把 stdout 置了 Stdio::null() —— 之前所有
    // sidecar 日志其实都被丢掉了（文件是 0 字节）。
    //
    // 时间戳用本地时区：否则与宿主日志差 8 小时，排查时得自己做时区换算。
    let filter = tracing_subscriber::EnvFilter::try_from_env("AIYA_LOG")
        .or_else(|_| tracing_subscriber::EnvFilter::try_from_default_env())
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(filter)
        .with_target(false)
        .with_timer(LocalTime)
        .init();

    let mut pipe_name: Option<String> = None;
    let mut selftest = false;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--pipe" => pipe_name = args.next(),
            "--selftest" => selftest = true,
            other => {
                eprintln!("未知参数 {other}（用法：--pipe <name> | --selftest）");
                std::process::exit(64);
            }
        }
    }

    if selftest {
        match aiya_audio::selftest_info() {
            Ok(names) => {
                println!("渲染设备 {} 个：", names.len());
                for (i, n) in names.iter().enumerate() {
                    println!("  [{i}] {n}");
                }
                std::process::exit(0);
            }
            Err(e) => {
                eprintln!("selftest 失败：{e}");
                std::process::exit(1);
            }
        }
    }

    let Some(pipe_name) = pipe_name else {
        eprintln!("缺少 --pipe 参数");
        std::process::exit(64);
    };

    if let Err(e) = serve_forever(&pipe_name) {
        eprintln!("sidecar 退出：{e}");
        std::process::exit(1);
    }
}

/// 建管道 → 等连接 → 会话 →（断开则）重开等重连；Shutdown 正常退出。
///
/// 为什么有 8s 退场窗口：host 崩溃/被任务管理器结束后，这里拿到的只是
/// 管道断开。此前无条件 `disconnect()` + `connect()` 永久等待 —— 进程
/// 常驻并占着 WASAPI 设备；PID 复用后还会让新 sidecar 建管道失败
/// （FILE_FLAG_FIRST_PIPE_INSTANCE），连带新宿主起不来。
/// host 真要重连会 spawn 全新进程（管道名带 PID），所以短暂等待后必须退出。
fn serve_forever(pipe_name: &str) -> Result<(), String> {
    // FILE_FLAG_FIRST_PIPE_INSTANCE：名字被残留实例占用时直接失败，
    // 让 host 的 spawn 重试逻辑兜底（快速重启场景）。
    let server = NamedPipe::new(pipe_name).map_err(|e| format!("创建管道 {pipe_name} 失败：{e}"))?;
    tracing::info!(%pipe_name, "sidecar 管道已创建，等待 host 连接");

    let mut orphan_deadline: Option<Instant> = None;
    loop {
        // 孤儿退场：host 走了，不要在这里守夜
        if let Some(d) = orphan_deadline {
            if Instant::now() >= d {
                tracing::warn!("host 已离开（8s 内无重连），sidecar 自动退出");
                return Ok(());
            }
        }
        server.connect().map_err(|e| format!("等待连接失败：{e}"))?;
        tracing::info!("host 已连接，开始握手");
        match run_session(&server) {
            Ok(SessionEnd::Shutdown) => {
                tracing::info!("收到 Shutdown，正常退出");
                return Ok(());
            }
            // Err = 管道断开/会话异常：留 8s 窗口等重连，超时即退出
            Err(e) => {
                tracing::warn!(%e, "会话结束，等待 host 重连（8s 后自动退出）");
                orphan_deadline = Some(Instant::now() + Duration::from_secs(8));
                let _ = server.disconnect();
            }
        }
    }
}

/// 一次完整会话：握手 → 引擎 ↔ 管道转发。
fn run_session(pipe: &NamedPipe) -> Result<SessionEnd, String> {
    // miow 没有 try_clone：复制原始句柄，读/写各持一个（同一管道实例）
    let read_half = dup(pipe)?;
    let write_half = dup(pipe)?;
    let mut write_half = BufWriter::new(write_half);
    let mut line_reader = BufReader::new(read_half);

    // ---- 握手：第一条必须是 Hello，版本不匹配 exit 2（不静默降级）----
    let mut line = String::new();
    read_line(&mut line_reader, &mut line).map_err(|e| format!("等握手失败：{e}"))?;
    let hello: HostToSidecar =
        serde_json::from_str(line.trim()).map_err(|e| format!("握手消息非法：{e}"))?;
    let protocol = match hello {
        HostToSidecar::Hello { protocol } => protocol,
        _ => return Err("首条消息不是 Hello".into()),
    };
    if protocol != PROTOCOL_VERSION {
        let msg = format!(
            "协议不兼容：host v{protocol} != sidecar v{PROTOCOL_VERSION}，拒绝启动"
        );
        eprintln!("{msg}");
        std::process::exit(2);
    }
    write_all_msg(&mut write_half, &SidecarToHost::Welcome { protocol: PROTOCOL_VERSION })?;
    tracing::info!(%protocol, "握手完成");

    // ---- 引擎 + 命令转发线程（reader 只解析入站消息并转交，engine 留主线程）----
    let mut engine = Engine::spawn().map_err(|e| format!("引擎启动失败：{e}"))?;
    let (pong_tx, pong_rx) = mpsc::channel::<()>();
    let (cmd_tx, cmd_rx) = mpsc::channel::<HostToSidecar>();

    let reader = std::thread::Builder::new()
        .name("sidecar-pipe-reader".into())
        .spawn(move || {
            let mut line = String::new();
            loop {
                line.clear();
                match read_line(&mut line_reader, &mut line) {
                    Ok(true) => {}
                    // EOF 或错误：host 没了（主循环写心跳/事件时也会失败，双保险）
                    _ => break,
                }
                let msg: HostToSidecar = match serde_json::from_str(line.trim()) {
                    Ok(m) => m,
                    Err(e) => {
                        tracing::warn!(%e, "非法消息，忽略");
                        continue;
                    }
                };
                if cmd_tx.send(msg).is_err() {
                    break;
                }
            }
        })
        .map_err(|e| format!("起 reader 线程失败：{e}"))?;

    // ---- 命令消费 + 事件/心跳写回循环（本线程）----
    let mut last_hb = Instant::now();
    let mut shutdown_seen = false;
    let end = loop {
        // 1) 入站命令
        while let Ok(msg) = cmd_rx.try_recv() {
            match msg {
                HostToSidecar::Cmd(cmd) => {
                    shutdown_seen |= matches!(cmd, aiya_audio::EngineCommand::Shutdown);
                    engine.send(cmd);
                }
                HostToSidecar::Ping => {
                    let _ = pong_tx.send(());
                }
                HostToSidecar::Hello { .. } => {} // 重复握手忽略
            }
        }
        // Shutdown 是最后一个命令（engine 线程随即退出），直接结束会话
        if shutdown_seen {
            break SessionEnd::Shutdown;
        }
        // 2) 出站事件
        if let Some(evt) = engine.try_recv() {
            write_all_msg(&mut write_half, &SidecarToHost::Evt(evt))?;
            continue;
        }
        // 3) Pong / 心跳
        if pong_rx.try_recv().is_ok() {
            write_all_msg(&mut write_half, &SidecarToHost::Pong)?;
            continue;
        }
        if last_hb.elapsed() >= Duration::from_secs(1) {
            // 心跳比 ADR 的 2s 更密，host 侧 5s 超时判定更稳
            write_all_msg(&mut write_half, &SidecarToHost::Hb)?;
            last_hb = Instant::now();
        }
        std::thread::sleep(Duration::from_millis(10));
    };

    engine.shutdown();
    // reader 线程阻塞在读上，管道关闭后随之退出；不 join 等它（Shutdown 要快）
    let _ = reader;
    Ok(end)
}

/// 复制管道句柄为独立 `NamedPipe`（读写分离）。
fn dup(pipe: &NamedPipe) -> Result<NamedPipe, String> {
    use winapi::um::winnt::HANDLE;
    let raw = pipe.as_raw_handle() as HANDLE;
    let mut out: HANDLE = std::ptr::null_mut();
    // DUPLICATE_SAME_ACCESS：同一管道实例的等价句柄，Read/Write 可并发
    let ok = unsafe {
        winapi::um::handleapi::DuplicateHandle(
            winapi::um::processthreadsapi::GetCurrentProcess(),
            raw,
            winapi::um::processthreadsapi::GetCurrentProcess(),
            &mut out,
            0,
            0,
            winapi::um::winnt::DUPLICATE_SAME_ACCESS,
        )
    };
    if ok == 0 {
        return Err("DuplicateHandle 失败".into());
    }
    Ok(unsafe { NamedPipe::from_raw_handle(out as RawHandle) })
}

/// 读一行；返回 false 表示流结束/错误。
fn read_line(reader: &mut impl BufRead, out: &mut String) -> std::io::Result<bool> {
    out.clear();
    match reader.read_line(out) {
        Ok(0) => Ok(false),
        Ok(_) => Ok(true),
        Err(e) => Err(e),
    }
}

/// 编码一行 JSON 并落盘（写失败 = 管道断开 = 会话结束）。
fn write_all_msg(writer: &mut impl Write, msg: &SidecarToHost) -> Result<(), String> {
    let bytes = encode_line(msg).map_err(|e| format!("编码失败：{e}"))?;
    writer.write_all(&bytes).and_then(|_| writer.flush()).map_err(|e| format!("写管道失败：{e}"))
}
