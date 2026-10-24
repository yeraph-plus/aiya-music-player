//! 播放状态类型：对齐 `@aiya/contracts/player.ts`。
//!
//! 状态里不放队列本身（5 万首 ID 数组不进每秒快照）；
//! 队列按版本号单独拉取（queueVersion 变化才重取）。

use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "lowercase")]
pub enum RepeatMode {
    Off,
    All,
    One,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
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
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct TrackAudio {
    pub codec: Option<String>,
    pub container: Option<String>,
    #[ts(type = "number | null")]
    pub sample_rate: Option<i64>,
    #[ts(type = "number | null")]
    pub bits_per_sample: Option<i64>,
    #[ts(type = "number | null")]
    pub bitrate: Option<i64>,
    #[ts(type = "number | null")]
    pub channels: Option<i64>,
}

/// 队列元素：Track 的最小子集（**不含 path**，path 只在宿主内部流转）。
/// 宿主层从 aiya-data TrackRow 转换而来。
///
/// **camelCase 是契约要求**：`current` 直接作为 PlayerState.current 下发，
/// UI 读 `artworkId` / `gainDb` / `audio` —— snake_case 会让封面/规格/归一化全部静默失效。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct TrackRef {
    pub id: String,
    pub title: String,
    pub artist: Option<String>,
    pub album: Option<String>,
    #[ts(type = "number | null")]
    pub duration_ms: Option<i64>,
    pub artwork_id: Option<String>,
    pub gain_db: Option<f64>,
    #[serde(default)]
    pub audio: Option<TrackAudio>,
    /// 引擎装载用；**不下发**（DTO 侧由 trackId 查询）。
    ///
    /// 曾经只是注释这么写、代码照发：`#[serde(default)]` 会把它一起序列化，
    /// UI 能从 `player:state.current.path` 拿到绝对路径。现在真的 skip。
    #[serde(default, skip_serializing)]
    #[ts(skip)]
    pub path: String,
}

/// 播放失败的分类。**给 UI 用**：决定"自动跳下一首"还是"提示/亮灰标"。
///
/// 为什么不用字符串：v1 的 `error: Option<String>` 只有一句话，UI 无法判断
/// "文件没了（跳过就行）"和"声卡拔了（必须提示）"，只能靠人肉看文案。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub enum AudioErrorCode {
    /// 文件不存在/被删
    FileMissing,
    /// 其他 IO：权限、占用、磁盘
    Io,
    /// 解码失败：编码不支持、文件损坏
    Decode,
    /// 输出设备：打不开、断开、不消费数据
    DeviceLost,
    /// 设备/格式不支持（如"必须独占"被拒）
    UnsupportedFormat,
    /// 未分类
    Unknown,
}

impl AudioErrorCode {
    /// wire 值 → 枚举。与 `aiya_audio::AudioErrorKind::as_str()` 一一对应；
    /// 未知值按 `Unknown`（新引擎给新分类时，旧宿主不该直接崩）。
    pub fn parse(s: &str) -> Self {
        match s {
            "fileMissing" => AudioErrorCode::FileMissing,
            "io" => AudioErrorCode::Io,
            "decode" => AudioErrorCode::Decode,
            "deviceLost" => AudioErrorCode::DeviceLost,
            "unsupportedFormat" => AudioErrorCode::UnsupportedFormat,
            _ => AudioErrorCode::Unknown,
        }
    }
}

/// 播放失败：分类 + 一句话。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct AudioError {
    pub code: AudioErrorCode,
    pub message: String,
}

impl AudioError {
    pub fn new(code: AudioErrorCode, message: impl Into<String>) -> Self {
        Self { code, message: message.into() }
    }
}

/// 播放状态快照（对齐 PlayerState 契约形状，camelCase serde）。
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct PlayerState {
    pub queue_length: usize,
    #[ts(type = "number")]
    pub queue_version: u64,
    /// 队列下标；-1 = 空队列
    #[ts(type = "number")]
    pub index: i64,
    /// 播放顺序位置（随机模式下与 index 不同）
    #[ts(type = "number")]
    pub cursor: i64,
    pub repeat: RepeatMode,
    pub shuffle: bool,
    pub status: PlaybackStatus,
    #[ts(type = "number")]
    pub position_ms: u64,
    #[ts(type = "number | null")]
    pub duration_ms: Option<u64>,
    pub volume: f64,
    pub muted: bool,
    pub rate: f64,
    pub error: Option<AudioError>,
    pub current: Option<TrackRef>,
    #[ts(type = "number")]
    pub token: u64,
    #[ts(type = "number")]
    pub updated_at: u64,
}

/// 队列快照（对齐 QueueSnapshot：ids + tracks 最小字段 + 游标）。
/// v1 只给 ids + index；tracks 全量字段由 library_tracks 查询补齐。
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct QueueSnapshot {
    pub ids: Vec<String>,
    pub index: usize,
    pub cursor: usize,
    #[ts(type = "number")]
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
    /// 移除指定位置（播放顺序 order 里的下标，与 queue_snapshot().ids 一致）
    Remove { indices: Vec<usize> },
    /// 重排：把 from 位置的曲目移到 to 位置（同一坐标系）
    Move { from: usize, to: usize },
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
    Failed { error: AudioError },
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
