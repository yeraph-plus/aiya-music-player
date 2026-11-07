//! 推流频道的传输形状（docs/68 §6/§12）。
//!
//! 与 `aiya-data::station` 的分工：DTO 是传输面（camelCase、时间毫秒、
//! track_ids 已解析成数组——UI 不做 JSON.parse）；repo 是持久化面（snake_case、
//! unix 秒、JSON 串）。组装发生在 server 壳（station.rs 命令层）。

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
    /// private | public（docs/68 §12.4）
    pub visibility: String,
    /// 频道级开关（停播保留配置与段目录，再开播热恢复）
    pub active: bool,
    /// 引擎进程在跑（单容器 = spawn 存活；双容器 = keeper .run 位在）
    pub running: bool,
    /// 最近回灌的曲目名（on_track → now.jsonl → 轮询；未播过 = null）
    pub now_playing: Option<String>,
}

/// 频道歌单（管理器编辑用）。manual 的 `track_ids` 已解析成数组。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct StationPlaylist {
    pub id: String,
    pub name: String,
    /// manual | random_all
    pub kind: String,
    #[ts(type = "Array<string>")]
    pub track_ids: Vec<String>,
    /// 规则引擎预留 JSON 串（MVP 恒 null）
    #[ts(type = "string | null")]
    pub rule_json: Option<String>,
}

/// 节目表行（时段 → 歌单绑定；end<=start 表跨零点，bit0=周一…bit6=周日）。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct StationSchedule {
    pub id: String,
    pub playlist_id: String,
    /// 'HH:MM'
    pub start_hm: String,
    pub end_hm: String,
    #[ts(type = "number")]
    pub days_mask: i64,
    #[ts(type = "number")]
    pub priority: i64,
}

/// 管理器编辑载荷（station_get 返回；set_playlists/set_schedules 的入参同形）。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct StationDetail {
    #[serde(flatten)]
    pub summary: StationSummary,
    pub playlists: Vec<StationPlaylist>,
    pub schedules: Vec<StationSchedule>,
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

/// station_set_playlists 入参（整表提交语义，docs/68 §3）。
#[derive(Debug, Clone, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct StationPlaylistEdit {
    pub name: String,
    /// manual | random_all
    pub kind: String,
    #[ts(type = "Array<string> | null")]
    pub track_ids: Option<Vec<String>>,
    #[ts(type = "string | null")]
    pub rule_json: Option<String>,
}

/// station_set_schedules 入参（整表提交）。
#[derive(Debug, Clone, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct StationScheduleEdit {
    pub playlist_id: String,
    pub start_hm: String,
    pub end_hm: String,
    #[ts(type = "number")]
    pub days_mask: i64,
    #[ts(type = "number")]
    pub priority: i64,
}
