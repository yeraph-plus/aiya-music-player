//! 索引增强（beets 侧 `enrich` 的宿主侧结果）的 IPC 形状（docs/45 §5.4）。
//!
//! 三个字段组对应三件用户能看见的事：
//! - [`EnrichmentWorklist`]：曲库索引卡上"要算什么 / 算过多少"；
//! - [`EnrichmentMatch`]：按情绪/舞曲性/BPM 筛出来的曲目（曲库页与智能歌单）；
//! - [`EnrichmentGc`]：清理残留行之后回了多少行（一次性动作的回执）。
//!
//! 计数一律 `u32`：这些数字的上限是曲目数（十万级），`i64` 在 TS 侧会被
//! ts-rs 映射成 `bigint`，而 JSON 过来的是 `number`——类型对不上。

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// 曲库索引卡的数据源。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct EnrichmentWorklist {
    /// 这次问的是哪个源（原样回显，UI 不必自己记）。
    pub source: String,
    /// 组件是否可用。`false` 时下面的数字仍然有效（都是历史结论），
    /// 但"开始分析"按钮必须禁用——UI 只按这一个布尔判断，不必再看 `note`。
    pub available: bool,
    /// 不可用的原因（一句话，可直接显示）。
    pub note: Option<String>,
    /// 曲库里**在线**的曲目数（`availability='missing'` 的不参与统计）。
    pub total_tracks: u32,
    /// 有结论且文件没变过。
    pub analyzed: u32,
    /// 跑通了但一个字段都没拿到（默认不重试，只有"强制重算"才带上）。
    pub empty: u32,
    /// 从来没分析过。
    pub missing: u32,
    /// 有结论但文件被改过（size/mtime 变了）。
    pub stale: u32,
    /// 关联行找不到对应曲目（换过曲库根之后的残留，等 GC 收）。
    pub orphan: u32,
    /// 点"开始分析"实际会算多少首 = missing + stale（+ empty，当 includeEmpty）。
    pub work: u32,
    pub include_empty: bool,
    /// 清单前若干条（给卡片显示"接下来要算什么"，最多 20 条）。
    pub preview: Vec<String>,
}

/// 一条按维度筛出来的结果。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct EnrichmentMatch {
    pub track_id: String,
    /// argmax 得到的情绪类（`mood_happy` 这类原串，UI 负责翻译成中文）。
    pub mood_label: Option<String>,
    pub mood_score: Option<f64>,
    pub bpm: Option<f64>,
    pub danceability: Option<f64>,
    /// 语义是 `average_loudness`（xtractor 没有 energy）。
    pub loudness: Option<f64>,
    /// 分析时刻（毫秒，全仓 DTO 惯例）。
    #[ts(type = "number")]
    pub analyzed_at_ms: i64,
}

/// 清理残留行的回执。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct EnrichmentGc {
    pub removed: u32,
    pub keep_days: u32,
}
