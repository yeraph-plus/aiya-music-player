//! 解码器测试：手工构造 WAV（symphonia 的 wav demuxer + pcm 解码器）。
//! MP3/FLAC 路径结构相同（Probe→decode→seek），真文件在开窗验证时覆盖。

use std::io::Write;
use std::path::Path;

use aiya_audio::decoder::Decoder;

/// 16-bit PCM WAV，单声道，指定采样率与秒数，1kHz 正弦波（首半）+ 静音（后半）。
fn write_test_wav(path: &Path, sample_rate: u32, seconds: f64) {
    let n = (sample_rate as f64 * seconds) as usize;
    let mut data = Vec::with_capacity(n * 2);
    for i in 0..n {
        let sample = if i < n / 2 {
            ((i as f32 / sample_rate as f32) * 2.0 * std::f32::consts::PI * 1000.0).sin() * 0.5
        } else {
            0.0
        };
        let v = (sample.clamp(-1.0, 1.0) * 32767.0) as i16;
        data.extend_from_slice(&v.to_le_bytes());
    }
    let data_len = data.len() as u32;
    let file_len = 36 + data_len;
    let mut f = std::fs::File::create(path).unwrap();
    f.write_all(b"RIFF").unwrap();
    f.write_all(&file_len.to_le_bytes()).unwrap();
    f.write_all(b"WAVEfmt ").unwrap();
    f.write_all(&16u32.to_le_bytes()).unwrap(); // fmt chunk size
    f.write_all(&1u16.to_le_bytes()).unwrap(); // PCM
    f.write_all(&1u16.to_le_bytes()).unwrap(); // mono
    f.write_all(&sample_rate.to_le_bytes()).unwrap();
    f.write_all(&(sample_rate * 2).to_le_bytes()).unwrap(); // byte rate
    f.write_all(&2u16.to_le_bytes()).unwrap(); // block align
    f.write_all(&16u16.to_le_bytes()).unwrap(); // bits
    f.write_all(b"data").unwrap();
    f.write_all(&data_len.to_le_bytes()).unwrap();
    f.write_all(&data).unwrap();
}

#[test]
fn wav_decode_positions_and_seek() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("t.wav");
    write_test_wav(&path, 44_100, 5.0); // 5 秒

    let mut dec = Decoder::open(&path).unwrap();
    assert_eq!(dec.sample_rate, 44_100);
    assert_eq!(dec.channels, 1);
    assert_eq!(dec.duration_ms, 5_000);

    // 读前 100ms：前 50ms 有信号、50ms 后（本测试的 1kHz 段只占前 2.5s，所以 100ms 内都有信号）
    let mut saw_signal = false;
    let mut total_samples = 0usize;
    loop {
        match dec.next_packet().unwrap() {
            Some(samples) => {
                total_samples += samples.len();
                if samples.iter().any(|s| s.abs() > 0.01) {
                    saw_signal = true;
                }
                if dec.position_ms() > 200 {
                    break;
                }
            }
            None => break,
        }
    }
    assert!(saw_signal, "decoded signal present");
    assert!(total_samples > 0);
    assert!(dec.position_ms() <= 300, "position sane: {}", dec.position_ms());

    // seek 到 1s（正弦段内）
    dec.seek_ms(1_000).unwrap();
    if let Some(samples) = dec.next_packet().unwrap() {
        assert!(samples.iter().any(|s| s.abs() > 0.01), "signal after seek into tone section");
    }
    assert!(dec.position_ms() >= 900, "position after seek: {}", dec.position_ms());

    // seek 到 3s（静音段：信号只占前 2.5s）
    dec.seek_ms(3_000).unwrap();
    if let Some(samples) = dec.next_packet().unwrap() {
        assert!(samples.iter().all(|s| s.abs() < 0.01), "silence after seek into quiet section");
    }
    assert!(dec.position_ms() >= 2_900, "position after seek 2: {}", dec.position_ms());
}
