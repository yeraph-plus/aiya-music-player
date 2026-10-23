//! 切歌冒烟：播放中发第二条 Load，断言新歌正常装载并播完。
//! 防回归：play_file 内部收到 Load 时 path 被丢弃 → 第二首歌永远不响（转圈 bug）。
use std::io::Write;
use std::time::{Duration, Instant};

use aiya_audio::{Engine, EngineCommand};

fn write_wav(path: &std::path::Path, sample_rate: u32, seconds: f64, freq: f32) {
    let n = (sample_rate as f64 * seconds) as usize;
    let mut data = Vec::with_capacity(n * 4);
    for i in 0..n {
        let s = ((i as f32 / sample_rate as f32) * 2.0 * std::f32::consts::PI * freq).sin() * 0.3;
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
    let dir = std::env::temp_dir().join("aiya-engine-smoke");
    std::fs::create_dir_all(&dir).unwrap();
    let wav_a = dir.join("tone_a.wav"); // 2 秒
    let wav_b = dir.join("tone_b.wav"); // 1 秒
    write_wav(&wav_a, 48_000, 2.0, 440.0);
    write_wav(&wav_b, 48_000, 1.0, 660.0);

    let mut engine = Engine::spawn().expect("engine spawn");
    engine.send(EngineCommand::Load(wav_a.to_string_lossy().into()));

    // 等第一首装载并实际开播
    let mut loaded_a = false;
    let deadline = Instant::now() + Duration::from_secs(4);
    while !loaded_a && Instant::now() < deadline {
        if let Some(aiya_audio::EngineEvent::Loaded { .. }) = engine.try_recv() {
            loaded_a = true;
        } else {
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    assert!(loaded_a, "first Load never completed");
    std::thread::sleep(Duration::from_millis(400)); // 让 A 真正进入渲染循环

    // 播放中切歌（旧代码：这条 Load 被吞 → 转圈）
    engine.send(EngineCommand::Load(wav_b.to_string_lossy().into()));

    // 断言 B 装载（duration=1000ms）且最终 EOF
    let mut loaded_b_ms = 0u64;
    let mut got_eof = false;
    let mut errored: Option<String> = None;
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        match engine.try_recv() {
            Some(aiya_audio::EngineEvent::Loaded { duration_ms }) => {
                loaded_b_ms = duration_ms;
                println!("Loaded(second): {duration_ms}ms");
            }
            Some(aiya_audio::EngineEvent::Eof) => {
                got_eof = true;
                break;
            }
            Some(aiya_audio::EngineEvent::Error { message }) => {
                errored = Some(message);
                break;
            }
            _ => std::thread::sleep(Duration::from_millis(10)),
        }
    }
    engine.shutdown();
    match errored {
        Some(e) => {
            eprintln!("ENGINE ERROR: {e}");
            std::process::exit(1);
        }
        None => {
            assert_eq!(loaded_b_ms, 1000, "second track must load (old bug: load swallowed)");
            assert!(got_eof, "second track never finished");
            println!("SWITCH SMOKE OK: mid-playback Load switched to 1000ms track and played to EOF");
        }
    }
}
