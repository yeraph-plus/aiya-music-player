//! 曲库刷新管线的诊断视图（docs/33 §7）。
//!
//! 为什么要有这份快照：自动刷新是**静默**的（有变化才 bump）。静默的好处是
//! 列表不乱抖，代价是"它到底有没有在工作"没法从界面上看出来。诊断区读这份
//! 快照，就能回答"上次什么时候刷的、为什么没刷、跑了多久"。
//!
//! 三个字段刻意分开：`changed`（有没有变化）、`sync`（beets 段跑没跑）、
//! `error`（哪一段出的错）。合成一个"成功/失败"会让人分不清
//! "磁盘侧好了但 beets 段失败" 这种真实状态。

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// 最近一次刷新的结果。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct RefreshSnapshot {
    /// timer / focus / action / manual
    pub trigger: String,
    /// 完成时刻（unix 毫秒）
    #[ts(type = "number")]
    pub at_ms: u64,
    /// 有没有真的写出变化（false = 没发 bump，UI 不会刷新）
    pub changed: bool,
    #[ts(type = "number")]
    pub scanned: usize,
    #[ts(type = "number")]
    pub imported: usize,
    /// 重新读标签并写了行的数量（外部改标签就体现在这里）
    #[ts(type = "number")]
    pub updated: usize,
    #[ts(type = "number")]
    pub missing: usize,
    #[ts(type = "number")]
    pub failed: usize,
    /// beets 段：ok / skipped（没装组件或磁盘没变化）/ failed
    pub sync: String,
    #[ts(type = "number")]
    pub elapsed_ms: u64,
    /// 失败原因（`sync` 为 failed，或整条管线失败时才有）
    pub error: Option<String>,
}

/// 自动刷新的当前配置（设置页/诊断区读）。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct RefreshState {
    /// 定时间隔（秒）；0 = 只保留"切回窗口时刷新"
    #[ts(type = "number")]
    pub interval_secs: u64,
    /// 窗口当前是否在前台（失焦时定时器不跑）
    pub focused: bool,
    /// 最近一次结果（启动后还没跑过就是 null）
    pub last: Option<RefreshSnapshot>,
}
