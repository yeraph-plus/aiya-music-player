//! AIYA Music 音频引擎。
//!
//! 职责（ADR-0006）：Symphonia 解码 + wasapi-rs 输出，PCM 链内保留 tap 位置
//! （解码后 → 音量前），频谱 tap 订阅制（P4 接入，关闭零开销）。
//! 不依赖 Tauri、不依赖数据库 —— path 进，事件出。

pub mod decoder;
pub mod engine;
/// 10 段 biquad EQ（协议 v6，docs/24 B5b）。
pub mod eq;
/// HLS (m3u8) 流式源（docs/22 G3）。对 engine 透明——只以 `DecodedStream`
/// 的 Hls 变体出现；模块内类型不进公共 API。
pub mod hls;
pub mod mmcss;
pub mod proto;
pub mod spectrum;

#[derive(Debug, thiserror::Error)]
pub enum AudioError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("decode: {0}")]
    Decode(String),
    /// 容器认识、但编码不支持（Opus/WMA/APE/WavPack/DSD…）。
    ///
    /// 与 `Decode` 分开的理由（docs/22 D3）：这不是"文件坏了"而是"格式没编进来"——
    /// 用户换文件/转码有用，重试没用。`codec` 是用户可读的格式名（如 "Opus"），
    /// 消息会原样进 UI 横幅，所以说人话。
    #[error("不支持的音频编码（{codec}）：播放器未内置该格式的解码器")]
    CodecUnsupported { codec: String },
    #[error("wasapi: {0}")]
    Wasapi(String),
    /// 设备/输出流层面的失败：流为空、不消费数据、恢复失败
    #[error("device: {0}")]
    Device(String),
    /// 设备或配置不支持（独占被拒、格式不匹配）
    #[error("unsupported: {0}")]
    Unsupported(String),
    #[error("{0}")]
    Custom(String),
}

/// 播放失败的分类：**给宿主/UI 用**，不是给日志用。
///
/// 为什么要在错误里带分类：宿主拿到的是 `String`（协议里只传一句话），
/// UI 却要据此决定"自动跳下一首"还是"弹提示/亮灰标"。靠解析消息前缀
/// 来判断等于把契约写在文案里，改一句话就崩。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioErrorKind {
    /// 文件不存在/被删（IO NotFound）
    FileMissing,
    /// 其他 IO：权限、占用、磁盘
    Io,
    /// 解码失败：文件损坏、容器不认识（probe 都过不去）
    Decode,
    /// 编码不支持：容器认识但解码器没编进来（Opus/WMA/APE/WavPack/DSD…）
    CodecUnsupported,
    /// 输出设备：打不开、断开、不消费数据
    Device,
    /// 设备不支持所请求的格式/模式（如必须独占被拒）
    Unsupported,
    /// 引擎内部故障（渲染线程 panic 的兜底上报，见 `Engine::spawn`）。
    ///
    /// 正常播放路径不该出现；sidecar 下出现即 panic=abort 将接手杀进程、
    /// 看门狗换代重建。宿主侧未知 kind 落到 `unknown`（`AudioErrorCode::parse`），
    /// 向后兼容。
    Internal,
    /// 兜底
    Unknown,
}

impl AudioErrorKind {
    /// wire 值（与 `aiya_player::AudioErrorCode` 的 serde 值一致）
    pub fn as_str(self) -> &'static str {
        match self {
            AudioErrorKind::FileMissing => "fileMissing",
            AudioErrorKind::Io => "io",
            AudioErrorKind::Decode => "decode",
            AudioErrorKind::CodecUnsupported => "codecUnsupported",
            AudioErrorKind::Device => "deviceLost",
            AudioErrorKind::Unsupported => "unsupportedFormat",
            AudioErrorKind::Internal => "internal",
            AudioErrorKind::Unknown => "unknown",
        }
    }
}

impl AudioError {
    pub fn kind(&self) -> AudioErrorKind {
        match self {
            AudioError::Io(e) if e.kind() == std::io::ErrorKind::NotFound => {
                AudioErrorKind::FileMissing
            }
            AudioError::Io(_) => AudioErrorKind::Io,
            AudioError::Decode(_) => AudioErrorKind::Decode,
            AudioError::CodecUnsupported { .. } => AudioErrorKind::CodecUnsupported,
            AudioError::Wasapi(_) | AudioError::Device(_) => AudioErrorKind::Device,
            AudioError::Unsupported(_) => AudioErrorKind::Unsupported,
            AudioError::Custom(_) => AudioErrorKind::Unknown,
        }
    }
}

pub use decoder::{DecodeBackend, DecodedStream};
pub use engine::{ClockPref, Engine, EngineCommand, EngineEvent, ModePref, OutputConfig};
pub use eq::{EqParams, EQ_BAND_HZ, EQ_GAIN_LIMIT_DB, EQ_PREGAIN_LIMIT_DB};
pub use proto::{encode_line, HostToSidecar, SidecarToHost, PROTOCOL_VERSION};

/// 设备自检（sidecar --selftest 用）：列出渲染设备名。
pub fn selftest_info() -> Result<Vec<String>, AudioError> {
    let _ = wasapi::initialize_mta();
    let enumerator =
        wasapi::DeviceEnumerator::new().map_err(|e| AudioError::Wasapi(e.to_string()))?;
    let collection = enumerator
        .get_device_collection(&wasapi::Direction::Render)
        .map_err(|e| AudioError::Wasapi(e.to_string()))?;
    let count = collection
        .get_nbr_devices()
        .map_err(|e| AudioError::Wasapi(e.to_string()))?;
    let mut names = Vec::new();
    for i in 0..count {
        if let Ok(dev) = collection.get_device_at_index(i) {
            if let Ok(name) = dev.get_friendlyname() {
                names.push(name);
            }
        }
    }
    Ok(names)
}
