// 生成 3 个不同音高的测试 WAV（5 秒各，带音量渐弱尾）
use std::io::Write;
use std::path::Path;

fn write_wav(path: &Path, sample_rate: u32, seconds: f64, freq: f32) {
    let n = (sample_rate as f64 * seconds) as usize;
    let mut data = Vec::with_capacity(n * 4);
    for i in 0..n {
        let t = i as f32 / sample_rate as f32;
        let env = (1.0 - i as f32 / n as f32).min(1.0);
        let s = (t * 2.0 * std::f32::consts::PI * freq).sin() * 0.3 * env;
        for _ in 0..2 { data.extend_from_slice(&s.to_le_bytes()); }
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
    let dir = Path::new("D:/MUSIC_Player/test-music");
    std::fs::create_dir_all(dir).unwrap();
    write_wav(&dir.join("低音测试 220Hz.wav"), 44_100, 5.0, 220.0);
    write_wav(&dir.join("中音测试 440Hz.wav"), 48_000, 5.0, 440.0);
    write_wav(&dir.join("高音测试 880Hz.wav"), 44_100, 5.0, 880.0);
    println!("done");
}
