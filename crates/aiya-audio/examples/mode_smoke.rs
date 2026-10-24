//! 输出配置冒烟：共享/独占 × 事件/轮询 × 缓冲 × 暂停释放 × 静音输出。
//!
//! 断言口径（每一项都能**证伪**，不是"跑完没 panic 就算过"）：
//! 1. 每种组合都回报 `AudioMode`，且 requested 与 mode 的关系自洽：
//!    - 请求共享 → 实际必须 shared（不该偷偷独占）
//!    - 请求独占 → 要么 exclusive（reason 为空），要么 shared + 非空原因
//!    - 请求独占必须 → 要么 exclusive，要么 Error（不回落）
//! 2. **写入速率**必须接近采样率：这是唯一能证伪"报在播、实际静音"的指标
//!    （独占曾因 padding 恒等于 buffer 全程插静音，而事件断言一路绿灯）。
//! 3. 时钟：显式选轮询必须报 polling。
//! 4. 暂停：1 秒内写入接近 0（淡出后真的不喂了）；Resume 后写入恢复。
//!    这条同时覆盖"独占暂停释放设备 → Resume 重开"的整条路径。
//! 5. null：不打开设备也能推进（写入≈采样率），且 mode=null。
//!
//! 会出声（8 秒 440Hz，音量很低）。用法：
//! `cargo run -p aiya-audio --example mode_smoke`（或 npm run smoke:mode）

use std::io::Write;
use std::time::{Duration, Instant};

use aiya_audio::{
    ClockPref, Engine, EngineCommand, EngineEvent, ModePref, OutputConfig,
};

const RATE: u32 = 48_000;

fn write_wav(path: &std::path::Path, sample_rate: u32, seconds: f64) {
    let n = (sample_rate as f64 * seconds) as usize;
    let mut data = Vec::with_capacity(n * 4);
    for i in 0..n {
        let s =
            ((i as f32 / sample_rate as f32) * 2.0 * std::f32::consts::PI * 440.0).sin() * 0.2;
        for _ in 0..2 {
            data.extend_from_slice(&s.to_le_bytes());
        }
    }
    let data_len = data.len() as u32;
    let mut f = std::fs::File::create(path).unwrap();
    f.write_all(b"RIFF").unwrap();
    f.write_all(&(36 + data_len).to_le_bytes()).unwrap();
    f.write_all(b"WAVEfmt ").unwrap();
    f.write_all(&16u32.to_le_bytes()).unwrap();
    f.write_all(&3u16.to_le_bytes()).unwrap(); // IEEE float
    f.write_all(&2u16.to_le_bytes()).unwrap();
    f.write_all(&sample_rate.to_le_bytes()).unwrap();
    f.write_all(&(sample_rate * 8).to_le_bytes()).unwrap();
    f.write_all(&8u16.to_le_bytes()).unwrap();
    f.write_all(&32u16.to_le_bytes()).unwrap();
    f.write_all(b"data").unwrap();
    f.write_all(&data_len.to_le_bytes()).unwrap();
    f.write_all(&data).unwrap();
}

/// 一次 AudioMode 快照（测试只关心这几个字段）。
#[derive(Debug, Clone)]
struct Mode {
    requested: String,
    mode: String,
    clock: String,
    buffer_ms: u32,
    format: String,
    reason: Option<String>,
}

/// 收一段时间的事件，返回 (最后一个 AudioMode, 是否收到 Error, 写入帧增量)。
fn drain(engine: &Engine, ms: u64) -> (Option<Mode>, Option<String>, u64) {
    let before = engine.written_frames();
    let mut last = None;
    let mut err = None;
    let deadline = Instant::now() + Duration::from_millis(ms);
    while Instant::now() < deadline {
        match engine.try_recv() {
            Some(EngineEvent::AudioMode {
                requested,
                mode,
                clock,
                buffer_ms,
                format,
                reason,
                ..
            }) => {
                last = Some(Mode { requested, mode, clock, buffer_ms, format, reason });
            }
            Some(EngineEvent::Error { message, .. }) => {
                err = Some(message);
            }
            Some(_) => {}
            None => std::thread::sleep(Duration::from_millis(5)),
        }
    }
    (last, err, engine.written_frames() - before)
}

fn cfg(mode: ModePref, buffer_ms: u32, clock: ClockPref) -> OutputConfig {
    OutputConfig { mode, buffer_ms, clock, release_on_pause: true }
}

/// 写入速率断言：允许切流空档，但必须 ≥70% 采样率。
fn assert_sound(delta: u64, ms: u64, what: &str) {
    let expect = RATE as u64 * ms / 1000;
    assert!(
        delta * 10 >= expect * 7,
        "{what}: {ms}ms 内只写 {delta} 帧（期望 ≈{expect}）——引擎在静音。\
         独占下若 padding 恒等于 buffer，可写空间会算成 0"
    );
    println!("  {what}: 写入 {delta} 帧 / {ms}ms（期望 ≈{expect}）✓");
}

fn main() {
    let dir = std::env::temp_dir().join("aiya-mode-smoke");
    std::fs::create_dir_all(&dir).unwrap();
    let wav = dir.join("tone.wav");
    write_wav(&wav, RATE, 40.0); // 够长：所有组合跑完之前不能 EOF
    let path = wav.to_string_lossy().to_string();

    let mut engine = Engine::spawn().expect("engine spawn");
    let mut token = 1u64;

    // ---------- 1. 共享 + 事件 ----------
    println!("[1] 共享 / 事件 / 自动缓冲");
    engine.send(EngineCommand::SetOutputConfig(cfg(ModePref::Shared, 0, ClockPref::Events)));
    engine.send(EngineCommand::Load { path: path.clone(), start_ms: 0, token });
    let (m, err, delta) = drain(&engine, 1200);
    assert!(err.is_none(), "共享模式不该报错：{err:?}");
    let m = m.expect("装载必须回报 AudioMode");
    assert_eq!(m.requested, "shared");
    assert_eq!(m.mode, "shared", "请求共享却拿到 {}", m.mode);
    assert_eq!(m.clock, "events", "显式事件驱动却报 {}", m.clock);
    assert!(m.reason.is_none(), "共享模式不该有降级原因：{:?}", m.reason);
    assert_sound(delta, 1200, "共享/事件");

    // ---------- 2. 共享 + 轮询 ----------
    println!("[2] 共享 / 轮询");
    engine.send(EngineCommand::SetOutputConfig(cfg(ModePref::Shared, 0, ClockPref::Polling)));
    let (m, err, delta) = drain(&engine, 1200);
    assert!(err.is_none(), "轮询共享不该报错：{err:?}");
    let m = m.expect("切时钟必须回报 AudioMode");
    assert_eq!(m.clock, "polling", "显式轮询却报 {}", m.clock);
    assert_eq!(m.mode, "shared");
    assert_sound(delta, 1200, "共享/轮询");

    // ---------- 3. 独占 + 事件（自动缓冲）----------
    println!("[3] 独占 / 事件 / 自动缓冲");
    token += 1;
    engine.send(EngineCommand::SetOutputConfig(cfg(ModePref::Exclusive, 0, ClockPref::Events)));
    let (m, err, delta) = drain(&engine, 1500);
    assert!(err.is_none(), "独占可回落，不该报 Error：{err:?}");
    let m = m.expect("切独占必须回报 AudioMode");
    assert_eq!(m.requested, "exclusive");
    if m.mode == "exclusive" {
        assert!(m.reason.is_none(), "拿到独占却带原因：{:?}", m.reason);
        println!("  独占 ✓ clock={} buffer={}ms format={}", m.clock, m.buffer_ms, m.format);
    } else {
        let r = m.reason.clone().unwrap_or_default();
        assert!(!r.is_empty(), "回落共享必须给出原因（可解释）");
        println!("  设备拒绝 → 回落共享，原因：{r}");
    }
    assert_sound(delta, 1500, "独占/事件");
    // reload 一次，确认配置在换曲后仍然生效（不是只在切流时有效）
    token += 1;
    engine.send(EngineCommand::Load { path: path.clone(), start_ms: 0, token });
    let (m2, err2, delta2) = drain(&engine, 1500);
    assert!(err2.is_none(), "换曲后不该报错：{err2:?}");
    let m2 = m2.expect("换曲后必须回报 AudioMode（配置要持续生效）");
    assert_eq!(m2.mode, m.mode, "换曲后模式变了：{} → {}", m.mode, m2.mode);
    assert_sound(delta2, 1500, "独占/事件（换曲后）");

    // ---------- 4. 独占 + 轮询 + 深缓冲 ----------
    println!("[4] 独占 / 轮询 / 200ms 缓冲");
    engine.send(EngineCommand::SetOutputConfig(cfg(ModePref::Exclusive, 200, ClockPref::Polling)));
    let (m, err, delta) = drain(&engine, 1500);
    assert!(err.is_none(), "独占轮询不该报 Error：{err:?}");
    let m = m.expect("切独占轮询必须回报 AudioMode");
    match (m.mode.as_str(), m.clock.as_str()) {
        ("exclusive", "polling") => {
            // 深缓冲轮询真的能用
            assert!(
                (m.buffer_ms as i64 - 200).abs() <= 2,
                "请求 200ms 缓冲，实际 {}ms",
                m.buffer_ms
            );
            println!("  独占轮询 ✓ buffer={}ms format={}", m.buffer_ms, m.format);
        }
        ("exclusive", other) => {
            // 本机实测：独占轮询在 200ms/100ms 缓冲下"假开"（Initialize 成功但
            // 设备一帧不消费）。引擎应当**自己发现并切到另一种时钟**，且给出原因。
            let r = m.reason.clone().unwrap_or_default();
            assert!(
                r.contains("不消费"),
                "时钟被改成 {other} 却没说明原因（用户会以为设置没生效）：{r:?}"
            );
            println!("  独占轮询哑火 → 自动切 {other}（{r}）");
        }
        (mode, _) => {
            println!("  设备拒绝独占轮询 → {mode}（{}）", m.reason.clone().unwrap_or_default());
        }
    }
    assert_sound(delta, 1500, "独占/轮询");

    // ---------- 5. 暂停（淡出 + 释放设备）与 Resume ----------
    println!("[5] 暂停 / 恢复");
    engine.send(EngineCommand::Pause);
    // 淡出 20ms + 设备缓冲排空（≤200ms）后再观察：之前允许写入（那正是淡出）
    let (_, err, fade) = drain(&engine, 400);
    assert!(err.is_none(), "暂停不该报错：{err:?}");
    let (_, _, quiet) = drain(&engine, 800);
    assert!(
        quiet < RATE as u64 / 20,
        "暂停稳定后 800ms 仍写了 {quiet} 帧（应 ≈0）——暂停没生效或 declick 卡住"
    );
    println!("  暂停：淡出期写 {fade} 帧，之后 800ms 写 {quiet} 帧 ✓");
    engine.send(EngineCommand::Resume);
    let (m, err, delta) = drain(&engine, 1200);
    assert!(err.is_none(), "Resume 不该报错：{err:?}");
    println!("  Resume 后 AudioMode={:?}", m.as_ref().map(|x| &x.mode));
    assert_sound(delta, 1200, "恢复");

    // ---------- 6. seek 不中断 ----------
    println!("[6] seek");
    engine.send(EngineCommand::Seek(10_000));
    let (_, err, delta) = drain(&engine, 1000);
    assert!(err.is_none(), "seek 不该报错：{err:?}");
    assert_sound(delta, 1000, "seek 后");

    // ---------- 7. 独占必须（设备不支持时必须报错，不能偷偷回落）----------
    println!("[7] 独占（必须）/ 事件");
    engine.send(EngineCommand::SetOutputConfig(cfg(
        ModePref::ExclusiveRequired,
        0,
        ClockPref::Events,
    )));
    let (m, err, _) = drain(&engine, 1500);
    match (err, m) {
        (Some(e), _) => {
            assert!(
                e.contains("独占"),
                "独占必须模式的错误应说明是独占问题，实际：{e}"
            );
            println!("  设备不支持 → 报错（符合预期）：{e}");
            // 报错后引擎回到等 Load 状态：切回共享应能恢复出声
            engine.send(EngineCommand::SetOutputConfig(cfg(ModePref::Shared, 0, ClockPref::Events)));
            token += 1;
            engine.send(EngineCommand::Load { path: path.clone(), start_ms: 0, token });
            let (m, err, delta) = drain(&engine, 1500);
            assert!(err.is_none(), "切回共享后应能恢复：{err:?}");
            assert_eq!(m.expect("恢复后应有 AudioMode").mode, "shared");
            assert_sound(delta, 1500, "报错后恢复");
        }
        (None, Some(m)) => {
            assert_eq!(m.mode, "exclusive", "必须独占却回落成 {}", m.mode);
            println!("  设备支持 → 独占 ✓");
        }
        (None, None) => panic!("既没有 Error 也没有 AudioMode，用户无从判断"),
    }

    // ---------- 8. null（不打开设备）----------
    println!("[8] null 输出");
    token += 1;
    engine.send(EngineCommand::SetOutputConfig(cfg(ModePref::Null, 0, ClockPref::Auto)));
    engine.send(EngineCommand::Load { path: path.clone(), start_ms: 0, token });
    let (m, err, delta) = drain(&engine, 1500);
    assert!(err.is_none(), "null 模式不该报错：{err:?}");
    let m = m.expect("null 模式必须回报 AudioMode");
    assert_eq!(m.mode, "null", "请求 null 却拿到 {}", m.mode);
    assert_eq!(m.clock, "null");
    // null 也按真实时间推进（这是它"位置照常走"的证据）
    assert_sound(delta, 1500, "null");

    // ---------- 9. null → 共享（切回来要能重新出声）----------
    println!("[9] null → 共享");
    token += 1;
    engine.send(EngineCommand::SetOutputConfig(cfg(ModePref::Shared, 0, ClockPref::Events)));
    engine.send(EngineCommand::Load { path: path.clone(), start_ms: 0, token });
    let (m, err, delta) = drain(&engine, 1500);
    assert!(err.is_none(), "从 null 切回共享不该报错：{err:?}");
    assert_eq!(m.expect("切回共享应有 AudioMode").mode, "shared");
    assert_sound(delta, 1500, "null → 共享");

    engine.shutdown();
    println!("\nMODE SMOKE OK（九项全过）");
}
