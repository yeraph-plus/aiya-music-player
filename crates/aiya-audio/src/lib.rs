//! AIYA Music 音频引擎。
//!
//! 职责（ADR-0006）：Symphonia 解码 + wasapi-rs 输出，PCM 链内保留 tap 位置
//! （解码后 → 音量前），频谱 tap 订阅制（P4 接入，关闭零开销）。
//! 不依赖 Tauri、不依赖数据库 —— path 进，事件出。

pub mod decoder;
pub mod engine;
pub mod mmcss;
pub mod proto;
pub mod spectrum;

#[derive(Debug, thiserror::Error)]
pub enum AudioError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("decode: {0}")]
    Decode(String),
    #[error("wasapi: {0}")]
    Wasapi(String),
    #[error("{0}")]
    Custom(String),
}

pub use decoder::Decoder;
pub use engine::{ClockPref, Engine, EngineCommand, EngineEvent, ModePref, OutputConfig};
pub use proto::{encode_line, HostToSidecar, SidecarToHost, PROTOCOL_VERSION};

/// 设备自检（sidecar --selftest 用）：列出渲染设备名。
pub fn selftest_info() -> Result<Vec<String>, AudioError> {
    let _ = wasapi::initialize_mta();
    let enumerator = wasapi::DeviceEnumerator::new().map_err(|e| AudioError::Wasapi(e.to_string()))?;
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
