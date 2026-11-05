//! 播放历史的形状（历史页 / "最近播放"）。
//!
//! 时间统一毫秒：DB 的 `history.played_at` 存 unix 秒，这里换算成毫秒并
//! 在字段名上写明（`firstPlayAtMs`）。v1 契约里叫 `firstPlayAt` 且是秒，
//! 与同页的 `totalMs` 混着两种单位——UI 侧要靠注释才知道该乘 1000。

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// 播放次数排行的一项。艺人名为 null = 曲目没有艺人标签（不是"未知艺人"）。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct HistoryTopTrack {
    pub id: String,
    pub title: String,
    pub artist: Option<String>,
    #[ts(type = "number")]
    pub play_count: i64,
}

/// 历史页汇总。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct HistoryStats {
    #[ts(type = "number")]
    pub total_plays: i64,
    /// 累计播放时长，毫秒（DB 存整秒，故为 1000 的倍数）
    #[ts(type = "number")]
    pub total_ms: i64,
    /// 至少听了 1 秒的条数
    #[ts(type = "number")]
    pub completed: i64,
    /// 0 秒就切走的条数
    #[ts(type = "number")]
    pub skipped: i64,
    /// 最早一条播放时间，毫秒；无历史为 null
    #[ts(type = "number | null")]
    pub first_play_at_ms: Option<i64>,
    /// 最近一条播放时间，毫秒；无历史为 null
    #[ts(type = "number | null")]
    pub last_play_at_ms: Option<i64>,
    /// 按播放次数前 10（已从曲库删除的曲目不计）
    pub top_tracks: Vec<HistoryTopTrack>,
}

/// 清空历史的结果。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct HistoryCleared {
    pub cleared: bool,
}
