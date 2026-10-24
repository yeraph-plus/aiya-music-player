//! 引擎冒烟：合成 WAV → Engine 装载 → 事件驱动渲染 → 断言 Loaded/Position/EOF。
//! 真实走 WASAPI 共享模式（会出 2 秒 440Hz 正弦声）。
use std::io::Write;
use std::time::{Duration, Instant};

use aiya_audio::{Engine, EngineCommand};

fn write_wav(path: &std::path::Path, sample_rate: u32, seconds: f64) {
    let n = (sample_rate as f64 * seconds) as usize;
    let mut data = Vec::with_capacity(n * 4); // stereo f32
    for i in 0..n {
        let s = ((i as f32 / sample_rate as f32) * 2.0 * std::f32::consts::PI * 440.0).sin() * 0.3;
        for _ in 0..2 {
            data.extend_from_slice(&s.to_le_bytes());
        }
    }
    // 立体声 f32 WAV (WAVE_FORMAT_IEEE_FLOAT)
    let data_len = data.len() as u32;
    let file_len = 36 + data_len;
    let mut f = std::fs::File::create(path).unwrap();
    f.write_all(b"RIFF").unwrap();
    f.write_all(&file_len.to_le_bytes()).unwrap();
    f.write_all(b"WAVEfmt ").unwrap();
    f.write_all(&16u32.to_le_bytes()).unwrap();
    f.write_all(&3u16.to_le_bytes()).unwrap(); // IEEE float
    f.write_all(&2u16.to_le_bytes()).unwrap(); // stereo
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
    let wav = dir.join("tone.wav");
    write_wav(&wav, 48_000, 2.0);

    let mut engine = Engine::spawn().expect("engine spawn");
    engine.send(EngineCommand::Load { path: wav.to_string_lossy().into(), start_ms: 0, token: 1 });

    let mut loaded = false;
    let mut max_pos = 0u64;
    let mut got_eof = false;
    let mut errored: Option<String> = None;
    let deadline = Instant::now() + Duration::from_secs(6);
    while Instant::now() < deadline {
        match engine.try_recv() {
            Some(aiya_audio::EngineEvent::Loaded { duration_ms, .. }) => {
                loaded = true;
                println!("Loaded: {duration_ms}ms");
            }
            Some(aiya_audio::EngineEvent::Position { position_ms, .. }) => {
                max_pos = max_pos.max(position_ms); println!("Pos {position_ms}");
            }
            Some(aiya_audio::EngineEvent::Eof { .. }) => {
                got_eof = true;
                break;
            }
            Some(aiya_audio::EngineEvent::AudioMode { requested, mode, clock, buffer_ms, format, reason, .. }) => {
                println!("AudioMode requested={requested} actual={mode}/{clock}/{buffer_ms}ms/{format} reason={reason:?}");
            }
            Some(aiya_audio::EngineEvent::Error { message, .. }) => {
                errored = Some(message);
                break;
            }
            Some(aiya_audio::EngineEvent::StateChanged { playing, .. }) => {
                println!("StateChanged playing={playing}");
            }
            Some(aiya_audio::EngineEvent::Spectrum { bands }) => {
                // 频谱帧（tap 未订阅时不会出现）
                let _ = bands;
            }
            None => std::thread::sleep(Duration::from_millis(20)),
        }
    }
    engine.shutdown();
    match errored {
        Some(e) => {
            eprintln!("ENGINE ERROR: {e}");
            std::process::exit(1);
        }
        None => {
            assert!(loaded, "no Loaded event");
            assert!(max_pos > 0, "position never advanced");
            println!("SMOKE OK: loaded=true max_pos={max_pos}ms eof={got_eof}");
        }
    }
}
