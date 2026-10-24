//! 歌单的形状。
//!
//! 时间字段带 `Ms` 后缀：DB 里 `created_at` / `updated_at` 存的是 **unix 秒**，
//! 而同一个对象上的 `durationMs` 是毫秒。同一个结构里混两种单位，UI 那边
//! 只能靠记；统一成毫秒并在名字上写清楚，是把"记"变成"看"（docs/17 §3.1）。

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// 歌单摘要（侧栏 + 详情页共用）。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistSummary {
    pub id: String,
    pub name: String,
    /// schema v1 没有这一列，恒为 null（保留字段是为了 v1 契约形状不变）
    pub description: Option<String>,
    #[ts(type = "number")]
    pub track_count: i64,
    /// 曲目时长合计，用于显示 "1 小时 12 分"
    #[ts(type = "number")]
    pub duration_ms: i64,
    /// 首个有封面的曲目的封面（自定义封面在 v1 未做）
    pub artwork_id: Option<String>,
    /// 创建时间，毫秒（DB 存秒）
    #[ts(type = "number")]
    pub created_at_ms: i64,
    /// 最后修改时间，毫秒（DB 存秒）
    #[ts(type = "number")]
    pub updated_at_ms: i64,
    /// 侧栏拖动排序的位置（0002 迁移新增；0 基稠密）
    #[ts(type = "number")]
    pub sort_position: i64,
}

/// 加歌结果：写进去几条、因去重跳过几条，以及更新后的摘要。
///
/// 带 `summary` 是因为加歌必然改变 `trackCount` / `durationMs`，
/// 让 UI 再查一次是把一次往返变成两次，且中间那一帧的数字是旧的。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistAddResult {
    #[ts(type = "number")]
    pub added: i64,
    #[ts(type = "number")]
    pub skipped: i64,
    pub summary: PlaylistSummary,
}

/// 删除歌单的结果。`deleted = false` 表示本来就不存在（幂等成功）。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistDeleted {
    pub deleted: bool,
}
