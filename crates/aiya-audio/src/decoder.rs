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
//! * **HLS 流怎么接**：`DecodedStream::open_hls(url)` → [`crate::hls::HlsStream`]
//!   变体（docs/22 G3，ADTS AAC 路线）。对 engine 透明：duration=0 表示直播，
//!   `seekable()=false`，engine 对不可 seek 的源直接忽略 Seek 命令。
//! * **mpv 怎么插**：将来加 `DecodeBackend::Mpv`，实现为子进程
//!   `mpv --no-video --ao=pcm --audio-format=s32le --really-quiet <file>`
//!   读 stdout PCM 管道，包装成同一个 `DecodedStream` 形状（sample_rate /
//!   channels / duration_ms / next_packet / seek_ms / position_ms）。mpv 场景
//!   `duration_ms`/`seek_ms` 由 mpv 的属性/命令应答填充，`next_packet` 返回
//!   定长 PCM 块。engine/output/queue 零改动。
//! * **错误怎么分类**：三个入口，不要混：
//!   ① 扩展名就在"注定放不出"名单里（WMA/APE/WavPack/DSD/Opus）→
//!   `CodecUnsupported`，**在 probe 之前**判（这些容器 symphonia 根本不认，
//!   probe 只会吐"文件坏了"）；
//!   ② probe 成功但 `get_codecs().make()` 失败 → `CodecUnsupported { codec }`
//!   （容器认、编码没编进 features）；
//!   ③ probe 失败（坏文件/未知容器）→ `Decode`。
//!   两种 `CodecUnsupported` 的 `codec` 都是人话格式名（见
//!   [`codec_display_name`] / [`known_unsupported_ext`]），UI 直接展示。

use std::fs::File;
use std::path::Path;

use symphonia::core::audio::{Channels, SampleBuffer, SignalSpec};
use symphonia::core::codecs::{
    Decoder as SymphoniaDecoder, DecoderOptions, CodecType, CODEC_TYPE_NULL,
};
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::{FormatOptions, FormatReader, SeekMode, SeekTo};
use symphonia::core::io::{MediaSource, MediaSourceStream};
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;
use symphonia::core::units::Time;

use crate::hls::HlsStream;
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
/// 统一形状是 D4 的契约面——任何后端（Symphonia / mpv 子进程 / HLS 流）
/// 都产出这个结构，engine 的渲染循环与 WASAPI 输出永远零改动。
pub struct DecodedStream {
    inner: Inner,
}

enum Inner {
    Symphonia(SymphoniaStream),
    Hls(HlsStream),
}

/// Symphonia 后端实现（本地文件 / 内存字节）。
struct SymphoniaStream {
    format: Box<dyn FormatReader>,
    decoder: Box<dyn SymphoniaDecoder>,
    track_id: u32,
    sample_rate: u32,
    channels: usize,
    /// 流信息推算的总时长（无标签时也能给进度条）
    duration_ms: u64,
    /// 容器报的帧数（改采样率时要按它重算时长，见 [`Self::next_packet`] 里的规格对齐）
    codec_n_frames: Option<u64>,
    /// 当前解码位置（毫秒），随包时间戳推进
    position_ms: u64,
    /// 交错样本复用缓冲
    sample_buf: SampleBuffer<f32>,
    /// 复用缓冲的帧容量（真实规格变化时按同一容量重建）
    max_frames: u64,
    /// `open` 阶段探到的第一个包：容器不报通道数时（ALAC/AAC）只有解码
    /// 之后才知道真实规格，探完的那个包是真音频，不能丢——首次
    /// [`Self::next_packet`] 原样还给它。
    first_packet: Option<Vec<f32>>,
    /// 首包是否已经交出去（下次调用时要回收它占的内存）
    first_served: bool,
    /// seek 目标（毫秒）。Ogg 按页、MP4 按 chunk 落点，seek 之后的第一批
    /// 包里可能有**早于目标**的音频，吐出去就是"拖到 5:00 先听到 4:59"。
    seek_target_ms: Option<u64>,
    eof: bool,
}

impl std::fmt::Debug for DecodedStream {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // 内部是 trait object，不实现 Debug；元数据足够定位"哪条流、走到哪"。
        f.debug_struct("DecodedStream")
            .field("sample_rate", &self.sample_rate())
            .field("channels", &self.channels())
            .field("duration_ms", &self.duration_ms())
            .field("position_ms", &self.position_ms())
            .field("seekable", &self.seekable())
            .finish()
    }
}

impl DecodedStream {
    /// 便捷入口：打开本地文件（engine 的全部用法）。
    pub fn open_file(path: &Path) -> Result<Self, AudioError> {
        // 先按扩展名拦"注定放不出"的格式，再让 symphonia 去 probe。
        //
        // 为什么必须有这一步（docs/22 D3 的另一半）：WMA/APE/WavPack/DSD 在
        // symphonia 0.5.x 里**连 demuxer 都没有**，probe 阶段就失败，落进
        // `Decode`（"文件坏了"）——用户看到的是 symphonia 原文，而不是
        // "WMA 不支持"。只有 Opus 这类"容器认、编码不认"的才走得到
        // `make()` 失败那条分类路径。扩展名表是这两类格式的统一出口。
        if let Some(name) = known_unsupported_ext(path) {
            return Err(AudioError::CodecUnsupported { codec: name.to_string() });
        }
        let src = File::open(path).map_err(AudioError::Io)?;
        let mut hint = Hint::new();
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            hint.with_extension(ext);
        }
        Self::open_with_backend(Box::new(src), &hint, DecodeBackend::Symphonia)
    }

    /// HLS (m3u8) 流（docs/22 G3）：ADTS AAC 路线，live 轮询 / VOD 顺序
    /// 拉段都在 `HlsStream` 内部完成。直播流 duration=0、不可 seek。
    pub fn open_hls(url: &str) -> Result<Self, AudioError> {
        Ok(Self { inner: Inner::Hls(HlsStream::new(url)?) })
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
            DecodeBackend::Symphonia => {
                Ok(Self { inner: Inner::Symphonia(SymphoniaStream::open(src, hint)?) })
            }
        }
    }

    pub fn sample_rate(&self) -> u32 {
        match &self.inner {
            Inner::Symphonia(s) => s.sample_rate,
            Inner::Hls(h) => h.sample_rate(),
        }
    }

    pub fn channels(&self) -> usize {
        match &self.inner {
            Inner::Symphonia(s) => s.channels,
            Inner::Hls(h) => h.channels(),
        }
    }

    pub fn duration_ms(&self) -> u64 {
        match &self.inner {
            Inner::Symphonia(s) => s.duration_ms,
            Inner::Hls(h) => h.duration_ms(),
        }
    }

    pub fn position_ms(&self) -> u64 {
        match &self.inner {
            Inner::Symphonia(s) => s.position_ms,
            Inner::Hls(h) => h.position_ms(),
        }
    }

    /// 直播流不可 seek（engine 据此忽略 Seek 命令而不是报错杀播放）。
    pub fn seekable(&self) -> bool {
        match &self.inner {
            Inner::Symphonia(_) => true,
            Inner::Hls(h) => h.seekable(),
        }
    }

    /// 下一包的交错 f32 样本。EOF 返回 Ok(None)。
    /// 返回的切片在下次调用前有效。
    pub fn next_packet(&mut self) -> Result<Option<&[f32]>, AudioError> {
        match &mut self.inner {
            Inner::Symphonia(s) => s.next_packet(),
            Inner::Hls(h) => h.next_packet(),
        }
    }

    /// 精确 seek 到毫秒位置。直播流返回 Err（engine 先查 [`Self::seekable`]）。
    pub fn seek_ms(&mut self, ms: u64) -> Result<(), AudioError> {
        match &mut self.inner {
            Inner::Symphonia(s) => s.seek_ms(ms),
            Inner::Hls(_) => Err(AudioError::Unsupported("直播流不支持拖动进度".into())),
        }
    }
}

impl SymphoniaStream {
    fn open(src: Box<dyn MediaSource>, hint: &Hint) -> Result<Self, AudioError> {
        let mss = MediaSourceStream::new(src, Default::default());

        // gapless 打开：让 demuxer 在包上带 trim_start/trim_end（MP3 编码器延迟、
        // Vorbis/FLAC 尾部 padding），并把时长按"去掉 padding"的口径报。
        // 不开的话，实测 0.6s 的 Vorbis fixture 报 616ms（尾部 740 样本 delay
        // 被算进去了），MP3 开头还有 576 样本静音——进度条总长和首尾听感都能
        // 被抓到。trim 由解码器消费（0.5.5 里 mp3 与 vorbis 自己裁，ALAC/PCM
        // 收到的 trim 是 0），所以这里只负责把开关打开、不自作聪明再裁一遍。
        let opts = FormatOptions { enable_gapless: true, ..Default::default() };
        let probed = symphonia::default::get_probe()
            .format(hint, mss, &opts, &MetadataOptions::default())
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
        // 通道布局：容器**不一定报**（isomp4 一个字段都不填，ALAC/AAC 的通道数
        // 只在 magic cookie 里，只有解码器自己知道）。这里的值只是占位，open
        // 末尾会用第一个解码包的真实规格覆盖（见 next_packet 里的规格对齐）。绝不能让它是
        // 空布局：`SampleBuffer::new` 拿 `channels.count()` 当除数，0 通道直接
        // 除零 panic（symphonia-core audio.rs:727）——引擎线程当场死，不是
        // 能恢复的错误。
        let ch_layout = track
            .codec_params
            .channels
            .filter(|c| c.count() > 0)
            .unwrap_or(Channels::FRONT_LEFT | Channels::FRONT_RIGHT);
        let channels = ch_layout.count();
        let codec_n_frames = track.codec_params.n_frames;
        let duration_ms = codec_n_frames
            .map(|f| (f * 1000) / (sample_rate as u64).max(1))
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
        let max_frames = track.codec_params.max_frames_per_packet.unwrap_or(0).max(65_535) as u64;

        let mut stream = Self {
            format,
            decoder,
            track_id,
            sample_rate,
            channels,
            duration_ms,
            codec_n_frames,
            position_ms: 0,
            sample_buf: SampleBuffer::<f32>::new(max_frames, SignalSpec::new(sample_rate, ch_layout)),
            max_frames,
            first_packet: None,
            first_served: false,
            seek_target_ms: None,
            eof: false,
        };
        // 探一包：拿到真实规格（ALAC/AAC 的通道数）并把这一包存起来还给它。
        // engine 拿到流之后马上要 sample_rate/channels 去配输出，规格必须是
        // 真的——错报成 2ch 会把单声道数据当立体声放（速度减半、噪声）。
        stream.peek_first_packet()?;
        Ok(stream)
    }

    /// 解第一个包：确认真实规格，并把样本缓存起来（丢掉就等于吞掉 4~8KB 音频）。
    fn peek_first_packet(&mut self) -> Result<(), AudioError> {
        let first = self.next_packet()?.map(|s| s.to_vec());
        self.first_packet = first;
        // 探包不算播放位置：engine 拿到流之后从 0 开始
        self.position_ms = 0;
        self.eof = false;
        Ok(())
    }

    fn next_packet(&mut self) -> Result<Option<&[f32]>, AudioError> {
        // 上次返回的首包切片此刻按契约已失效（"切片在下次调用前有效"），回收内存
        if self.first_served {
            self.first_packet = None;
        }
        if self.first_packet.is_some() {
            self.first_served = true;
            return Ok(self.first_packet.as_deref());
        }
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
                            // 真实规格以解码器为准（ALAC/AAC 的容器层没有通道数）。
                            // 只在真的不一样时重建缓冲，正常格式零开销。
                            //
                            // 这里刻意**逐字段**写而不是调 `&mut self` 方法：
                            // `decoded` 借着 `self.decoder`，方法调用会借用整个
                            // self；逐字段写只借 self.sample_buf 等其余字段。
                            let spec = *decoded.spec();
                            if (spec.rate != self.sample_rate
                                || spec.channels.count() != self.channels)
                                && spec.rate > 0
                                && spec.channels.count() > 0
                            {
                                tracing::debug!(
                                    "解码规格以解码器为准：{}Hz × {}ch（容器报 {}Hz × {}ch）",
                                    spec.rate,
                                    spec.channels.count(),
                                    self.sample_rate,
                                    self.channels
                                );
                                self.sample_rate = spec.rate;
                                self.channels = spec.channels.count();
                                // 采样率变了要按容器给的帧数重算时长（m4a/aiff 都有）
                                if let Some(f) = self.codec_n_frames {
                                    self.duration_ms = (f * 1000) / spec.rate as u64;
                                }
                                self.sample_buf =
                                    SampleBuffer::<f32>::new(self.max_frames, spec);
                            }
                            let rate = (self.sample_rate as u64).max(1);
                            let ts_ms = packet.ts().saturating_mul(1000) / rate;
                            // seek 后的丢弃：落点之前的整包都不要（否则"拖到 5:00
                            // 先听到 4:59"）。跨过目标的那个包整包输出——只早
                            // 一包（几十毫秒），比切样本更省事也更不容易出错。
                            if let Some(target) = self.seek_target_ms {
                                let pkt_end_ms =
                                    ts_ms.saturating_add(packet.dur().saturating_mul(1000) / rate);
                                if pkt_end_ms <= target {
                                    continue;
                                }
                                self.seek_target_ms = None;
                                self.position_ms = ts_ms.max(target);
                            } else {
                                self.position_ms = ts_ms;
                            }
                            self.sample_buf.copy_interleaved_ref(decoded);
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

    fn seek_ms(&mut self, ms: u64) -> Result<(), AudioError> {
        // Time 的整数 From 是"秒"；毫秒要用 new(seconds, frac)
        let target = Time::new(ms / 1000, (ms % 1000) as f64 / 1000.0);
        // 先重置解码器（trait 文档要求：seek 之后解码器状态无效）。不重置时
        // Vorbis 会把上一段的 MDCT 重叠窗带进新位置，前一两包是旧内容。
        self.decoder.reset();
        self.format
            .seek(SeekMode::Accurate, SeekTo::Time { time: target, track_id: Some(self.track_id) })
            .map_err(|e| AudioError::Decode(e.to_string()))?;
        self.position_ms = ms;
        // 落点由 seek 的粗细决定（Ogg 按页、MP4 按 chunk）：接下来的包可能
        // 早于目标，交给 next_packet 丢掉；这里只记目标。
        self.seek_target_ms = Some(ms);
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
        // 兜底用 Display（它内部就是 `{:#x}`）而不是 Debug：数值形态与
        // symphonia 源码里 `decl_codec_type` 的常量写法一致，诊断页拿着能
        // 直接对回源码（Debug 会印成 `CodecType(4096)`，没法和源码对）。
        other => format!("未知编码 {other}"),
    }
}

/// "注定放不出"的扩展名 → 人话格式名。
///
/// 只列**扫描器会收、但 symphonia 0.5.x 根本没有 demuxer/decoder** 的格式
/// （docs/21 §3.1 的矩阵）。这些文件在 probe 阶段就会失败，落进 `Decode`
/// 会让用户以为"文件坏了"——所以必须在打开前拦一次。
///
/// 维护规则：symphonia 哪天补上某个 feature，对应条目必须一起删掉，
/// 否则会被这里误拦（这是本表唯一的失效方式，改动 Cargo features 时顺手看一眼）。
fn known_unsupported_ext(path: &Path) -> Option<&'static str> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    Some(match ext.as_str() {
        "opus" => "Opus",
        "wma" => "WMA",
        "ape" => "Monkey's Audio (APE)",
        "wv" => "WavPack",
        "dsf" | "dff" => "DSD",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AudioErrorKind;
    use symphonia::core::codecs::*;

    #[test]
    fn unsupported_codecs_have_readable_names() {
        // 这些是"会被装进库但放不出"的高频格式（docs/21 格式矩阵），
        // 名字必须说人话——UI 横幅直接展示这条消息。
        assert_eq!(codec_display_name(CODEC_TYPE_OPUS), "Opus");
        assert_eq!(codec_display_name(CODEC_TYPE_WMA), "WMA");
        assert_eq!(codec_display_name(CODEC_TYPE_MONKEYS_AUDIO), "Monkey's Audio (APE)");
        assert_eq!(codec_display_name(CODEC_TYPE_WAVPACK), "WavPack");
        assert_eq!(codec_display_name(CODEC_TYPE_ADPCM_MS), "ADPCM");
        // 兜底：没收录的编码不许 panic，且要留一个能和 symphonia 常量对上的
        // 十六进制数（Debug 会印成 `CodecType(4096)`，对不上源码）
        let unknown = codec_display_name(CODEC_TYPE_MP3);
        assert!(unknown.contains("0x"), "兜底要带十六进制常量：{unknown}");
        assert!(!unknown.contains("CodecType"), "兜底不许漏 Debug 形状：{unknown}");
    }

    /// 扩展名预分类（docs/22 D3 的另一半）：WMA/APE/WavPack/DSD 在
    /// symphonia 0.5.x 里连 demuxer 都没有，probe 必失败——不预判的话
    /// 用户看到的是"文件坏了"，而不是"这个格式不支持"。
    #[test]
    fn known_unsupported_extensions_are_classified_before_probe() {
        let dir = tempfile::tempdir().unwrap();
        // 内容无所谓：预分类在 File::open 之前，文件存在与否都不影响结论
        for (name, expect) in [
            ("a.wma", "WMA"),
            ("b.APE", "Monkey's Audio (APE)"), // 大小写不敏感
            ("c.wv", "WavPack"),
            ("d.dsf", "DSD"),
            ("e.opus", "Opus"),
        ] {
            let path = dir.path().join(name);
            let err = DecodedStream::open_file(&path).unwrap_err();
            assert_eq!(err.kind(), AudioErrorKind::CodecUnsupported, "{name}: {err}");
            assert!(err.to_string().contains(expect), "{name} 的名不对：{err}");
        }
        // 名单外的不许误拦：master 格式必须真进 probe（这里必然因缺文件报 Io）
        let err = DecodedStream::open_file(&dir.path().join("f.flac")).unwrap_err();
        assert_eq!(err.kind(), AudioErrorKind::FileMissing, "flac 不该被预拦截：{err}");
    }
}
