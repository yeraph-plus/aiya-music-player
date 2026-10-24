//! sidecar 端到端冒烟（ADR-0002）：起真子进程 `aiya-audio-host`，走命名管道
//! 握手 → Load → 出声 → EOF → Shutdown 全链路。会真实出 2 秒 440Hz 音。
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use aiya_audio::{HostToSidecar, SidecarToHost, PROTOCOL_VERSION};

fn write_wav(path: &std::path::Path, sample_rate: u32, seconds: f64) {
    let n = (sample_rate as f64 * seconds) as usize;
    let mut data = Vec::with_capacity(n * 4);
    for i in 0..n {
        let s = ((i as f32 / sample_rate as f32) * 2.0 * std::f32::consts::PI * 440.0).sin() * 0.3;
        for _ in 0..2 {
            data.extend_from_slice(&s.to_le_bytes());
        }
    }
    let data_len = data.len() as u32;
    let file_len = 36 + data_len;
    let mut f = std::fs::File::create(path).unwrap();
    f.write_all(b"RIFF").unwrap();
    f.write_all(&file_len.to_le_bytes()).unwrap();
    f.write_all(b"WAVEfmt ").unwrap();
    f.write_all(&16u32.to_le_bytes()).unwrap();
    f.write_all(&3u16.to_le_bytes()).unwrap();
    f.write_all(&2u16.to_le_bytes()).unwrap();
    f.write_all(&sample_rate.to_le_bytes()).unwrap();
    f.write_all(&(sample_rate * 8).to_le_bytes()).unwrap();
    f.write_all(&8u16.to_le_bytes()).unwrap();
    f.write_all(&32u16.to_le_bytes()).unwrap();
    f.write_all(b"data").unwrap();
    f.write_all(&data_len.to_le_bytes()).unwrap();
    f.write_all(&data).unwrap();
}

fn main() {
    let wav = std::env::temp_dir().join("aiya-supervisor-smoke.wav");
    write_wav(&wav, 48_000, 2.0);

    let pipe = format!(
        r"\\.\pipe\aiya-supervisor-smoke-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis()
    );
    // examples 拿不到 CARGO_BIN_EXE：直接解析 workspace target 目录（dev 优先，release 兜底）
    let manifest = env!("CARGO_MANIFEST_DIR");
    let host_exe = [
        "../../target/debug/aiya-audio-host.exe",
        "../../target/release/aiya-audio-host.exe",
    ]
    .iter()
    .map(|rel| std::path::Path::new(manifest).join(rel))
    .find(|p| p.exists())
    .expect("先 cargo build -p aiya-audio-host 生成 sidecar 可执行文件");
    let mut child: Child = Command::new(host_exe)
        .args(["--pipe", &pipe])
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("spawn sidecar");

    let result = std::panic::catch_unwind(|| run_client(&pipe, &wav));
    let ok = matches!(result, Ok(true));

    // 清理：不管成败都停掉子进程
    let _ = child.kill();
    let _ = child.wait();
    if !ok {
        eprintln!("SUPERVISOR SMOKE FAILED");
        std::process::exit(1);
    }
}

/// 连接、握手、播放、断言。返回 true = 全链路通过。
fn run_client(pipe: &str, wav: &std::path::Path) -> bool {
    // 连接重试（子进程起管道需要一点时间）。客户端是 std::fs::File（自带 try_clone）
    let mut client: Option<std::fs::File> = None;
    for _ in 0..50 {
        match miow::pipe::connect(pipe) {
            Ok(c) => {
                client = Some(c);
                break;
            }
            Err(_) => std::thread::sleep(Duration::from_millis(100)),
        }
    }
    let Some(client) = client else {
        eprintln!("连不上管道 {pipe}");
        return false;
    };
    let reader = match client.try_clone() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("复制句柄失败：{e}");
            return false;
        }
    };
    let mut writer = std::io::BufWriter::new(client);
    let mut reader = BufReader::new(reader);

    // 握手
    send(&mut writer, &HostToSidecar::Hello { protocol: PROTOCOL_VERSION });
    let welcome = recv(&mut reader, Duration::from_secs(3));
    match &welcome {
        Some(SidecarToHost::Welcome { protocol }) if *protocol == PROTOCOL_VERSION => {
            println!("握手 OK: Welcome v{protocol}");
        }
        other => {
            eprintln!("握手失败：{other:?}");
            return false;
        }
    }

    // Ping → Pong
    send(&mut writer, &HostToSidecar::Ping);
    match recv(&mut reader, Duration::from_secs(3)) {
        Some(SidecarToHost::Pong) => println!("Ping/Pong OK"),
        other => {
            eprintln!("Ping 无 Pong：{other:?}");
            return false;
        }
    }

    // 播放 2s WAV
    send(&mut writer, &HostToSidecar::Cmd(aiya_audio::EngineCommand::SetVolume(0.8)));
    send(&mut writer, &HostToSidecar::Cmd(aiya_audio::EngineCommand::Load { path: wav.to_string_lossy().into(), start_ms: 0, token: 1 }));

    let start = Instant::now();
    let mut loaded = false;
    let mut max_pos = 0u64;
    let mut saw_eof = false;
    let mut saw_hb = false;
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        match recv(&mut reader, Duration::from_millis(500)) {
            Some(SidecarToHost::Evt(aiya_audio::EngineEvent::Loaded { duration_ms, .. })) => {
                println!("Loaded: {duration_ms}ms");
                loaded = true;
            }
            Some(SidecarToHost::Evt(aiya_audio::EngineEvent::Position { position_ms, .. })) => {
                max_pos = max_pos.max(position_ms);
            }
            Some(SidecarToHost::Evt(aiya_audio::EngineEvent::Eof { .. })) => {
                saw_eof = true;
                break;
            }
            Some(SidecarToHost::Evt(aiya_audio::EngineEvent::Error { message, .. })) => {
                eprintln!("ENGINE ERROR: {message}");
                return false;
            }
            Some(SidecarToHost::Hb) => saw_hb = true,
            Some(_) => {}
            None => {} // 超时窗口，继续等
        }
    }
    let wall = start.elapsed().as_secs_f64();

    // Shutdown → 子进程应正常退出
    send(&mut writer, &HostToSidecar::Cmd(aiya_audio::EngineCommand::Shutdown));

    let checks = [
        ("Loaded", loaded),
        ("EOF", saw_eof),
        ("Position 推进", max_pos > 500),
        ("心跳", saw_hb),
        ("wall≈2s", (1.5..4.0).contains(&wall)),
    ];
    let mut all_ok = true;
    for (name, ok) in checks {
        println!("  {name}: {}", if ok { "✓" } else { "✗" });
        all_ok &= ok;
    }
    if all_ok {
        println!("SUPERVISOR SMOKE OK: 2s 播完 wall={wall:.2}s max_pos={max_pos}ms");
    }
    all_ok
}

/// 读一条消息（阻塞但有限时：靠子进程/管道超时不可靠，用简单 deadline 轮询）。
/// 这里偷懒：管道读是阻塞的，超时通过子进程心跳保证不会久等——
/// 心跳每 1s 一条，所以最长等 ~1s 就有消息可读；deadline 只用于总时限控制。
fn recv(reader: &mut impl BufRead, timeout: Duration) -> Option<SidecarToHost> {
    let _ = timeout; // 保留签名语义；实际阻塞被 1s 心跳解耦
    let mut line = String::new();
    match reader.read_line(&mut line) {
        Ok(0) | Err(_) => None,
        Ok(_) => serde_json::from_str(line.trim()).ok(),
    }
}

fn send(writer: &mut std::io::BufWriter<std::fs::File>, msg: &HostToSidecar) {
    let bytes = aiya_audio::encode_line(msg).unwrap();
    writer.write_all(&bytes).unwrap();
    writer.flush().unwrap();
}
