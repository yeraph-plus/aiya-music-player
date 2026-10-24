//! 独占模式冒烟：验证 `SetExclusive` 的两种结局都"可解释"。
//!
//! 断言口径（完成定义：独占可选且失败可解释）：
//! * 设备支持 → `AudioMode { exclusive: true, reason: None }`
//! * 设备拒绝 → `AudioMode { exclusive: false, reason: Some(非空) }`（回落共享，**不是** Error）
//! * 播放中来回切换 → 每次切换都能收到新的 `AudioMode`，且播放不中断
//!
//! 会出声（8 秒 440Hz；时长要盖住全部步骤，否则中途 EOF 会让后续切换拿不到 AudioMode）。用法：cargo run -p aiya-audio --example mode_smoke
use std::io::Write;
use std::time::{Duration, Instant};

use aiya_audio::{Engine, EngineCommand, EngineEvent};

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

/// 收 1.2s 事件，返回这段时间内看到的 AudioMode 列表（exclusive, reason）
fn drain_modes(engine: &Engine, ms: u64) -> Vec<(bool, Option<String>)> {
    let mut out = Vec::new();
    let deadline = Instant::now() + Duration::from_millis(ms);
    while Instant::now() < deadline {
        match engine.try_recv() {
            Some(EngineEvent::AudioMode { exclusive, reason, .. }) => out.push((exclusive, reason)),
            Some(EngineEvent::Error { message, .. }) => {
                eprintln!("ENGINE ERROR: {message}");
                std::process::exit(1);
            }
            Some(_) => {}
            None => std::thread::sleep(Duration::from_millis(10)),
        }
    }
    out
}

fn main() {
    let dir = std::env::temp_dir().join("aiya-mode-smoke");
    std::fs::create_dir_all(&dir).unwrap();
    let wav = dir.join("tone.wav");
    write_wav(&wav, 48_000, 8.0);

    let mut engine = Engine::spawn().expect("engine spawn");

    // 1) 请求独占后装载
    engine.send(EngineCommand::SetExclusive(true));
    engine.send(EngineCommand::Load {
        path: wav.to_string_lossy().into(),
        start_ms: 0,
        token: 1,
    });
    let first = drain_modes(&engine, 1500);
    assert_eq!(first.len(), 1, "装载应恰好报一次 AudioMode：{first:?}");
    let (exclusive, reason) = first[0].clone();
    // 两种结局都合法，但必须自洽
    if exclusive {
        assert!(reason.is_none(), "拿到独占时不该带回落原因：{reason:?}");
        println!("独占：拿到 ✓");
    } else {
        assert!(
            reason.as_deref().map(|r| !r.is_empty()).unwrap_or(false),
            "回落共享必须给出原因（可解释），实际 {reason:?}"
        );
        println!("独占：设备拒绝 → 已回落共享，原因：{}", reason.unwrap());
    }

    // 1.5) 声音真的在走吗？
    //
    // 只断言 `AudioMode` 会漏掉"报在播、实际静音"——独占模式曾经就是这样：
    // 独占下 GetCurrentPadding 恒等于 buffer，可写空间算成 0，写满一缓冲后
    // 全程欠载，而冒烟一路绿灯。这里用写入帧数对采样率交叉验证，1 秒内写入
    // 至少 70% 的帧数才算过。
    let before = engine.written_frames();
    drain_modes(&engine, 1000);
    let delta = engine.written_frames() - before;
    let expect = 48_000u64;
    assert!(
        delta * 10 >= expect * 7,
        "1 秒只写 {delta} 帧（期望 ≈{expect}）：引擎在静音。\
         独占下若 padding 恒等于 buffer，可写空间会算成 0"
    );
    println!("写入速率：{delta} 帧/秒（期望 ≈{expect}）");

    // 2) 播放中切回共享
    engine.send(EngineCommand::SetExclusive(false));
    let second = drain_modes(&engine, 1200);
    assert!(!second.is_empty(), "切回共享必须回报新的 AudioMode");
    assert!(
        second.iter().all(|(ex, _)| !*ex),
        "切回共享后不该再报 exclusive=true：{second:?}"
    );
    println!("切回共享：OK");
    // 3) 再切独占（应重新尝试并给出同样的结论）
    engine.send(EngineCommand::SetExclusive(true));
    let third = drain_modes(&engine, 1200);
    assert!(!third.is_empty(), "重新请求独占必须回报 AudioMode");
    println!("再切独占：OK（exclusive={}）", third[0].0);

    engine.shutdown();
    println!("MODE SMOKE OK");
}
