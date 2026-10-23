//! Symphonia 解码：文件 → 交错 f32 样本流。
//!
//! 为什么 f32：WASAPI 共享模式引擎 mix format 通常是 f32，
//! 且 AUTOCONVERTPCM 打开后引擎负责源格式 → 引擎格式转换（含 SRC）。
//! 位置时钟：包时间戳（ts）÷ 采样率 = 毫秒，seek 后以 seek 目标为锚。

use std::fs::File;
use std::path::Path;

use symphonia::core::audio::{SampleBuffer, SignalSpec};
use symphonia::core::codecs::{Decoder as SymphoniaDecoder, DecoderOptions, CODEC_TYPE_NULL};
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::{FormatOptions, FormatReader, SeekMode, SeekTo};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;
use symphonia::core::units::Time;

use crate::AudioError;

pub struct Decoder {
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

impl Decoder {
    pub fn open(path: &Path) -> Result<Self, AudioError> {
        let src = File::open(path).map_err(AudioError::Io)?;
        let mss = MediaSourceStream::new(Box::new(src), Default::default());

        let mut hint = Hint::new();
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            hint.with_extension(ext);
        }

        let probed = symphonia::default::get_probe()
            .format(&hint, mss, &FormatOptions::default(), &MetadataOptions::default())
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

        let decoder = symphonia::default::get_codecs()
            .make(&track.codec_params, &DecoderOptions::default())
            .map_err(|e| AudioError::Decode(e.to_string()))?;

        // 复用缓冲按 100ms 容量（所有正常容器单包都远小于此）
        let spec = SignalSpec::new(sample_rate, track.codec_params.channels.unwrap_or_default());
        let sample_buf = SampleBuffer::<f32>::new((sample_rate as u64 / 10).max(4096), spec);

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
