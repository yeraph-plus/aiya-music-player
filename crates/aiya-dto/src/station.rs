//! 推流频道的传输形状。
//!
//! DTO 提供频道摘要与版本化节目配置，不包含媒体路径。
//! server 组合频道持久化数据、系统歌单引用与引擎运行态。

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// 频道列表卡（收听 tab 的站单卡片与"我的频道"列表共用）。
/// `running/nowPlaying` 是运行时字段（server 壳从频道控制层组装），非 DB 列。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct StationSummary {
    pub id: String,
    pub name: String,
    pub subtitle: Option<String>,
    pub description: Option<String>,
    pub cover_url: Option<String>,
    /// `/hls/{hls_dir}/live.m3u8`——相对同源；反代场景随部署基址
    pub hls_url: String,
    /// private | public
    pub visibility: String,
    /// 频道级开关（停播保留配置与段目录，再开播热恢复）
    pub active: bool,
    /// 引擎进程在跑（单容器 = spawn 存活；双容器 = keeper ON 与 .pid 位在）
    pub running: bool,
    /// 最近回灌的曲目名（on_track → now.jsonl → 轮询；未播过 = null）
    pub now_playing: Option<String>,
}

/// 频道详情与节目配置（station_get 返回）。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct StationDetail {
    #[serde(flatten)]
    pub summary: StationSummary,
    pub program: StationProgram,
}

/// 歌单内的播放顺序。
#[derive(Debug, Clone, Serialize, Deserialize, TS, PartialEq, Eq)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub enum StationPlayMode {
    Ordered,
    Shuffle,
}

/// 周时段；星期掩码以开始日为准，结束早于开始表示跨午夜，24:00 表示当日结束。
#[derive(Debug, Clone, Serialize, Deserialize, TS, PartialEq, Eq)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct StationSlot {
    pub start_hm: String,
    pub end_hm: String,
    pub days_mask: u8,
}

/// 频道队列草稿；歌单内容由系统歌单解析，不随配置复制。
#[derive(Debug, Clone, Serialize, Deserialize, TS, PartialEq, Eq)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct StationQueueItem {
    /// 系统歌单 ID；__library__ 表示当前全曲库。
    pub playlist_id: String,
    pub play_mode: StationPlayMode,
    /// 同一歌单的全部时段共用优先级。
    pub priority: u16,
    pub scheduled: bool,
    pub slots: Vec<StationSlot>,
}

/// 队列来源的显示信息，不含本地文件路径。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct StationQueueSource {
    pub playlist_id: String,
    pub name: String,
    pub track_count: u32,
    pub available_count: u32,
    pub exists: bool,
}

/// 节目不可用原因；星期以周一为 0，来源名称由 sources 解析。
#[derive(Debug, Clone, Serialize, Deserialize, TS, PartialEq, Eq)]
#[ts(export)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum StationProgramIssue {
    EmptyQueue,
    MissingSource { playlist_id: String },
    EmptySource { playlist_id: String },
    UncoveredTime { weekday: u8, time: String },
}

/// 配置、来源可用性与运行时应用状态。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct StationProgram {
    pub revision: String,
    pub items: Vec<StationQueueItem>,
    pub sources: Vec<StationQueueSource>,
    pub needs_apply: bool,
    pub timezone: Option<String>,
    pub issues: Vec<StationProgramIssue>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct StationProgramEdit {
    pub revision: String,
    pub items: Vec<StationQueueItem>,
}

/// 创建/更新载荷（station_create / station_update）。
#[derive(Debug, Clone, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct StationEdit {
    pub name: String,
    pub subtitle: Option<String>,
    pub description: Option<String>,
    pub cover_url: Option<String>,
    /// private | public；缺省 private
    #[ts(type = "string | null")]
    pub visibility: Option<String>,
}

impl StationEdit {
    pub fn visibility_or_default(&self) -> &str {
        self.visibility.as_deref().unwrap_or("private")
    }
}
