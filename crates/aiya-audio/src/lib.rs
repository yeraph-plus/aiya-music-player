//! AIYA Music 音频引擎。
//!
//! 职责（ADR-0006）：Symphonia 解码 + wasapi-rs 输出，PCM 链内保留 tap 位置
//! （解码后 → 音量前），频谱 tap 订阅制（P4 接入，关闭零开销）。
//! 不依赖 Tauri、不依赖数据库 —— path 进，事件出。

pub mod decoder;
pub mod engine;

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
pub use engine::{Engine, EngineCommand, EngineEvent};
