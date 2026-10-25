//! 关于页 / 诊断页的形状。

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::output::{AudioModeInfo, MediaKeysInfo, PowerInfo, StreamsInfo};
use super::ApiErrorCode;

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

/// 频谱 tap 开关的回应（回显，便于 UI 确认订阅状态）。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct SpectrumToggle {
    pub enabled: bool,
}

/// 一次列表查询的耗时样本（诊断页"最近查询"表）。
///
/// `took_ms` 是**端到端**耗时（命令入口 → 拿到 `TrackPage`），含 DB 线程排队；
/// `db_took_ms` 是 SQL 自己的耗时。两个都要：差值就是排队成本。5 万首深分页
/// 若 SQL 很快而端到端慢，说明瓶颈在串行队列而不是索引 —— 这是这张表存在的
/// 唯一理由，只记一个数字就白记了。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct ListTiming {
    /// unix 毫秒
    #[ts(type = "number")]
    pub at_ms: i64,
    /// 数据来源：library / playlist / recent / mostPlayed
    pub source: String,
    /// 本次返回行数
    #[ts(type = "number")]
    pub rows: i64,
    /// 该查询条件下命中总数（不是本次返回数）
    #[ts(type = "number")]
    pub total: i64,
    /// 端到端耗时（毫秒）
    pub took_ms: f64,
    /// SQL 执行耗时（毫秒，来自 `TrackPage::took_ms`）
    pub db_took_ms: f64,
    /// 本次用的游标；首屏为 null
    pub cursor: Option<String>,
}

/// 一条错误记录（诊断页"最近错误"）。
///
/// 采集点在 `ApiError` 的构造函数里 —— 全部 53 个命令的错误路径都会经过那里，
/// 不用逐个命令手写埋点（那样必然漏）。代价是**没有命令名**：构造函数不知道
/// 自己是谁返回的。宁可缺这个字段，也不编一个假值。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct RecentError {
    /// unix 毫秒
    #[ts(type = "number")]
    pub at_ms: i64,
    /// 与 `ApiError.code` 同一套分类
    pub code: ApiErrorCode,
    /// 面向用户的一句话（同 `ApiError.message`）
    pub message: String,
    /// 技术细节（同 `ApiError.detail`）
    pub detail: Option<String>,
}

/// 诊断页全量快照。
///
/// 三个"以前恒为空"的数组里，两个已接真实数据源（`list_timings` 在
/// `library_list` 埋点、`recent_errors` 挂在 `ApiError` 构造上）；
/// `shortcut_conflicts` **删掉了** —— 快捷键的 action 清单与绑定解析规则
/// 只存在于前端（`lib/shortcuts.ts`），后端要算就得抄一份 action 表和
/// 一遍规范化算法，两处实现必然漂移。诊断页改调前端的 `findConflicts`，
/// 与设置页的冲突提示同源。
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
    pub list_timings: Vec<ListTiming>,
    pub recent_errors: Vec<RecentError>,
    pub media_keys: MediaKeysInfo,
    pub power: PowerInfo,
    pub tray: bool,
    pub audio_mode: AudioModeInfo,
}
