//! AIYA Music 播放状态机（ADR-0003：状态唯一真相在 Rust）。
//!
//! 纯逻辑 crate：命令进 → 状态出 + 引擎指令出。
//! 引擎适配（aiya-audio 翻译层）在 src-tauri —— 本 crate 不依赖任何平台细节。

pub mod actor;
pub mod state;

pub use actor::PlayerActor;
pub use state::{
    PlaybackStatus, PlayerCommand, PlayerState, QueueOp, QueueSnapshot, RepeatMode, TrackAudio,
    TrackRef,
};
