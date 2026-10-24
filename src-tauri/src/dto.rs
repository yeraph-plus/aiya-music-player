//! IPC DTO：跨进程边界的**唯一**形状来源（docs/17）。
//!
//! 为什么要有这一层（而不是继续 `json!` 手拼）：
//! - `json!` 拼出来的字段名/嵌套形状没有任何编译期约束，TS 侧只能手抄
//!   （`ui/contracts` 1301 行全靠人肉同步，已出过两次事）；
//! - 这里的每个结构体都 `derive(TS)`，由 `npm run gen:types` 生成到
//!   `ui/src/lib/ipc/generated/`，字段一改，TS 侧跟着变，`git diff` 会红。
//!
//! 约定（docs/17 §3）：
//! - 字段 Rust 侧 snake_case，wire 上一律 camelCase；
//! - 时间**一律毫秒**（`*Ms`），id 一律 `String`，缺值用 `Option` → `null`；
//! - 整数一律 `#[ts(type = "number")]`：ts-rs 默认把 `i64/u64` 映射成 `bigint`，
//!   而 serde_json 发的是 JSON number，JS 侧 `JSON.parse` 得到的是 `number`。
//!   `Option<i64>` 必须写成 `#[ts(type = "number | null")]`——`ts(type)` 会覆盖
//!   整个类型，连 `| null` 都要自己带上。

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// 命令失败的统一形状。替代 `{ok:false, error:{code,message}}` 信封。
///
/// 为什么不再用信封：错误本来就有两条通道（Tauri 自己 reject + `ok:false`），
/// UI 每处要判两次；而且信封逼着命令返回 `Value`，typed struct 永远进不来。
/// 现在命令返回 `Result<T, ApiError>`，Tauri 把 `Err` 序列化后直接 reject。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct ApiError {
    pub code: ApiErrorCode,
    /// 面向用户的一句话，可直接显示
    pub message: String,
    /// 面向诊断页/日志的技术细节，不进 UI 文案。
    ///
    /// 不用 `skip_serializing_if`：字段缺席与 `null` 在 TS 侧是两种类型
    /// （`detail?: string` vs `detail: string | null`），ts-rs 生成的是后者，
    /// 于是"省一个 null"会让类型与运行期对不上。宁可多两个字节。
    pub detail: Option<String>,
    /// UI 是否值得提供"重试"（设备断开、扫描冲突 = true；参数错 = false）
    pub retryable: bool,
}

/// 错误分类。刻意比旧码少：旧实现里 `E_INTERNAL` 一个码占了 26/41，
/// 等于没有分类；这里按"UI 该怎么处理"来分，而不是按"哪段代码出错"来分。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub enum ApiErrorCode {
    /// 兜底：没预料到的内部错误
    Internal,
    /// 目标不存在（曲目/歌单/文件）
    NotFound,
    /// 参数非法（含形状不符、名字不合规、路径不合规）
    InvalidArg,
    /// 正忙（扫描进行中等），`retryable` 通常为 true
    Busy,
    /// IO 失败：文件被占用、磁盘满、权限
    Io,
    /// 找不到 beets（Python 侧车未安装/未初始化）
    BeetsMissing,
    /// 设备/格式不支持（如必须独占但设备拒绝）
    Unsupported,
    /// 音频设备丢失（拔声卡、驱动崩）
    DeviceLost,
}

impl ApiError {
    pub fn new(code: ApiErrorCode, message: impl Into<String>) -> Self {
        Self { code, message: message.into(), detail: None, retryable: false }
    }

    // 只保留"现在真的在用"的构造器：B3 迁移到 busy/notFound 时再补，
    // 提前写一堆没人调的 helper 就是给以后留死代码（编译器的 dead_code
    // 警告会替我们盯着，删掉反而安全）。

    /// 参数非法：不重试
    pub fn invalid_arg(message: impl Into<String>) -> Self {
        Self::new(ApiErrorCode::InvalidArg, message)
    }

    /// 内部错误：把技术细节塞进 `detail`，`message` 保持可读
    pub fn internal(detail: impl std::fmt::Display) -> Self {
        Self {
            code: ApiErrorCode::Internal,
            message: "内部错误".into(),
            detail: Some(detail.to_string()),
            retryable: false,
        }
    }
}

impl From<aiya_data::DbError> for ApiError {
    fn from(e: aiya_data::DbError) -> Self {
        ApiError::internal(e)
    }
}

/// 应用信息（关于页/诊断页）。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub version: String,
    /// 旧字段名沿用：v1 是 Electron，现在恒为 "tauri"
    pub electron: String,
    pub platform: String,
    pub data_dir: String,
}

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

/// 频谱 tap 开关的回应（回显，便于 UI 确认订阅状态）。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct SpectrumToggle {
    pub enabled: bool,
}

/* ---------------- 诊断页 ---------------- */

/// 用户设置里的输出偏好（"我想要什么"）。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct DesiredOutput {
    pub mode: String,
    #[ts(type = "number")]
    pub buffer_ms: u32,
    pub clock: String,
    pub release_on_pause: bool,
}

/// 音频实际状态（"设备到底给了什么"）。
///
/// 三档都列出来是因为"独占 50ms 轮询"和"共享 200ms 事件"的听感差异远小于
/// 它们的配置差异，用户只能靠这里判断实际生效的是什么。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct AudioModeInfo {
    pub requested: String,
    pub mode: String,
    pub clock: String,
    #[ts(type = "number")]
    pub buffer_ms: u32,
    pub format: String,
    /// 回落/自愈原因；完全按请求生效时为 null
    pub note: Option<String>,
    pub desired: DesiredOutput,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct MediaKeysInfo {
    pub enabled: bool,
    pub active: Vec<String>,
    pub failed: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct PowerInfo {
    pub active: bool,
    /// 实际用的机制名（`SetThreadExecutionState`），未启用为 null
    #[serde(rename = "type")]
    pub kind: Option<String>,
}

/// 音频流计数（诊断页）。引擎侧暂只暴露 written_frames，这里保留结构。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct StreamsInfo {
    #[ts(type = "number")]
    pub opened: i64,
    #[ts(type = "number")]
    pub closed: i64,
    #[ts(type = "number")]
    pub active: i64,
}

/// 诊断页全量快照。
///
/// `list_timings` / `recent_errors` / `shortcut_conflicts` 目前恒为空数组
/// （没有数据源）。刻意留着形状而不是删字段：UI 已有占位区，删了要改 UI，
/// 而且这三个迟早要有（性能基线、错误台账、快捷键冲突检测）。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct Diagnostics {
    pub version: String,
    pub electron: String,
    pub platform: String,
    pub data_dir: String,
    pub log_dir: Option<String>,
    pub log_file: Option<String>,
    #[ts(type = "number")]
    pub schema_version: i64,
    #[ts(type = "number")]
    pub db_bytes: i64,
    #[ts(type = "number")]
    pub tracks: i64,
    #[ts(type = "number")]
    pub albums: i64,
    #[ts(type = "number")]
    pub artists: i64,
    #[ts(type = "number")]
    pub playlists: i64,
    #[ts(type = "number")]
    pub history_rows: i64,
    #[ts(type = "number")]
    pub total_duration_ms: i64,
    pub streams: StreamsInfo,
    #[ts(type = "unknown[]")]
    pub list_timings: Vec<serde_json::Value>,
    #[ts(type = "unknown[]")]
    pub recent_errors: Vec<serde_json::Value>,
    #[ts(type = "unknown[]")]
    pub shortcut_conflicts: Vec<serde_json::Value>,
    pub media_keys: MediaKeysInfo,
    pub power: PowerInfo,
    pub tray: bool,
    pub audio_mode: AudioModeInfo,
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

/// `audio:mode` 事件的载荷：设备实际给了什么。
///
/// 单独立一个类型而不是复用 `AudioModeInfo`：诊断页要连带 `desired`（用户设置），
/// 事件只报"现在这一秒的实际状态"，不带期望值——否则每帧都要读一次设置。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct AudioModeEvent {
    pub requested: String,
    pub mode: String,
    pub clock: String,
    #[ts(type = "number")]
    pub buffer_ms: u32,
    pub format: String,
    /// 回落/自愈原因；完全按请求生效时为 null（不再用空字符串表示"没有"）
    pub reason: Option<String>,
}
