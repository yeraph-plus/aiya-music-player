//! 解码器测试，三块：
//!
//! 1. **手工构造 WAV**（RIFF 十几行就能拼）：验 symphonia 的 wav demuxer +
//!    pcm 解码器 + seek 语义。
//! 2. **错误分类**（docs/22 G1）：OGG-Opus 可以纯手工拼出来（容器页 +
//!    OpusHead + OpusTags + 一页假数据，CRC32 是 15 行代码），正好走完整
//!    probe→make 失败路径，锁定 `codecUnsupported` 的分类与格式名。
//! 3. **真文件 fixture**（Vorbis / ALAC / AAC / MP3 / AIFF）：熵编码拼不出来，
//!    用 `tests/fixtures/gen_fixtures.py` 一次性生成并入库（约 111KB）。这几个
//!    格式此前"编进来了但没有任何样本跑过"——只有真解码一遍才算验过。
//!    ALAC/AAC 那份还顺手暴露了 MP4 容器不报通道数 → SampleBuffer 除零 panic。

use std::io::Write;
use std::path::{Path, PathBuf};

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
    while let Some(samples) = dec.next_packet().unwrap() {
        total_samples += samples.len();
        if samples.iter().any(|s| s.abs() > 0.01) {
            saw_signal = true;
        }
        if dec.position_ms() > 200 {
            break;
        }
    }
    assert!(saw_signal, "decoded signal present");
    assert!(total_samples > 0);
    assert!(
        dec.position_ms() <= 300,
        "position sane: {}",
        dec.position_ms()
    );

    // seek 到 1s（正弦段内）
    dec.seek_ms(1_000).unwrap();
    if let Some(samples) = dec.next_packet().unwrap() {
        assert!(
            samples.iter().any(|s| s.abs() > 0.01),
            "signal after seek into tone section"
        );
    }
    assert!(
        dec.position_ms() >= 900,
        "position after seek: {}",
        dec.position_ms()
    );

    // seek 到 3s（静音段：信号只占前 2.5s）
    dec.seek_ms(3_000).unwrap();
    if let Some(samples) = dec.next_packet().unwrap() {
        assert!(
            samples.iter().all(|s| s.abs() < 0.01),
            "silence after seek into quiet section"
        );
    }
    assert!(
        dec.position_ms() >= 2_900,
        "position after seek 2: {}",
        dec.position_ms()
    );
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
    f.write_all(&ogg_page(0x00, 0x1234, 2, &[0x08; 32]))
        .unwrap();
}

#[test]
fn unsupported_codec_classified_as_codec_unsupported() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("opus.ogg");
    write_test_ogg_opus(&path);

    // features 里没有 opus（docs/22 D1 明确不做），probe 认识 OGG 容器、
    // make() 阶段失败 → 必须报 codecUnsupported 且带可读格式名
    let err = DecodedStream::open_file(&path).unwrap_err();
    assert_eq!(
        err.kind(),
        AudioErrorKind::CodecUnsupported,
        "实际错误：{err}"
    );
    assert!(
        err.to_string().contains("Opus"),
        "错误消息要带格式名：{err}"
    );
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

/* ------------- 真文件 fixture：Vorbis / ALAC / AAC / MP3 / AIFF ------------- */

/// fixture 的约定（与 gen_fixtures.py 顶部的常量一一对应）：
/// 44.1kHz 单声道，0.6s，前 0.3s 是 1kHz 正弦（幅度 0.5），后 0.3s 静音。
const FX_SR: u32 = 44_100;
const FX_FRAMES: usize = 26_460; // 600ms × 44.1kHz
const FX_TONE_END: usize = 300 * (FX_SR as usize) / 1000;
const FX_TAIL_START: usize = 350 * (FX_SR as usize) / 1000;

/// 每个 fixture 该报多少时长，以及**为什么是这个数**。
///
/// 这几个数字是这批 fixture 的直接产出：偏差全部来自容器/编码器口径，
/// 但每一个都得能解释——否则下次有人（包括未来的我）会把它当 bug 重查一遍：
///
/// - vorbis 600：gapless 打开后按末页 granule 算，精确
/// - alac   600：MP4 的 stts 精确
/// - mp3    600：LAME 头 + gapless trim（576 样本编码器延迟）后精确
/// - aiff   600：裸 PCM，data 字节数 / 帧长
/// - aac    **623**：MP4 的 elst（prime/padding 裁剪表）symphonia 0.5.5 不读，
///   多出 1188 样本。已知偏差——AAC 的进度条总长会多 4%，末尾多 27ms 静音；
///   真要修得等 0.6（或者自己解析 elst），不在本批范围
const FX_EXPECT: &[(&str, u64)] = &[
    ("tone-vorbis.ogg", 600),
    ("tone-alac.m4a", 600),
    ("tone-mp3.mp3", 600),
    ("tone-pcm.aiff", 600),
    ("tone-aac.m4a", 623),
];

fn fixture(name: &str) -> PathBuf {
    let p = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name);
    assert!(
        p.exists(),
        "缺 fixture {p:?}——在 tests/fixtures/ 下跑 gen_fixtures.py 生成（见那边的文件头）"
    );
    p
}

/// 把一条流读到底：返回 (总帧数, 正弦段峰值, 静音段峰值)。
/// 取峰值而不是"有没有信号"：这几个都是有损/无损压缩，边界处有窗函数
/// 过渡，只看 bool 会把"几乎全静音"的坏解码也判成通过。
fn decode_stats(path: &Path) -> (usize, f32, f32) {
    let mut dec = DecodedStream::open_file(path)
        .unwrap_or_else(|e| panic!("{path:?} 打不开：{e}（kind={:?}）", e.kind()));
    // 通道数在这里就是硬断言：MP4 容器不报通道数时，报错会变成 2ch
    // （曾经直接除零 panic），单声道 fixture 正好锁死这条
    assert_eq!(dec.sample_rate(), FX_SR, "{path:?} 采样率");
    assert_eq!(dec.channels(), 1, "{path:?} 声道数");

    let (mut frames, mut tone_peak, mut tail_peak) = (0usize, 0f32, 0f32);
    while let Some(samples) = dec.next_packet().unwrap() {
        for (i, v) in samples.iter().enumerate() {
            let f = frames + i; // 单声道：sample 下标 == 帧下标
            if f < FX_TONE_END {
                tone_peak = tone_peak.max(v.abs());
            } else if f >= FX_TAIL_START {
                tail_peak = tail_peak.max(v.abs());
            }
        }
        frames += samples.len();
    }
    (frames, tone_peak, tail_peak)
}

/// 共用的验收口径：**真的解出 0.6s、真的有两段不同内容**。
///
/// 帧数写成"不许少、最多多两包"而不是 ±1%：Vorbis 的 OGG demuxer 在 0.5.5
/// 里没把尾部 end-trim 交到解码器手上（实测多 740 帧≈17ms），AAC 更是整段
/// prime/padding 都在（多 1188 帧）。我们不按 duration 硬裁——那个值对没有
/// Xing 头的 MP3 是估算的，硬裁会切掉真实音频。少一帧就说明丢音，必须失败。
fn check_fixture(name: &str) {
    let path = fixture(name);
    let (frames, tone_peak, tail_peak) = decode_stats(&path);
    assert!(
        (FX_FRAMES..=FX_FRAMES + 2 * 1024).contains(&frames),
        "{name} 解码出 {frames} 帧，期望 {FX_FRAMES}~{}",
        FX_FRAMES + 2 * 1024
    );
    assert!(
        tone_peak > 0.3,
        "{name} 正弦段峰值只有 {tone_peak}——信号没解出来"
    );
    assert!(
        tail_peak < 0.02,
        "{name} 静音段峰值 {tail_peak}——尾部不是静音，解错了"
    );
}

#[test]
fn vorbis_fixture_decodes() {
    check_fixture("tone-vorbis.ogg");
}

#[test]
fn alac_fixture_decodes() {
    // isomp4 demuxer + alac 解码器：这条曾经在打开阶段就 panic（容器不报通道数）
    check_fixture("tone-alac.m4a");
}

#[test]
fn aac_fixture_decodes() {
    // 同一个 MP4 容器、另一条编码器路径（AAC 才是 Apple 生态的主力格式）
    check_fixture("tone-aac.m4a");
}

#[test]
fn mp3_fixture_decodes() {
    // libmp3lame：这条验 gapless 的编码器延迟裁剪（前 576 样本静音）
    check_fixture("tone-mp3.mp3");
}

#[test]
fn aiff_fixture_decodes() {
    // AIFF = 大端 PCM：这条同时验字节序（错的话峰值会变成噪声或者 0）
    check_fixture("tone-pcm.aiff");
}

/// 五个格式都必须报告可 seek，且 seek 到位后内容对得上。
/// 单独测是因为 seek 走各自不同的索引（Vorbis 用页 granule、MP4 用 stts、
/// AIFF 直接算字节偏移）——解码对不代表能拖。
#[test]
fn all_fixtures_seek() {
    for (name, _) in FX_EXPECT {
        let mut dec = DecodedStream::open_file(&fixture(name)).unwrap();
        assert!(dec.seekable(), "{name} 报不可 seek");

        // 拖进正弦段：必须还有信号
        dec.seek_ms(100).unwrap();
        let mut tone_peak = 0f32;
        for _ in 0..8 {
            match dec.next_packet().unwrap() {
                Some(s) => tone_peak = tone_peak.max(s.iter().fold(0f32, |a, v| a.max(v.abs()))),
                None => break,
            }
        }
        assert!(
            tone_peak > 0.3,
            "{name} seek 到 100ms 后峰值只有 {tone_peak}"
        );

        // 拖进静音段：必须安静。容差比 check_fixture 松，因为 seek 落点可能
        // 落在窗函数过渡里，前一两包带一点余音属于正常（这也是我们不开
        // "按 duration 硬裁"的同一个理由：边界本来就模糊）。
        dec.seek_ms(500).unwrap();
        let mut tail_peak = 0f32;
        for _ in 0..8 {
            match dec.next_packet().unwrap() {
                Some(s) => tail_peak = tail_peak.max(s.iter().fold(0f32, |a, v| a.max(v.abs()))),
                None => break,
            }
        }
        assert!(
            tail_peak < 0.05,
            "{name} seek 到 500ms 后峰值 {tail_peak}——拖到静音区还在响"
        );

        // seek 之后位置只能前进：Ogg/MP4 的落点包可能早于目标，吐出去
        // 就是"拖到 5:00 先听到 4:59"，所以 next_packet 会丢到目标之后
        assert!(
            dec.position_ms() >= 500,
            "{name} seek 到 500ms 之后位置回退到 {}ms",
            dec.position_ms()
        );
    }
}

/// duration_ms：能算就得算对。这个值直接决定进度条总长和"拖到最后 5 秒"
/// 能不能用，报 0 就等于没有进度条。每个格式的期望值与理由见 [`FX_EXPECT`]。
#[test]
fn fixture_durations_are_known() {
    for (name, expect) in FX_EXPECT {
        let dec = DecodedStream::open_file(&fixture(name)).unwrap();
        let d = dec.duration_ms();
        assert!(
            d.abs_diff(*expect) <= 15,
            "{name} duration_ms = {d}，期望 {expect}±15（差太多就是容器时长没读对）"
        );
    }
}
