//! 播放状态类型：对齐 `@aiya/contracts/player.ts`。
//!
//! 状态里不放队列本身（5 万首 ID 数组不进每秒快照）；
//! 队列按版本号单独拉取（queueVersion 变化才重取）。

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RepeatMode {
    Off,
    All,
    One,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PlaybackStatus {
    Idle,
    Loading,
    Playing,
    Paused,
    Error,
}

/// 队列元素：Track 的最小子集 + 播放需要的 path。
/// 宿主层从 aiya-data TrackRow 转换而来。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackRef {
    pub id: String,
    pub title: String,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub duration_ms: Option<i64>,
    pub artwork_id: Option<String>,
    pub gain_db: Option<f64>,
    /// 引擎装载用；不出现在对外 DTO（DTO 侧由 trackId 查询）
    #[serde(default)]
    pub path: String,
}

/// 播放状态快照（对齐 PlayerState 契约形状，camelCase serde）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayerState {
    pub queue_length: usize,
    pub queue_version: u64,
    /// 队列下标；-1 = 空队列
    pub index: i64,
    /// 播放顺序位置（随机模式下与 index 不同）
    pub cursor: i64,
    pub repeat: RepeatMode,
    pub shuffle: bool,
    pub status: PlaybackStatus,
    pub position_ms: u64,
    pub duration_ms: Option<u64>,
    pub volume: f64,
    pub muted: bool,
    pub rate: f64,
    pub error: Option<String>,
    pub current: Option<TrackRef>,
    pub token: u64,
    pub updated_at: u64,
}

/// 队列快照（对齐 QueueSnapshot：ids + tracks 最小字段 + 游标）。
/// v1 只给 ids + index；tracks 全量字段由 library_tracks 查询补齐。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QueueSnapshot {
    pub ids: Vec<String>,
    pub index: usize,
    pub cursor: usize,
    pub version: u64,
    pub took_ms: u32,
}

/// 队列变更操作（对齐 UI 侧 QueueInsertMode 语义）。
#[derive(Debug, Clone)]
pub enum QueueOp {
    /// 替换整个队列并从 offset 播放
    ReplaceAll { tracks: Vec<TrackRef>, start: usize },
    /// 追加到队尾
    Append { tracks: Vec<TrackRef> },
    /// "下一首播放"
    PlayNext { tracks: Vec<TrackRef> },
    /// 移除指定下标
    Remove { indices: Vec<usize> },
}

/// PlayerActor 接受的命令。
#[derive(Debug, Clone)]
pub enum PlayerCommand {
    TogglePause,
    Next,
    Prev,
    Seek(u64),
    SetVolume { volume: f64, muted: bool },
    SetRepeat(RepeatMode),
    SetRate(f64),
    ToggleShuffle,
    Queue(QueueOp),
    /// 引擎回报：装载完成（含总时长）
    Loaded { token: u64, duration_ms: u64 },
    /// 引擎回报：位置推进
    Position { position_ms: u64 },
    /// 引擎回报：曲目自然播完
    Ended { token: u64 },
    /// 引擎回报：错误
    Failed { message: String },
}
