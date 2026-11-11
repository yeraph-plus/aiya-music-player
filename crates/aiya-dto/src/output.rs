//! 音频输出相关的形状：用户期望（`DesiredOutput`）、设备实际状态
//! （`AudioModeEvent`）。

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

/// `audio:mode` 事件的载荷：设备实际给了什么。
///
/// 只走事件不走快照：设置页的输出状态随事件实时刷新（"独占 50ms 轮询"和
/// "共享 200ms 事件"的听感差异远小于配置差异，用户靠这里判断实际生效的
/// 是什么），诊断快照不再重复携带一份。
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

/// EQ 设置（协议 v6；音色双旋钮为低音/高音两个独立 shelf）。
///
/// `bands` 固定 10 段（31.25 Hz → 16 kHz，见 aiya-audio `EQ_BAND_HZ`），
/// 单位 dB，引擎侧 clamp 到 ±12；`pregain` 同单位，抗削顶预留；
/// `bassDb`/`trebleDb` 是独立的低音/高音 shelf 旋钮，与图示段
/// 正交。持久化在 settings 表独立键 `eq` 下（不进 `app_settings`）：EQ 调整
/// 走 `player_set_eq` 专用命令，避免与设置页的整体写互相覆盖。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct EqSettings {
    pub enabled: bool,
    pub bands: Vec<f32>,
    #[serde(default)]
    pub bass_db: f32,
    #[serde(default)]
    pub treble_db: f32,
    pub pregain: f32,
}
