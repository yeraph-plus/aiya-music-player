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

/// 音频规格（播放条徽章/规格显示；对齐 contracts 的 AudioFormat）。
/// 列表查询不填（None），只有播放条数据源（单曲级查询）会带。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackAudio {
    pub codec: Option<String>,
    pub container: Option<String>,
    pub sample_rate: Option<i64>,
    pub bits_per_sample: Option<i64>,
    pub bitrate: Option<i64>,
    pub channels: Option<i64>,
}

/// 队列元素：Track 的最小子集 + 播放需要的 path。
/// 宿主层从 aiya-data TrackRow 转换而来。
///
/// **camelCase 是契约要求**：`current` 直接作为 PlayerState.current 下发，
/// UI 读 `artworkId` / `gainDb` / `audio` —— snake_case 会让封面/规格/归一化全部静默失效。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackRef {
    pub id: String,
    pub title: String,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub duration_ms: Option<i64>,
    pub artwork_id: Option<String>,
    pub gain_db: Option<f64>,
    #[serde(default)]
    pub audio: Option<TrackAudio>,
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

#[cfg(test)]
mod tests {
    use super::*;

    /// 快照形状锚点：`current` 是 UI 播放条唯一数据源，
    /// 必须 camelCase（artworkId/gainDb/audio），否则封面/规格/归一化静默失效。
    #[test]
    fn current_serializes_camel_case() {
        let st = PlayerState {
            queue_length: 1,
            queue_version: 1,
            index: 0,
            cursor: 0,
            repeat: RepeatMode::Off,
            shuffle: false,
            status: PlaybackStatus::Playing,
            position_ms: 1000,
            duration_ms: Some(5000),
            volume: 0.8,
            muted: false,
            rate: 1.0,
            error: None,
            current: Some(TrackRef {
                id: "t1".into(),
                title: "曲".into(),
                artist: Some("人".into()),
                album: None,
                duration_ms: Some(5000),
                artwork_id: Some("aw_x".into()),
                gain_db: Some(-3.0),
                audio: Some(TrackAudio {
                    codec: Some("flac".into()),
                    container: None,
                    sample_rate: Some(48000),
                    bits_per_sample: Some(24),
                    bitrate: None,
                    channels: Some(2),
                }),
                path: "D:/m/a.flac".into(),
            }),
            token: 1,
            updated_at: 0,
        };
        let v = serde_json::to_value(&st).unwrap();
        let cur = &v["current"];
        assert_eq!(cur["artworkId"], "aw_x", "封面字段必须 camelCase");
        assert_eq!(cur["gainDb"], -3.0, "归一化字段必须 camelCase");
        assert_eq!(cur["durationMs"], 5000);
        assert_eq!(cur["audio"]["sampleRate"], 48000, "规格字段必须 camelCase");
        assert_eq!(cur["audio"]["bitsPerSample"], 24);
        // 顶层快照字段本来就 camelCase，锚定防回归
        assert!(v.get("positionMs").is_some());
        assert!(v.get("durationMs").is_some());
        assert!(v.get("queueLength").is_some());
    }
}
