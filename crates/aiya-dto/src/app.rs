//! 关于页 / 诊断页的形状。

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::ApiErrorCode;

/// 应用信息（侧栏数据目录展示，唯一消费方）。版本等识别信息走诊断快照。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub data_dir: String,
}

/// 频谱 tap 开关的回应（回显，便于 UI 确认订阅状态）。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct SpectrumToggle {
    pub enabled: bool,
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

/// 诊断快照（设置→系统/关于）。
///
/// 只装有消费方的字段。v1 曾按"诊断页"设想塞过 20 个字段，压平审计后
/// 只留三个：统计与 `LibraryStats` 重复、流计数恒 0、audioMode 的真实
/// 出口是 `audio:mode` 事件、平台/日志路径从未被 UI 读过。`shortcut_conflicts`
/// 更早之前就删了——action 清单只在前端存在，后端要算就得抄一份实现。
/// media_keys 随"系统媒体键"开关退役（SMTC 常开，健康走"最近错误"台账）。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct Diagnostics {
    pub version: String,
    pub data_dir: String,
    pub recent_errors: Vec<RecentError>,
}
