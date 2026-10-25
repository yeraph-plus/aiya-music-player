//! 解码：媒体源 → 交错 f32 样本流（`DecodedStream`）。
//!
//! 为什么 f32：WASAPI 共享模式引擎 mix format 通常是 f32，
//! 且 AUTOCONVERTPCM 打开后引擎负责源格式 → 引擎格式转换（含 SRC）。
//! 位置时钟：包时间戳（ts）÷ 采样率 = 毫秒，seek 后以 seek 目标为锚。
//!
//! # 后端扩展指南（docs/22 D4 插入缝，G1 落锤）
//!
//! engine / WASAPI 输出 / 队列**永远只认 [`DecodedStream`]**：构造收敛在
//! [`DecodedStream::open_with_backend`] 单入口，后端用 [`DecodeBackend`] 枚举
//! 表达。engine.rs 里没有任何 probe/codec 知识。
//!
//! * **Symphonia 特性怎么加**：只动根 `Cargo.toml` 的 symphonia features
//!   （docs/22 D1 只开主流格式；新格式 = 加 feature + 本文件 codec 名表补一行）。
//! * **mpv 怎么插**：将来加 `DecodeBackend::Mpv`，实现为子进程
//!   `mpv --no-video --ao=pcm --audio-format=s32le --really-quiet <file>`
//!   读 stdout PCM 管道，包装成同一个 `DecodedStream` 形状（sample_rate /
//!   channels / duration_ms / next_packet / seek_ms / position_ms）。mpv 场景
//!   `duration_ms`/`seek_ms` 由 mpv 的属性/命令应答填充，`next_packet` 返回
//!   定长 PCM 块。engine/output/queue 零改动。
//! * **错误怎么分类**：probe 失败（容器不认识/文件坏）→ `Decode`；
//!   probe 成功但 `get_codecs().make()` 失败 → `CodecUnsupported { codec }`
//!   （在产生处查 [`codec_display_name`]，说人话，UI 直接展示）。

use std::fs::File;
use std::path::Path;

use symphonia::core::audio::{SampleBuffer, SignalSpec};
use symphonia::core::codecs::{
    Decoder as SymphoniaDecoder, DecoderOptions, CodecType, CODEC_TYPE_NULL,
};
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::{FormatOptions, FormatReader, SeekMode, SeekTo};
use symphonia::core::io::{MediaSource, MediaSourceStream};
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;
use symphonia::core::units::Time;

use crate::AudioError;

/// 解码后端。今天只有 Symphonia；mpv 将来从这里插入（见模块头注释），
/// engine 对后端无感知——这是"想引入 mpv 也可以从中插入"的全部保障。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecodeBackend {
    /// 纯 Rust 进程内解码（docs/22 D1：主流格式，无 ffmpeg / 无内置 mpv）
    Symphonia,
}

/// 解码后的流：逐包交错 f32 + 元数据 + seek。
///
/// 统一形状是 D4 的契约面——将来任何后端（Symphonia / mpv 子进程 / 远程流）
/// 都产出这个结构，engine 的渲染循环与 WASAPI 输出永远零改动。
pub struct DecodedStream {
    format: Box<dyn FormatReader>,
    decoder: Box<dyn SymphoniaDecoder>,
    track_id: u32,
    pub sample_rate: u32,
    pub channels: usize,
    /// 流信息推算的总时长（无标签时也能给进度条）
    pub duration_ms: u64,
    /// 当前解码位置（毫秒），随包时间戳推进
    position_ms: u64,
    /// 交错样本复用缓冲
    sample_buf: SampleBuffer<f32>,
    eof: bool,
}

impl std::fmt::Debug for DecodedStream {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // 内部的 FormatReader/Decoder 是 trait object，不实现 Debug；
        // 元数据足够定位"哪条流、走到哪"。
        f.debug_struct("DecodedStream")
            .field("sample_rate", &self.sample_rate)
            .field("channels", &self.channels)
            .field("duration_ms", &self.duration_ms)
            .field("position_ms", &self.position_ms)
            .field("eof", &self.eof)
            .finish()
    }
}

impl DecodedStream {
    /// 便捷入口：打开本地文件（engine 的全部用法）。
    pub fn open_file(path: &Path) -> Result<Self, AudioError> {
        let src = File::open(path).map_err(AudioError::Io)?;
        let mut hint = Hint::new();
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            hint.with_extension(ext);
        }
        Self::open_with_backend(Box::new(src), &hint, DecodeBackend::Symphonia)
    }

    /// **唯一构造入口**（docs/22 §2 D4）：engine 与未来调用方都从这里进，
    /// 后端选择只发生在此处。mpv 后端不吃 `MediaSource`（是子进程管道），
    /// 届时此签名会多一个入口或参数——但 `DecodedStream` 形状与 engine 不变。
    pub fn open_with_backend(
        src: Box<dyn MediaSource>,
        hint: &Hint,
        backend: DecodeBackend,
    ) -> Result<Self, AudioError> {
        match backend {
            DecodeBackend::Symphonia => Self::open_symphonia(src, hint),
        }
    }

    fn open_symphonia(src: Box<dyn MediaSource>, hint: &Hint) -> Result<Self, AudioError> {
        let mss = MediaSourceStream::new(src, Default::default());

        let probed = symphonia::default::get_probe()
            .format(hint, mss, &FormatOptions::default(), &MetadataOptions::default())
            // probe 失败 = 容器不认识 / 文件损坏 → decodeFailed（docs/22 D3）
            .map_err(|e| AudioError::Decode(e.to_string()))?;

        let format = probed.format;
        let track = format
            .tracks()
            .iter()
            .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
            .ok_or_else(|| AudioError::Decode("no audio track".into()))?
            .clone();
        let track_id = track.id;

        let sample_rate = track.codec_params.sample_rate.unwrap_or(44_100);
        let channels = track
            .codec_params
            .channels
            .map(|c| c.count())
            .unwrap_or(2);
        let duration_ms = track
            .codec_params
            .n_frames
            .map(|f| (f as u64 * 1000) / (sample_rate as u64).max(1))
            .unwrap_or(0);

        let codec = track.codec_params.codec;
        let decoder = symphonia::default::get_codecs()
            .make(&track.codec_params, &DecoderOptions::default())
            // probe 认识容器、但编码器没编进 features（Opus/WMA/APE/WavPack/DSD…）
            // → 单独分类：用户该做的是换文件/转码，不是重试（docs/22 D3）
            .map_err(|e| match codec {
                CODEC_TYPE_NULL => AudioError::Decode(e.to_string()),
                _ => AudioError::CodecUnsupported { codec: codec_display_name(codec) },
            })?;

        // 复用缓冲容量：P1——此前按 100ms 固定分配，但 FLAC 允许单包最大
        // 65535 帧，容量不足时 copy_interleaved_ref 的 assert 直接 panic
        // （引擎线程死亡、音频静默中断）。统一给到 65535 帧下限
        // （2ch f32 ≈ 512KB，一次性分配可忽略），并尊重轨道声明的更大值。
        let max_frames = track.codec_params.max_frames_per_packet.unwrap_or(0).max(65_535);
        let spec = SignalSpec::new(sample_rate, track.codec_params.channels.unwrap_or_default());
        let sample_buf = SampleBuffer::<f32>::new(max_frames as u64, spec);

        Ok(Self {
            format,
            decoder,
            track_id,
            sample_rate,
            channels,
            duration_ms,
            position_ms: 0,
            sample_buf,
            eof: false,
        })
    }

    pub fn position_ms(&self) -> u64 {
        self.position_ms
    }

    /// 下一包的交错 f32 样本。EOF 返回 Ok(None)。
    /// 返回的切片在下次调用前有效。
    pub fn next_packet(&mut self) -> Result<Option<&[f32]>, AudioError> {
        if self.eof {
            return Ok(None);
        }
        loop {
            match self.format.next_packet() {
                Ok(packet) => {
                    if packet.track_id() != self.track_id {
                        continue; // 附加流
                    }
                    match self.decoder.decode(&packet) {
                        Ok(decoded) => {
                            self.sample_buf.copy_interleaved_ref(decoded);
                            self.position_ms = packet.ts().saturating_mul(1000)
                                / (self.sample_rate as u64).max(1);
                            return Ok(Some(self.sample_buf.samples()));
                        }
                        Err(SymphoniaError::DecodeError(_)) => continue, // 坏包跳过
                        Err(e) => return Err(AudioError::Decode(e.to_string())),
                    }
                }
                Err(SymphoniaError::IoError(err))
                    if err.kind() == std::io::ErrorKind::UnexpectedEof =>
                {
                    self.eof = true;
                    return Ok(None);
                }
                Err(SymphoniaError::ResetRequired) => {
                    self.eof = true;
                    return Ok(None);
                }
                Err(e) => return Err(AudioError::Decode(e.to_string())),
            }
        }
    }

    /// 精确 seek 到毫秒位置。
    pub fn seek_ms(&mut self, ms: u64) -> Result<(), AudioError> {
        // Time 的整数 From 是"秒"；毫秒要用 new(seconds, frac)
        let target = Time::new(ms / 1000, (ms % 1000) as f64 / 1000.0);
        self.format
            .seek(SeekMode::Accurate, SeekTo::Time { time: target, track_id: Some(self.track_id) })
            .map_err(|e| AudioError::Decode(e.to_string()))?;
        self.position_ms = ms;
        self.eof = false;
        Ok(())
    }
}

/// `CodecType` → 用户可读格式名（docs/22 D3：错误信息说人话）。
///
/// 只需要覆盖"会被装进库但解码不了"的那些；能正常解码的格式永远走不到
/// 这里。没收录的落 Debug 格式兜底，至少诊断页里能对回常量。
fn codec_display_name(codec: CodecType) -> String {
    use symphonia::core::codecs::*;
    match codec {
        CODEC_TYPE_OPUS => "Opus".into(),
        CODEC_TYPE_WMA => "WMA".into(),
        CODEC_TYPE_MONKEYS_AUDIO => "Monkey's Audio (APE)".into(),
        CODEC_TYPE_WAVPACK => "WavPack".into(),
        CODEC_TYPE_TTA => "TTA".into(),
        CODEC_TYPE_SPEEX => "Speex".into(),
        CODEC_TYPE_MUSEPACK => "Musepack".into(),
        CODEC_TYPE_ATRAC1 => "ATRAC1".into(),
        CODEC_TYPE_ATRAC3 => "ATRAC3".into(),
        CODEC_TYPE_ATRAC3PLUS => "ATRAC3+".into(),
        CODEC_TYPE_ATRAC9 => "ATRAC9".into(),
        CODEC_TYPE_DCA => "DTS".into(),
        CODEC_TYPE_EAC3 => "E-AC-3".into(),
        CODEC_TYPE_AC4 => "AC-4".into(),
        CODEC_TYPE_MP1 => "MPEG Layer I".into(),
        CODEC_TYPE_MP2 => "MPEG Layer II".into(),
        CODEC_TYPE_VORBIS => "Vorbis".into(),
        CODEC_TYPE_ALAC => "ALAC".into(),
        CODEC_TYPE_ADPCM_G722
        | CODEC_TYPE_ADPCM_G726
        | CODEC_TYPE_ADPCM_G726LE
        | CODEC_TYPE_ADPCM_MS
        | CODEC_TYPE_ADPCM_IMA_WAV
        | CODEC_TYPE_ADPCM_IMA_QT => "ADPCM".into(),
        CODEC_TYPE_PCM_ALAW => "A-law PCM".into(),
        CODEC_TYPE_PCM_MULAW => "μ-law PCM".into(),
        other => format!("{other:?}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use symphonia::core::codecs::*;

    #[test]
    fn unsupported_codecs_have_readable_names() {
        // 这些是"会被扫描入库但放不出"的高频格式（docs/21 格式矩阵），
        // 名字必须说人话——UI 横幅直接展示这条消息。
        assert_eq!(codec_display_name(CODEC_TYPE_OPUS), "Opus");
        assert_eq!(codec_display_name(CODEC_TYPE_WMA), "WMA");
        assert_eq!(codec_display_name(CODEC_TYPE_MONKEYS_AUDIO), "Monkey's Audio (APE)");
        assert_eq!(codec_display_name(CODEC_TYPE_WAVPACK), "WavPack");
        assert_eq!(codec_display_name(CODEC_TYPE_ADPCM_MS), "ADPCM");
        // 兜底不能 panic：MP3 走不到分类路径（能正常解码），拿它验证兜底输出
        assert_eq!(codec_display_name(CODEC_TYPE_MP3), format!("{:?}", CODEC_TYPE_MP3));
    }
}
