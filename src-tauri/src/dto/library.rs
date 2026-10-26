//! 曲库页的形状：列表详情、统计、扫描、选择、移除。
//!
//! `ScanResult` / `ScanError` 不在这里——它们是 `aiya-data::scanner` 的返回值，
//! 就地 `derive(TS)`（与 `repo::TrackRow` 同一处理：数据层自己的返回类型
//! 跟着数据层走，宿主只负责转发）。理由是这样扫描器改字段时，编译期就能
//! 连带把 TS 生成物拉红，不需要人在两处同步。

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// 库统计（侧栏/设置页）。字段名沿用 v1 契约，避免 UI 大改。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct LibraryStats {
    #[ts(type = "number")]
    pub tracks: i64,
    #[ts(type = "number")]
    pub albums: i64,
    #[ts(type = "number")]
    pub artists: i64,
    /// 文件位置数（新 schema 一曲一位置，故等于曲目数）
    #[ts(type = "number")]
    pub locations: i64,
    /// 位置缺失数（availability ≠ online）
    #[ts(type = "number")]
    pub missing_locations: i64,
    #[ts(type = "number")]
    pub total_duration_ms: i64,
    #[ts(type = "number")]
    pub db_bytes: i64,
    #[ts(type = "number")]
    pub schema_version: i64,
}

/// 单曲详情：列表字段 + 音频规格（`library_tracks` 专用）。
///
/// 为什么不复用 `TrackRow` 加个可选字段：列表查询**约定不带** audio spec
/// （5 万首列表里塞 codec/bitrate 是纯浪费），而 `skip_serializing_if` 会让
/// "字段缺失"和"值为 null"在 TS 侧无法区分。两个结构 + 显式 From 更诚实：
/// `TrackRow` 加字段时，下面的构造会编译失败，强制同步。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct TrackDetail {
    pub id: String,
    pub title: String,
    pub artist_id: Option<String>,
    pub artist: Option<String>,
    pub album_id: Option<String>,
    pub album: Option<String>,
    pub album_artist: Option<String>,
    #[ts(type = "number | null")]
    pub duration_ms: Option<i64>,
    #[ts(type = "number | null")]
    pub track_no: Option<i64>,
    #[ts(type = "number | null")]
    pub disc_no: Option<i64>,
    #[ts(type = "number | null")]
    pub year: Option<i64>,
    /// 入库时间，毫秒（DB 存秒）
    #[ts(type = "number")]
    pub added_at_ms: i64,
    pub artwork_id: Option<String>,
    #[ts(type = "number")]
    pub play_count: i64,
    /// 最后播放时间，毫秒（DB 存秒）；从未播过为 null
    #[ts(type = "number | null")]
    pub last_played_at_ms: Option<i64>,
    pub gain_db: Option<f64>,
    /// online / offline / missing
    pub availability: String,
    /// 音频规格；标签里没有则为 null
    pub audio: Option<aiya_player::TrackAudio>,
}

impl TrackDetail {
    /// 从行 + 音频规格构造。字段逐个搬，`TrackRow` 变更时这里会编译失败。
    pub fn new(t: aiya_data::repo::TrackRow, audio: Option<aiya_player::TrackAudio>) -> Self {
        Self {
            id: t.id,
            title: t.title,
            artist_id: t.artist_id,
            artist: t.artist,
            album_id: t.album_id,
            album: t.album,
            album_artist: t.album_artist,
            duration_ms: t.duration_ms,
            track_no: t.track_no,
            disc_no: t.disc_no,
            year: t.year,
            added_at_ms: t.added_at_ms,
            artwork_id: t.artwork_id,
            play_count: t.play_count,
            last_played_at_ms: t.last_played_at_ms,
            gain_db: t.gain_db,
            availability: t.availability,
            audio,
        }
    }
}

/// 取消扫描的回应。`cancelled = false` 表示当时根本没在扫（不是失败）。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct ScanCancel {
    pub cancelled: bool,
}

/// 选目录成功后的路径。用户取消选择时命令返回 `null` 而不是空字符串。
///
/// docs/29 批 A：`roots` 是选择后的**完整根列表**（多目录并集单库），
/// UI 拿它直接回填 textarea，省一次 roots_get 往返。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct ChosenRoot {
    pub path: String,
    pub roots: Vec<String>,
}

/// 扫描根列表（settings 键 `library.roots`，多目录并集单库）。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct LibraryRoots {
    pub roots: Vec<String>,
}

/// 在文件管理器中定位曲目。
///
/// `reason` 为空表示成功；否则是可显示的原因（"曲目不在库中" /
/// "explorer 启动失败"）。不用错误通道：定位失败不该让 UI 弹错误框，
/// 用户点一下没反应就是"文件没了"，话说明白即可。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct RevealResult {
    pub revealed: bool,
    pub reason: Option<String>,
}

/// 从曲库移除（只删索引，不动磁盘文件）。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct RemoveTracks {
    #[ts(type = "number")]
    pub removed: i64,
}
