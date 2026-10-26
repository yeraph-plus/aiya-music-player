//! 音频输出相关的形状：用户期望（`DesiredOutput`）、设备实际状态
//! （`AudioModeInfo` / `AudioModeEvent`）、系统集成状态。

use serde::{Deserialize, Serialize};
use ts_rs::TS;

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

/// EQ 设置（协议 v6，docs/24 B5b）。
///
/// `bands` 固定 10 段（31.25 Hz → 16 kHz，见 aiya-audio `EQ_BAND_HZ`），
/// 单位 dB，引擎侧 clamp 到 ±12；`pregain` 同单位，抗削顶预留。
/// 持久化在 settings 表独立键 `eq` 下（不进 `app_settings`）：EQ 调整走
/// `player_set_eq` 专用命令，避免与设置页的整体写互相覆盖。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct EqSettings {
    pub enabled: bool,
    pub bands: Vec<f32>,
    pub pregain: f32,
}
