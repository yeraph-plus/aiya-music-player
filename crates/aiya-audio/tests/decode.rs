//! 解码器测试：手工构造 WAV（symphonia 的 wav demuxer + pcm 解码器）。
//! MP3/FLAC 路径结构相同（Probe→decode→seek），真文件在开窗验证时覆盖。
//!
//! 错误分类测试（docs/22 G1）也在这里：没有 ffmpeg 造不出 Vorbis/ALAC 之外的
//! 高频"收了放不出"样本，但 **OGG-Opus 可以纯手工拼出来**（容器页 + OpusHead
//! + OpusTags + 一页假数据，CRC32 是 15 行代码）——这正好走完整 probe→make
//! 失败路径，锁定 codecUnsupported 的分类与格式名，不需要任何二进制 fixture。

use std::io::Write;
use std::path::Path;

use aiya_audio::{AudioErrorKind, DecodedStream};

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

    let mut dec = DecodedStream::open_file(&path).unwrap();
    assert_eq!(dec.sample_rate(), 44_100);
    assert_eq!(dec.channels(), 1);
    assert_eq!(dec.duration_ms(), 5_000);

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

/* ---------------- 错误分类（docs/22 G1） ---------------- */

/// Ogg CRC32：多项式 0x04c11db7，初值 0，MSB 先行，无终值异或
/// （与 symphonia-core checksum/crc32.rs 同参，已在本地源码核实）。
fn ogg_crc(data: &[u8]) -> u32 {
    let mut crc: u32 = 0;
    for &b in data {
        crc ^= u32::from(b) << 24;
        for _ in 0..8 {
            crc = if crc & 0x8000_0000 != 0 {
                (crc << 1) ^ 0x04c1_1db7
            } else {
                crc << 1
            };
        }
    }
    crc
}

/// 拼一个 OGG 页。`payload` 必须每段 ≤255 字节（这里全部单段）。
fn ogg_page(header_type: u8, serial: u32, seq: u32, payload: &[u8]) -> Vec<u8> {
    assert!(payload.len() <= 255, "测试 fixture 单段就够");
    let mut page = Vec::with_capacity(27 + 1 + payload.len());
    page.extend_from_slice(b"OggS");
    page.push(0); // version
    page.push(header_type);
    page.extend_from_slice(&0u64.to_le_bytes()); // granule position
    page.extend_from_slice(&serial.to_le_bytes());
    page.extend_from_slice(&seq.to_le_bytes());
    page.extend_from_slice(&[0; 4]); // CRC 占位（计算时必须为 0）
    page.push(1); // segment count
    page.push(payload.len() as u8); // lacing value
    page.extend_from_slice(payload);
    let crc = ogg_crc(&page);
    page[22..26].copy_from_slice(&crc.to_le_bytes());
    page
}

/// 最小 OGG-Opus：BOS(OpusHead) + OpusTags + 一页假数据。
/// symphonia 的 OGG demuxer 在 try_new 里会把三页全部读完（等首个数据包），
/// 轨道编码型别在此期间已注册为 CODEC_TYPE_OPUS。
fn write_test_ogg_opus(path: &Path) {
    let opus_head: &[u8] = &[
        // "OpusHead" + version + channels + preskip + sample rate + gain + mapping
        b'O', b'p', b'u', b's', b'H', b'e', b'a', b'd', 1, 2, 0, 0, 0x80, 0xBB, 0, 0, 0, 0, 0,
    ];
    let mut opus_tags = Vec::new();
    opus_tags.extend_from_slice(b"OpusTags");
    opus_tags.extend_from_slice(&0u32.to_le_bytes()); // vendor 长度
    opus_tags.extend_from_slice(&0u32.to_le_bytes()); // comment 数
    let mut f = std::fs::File::create(path).unwrap();
    f.write_all(&ogg_page(0x02, 0x1234, 0, opus_head)).unwrap();
    f.write_all(&ogg_page(0x00, 0x1234, 1, &opus_tags)).unwrap();
    f.write_all(&ogg_page(0x00, 0x1234, 2, &[0x08; 32])).unwrap();
}

#[test]
fn unsupported_codec_classified_as_codec_unsupported() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("opus.ogg");
    write_test_ogg_opus(&path);

    // features 里没有 opus（docs/22 D1 明确不做），probe 认识 OGG 容器、
    // make() 阶段失败 → 必须报 codecUnsupported 且带可读格式名
    let err = DecodedStream::open_file(&path).unwrap_err();
    assert_eq!(err.kind(), AudioErrorKind::CodecUnsupported, "实际错误：{err}");
    assert!(err.to_string().contains("Opus"), "错误消息要带格式名：{err}");
}

#[test]
fn garbage_file_classified_as_decode_failed() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("broken.ogg");
    // 长度超过 probe 扫描窗的纯垃圾：容器都认不出 → decodeFailed
    std::fs::write(&path, vec![b'x'; 8192]).unwrap();

    let err = DecodedStream::open_file(&path).unwrap_err();
    assert_eq!(err.kind(), AudioErrorKind::Decode, "实际错误：{err}");
}

#[test]
fn missing_file_classified_as_file_missing() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("no-such-file.mp3");
    let err = DecodedStream::open_file(&path).unwrap_err();
    assert_eq!(err.kind(), AudioErrorKind::FileMissing);
}
