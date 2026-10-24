//! 启动起点冒烟：`Load { start_ms }` 是否真的"从未播放，而是从中间出声"。
//!
//! 对应 P1-12：Actor 算出了恢复点，适配层却 `let _ = start_ms` 丢掉，
//! `EngineCommand::Load` 里根本没有这个字段 —— 于是"记住进度"从不生效，
//! UI 显示 30:00、耳朵听到的是第 0 秒。
//!
//! 断言方式：装载 8 秒素材、起点 5000ms。
//! * 第一条 Position 必须已经 ≥ 4800（说明解码器确实 seek 过去了，
//!   而不是从 0 开始播）；
//! * 播完的时刻必须明显短于"从头播"（~8s），证明没有先把前 5 秒放一遍。
//!
//! 跑：cargo run -p aiya-audio --example start_offset_smoke

use std::io::Write;
use std::time::{Duration, Instant};

use aiya_audio::{Engine, EngineCommand};

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
    const START_MS: u64 = 5_000;
    let dir = std::env::temp_dir().join("aiya-start-offset-smoke");
    std::fs::create_dir_all(&dir).unwrap();
    let wav = dir.join("八秒.wav"); // 顺带覆盖中文文件名
    write_wav(&wav, 48_000, 8.0);

    let mut engine = Engine::spawn().expect("engine spawn");
    let started = Instant::now();
    engine.send(EngineCommand::Load {
        path: wav.to_string_lossy().into(),
        start_ms: START_MS,
        token: 1,
    });

    let mut duration = 0u64;
    let mut first_pos: Option<u64> = None;
    let mut max_pos = 0u64;
    let mut eof = false;
    let mut errored: Option<String> = None;
    let deadline = Instant::now() + Duration::from_secs(12);
    while Instant::now() < deadline {
        match engine.try_recv() {
            Some(aiya_audio::EngineEvent::Loaded { duration_ms, token }) => {
                assert_eq!(token, 1, "Loaded 必须回显 Load 的 token");
                duration = duration_ms;
                println!("Loaded: {duration_ms}ms（起点请求 {START_MS}ms）");
            }
            Some(aiya_audio::EngineEvent::Position { position_ms, token }) => {
                assert_eq!(token, 1);
                if first_pos.is_none() {
                    first_pos = Some(position_ms);
                    println!("首个 Position: {position_ms}ms");
                }
                max_pos = max_pos.max(position_ms);
            }
            Some(aiya_audio::EngineEvent::Eof { .. }) => {
                eof = true;
                break;
            }
            Some(aiya_audio::EngineEvent::AudioMode { requested, mode, clock, buffer_ms, format, reason, .. }) => {
                println!("AudioMode requested={requested} actual={mode}/{clock}/{buffer_ms}ms/{format} reason={reason:?}");
            }
            Some(aiya_audio::EngineEvent::Error { message, .. }) => {
                errored = Some(message);
                break;
            }
            Some(_) => {}
            None => std::thread::sleep(Duration::from_millis(20)),
        }
    }
    engine.shutdown();
    let wall = started.elapsed();

    if let Some(e) = errored {
        eprintln!("ENGINE ERROR: {e}");
        std::process::exit(1);
    }
    let first = first_pos.expect("没有 Position 事件");
    // 允许 200ms 余量（重采样/设备缓冲带来的对齐误差），但绝不允许"从 0 开始"
    assert!(
        first + 200 >= START_MS,
        "第一条 Position 是 {first}ms，起点 {START_MS}ms 没生效（start_ms 被丢弃 → 从头播）"
    );
    assert!(duration >= 7_500, "素材时长异常: {duration}ms");
    assert!(eof, "没播完（可能卡住）");
    assert!(max_pos >= START_MS, "位置没有推进过起点");
    // 从 5s 起播 8s 素材 ≈ 3s；从头播会多出 5 秒。给足余量取 6s 上限。
    assert!(
        wall < Duration::from_secs(6),
        "实际耗时 {wall:?} 太长，说明先从头播了一遍（start_ms 失效）"
    );
    println!(
        "START OFFSET SMOKE OK: 起点生效 first_pos={first}ms wall={:.2}s max_pos={max_pos}ms",
        wall.as_secs_f32()
    );
}
