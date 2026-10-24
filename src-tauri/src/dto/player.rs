//! 播放控制的形状。
//!
//! 播放**状态**本体（`PlayerState` / `QueueSnapshot`）不在这里：它在
//! `aiya-player`，是状态机的对外快照，早已 `derive(TS)`。这里只放宿主
//! 自己的回执（它们是"命令的返回值"，不是"状态"）。

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// `player_report` 的回执。
///
/// 这个命令是 v1（`<audio>` 出口）留下的回报通道，引擎搬到 Rust 之后
/// 只剩诊断价值——所以刻意**不给 report 参数定形状**：旧 UI 仍在按
/// 联合类型发事件，把它钉死只会让老代码先挂掉。回执给个 ack 就够，
/// UI 侧本来也不看返回值。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct ReportAck {
    pub received: bool,
}
