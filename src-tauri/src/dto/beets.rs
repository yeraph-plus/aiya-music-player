//! beets 环境探测与导入回执。
//!
//! `BeetsStatus` 从 `beets.rs` 搬到这里：它是设置页的数据源，是**契约**，
//! 不是 beets 子进程的内部账本。放一起的好处是 `grep dto::` 就能看全
//! IPC 形状；`beets.rs` 里 `pub use` 回来，调用点不用改。
//!
//! docs/33 M2 之后，这份形状里的环境信息**全部由工具自报**（播放器不再探测）；
//! `plugin_health` 就是为此加的：`plugins` 是"配置里写了什么"（愿望清单），
//! `plugin_health` 才是"实际能干哪些活"（三态）。

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// 设置页 beets 状态区数据源。
///
/// 全部字段可空是刻意的：探测失败时不是"没数据"，而是"哪一步拿到了什么"
/// 都要显示——设置页要能告诉用户是没装、装了但没配对、还是配置目录不存在。
/// `error_code` / `error_message` 只在失败时填。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct BeetsStatus {
    pub found: bool,
    /// 工具的启动程序（`aiya-beets.exe`，或源码树里的解释器）。
    pub interpreter: Option<String>,
    /// 启动参数（如 `-m aiya_beets`）。单文件入口时为空。
    pub script_path: Option<String>,
    /// 入口是从哪找到的：env / settings / path / dev。
    ///
    /// 排障第一个问题永远是"为什么用的是这个"，让发现层直接回答，
    /// 而不是让用户去翻日志。
    pub entry_source: Option<String>,
    #[ts(type = "number | null")]
    pub protocol_version: Option<u32>,
    pub beets_version: Option<String>,
    pub python_version: Option<String>,
    /// 运行时是否在支持窗口内（工具自报；None = 老版本工具没报）。
    pub python_supported: Option<bool>,
    pub library_path: Option<String>,
    pub library_exists: Option<bool>,
    /// 库根目录（**显式配置**的 `directory`；None = 没配过）。
    ///
    /// 为什么单独给一个字段：配置里恒有默认值（`~/Music`），拿它显示会误导
    /// 用户；而"整理文件"的目标根只认显式配置的那一个（没配就拒绝执行）。
    /// UI 用它显示目标目录，并在 None 时禁用"整理文件"。
    pub library_directory: Option<String>,
    /// 配置里写着的插件名（愿望清单，不等于能干活——见 `plugin_health`）。
    pub plugins: Option<Vec<String>>,
    /// 插件三态自检结果（工具侧自报）。
    pub plugin_health: Option<Vec<BeetsPluginHealth>>,
    pub plugin_summary: Option<BeetsPluginSummary>,
    pub config_dir: Option<String>,
    /// 工具的数据根（播放器给的那个目录）。
    pub data_home: Option<String>,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
}

/// 单个插件的三态。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct BeetsPluginHealth {
    pub name: String,
    /// `ready` | `missing_deps` | `unavailable`
    pub state: String,
    /// 配置里是否启用了它。
    pub enabled: bool,
    /// 缺什么（Python 包名或外部命令名）；`ready` 时为空。
    pub missing: Vec<String>,
    /// 开它是为了什么（工具侧声明，UI 直接显示）。
    pub note: String,
}

/// 插件状态计数（给 UI 一行话用）。
#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct BeetsPluginSummary {
    #[ts(type = "number")]
    pub ready: u32,
    #[ts(type = "number")]
    pub missing_deps: u32,
    #[ts(type = "number")]
    pub unavailable: u32,
}

/// 插件市场目录项（docs/40 §3 白名单版）。
///
/// 形状与工具侧 `market_report()` 逐字对齐（name/note/source/state/enabled/
/// missing）。`source` 是 `builtin` 或 `pypi:<包名>`——UI 只对 pypi 档且非
/// ready 的项给「安装」按钮；builtin 档随 beets 内置，启停归 config，
/// 目录报告里恒为 false。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct BeetsMarketItem {
    pub name: String,
    pub note: String,
    /// `builtin` | `pypi:<包名>`
    pub source: String,
    /// `ready` | `missing_deps` | `unavailable`
    pub state: String,
    pub enabled: bool,
    /// 缺什么（Python 包名或外部命令名）；`ready` 时为空。
    pub missing: Vec<String>,
}

/// "任务已启动"（preview / apply / sync 三个命令共用）。
///
/// 这些命令是**异步**的：真正进度走 `beets:import` 事件流，返回值只表示
/// "接单了"。所以这里恒为 true——保留字段而不是返回 `()`，是为了将来能
/// 表达"已有会话在跑，这次被忽略"而不改签名。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct JobStarted {
    pub started: bool,
}

/// 取消请求的回执：`false` 表示当时没有可取消的会话（幂等，不算错）。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct CancelRequested {
    pub requested: bool,
}

/// beets 配置快照（docs/40 十三轮：待整理目录 + 唯一整理输出根）。
///
/// 三个键各管一件事：inbox 是"待整理的歌从哪来"，targetRoot 是"整理好
/// 的歌落到哪个曲库目录"，importedCount 是防重复导入的排除表大小。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct BeetsConfig {
    /// 待整理目录（用户自选的外部目录；未设置 = None）
    pub inbox: Option<String>,
    /// 用户勾选的整理输出根（必须是曲库目录之一；未勾选 = None）
    pub target_root: Option<String>,
    /// 实际生效的整理输出根：targetRoot 有效用它，否则回退第一个曲库目录
    /// （兼容"从来没碰过这个勾选框"的老配置——行为与十三轮之前一致）
    pub effective_target: Option<String>,
    /// 已导入记录条数（inbox 排除表，防 copy 模式重复入库）
    pub imported_count: usize,
}
