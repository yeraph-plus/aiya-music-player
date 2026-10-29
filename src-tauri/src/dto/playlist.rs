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
    /// manual / smart（0003 迁移；0003 之前的行默认 manual）
    pub kind: String,
    /// 动态歌单规则 JSON 原文（手动歌单恒 null；UI 用 JSON.parse 解析——
    /// 存字符串而不是对象，是为了 DTO 形状稳定：规则内部结构归 smart.rs 管）
    #[ts(type = "string | null")]
    pub rule_json: Option<String>,
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

/// 刷新动态歌单的结果。`added` = 本次新追加的匹配数；`total` = 刷新后
/// 表内总曲目数（追加只进不出）。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistRefresh {
    #[ts(type = "number")]
    pub added: i64,
    #[ts(type = "number")]
    pub total: i64,
}

/// 删除歌单的结果。`deleted = false` 表示本来就不存在（幂等成功）。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistDeleted {
    pub deleted: bool,
}

/// 导入时一条没匹配上的路径（m3u/m3u8 导入，docs/22 G2）。
/// 形状对齐扫描错误报告：`line` 能指回原文件第几行，`raw` 原样展示。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistImportUnmatched {
    /// 原文件中的行号（1 基）
    #[ts(type = "number")]
    pub line: i64,
    /// 原始文本（URL 解码前）
    pub raw: String,
    /// 未匹配原因：`notFound`（曲库中没有这个路径）
    pub reason: String,
}

/// 导入结果。`playlist = null` 有两种含义，都**不是错误**：
/// ① 用户在文件选择框里取消（`matched = 0`、`unmatched` 为空）；
/// ② 文件解析出来了，但一条路径都没匹配到曲库（`unmatched` 有内容，
///    提示用户先扫描曲库，不建空歌单）。
///
/// `matched` 是真正落库的条数（已去重），`duplicates` 是因"已在歌单里"
/// 被跳过的条数——两个数必须分开给：导入 10 条显示匹配 8 条时，用户要能
/// 分清"少的是没找到"还是"本来就有"。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistImportResult {
    pub playlist: Option<PlaylistSummary>,
    #[ts(type = "number")]
    pub matched: i64,
    #[ts(type = "number")]
    pub duplicates: i64,
    pub unmatched: Vec<PlaylistImportUnmatched>,
}

/// 导出结果。`count` 是写进文件的条目数（= 歌单当前曲目数）。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistExportResult {
    pub path: String,
    #[ts(type = "number")]
    pub count: i64,
}
