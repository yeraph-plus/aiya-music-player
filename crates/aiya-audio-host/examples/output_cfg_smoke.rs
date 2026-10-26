//! 输出配置的**管道层**冒烟：起真子进程 `aiya-audio-host`，用 v4 协议把
//! `SetOutputConfig` 送进去，再读回 `audioMode` 事件。
//!
//! 为什么单独做这一条（而不是只靠 `mode_smoke`）：
//! - `mode_smoke` 直接驱动引擎，验的是"引擎听得懂配置"；
//! - 这里验的是**宿主与 sidecar 的接缝**：`setOutputConfig` 的 wire 形状、
//!   sidecar 的 `output` 记账、`audioMode` 的回流、播放中切 Null 不中断解码。
//!   历史上恰恰是这个接缝出过事（host v4 / sidecar v3 握手被拒、切 Null 后
//!   仍报 exclusive）。
//!
//! 会真开音频设备；断言只要求"配置被正确理解并如实回报"，不与设备能力挂钩
//! （独占被拒＝合法结果，只要 reason 说清楚）。

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use aiya_audio::{EngineCommand, EngineEvent, HostToSidecar, OutputConfig, SidecarToHost, PROTOCOL_VERSION};

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
    let wav = std::env::temp_dir().join("aiya-output-cfg-smoke.wav");
    write_wav(&wav, 48_000, 6.0);

    let pipe = format!(
        r"\\.\pipe\aiya-output-cfg-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis()
    );
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

    let _ = child.kill();
    let _ = child.wait();
    if !ok {
        eprintln!("OUTPUT CFG SMOKE FAILED");
        std::process::exit(1);
    }
    println!("OUTPUT CFG SMOKE OK");
}

struct Conn {
    w: std::io::BufWriter<std::fs::File>,
    r: BufReader<std::fs::File>,
}

impl Conn {
    fn send(&mut self, msg: &HostToSidecar) {
        let bytes = aiya_audio::encode_line(msg).unwrap();
        self.w.write_all(&bytes).unwrap();
        self.w.flush().unwrap();
    }
    fn cmd(&mut self, c: EngineCommand) {
        self.send(&HostToSidecar::Cmd(c));
    }
    /// 读到下一个 `audioMode` 事件（其它事件丢弃，超时返回 None）。
    fn wait_mode(&mut self, limit: Duration) -> Option<EngineEvent> {
        let deadline = Instant::now() + limit;
        while Instant::now() < deadline {
            let mut line = String::new();
            match self.r.read_line(&mut line) {
                Ok(0) | Err(_) => return None,
                Ok(_) => {}
            }
            match serde_json::from_str::<SidecarToHost>(line.trim()) {
                Ok(SidecarToHost::Evt(ev @ EngineEvent::AudioMode { .. })) => return Some(ev),
                Ok(SidecarToHost::Evt(EngineEvent::Error { message, .. })) => {
                    eprintln!("ENGINE ERROR: {message}");
                    return None;
                }
                _ => {}
            }
        }
        None
    }
    /// 读一会儿事件，返回这段时间里看到的最大 position（用来判断"还在走"）。
    fn max_position(&mut self, dur: Duration) -> u64 {
        let deadline = Instant::now() + dur;
        let mut max = 0;
        while Instant::now() < deadline {
            let mut line = String::new();
            match self.r.read_line(&mut line) {
                Ok(0) | Err(_) => break,
                Ok(_) => {}
            }
            if let Ok(SidecarToHost::Evt(EngineEvent::Position { position_ms, .. })) =
                serde_json::from_str::<SidecarToHost>(line.trim())
            {
                max = max.max(position_ms);
            }
        }
        max
    }
}

fn run_client(pipe: &str, wav: &std::path::Path) -> bool {
    let mut client = None;
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
    let reader = client.try_clone().expect("复制句柄");
    let mut c = Conn { w: std::io::BufWriter::new(client), r: BufReader::new(reader) };

    // 1) v4 握手
    c.send(&HostToSidecar::Hello { protocol: PROTOCOL_VERSION });
    let mut line = String::new();
    if c.r.read_line(&mut line).is_err() {
        return false;
    }
    match serde_json::from_str::<SidecarToHost>(line.trim()) {
        Ok(SidecarToHost::Welcome { protocol }) if protocol == PROTOCOL_VERSION => {
            println!("握手 OK: v{protocol}");
        }
        other => {
            eprintln!("握手失败（sidecar 是旧版本？）：{other:?} —— 先 cargo build -p aiya-audio-host");
            return false;
        }
    }

    let mut ok = true;
    let mut check = |name: &str, pass: bool, note: String| {
        println!("  {name}: {} {note}", if pass { "✓" } else { "✗" });
        ok &= pass;
    };

    // 2) 共享 / 轮询 / 显式 120ms：装载前就下发，验"暂存配置，首曲 Load 才开流"
    c.cmd(EngineCommand::SetOutputConfig(OutputConfig::from_parts("shared", 120, "polling", true)));
    c.cmd(EngineCommand::SetVolume(0.0));
    c.cmd(EngineCommand::Load {
        path: wav.to_string_lossy().into(),
        start_ms: 0,
        token: 1,
        paused: false,
    });
    let shared = c.wait_mode(Duration::from_secs(4));
    let (s_req, s_mode, s_clock, s_buf) = match &shared {
        Some(EngineEvent::AudioMode { requested, mode, clock, buffer_ms, .. }) => {
            (requested.clone(), mode.clone(), clock.clone(), *buffer_ms)
        }
        _ => (String::from("?"), String::from("?"), String::from("?"), 0),
    };
    check(
        "共享/轮询 120ms 生效",
        s_req == "shared" && s_mode == "shared" && s_clock == "polling" && s_buf >= 100,
        format!("requested={s_req} mode={s_mode} clock={s_clock} buffer={s_buf}ms"),
    );

    // 3) 播放中切独占（事件）：允许回落，但 requested 必须原样回报
    c.cmd(EngineCommand::SetOutputConfig(OutputConfig::from_parts("exclusive", 0, "events", true)));
    let excl = c.wait_mode(Duration::from_secs(4));
    let (e_req, e_mode, e_clock, e_reason) = match &excl {
        Some(EngineEvent::AudioMode { requested, mode, clock, reason, .. }) => {
            (requested.clone(), mode.clone(), clock.clone(), reason.clone().unwrap_or_default())
        }
        _ => Default::default(),
    };
    check(
        "独占请求被如实回报",
        e_req == "exclusive" && (e_mode == "exclusive" || !e_reason.is_empty()),
        format!("mode={e_mode} clock={e_clock} reason={e_reason:?}"),
    );
    // 切流后必须还在走（重建流不等于停止播放）
    let pos_after_excl = c.max_position(Duration::from_millis(900));
    check("切独占后仍在推进", pos_after_excl > 0, format!("pos={pos_after_excl}ms"));

    // 4) 切 Null：不出声但解码/进度照走，且回报 mode=null
    let before_null = pos_after_excl;
    c.cmd(EngineCommand::SetOutputConfig(OutputConfig::from_parts("null", 0, "auto", true)));
    let null_ev = c.wait_mode(Duration::from_secs(4));
    let n_mode = match &null_ev {
        Some(EngineEvent::AudioMode { mode, .. }) => mode.clone(),
        _ => String::from("?"),
    };
    check("切 Null 回报 mode=null", n_mode == "null", format!("mode={n_mode}"));
    let pos_null = c.max_position(Duration::from_millis(900));
    check(
        "Null 下进度仍推进",
        pos_null > before_null,
        format!("{before_null}ms → {pos_null}ms"),
    );

    // 5) Null → 共享：设备要重新打开，播放不能断
    c.cmd(EngineCommand::SetOutputConfig(OutputConfig::from_parts("shared", 0, "auto", true)));
    let back = c.wait_mode(Duration::from_secs(4));
    let b_mode = match &back {
        Some(EngineEvent::AudioMode { mode, .. }) => mode.clone(),
        _ => String::from("?"),
    };
    check("Null → 共享 恢复出流", b_mode == "shared", format!("mode={b_mode}"));
    let pos_back = c.max_position(Duration::from_millis(900));
    check(
        "恢复后进度继续",
        pos_back > pos_null,
        format!("{pos_null}ms → {pos_back}ms"),
    );

    c.cmd(EngineCommand::Shutdown);
    ok
}
