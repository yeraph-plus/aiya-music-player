//! 关于页 / 诊断页的形状。

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::output::{AudioModeInfo, MediaKeysInfo, PowerInfo, StreamsInfo};

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
