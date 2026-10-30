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
    /// v2：宿主拿工具自报的 `schemaHash` 与编译期嵌入的镜像哈希比对的结果。
    ///
    /// 不是工具的线上字段——是宿主算出来的结论（docs/35 P1-4 闭合）。
    /// `false` 时 UI 出警告条并禁用整理按钮：契约不一致时执行变更等于
    /// 让两套对不上口径的代码同时写一个库。
    pub schema_matched: bool,
    /// v2：库文件锁三态（`free|held|unknown`，工具自报原样透传）。
    pub library_lock: Option<String>,
    /// v2：held 时的持有者（`pid=…,cmd=…`）；free/unknown 为 null。
    pub library_lock_holder: Option<String>,
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
    /// 这个插件要装的 pip 包（空 = 不需要额外包）。新版工具才有，
    /// 旧工具缺这个键时要能解析（`default`），否则整条目录静默变 None。
    #[serde(default)]
    pub packages: Vec<String>,
    /// `ready` | `missing_deps` | `unavailable`
    pub state: String,
    pub enabled: bool,
    /// 缺什么（Python 包名或外部命令名）；`ready` 时为空。
    pub missing: Vec<String>,
}

/// 插件表里的一条（**编译期内嵌**，不需要装过组件就能拿到）。
///
/// 来源是 `beets-tool/src/aiya_beets/data/plugins.json`，与工具侧、打包脚本、
/// 安装器读的是同一份字节（docs/51 §1）。装了组件之后 UI 仍可以用
/// `beets_plugin_catalog` 拿三态；这条命令只回答"有哪些插件、各自要装什么"。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct BeetsPluginItem {
    pub name: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub note: String,
    /// 插件随 beets 提供（false = 由 `packages` 里的包提供）
    #[serde(default)]
    pub builtin: bool,
    /// pip 目标（插件包自身 + 依赖），带版本约束
    #[serde(default)]
    pub packages: Vec<String>,
    /// 外部二进制（ffmpeg / fpcalc …），只提示不代装
    #[serde(default)]
    pub exe: Vec<String>,
    /// 默认会写音频文件或产出文件（索引家结构上不收）
    #[serde(default)]
    pub writes: bool,
    #[serde(default)]
    pub network: bool,
    /// 免配置包预置
    #[serde(default)]
    pub bundle: bool,
    /// 首次生成 config 时启用（由 JSON 的 `profiles.workbench` 注入）
    #[serde(default)]
    pub default: bool,
    /// 索引家默认启用（由 `profiles.index` 注入）
    #[serde(default)]
    pub index_default: bool,
}

/// 本机 Python 探测结果（路径 B 的前置条件）。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct BeetsPythonProbe {
    pub found: bool,
    /// 绝对路径（`py -0p` 给的就是绝对路径）
    pub path: Option<String>,
    /// `3.12.4` 这样的版本号
    pub version: Option<String>,
    /// 是否在工具侧的支持窗口内（>=3.10 <3.15）
    pub supported: bool,
    /// 探测来源：`py-launcher` | `python` | `python3`
    pub source: Option<String>,
    /// 找不到时的说明（给 UI 直接显示，不要让它自己编文案）
    pub reason: Option<String>,
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

/* ---------------- beets 查询页（docs/47） ---------------- */

/// 查询命中行。
///
/// 刻意**不带 `track_id`**：这是 beets 库里的条目，不是应用曲库里的曲目。
/// 两者能对上只是因为有同一个文件——"存为歌单"时才按 `path` 去曲库查一次，
/// 查不到就计入"未入库"（而不是现造一行假曲目）。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct BeetsQueryHit {
    /// beets 自己的 item id。只在同一个库里成立，**不要**拿它当稳定键。
    #[ts(type = "number")]
    pub id: i64,
    /// 文件绝对路径——跨库唯一能对上的东西。
    pub path: String,
    /// 回带字段（`title`/`artist`/`album` 恒有，其余看用户勾了什么）。
    #[ts(type = "Record<string, unknown>")]
    pub fields: std::collections::BTreeMap<String, serde_json::Value>,
}

/// 一页查询结果。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct BeetsQueryPage {
    pub items: Vec<BeetsQueryHit>,
    /// **应用 limit 之前**的真实命中数（工具侧 `SELECT COUNT(*)`，不反序列化）。
    pub total: u32,
    /// 还有更多没返回。
    pub truncated: bool,
}

/// 一个可查字段。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct BeetsField {
    pub name: String,
    /// integer / float / string / bool / date / duration / flexible
    #[serde(rename = "type")]
    pub kind: String,
    /// builtin（beets 内置）/ flexible（插件写的）
    pub source: String,
}

/// 字段清单 + 识别源三态。
///
/// `sources` 保持原样透传而不是建模成固定结构：源是工具侧的概念（装没装、
/// 接没接线），宿主只负责显示。给一个 `{name, state}` 的结构会诱使宿主去
/// 判断"这个源能不能用"，然后就等着两边漂移。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct BeetsFieldCatalog {
    pub fields: Vec<BeetsField>,
    #[ts(type = "unknown")]
    pub sources: serde_json::Value,
}
