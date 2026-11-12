//! beets 侧工具（`aiya-beets`）的宿主侧：**发现入口 + 结果落地**。
//!
//! 这个文件里没有 beets 的环境知识：
//! 没有 include_str!、没有解释器探测、没有 config.yaml 拼装、没有插件清单。
//! 播放器只做两件事：
//!   ① 找到工具的入口（四个来源，见 `discover`），带上数据目录起进程；
//!   ② 把工具报回来的结果落地（协议形状 → UI DTO、dump → 自有库对账）。
//!
//! 工具怎么装、装到哪、配置怎么写、开哪些插件，全归 `beets-tool/` 那边。
//! 唯一共享的东西是数据目录约定 `{app_data}/beets`。
//!
//! **未安装时核心播放功能必须全通**——beets 是增强能力，不是依赖。
use crate::{BeetsHost, InstallGuard};

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use aiya_beets_client::{BeetsTool, EventSink, Launcher, StatusReport, StdioTool, ToolError};

/// 设置页 beets 状态区数据源。形状定义在 `dto::beets`（契约归 dto），
/// 这里 `pub use` 暴露，本文件其余调用点不用改。
pub use aiya_dto::beets::BeetsStatus;

/* 发现 */

/// 工具入口：程序 + 参数。
///
/// 为什么是「程序 + 参数」而不是单个可执行路径：入口有两种形态——便携包
/// 解释器（`<bundle>/python.exe -m aiya_beets`，带参数）与独立可执行
/// （`aiya-beets.exe`，无参数）。统一成这个形状后，两种形态共用同一套代码。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolEntry {
    pub program: String,
    pub args: Vec<String>,
}

/// 入口是从哪找到的。UI/日志要显示它——"为什么用的是这个"是排障的第一个问题。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntrySource {
    /// `AIYA_BEETS_BIN` 环境变量（开发/排障）
    Env,
    /// 设置里记录的路径（安装器写的）
    Settings,
    /// PATH 上的 `aiya-beets`
    Path,
}

impl EntrySource {
    pub fn as_str(self) -> &'static str {
        match self {
            EntrySource::Env => "env",
            EntrySource::Settings => "settings",
            EntrySource::Path => "path",
        }
    }
}

/// 数据目录约定：`{app_data}/beets`。
///
/// 这是播放器与工具**唯一共享的约定**：播放器决定"数据放哪"
/// （应用数据目录归宿主管），工具决定"里面怎么摆"。不传的话，工具会去
/// `%APPDATA%\beets`，与用户自己的 beets 抢同一个库。
pub fn data_home(host: &dyn BeetsHost) -> PathBuf {
    host.data_root().join("beets")
}

/// beets 的"双家"：工作台家与索引家。
///
/// 为什么必须要两个家（不是"一个家两种模式"）：
/// - **工作台家**（`{app_data}/beets`）是用户自己的 beets 库，允许写标签、
///   允许整理文件——"整理库/重命名/写回标签"这四件套是它的正当职责。
/// - **索引家**（`{app_data}/beets-index`）只为一件事存在：**分析**。分析要
///   把文件重导一遍（`beet import -R`），如果跑在用户库上，那一步会按用户的
///   config 写标签、搬文件，甚至因为 `duplicate_action` 把条目删掉重来。
///   索引家把 `directory` 钉在 `{home}/scratch`、写盘开关全部关死，最坏
///   结果是"库里多几条记录"，不是"用户的歌被改了"。
///
/// 两个家必须由**宿主显式下发**（`AIYA_BEETS_HOME` + `BEETSDIR` + `AIYA_BEETS_PROFILE`），
/// 不能让工具自己猜：猜错的后果是拿用户库当草稿纸。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Profile {
    /// 用户的 beets 库：整理、写回、查询、建歌单
    Workbench,
    /// 应用独占的只读分析库：索引增强
    Index,
}

impl Profile {
    fn name(self) -> &'static str {
        match self {
            Profile::Workbench => "workbench",
            Profile::Index => "index",
        }
    }

    fn home(self, host: &dyn BeetsHost) -> PathBuf {
        match self {
            Profile::Workbench => data_home(host),
            Profile::Index => index_home(host),
        }
    }
}

/// 索引家的数据目录：`{app_data}/beets-index`。
///
/// 与工作台家并列，**不是**它的子目录：beets 会往 home 里放 `config.yaml`、
/// `library.db`、`state.pickle`，两个家共用父目录只会让"手滑清了工作台"
/// 顺带清掉索引。
pub fn index_home(host: &dyn BeetsHost) -> PathBuf {
    host.data_root().join("beets-index")
}

/// beets 模块自有的派生数据（索引增强结论）。
///
/// 放在索引家里而不是应用库里：见 `crates/aiya-beets-store` 的头注。一句话，
/// 它是 beets 的中间产物，应用的数据模型里不该有它。
pub fn enrich_db(host: &dyn BeetsHost) -> PathBuf {
    index_home(host).join("enrich.db")
}

/* 模块自有配置：不在应用 settings 表里 */

/// beets 模块自己的配置，落在 `{data_home}/settings.json`。
///
/// ## 为什么不放在应用的 settings 表
///
/// 这些键（`beets.toolPath` / `beets.targetRoot` / `beets.inboxImported` …）
/// 不是应用设置：应用从不读它们来做任何决定，只有 beets 模块自己读。放进
/// 应用表里有两个真实代价：
///
/// 1. **数据模型耦合**：应用的 settings 里出现"因为装了 beets 才有"的键，
///    备份、迁移、"导出我的设置"都会被它们污染。
/// 2. **写路径耦合**：让**应用**在曲库根变化时去改 beets 的配置，方向反了；
///    读时校验（target 不在扫描根里就当没设，目录加回来时自己恢复）没有
///    这个问题，也比"被悄悄清掉"更好。
///
/// ## 为什么是 json 不是又一张表
///
/// 五个标量 + 一个路径数组，没有查询需求。开一个 SQLite 文件只为存这几十字节，
/// 换来的是又一份 schema、又一次迁移。
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// 用户显式指定的解释器/可执行文件（设置里填的那个）
    pub tool_path: Option<String>,
    pub tool_args: Option<String>,
    /// 整理输出根（必须是某个扫描根）
    pub target_root: Option<String>,
    /// 待整理目录
    pub inbox_path: Option<String>,
    /// 已从待整理目录成功导入的源路径（FIFO，防重复）
    pub inbox_imported: Vec<String>,
    /// 用户在安装时勾选的插件（**启用**清单，不是包清单）。
    ///
    /// 装了不启用等于白装：`config.yaml` 的 `plugins:` 行由工具侧渲染，
    /// 它读这个键（`cfg._sync_plugins`）。`None` = 没装过/没选过，
    /// 工具侧回落到 `plugins.json` 的 `profiles.workbench`。
    pub plugins: Option<Vec<String>>,
}

/// 读改写串行化：配置只有几 KB，但"读 → 改 → 写"不持锁会在两个命令同时
/// 到达时丢掉其中一次的改动（用户看到的是"设置保存了又变回去"）。
static SETTINGS_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

pub fn settings_path(host: &dyn BeetsHost) -> PathBuf {
    data_home(host).join("settings.json")
}

/// 读配置。任何读取失败（不存在、坏 JSON、权限）都回落默认值：
/// 一份坏掉的配置文件不该让 beets 模块整体不可用——用户重填一次就好。
pub fn load_settings(host: &dyn BeetsHost) -> Settings {
    load_at(&settings_path(host))
}

/// 路径版（可测）：坏文件 / 不存在 / 权限不足一律回落默认值。
fn load_at(path: &std::path::Path) -> Settings {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

/// 原子写：先写 `.tmp` 再 rename。
///
/// 直接覆盖写有两个失败模式：写到一半断电得到半个 JSON（下次读回落默认值，
/// 用户的配置静默消失），以及并发读到空文件。rename 在 Windows 上同名替换
/// 是原子的（同卷）。
fn write_settings(host: &dyn BeetsHost, s: &Settings) -> Result<(), String> {
    write_at(&settings_path(host), s)
}

/// 路径版（可测）。临时文件名带进程号：两个实例同时写时，共用 `.tmp`
/// 会让后写的把先写的内容 rename 进去（丢一次改动）。
fn write_at(path: &std::path::Path, s: &Settings) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let tmp = path.with_extension(format!("json.{}.tmp", std::process::id()));
    let body = serde_json::to_vec_pretty(s).map_err(|e| e.to_string())?;
    std::fs::write(&tmp, body).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        e.to_string()
    })
}

/// 读 → 改 → 写。返回值是闭包的返回值，便于"改完把新视图给调用方"。
pub fn update_settings<R>(
    host: &dyn BeetsHost,
    f: impl FnOnce(&mut Settings) -> R,
) -> Result<R, String> {
    let _guard = SETTINGS_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut s = load_settings(host);
    let out = f(&mut s);
    write_settings(host, &s)?;
    Ok(out)
}

/// 待整理目录排除表的硬上限：超过丢最老的。
///
/// 2000 条对"待整理目录"是够宽裕的滚动窗口，同时挡住"常年 copy 用户"
/// 把配置文件撑到几 MB。
pub const INBOX_IMPORTED_CAP: usize = 2000;

/// 记下"已成功导入的源文件"（排除表，防 copy 模式重复入库）。
///
/// 幂等：同一路径（大小写/分隔符不敏感）只留一条。
pub fn remember_imported(host: &dyn BeetsHost, paths: Vec<String>) -> Result<(), String> {
    update_settings(host, move |s| {
        for p in paths {
            let p = p.trim();
            if p.is_empty() {
                continue;
            }
            let pc = crate::canon_path(p);
            if !s.inbox_imported.iter().any(|e| crate::canon_path(e) == pc) {
                s.inbox_imported.push(p.to_string());
            }
        }
        if s.inbox_imported.len() > INBOX_IMPORTED_CAP {
            let drop = s.inbox_imported.len() - INBOX_IMPORTED_CAP;
            s.inbox_imported.drain(0..drop);
        }
    })
}

/// 配置视图（UI 的待整理目录行与输出根 radio 都从这里读）。
///
/// `roots` 由调用方给（应用知道用户的曲库目录，本模块不该自己查）：
/// `effective_target` 就是"用户勾的和曲库现状对不上时该用哪个"这条规则，
/// 规则在这里，事实从外面来。
pub fn config_view(host: &dyn BeetsHost, roots: &[String]) -> aiya_dto::beets::BeetsConfig {
    config_view_of(&load_settings(host), roots)
}

/// 纯函数版（可测）：配置 + 曲库目录 → 视图。
///
/// 落点合法性走 [`pick_dest`]，与 `beets_set_target` 的校验、`tool_env` 的
/// 下发**同一套判据**：三处各写各的时，放宽子目录会漏掉其中一处——UI 显示
/// 曲库根而 `tool_env` 已下发子目录。
fn config_view_of(s: &Settings, roots: &[String]) -> aiya_dto::beets::BeetsConfig {
    let target = trimmed(s.target_root.clone());
    let effective = pick_dest(target.clone(), roots);
    aiya_dto::beets::BeetsConfig {
        inbox: trimmed(s.inbox_path.clone()),
        target_root: target,
        effective_target: effective,
        imported_count: s.inbox_imported.len(),
    }
}

/// 把应用 settings 表里的旧键一次性搬进来（幂等）。
///
/// 只在 `settings.json` 还不存在时执行：用户已经在新文件里改过配置之后，
/// 再用旧值覆盖他等于"每次启动都回滚一次设置"。
///
/// 返回 `true` 表示真的搬过（调用方据此清掉旧键）。
pub fn adopt_legacy_settings(host: &dyn BeetsHost, legacy: Settings) -> bool {
    let path = settings_path(host);
    if path.exists() {
        return false;
    }
    write_settings(host, &legacy).is_ok()
}

/// 一次取两个事实：**曲库根**（索引家的分析对象）与**整理落点**（整理家的
/// `directory`，copy/move 的目标）。
///
/// 为什么必须分开：落点常常只是曲库里一个子目录（`曲库/整理`）。
/// 两者合成一个变量时，用户改落点会把索引范围一起缩成那一小块——症状是
/// "索引跑完了，可查询里只有几条"，而配置看起来完全合规。
///
/// 优先级：
/// * 曲库根 = **第一个扫描根**，永远不取勾选的落点；
/// * 落点 = 勾选值（若它仍是某个扫描根）否则退回第一个扫描根；老配置（没有
///   勾选值）零迁移。
fn roots_for_tools(host: &dyn BeetsHost) -> (Option<String>, Option<String>) {
    // 勾选值来自**本模块自己的配置**，扫描根来自**应用**（用户事实）。两者分开取：配置和应用库各有各的写者，混在一次读里会让"谁的目录算数" 这个问题有两个答案。
    let target = trimmed(load_settings(host).target_root);
    host.library_roots()
        .map(|roots| (roots.first().cloned(), pick_dest(target, &roots)))
        .unwrap_or((None, None))
}

/// 落点取哪个：勾选值合法就用它，否则回退第一个扫描根。
///
/// 合法性判据与 `beets_set_target` **必须同一条**（复用同一个函数）：落点可以
/// 是曲库目录本身，也可以是它的子目录。
///
/// 判据分离的后果：「选了子目录当落点」被静默判为非法、回退成曲库根——设置里
/// 明明写着 `D:\Music\整理`，实际整理却把文件倒进 `D:\Music`。由 `pick_dest_*`
/// 的单测钉住。
fn pick_dest(target: Option<String>, roots: &[String]) -> Option<String> {
    target
        .filter(|t| crate::path_within_roots(t, roots))
        .or_else(|| roots.first().cloned())
}

/// 去空白 + 空串归一成 `None`（配置里空串和没填是同一件事）。
fn trimmed(v: Option<String>) -> Option<String> {
    v.map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

/// 参数串切分：空白分隔，支持用双引号包住带空格的项。
///
/// 不为它引入 shell 词法库——只需要覆盖 `-m aiya_beets` 和
/// `"C://Program Files//x//y.exe" --flag` 这两种。
fn split_args(raw: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    for ch in raw.chars() {
        match ch {
            '"' => quoted = !quoted,
            c if c.is_whitespace() && !quoted => {
                if !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
            }
            c => cur.push(c),
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// PATH 上找可执行文件（Windows 按 PATHEXT 试后缀）。
fn find_on_path(name: &str) -> Option<PathBuf> {
    let exts: Vec<String> = if cfg!(windows) {
        std::env::var("PATHEXT")
            .unwrap_or_else(|_| ".EXE;.CMD;.BAT".into())
            .split(';')
            .map(|s| s.to_ascii_lowercase())
            .filter(|s| !s.is_empty())
            .collect()
    } else {
        vec![String::new()]
    };
    let paths = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&paths) {
        for ext in &exts {
            let cand = dir.join(format!("{name}{ext}"));
            if cand.is_file() {
                return Some(cand);
            }
        }
    }
    None
}

/// 按优先级找工具入口。
///
/// 顺序理由：显式指定永远第一 → 安装器写下的路径最可靠 → PATH 兜底。播放器
/// **不维护第二套环境**：开发机上要跑 beets 功能，与用户一样走安装流。
///
/// **刻意没有"猜解释器布局"这一条**：设置里的入口是安装器**写下的**，不是
/// 探测出来的。播放器不关心解释器怎么布局，只记住"入口在哪"。
pub fn discover(host: &dyn BeetsHost) -> Option<(ToolEntry, EntrySource)> {
    if let Ok(program) = std::env::var("AIYA_BEETS_BIN") {
        if !program.trim().is_empty() {
            let args = std::env::var("AIYA_BEETS_BIN_ARGS")
                .map(|s| split_args(&s))
                .unwrap_or_default();
            return Some((ToolEntry { program, args }, EntrySource::Env));
        }
    }

    let cfg = load_settings(host);
    if let Some(program) = trimmed(cfg.tool_path) {
        let args = trimmed(cfg.tool_args)
            .map(|s| split_args(&s))
            .unwrap_or_default();
        if PathBuf::from(&program).is_file() {
            return Some((ToolEntry { program, args }, EntrySource::Settings));
        }
        tracing::warn!(path = %program, "设置里的 beets.toolPath 已不存在，继续找下一个来源");
    }

    if let Some(found) = find_on_path("aiya-beets") {
        return Some((
            ToolEntry {
                program: found.to_string_lossy().into_owned(),
                args: Vec::new(),
            },
            EntrySource::Path,
        ));
    }

    None
}

/// 起进程时的环境。只给**事实**，不给 beets 知识。
///
/// * `AIYA_BEETS_HOME`：数据根（播放器给）
/// * `BEETSDIR`：**必须显式下发且与上面一致**
/// * `AIYA_BEETS_SCAN_ROOT`：**曲库根**（索引家的分析对象，永不取落点）
/// * `AIYA_BEETS_DEST`：**整理落点**（整理家写进 `directory`，copy/move 的目标）
/// * `PYTHONIOENCODING` / `PYTHONUTF8`：管道编码钉 UTF-8。工具内部也 reconfigure
///   了一次，这里是双保险（工具被替换/被单独调用时仍正确）。这两个是 Python
///   通用变量，不是 beets 知识，留着不亏。
///
/// ## 为什么要显式下发 `BEETSDIR`
///
/// 工具侧的家（`config.yaml` / 锁 / ops 登记簿 / 断点）全按 `AIYA_BEETS_HOME`
/// 定位，而 beets 自己只认 `BEETSDIR`。外部环境里残留的 `BEETSDIR` 会让
/// **beets 写 A 家、锁建在 B 家**：双家隔离静默失效，症状是"索引跑完了
/// 却查不到"。所以两侧都钉死：宿主显式覆盖（优先于继承值），
/// 工具侧在两者不等时 fail-fast。
fn tool_env(host: &dyn BeetsHost, profile: Profile) -> Vec<(String, String)> {
    let (root, dest) = roots_for_tools(host);
    env_pairs(&profile.home(host), root, dest, profile)
}

/// `tool_env` 的纯函数部分：给定事实 → 环境键值对（可单测，不需要 AppHandle）。
///
/// `AIYA_BEETS_PROFILE` **必须显式下发**，即便值是默认的 `workbench`：
/// 工具侧把"没收到"当默认是它的宽容，不是宿主的许可证。一旦哪天默认改了，
/// 靠隐式默认的调用方会静默换家——分析跑进用户库这种事不该由默认值决定。
fn env_pairs(
    home: &std::path::Path,
    root: Option<String>,
    dest: Option<String>,
    profile: Profile,
) -> Vec<(String, String)> {
    let home_str = home.to_string_lossy().into_owned();
    let mut env = vec![
        ("PYTHONIOENCODING".to_string(), "utf-8".to_string()),
        ("PYTHONUTF8".to_string(), "1".to_string()),
        // 工具侧硬约束：`BEETSDIR` 与 `AIYA_BEETS_HOME` 不同值就拒跑
        //（否则锁与库分家）。两个键都由这里给，值必然相同。
        ("AIYA_BEETS_HOME".to_string(), home_str.clone()),
        ("BEETSDIR".to_string(), home_str),
        ("AIYA_BEETS_PROFILE".to_string(), profile.name().to_string()),
    ];
    if let Some(root) = root {
        env.push(("AIYA_BEETS_SCAN_ROOT".to_string(), root));
    }
    if let Some(dest) = dest {
        env.push(("AIYA_BEETS_DEST".to_string(), dest));
    }
    env
}

/// 入口 + 环境 → 可直接起进程的描述。
fn launcher_for(host: &dyn BeetsHost, entry: &ToolEntry, profile: Profile) -> Launcher {
    Launcher::new(&entry.program)
        .args(entry.args.clone())
        .envs(tool_env(host, profile))
}

/// 事件出口：把 client 抽象出来的通道接到 Tauri 事件上。
///
/// 这层适配只有一行实质代码，但它把「crate 不依赖 Tauri」变成硬约束——
/// client 里再也写不出 `host.emit(...)`，宿主形态也就不会渗回协议层。
/// 协议版本闸门：`None` = 兼容；`Some(文案)` = 拒用，并在文案里说清升哪一边。
///
/// 为什么不做自动协商：协商会产生"一半功能能用一半不能"的
/// 模糊状态，比直接失败更难排查。两侧各自独立升级，唯一安全的做法是**要么
/// 完全匹配、要么明确拒绝**。
///
/// 注意 `found: false` 的语义是"这个工具现在不可用"，不是"没装"：
/// 入口、版本、数据根照样全部显示出来，用户能看到"我装的是 v3，它要 v1"。
fn version_mismatch(got: Option<u32>) -> Option<String> {
    let want = aiya_beets_client::PROTOCOL_VERSION;
    match got {
        Some(v) if v == want => None,
        Some(v) if v < want => Some(format!(
            "beets component protocol v{v}, player needs v{want}: update the component",
        )),
        Some(v) => Some(format!(
            "component protocol v{v} is newer than the player's v{want}: update the player",
        )),
        None => Some(format!(
            "component did not report a protocol version (expected v{want}): reinstall the component",
        )),
    }
}

/// 协议报告 → UI 契约。
///
/// 两边字段名不同是刻意的：crate 用中性词（`tool_version` / `runtime_version` /
/// `library_root`，因为它不知道对面是什么），UI 沿用用户看得懂的说法。
/// 转换只在这一处，改协议时只要改这个函数。
fn status_from(entry: &ToolEntry, source: EntrySource, report: StatusReport) -> BeetsStatus {
    let mismatch = version_mismatch(report.protocol_version);
    // 先算结论再移字段：schema_matches 只借 report，字面量会把字段 move 走
    let matched = schema_matches(&report);
    BeetsStatus {
        found: mismatch.is_none(),
        interpreter: Some(entry.program.clone()),
        script_path: (!entry.args.is_empty()).then(|| entry.args.join(" ")),
        entry_source: Some(source.as_str().into()),
        protocol_version: report.protocol_version,
        beets_version: report.tool_version,
        python_version: report.runtime_version,
        library_path: report.library_path,
        library_exists: report.library_exists,
        library_directory: report.library_root,
        plugins: report.plugins,
        // 能力段是工具自报的 JSON，这里才落地成 UI 形状。
        // 解析失败不算致命：老版本工具没这个字段，UI 会显示"未知"。
        plugin_health: report
            .capability_health
            .and_then(|v| serde_json::from_value(v).ok()),
        plugin_summary: report
            .capability_summary
            .and_then(|v| serde_json::from_value(v).ok()),
        config_dir: report.config_dir,
        data_home: report.data_home,
        schema_matched: matched,
        library_lock: report.library_lock,
        library_lock_holder: report.library_lock_holder,
        error_code: mismatch.as_ref().map(|_| "E_PROTOCOL_MISMATCH".to_string()),
        error_message: mismatch,
    }
}

/// 编译期嵌入的镜像契约指纹（`protocol/aiya-beets-v2.meta.json` 的 sha256）。
///
/// 工具自报的 `schemaHash` 与它一比就知道"装的是不是同一版契约"——版本号
/// 相同但 schema 字节不同（装错版本、手改过文件）骗不过宿主。meta 是提交
/// 进仓的合法 JSON，include_str 把"文件不存在"变成编译错误。
fn expected_schema_sha256() -> &'static str {
    static HASH: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    HASH.get_or_init(|| {
        serde_json::from_str::<serde_json::Value>(include_str!(
            "../../../protocol/aiya-beets-v2.meta.json"
        ))
        .ok()
        .and_then(|m| m.get("sha256").and_then(|v| v.as_str()).map(str::to_string))
        .unwrap_or_default()
    })
}

/// 契约指纹比对：工具没报（老版本）或对不上都算不匹配——严格失败，不猜
/// （与 E_LIBRARY_DIR_MISMATCH 同一条设计准则）。
fn schema_matches(report: &StatusReport) -> bool {
    match &report.schema_hash {
        Some(h) => h.eq_ignore_ascii_case(expected_schema_sha256()),
        None => false,
    }
}

/// 探测失败时的空壳（带诊断）。`entry` 有值时保留入口信息——用户需要知道
/// "试着用了哪个程序"，否则报错只有一句"不可用"。
fn absent(
    code: &str,
    message: Option<String>,
    entry: Option<&ToolEntry>,
    source: Option<EntrySource>,
) -> BeetsStatus {
    BeetsStatus {
        found: false,
        interpreter: entry.map(|e| e.program.clone()),
        script_path: entry.and_then(|e| (!e.args.is_empty()).then(|| e.args.join(" "))),
        entry_source: source.map(|s| s.as_str().to_string()),
        protocol_version: None,
        beets_version: None,
        python_version: None,
        library_path: None,
        library_exists: None,
        library_directory: None,
        plugins: None,
        plugin_health: None,
        plugin_summary: None,
        config_dir: None,
        data_home: None,
        // 没拿到报告就谈不上"契约匹配"；UI 对 found:false 本来就整体禁用
        schema_matched: false,
        library_lock: None,
        library_lock_holder: None,
        error_code: Some(code.into()),
        error_message: message,
    }
}

/// 一次 status 查询。
///
/// 两种失败要分开：
/// - 工具**明确说不行**（`Rejected`）→ `Ok(found:false)`：程序活着但环境不全，
///   这是"有诊断信息"的正常结果，设置页要原样显示那段诊断。
/// - 起不来 / 超时 / 协议乱 → `Err`，由 `probe` 包成不可达。
fn query_sidecar(
    host: &dyn BeetsHost,
    entry: &ToolEntry,
    source: EntrySource,
    timeout: Duration,
) -> Result<BeetsStatus, ToolError> {
    let tool = StdioTool::new(launcher_for(host, entry, Profile::Workbench));
    match tool.status(timeout) {
        Ok(report) => Ok(status_from(entry, source, report)),
        // 工具自己回了 ok:false —— 那是"真的不可用"，带着它给的码
        Err(ToolError::Rejected {
            code,
            stage,
            message,
        }) => Ok(absent(
            &code,
            Some(format!("[{stage}] {message}")),
            Some(entry),
            Some(source),
        )),
        // 其余原样上抛：Busy 与真正的不可达必须由调用方分开处理
        Err(e) => Err(e),
    }
}

/// 会话槽被占时的状态：组件**在**，只是这一刻读不到细节。
///
/// 不能复用 [`absent`]：那条会把 `found` 置 false，UI 于是显示「未安装 beets」
/// 并跳去安装页——而真相是预览/整理还在跑（整理一个批次
/// 要发很多次查询），期间页面自己跳去安装页，看起来像组件凭空没了。
///
/// `schema_matched` 保持 false（没拿到报告就谈不上契约匹配），UI 因此会禁用
/// 写操作——这正合适：槽被占时本来也发不出写命令。
fn busy_status(entry: &ToolEntry, source: EntrySource) -> BeetsStatus {
    BeetsStatus {
        found: true,
        interpreter: Some(entry.program.clone()),
        script_path: (!entry.args.is_empty()).then(|| entry.args.join(" ")),
        entry_source: Some(source.as_str().to_string()),
        protocol_version: None,
        beets_version: None,
        python_version: None,
        library_path: None,
        library_exists: None,
        library_directory: None,
        plugins: None,
        plugin_health: None,
        plugin_summary: None,
        config_dir: None,
        data_home: None,
        schema_matched: false,
        library_lock: Some("held".into()),
        library_lock_holder: Some("this host is running a beets task".into()),
        error_code: Some("E_BEETS_BUSY".into()),
        error_message: Some("the component is running another task".into()),
    }
}

/// 当前状态。找不到入口 → not_installed（UI 显示安装指引，播放功能不受影响）。
pub fn probe(host: &dyn BeetsHost) -> BeetsStatus {
    let Some((entry, source)) = discover(host) else {
        return absent(
            "E_BEETS_NOT_INSTALLED",
            Some(
                "aiya-beets component not found: install it below or point to an existing path"
                    .into(),
            ),
            None,
            None,
        );
    };
    match query_sidecar(host, &entry, source, Duration::from_secs(10)) {
        Ok(status) => status,
        // 会话槽被占：组件在，只是这一刻问不到明细。**不能**当成不可达，
        // 否则"正在跑整理"会被 UI 读成"beets 没装"。
        Err(ToolError::Busy) => busy_status(&entry, source),
        Err(e) => absent(
            "E_BEETS_UNREACHABLE",
            Some(e.to_string()),
            Some(&entry),
            Some(source),
        ),
    }
}

/* 安装：单轨（便携包）播放器只做四件事：下载 → 展开 → 记下入口 → 跑一次 status 验证。它**不建 venv、不装 pip 包、不碰 Python 工具链、不探测系统 Python**—— 唯一的分发物是便携包（自带解释器与 pip）。换插件同样不出这套动作：操作桥 spawn 包内解释器跑
`install --plugins …`（见 run_env_manager）。成功判据刻意不是"退出码 0"，而是"装完 probe 得到 found"。退出码可能骗人：pip 装到一半也能 0，套件包解压成功也可能缺 DLL。 */

/// 发布物的默认位置。
///
/// 优先级见 [`release_base`]：环境变量 > 设置 > 这里。做成可配置而不是硬编码，
/// 是因为"发布到哪"是**发布流程**的事，不该逼着播放器改代码再发版；
/// 也让下载路径能在本地用 `python -m http.server` 真实走一遍
/// （`AIYA_BEETS_RELEASE_BASE=http://127.0.0.1:8000`）。
///
/// 通道 tag 是 `beets-portable` 而不是 `beets-latest`：beets 包每次更新都要整套
/// 实测，不自称 latest；版本 release 与通道一律 prerelease，仓库 "Latest" 永远
/// 只落在播放器上。
///
/// 为什么不用 `releases/latest/download`：它指"最近创建的 release"，而 beets
/// 与播放器是两个独立发布节奏，latest 会在两边之间来回指、时不时 404。
///
/// `release-beets.yml` 每次发布都会把同样两个资产 `--clobber` 到
/// `beets-portable` 这个 prerelease 上，所以这里的 URL 永久稳定，与播放器发布
/// 完全解耦。用浮动 tag 而不是查 GitHub API：零限流、零 JSON 解析、出问题时
/// `git ls-remote --tags` 就能看见。
///
/// 下载失败会给"下到了什么"的明确错误（见 `verify_asset`），而不是静默卡住。
const RELEASE_BASE: &str =
    "https://github.com/yeraph-plus/aiya-music-player/releases/download/beets-portable";
const BUNDLE_ASSET_WIN: &str = "beets-portable-win-x64.zip";
/// Linux 轨产物：服务器容器走 `AIYA_BEETS_BUNDLE` 本地文件，
/// 这个名字只在"无本地覆盖、按平台下载"时被用到。
const BUNDLE_ASSET_LINUX: &str = "beets-portable-linux-x64.zip";

fn bundle_asset() -> &'static str {
    if cfg!(target_os = "windows") {
        BUNDLE_ASSET_WIN
    } else {
        BUNDLE_ASSET_LINUX
    }
}

/// 发布物根地址：`AIYA_BEETS_RELEASE_BASE` > 默认常量。
///
/// schema、mock 里都没有入口，也就是**写了没人能设**——三层优先级实际只有两层
/// 生效，剩下一层是死分支。真要支持镜像源，顺序是先给入口再留分支，反过来等于
/// 没做。环境变量这一层够用：CI 与本地演练都用它指过去。
///
/// 尾斜杠统一去掉：`{base}/{asset}` 拼出来不能出现 `//`（有些静态服务器会 404）。
fn release_base() -> String {
    let raw = std::env::var("AIYA_BEETS_RELEASE_BASE")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| RELEASE_BASE.to_string());
    raw.trim().trim_end_matches('/').to_string()
}

fn install_root(host: &dyn BeetsHost) -> PathBuf {
    host.data_root().join("beets-tool")
}

/// 发布物的形状：zip 容器。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AssetKind {
    /// 便携包：`beets-portable-win-x64.zip`
    Bundle,
}

impl AssetKind {
    fn label(self) -> &'static str {
        match self {
            AssetKind::Bundle => "portable bundle (.zip)",
        }
    }
}

/// 下载到本地（覆盖写）。
///
/// 抽成独立函数是为了能被测：真机走一遍 release 下载要等发布物，
/// 而这条路径上的坑（404 页面被当成二进制、"成功"写了 0 字节）与网络无关，
/// 用本地 TCP 服务就能复现。
/// 下载上限：套件包实际约 28MB，留约 9 倍余量。
///
/// 为什么要有：`std::io::copy` 会一直写到磁盘满。`verify_asset` 是**下载完
/// 才跑**的，那时几个 GB 已经落盘——地址写错（指到大文件）、或被劫持到别的
/// 主机，代价由用户的磁盘承担。声明长度超限时连写都不开始。
const ASSET_MAX_BYTES: u64 = 256 * 1024 * 1024;

fn download_to(url: &str, dest: &std::path::Path) -> Result<u64, String> {
    use std::io::Read as _;
    let resp = ureq::get(url)
        .timeout(std::time::Duration::from_secs(120))
        .call()
        .map_err(|e| format!("download failed ({url}): {e}"))?;
    if let Some(len) = resp
        .header("content-length")
        .and_then(|v| v.trim().parse::<u64>().ok())
    {
        if len > ASSET_MAX_BYTES {
            return Err(format!(
                "download declares {} MB, over the {} MB limit (wrong address?): {url}",
                len / 1024 / 1024,
                ASSET_MAX_BYTES / 1024 / 1024
            ));
        }
    }
    let mut out =
        std::fs::File::create(dest).map_err(|e| format!("failed to write temp file: {e}"))?;
    // take(上限+1)：多读 1 字节用来区分"正好在上限"与"超了"
    let mut body = resp.into_reader().take(ASSET_MAX_BYTES + 1);
    let n = std::io::copy(&mut body, &mut out)
        .map_err(|e| format!("failed to write the download: {e}"))?;
    if n > ASSET_MAX_BYTES {
        drop(out);
        let _ = std::fs::remove_file(dest);
        return Err(format!(
            "download exceeded the {} MB limit, aborted (wrong address?): {url}",
            ASSET_MAX_BYTES / 1024 / 1024
        ));
    }
    Ok(n)
}

/// 下载/落盘之后的形状校验。
///
/// 为什么必须有：release 还没发布时，GitHub 返回的是一个 200 的 HTML 页面
/// （"Not Found"），`ureq` 不认为是错误。不校验的话，用户看到的是后面
/// `ZipArchive::new` 的一句"不是有效 zip"，完全指不到"没发布/地址错了"。
fn verify_asset(path: &std::path::Path, kind: AssetKind) -> Result<usize, String> {
    let size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    if size == 0 {
        return Err(format!(
            "{} is an empty file (download did not succeed)",
            kind.label()
        ));
    }
    let file =
        std::fs::File::open(path).map_err(|e| format!("failed to open {}: {e}", kind.label()))?;
    match zip::ZipArchive::new(file) {
        Ok(z) if !z.is_empty() => Ok(z.len()),
        Ok(_) => Err(format!(
            "{} is an empty archive (build problem)",
            kind.label()
        )),
        Err(_) => Err(format!(
            "{} is not a valid archive ({} bytes) - probably an error page, \
             or this release asset has not been uploaded yet",
            kind.label(),
            size
        )),
    }
}

/// 下载物的 sha256 校验。
///
/// 资产是 zip 套件包：宿主校验后整包解压，包内解释器自足，是全链路里权限最高
/// 的工件，不能只验"是个 zip"。发布通道随包附 `SHA256SUMS`（release-beets.yml 生成上传），下载后逐行比对。它防的是**传输与落盘完整性**（截断、代理
/// 篡改、404 页、拿错资产），不防 GitHub 仓库本身被入侵——那要把哈希钉进
/// 宿主二进制，但 beets 与播放器是两个发布节奏（固定下载通道 beets-portable），
/// 钉死等于每次 beets 发版都要重发播放器。所以这不是完备的
/// 供应链防线；真要镜像源，先给入口再留分支（见 `release_base`）。
fn verify_checksum(path: &std::path::Path, asset: &str, base: &str) -> Result<(), String> {
    use sha2::{Digest, Sha256};
    use std::io::Read as _;

    let sums_url = format!("{base}/SHA256SUMS");
    let mut sums = String::new();
    let resp = ureq::get(&sums_url)
        .timeout(std::time::Duration::from_secs(30))
        .call()
        .map_err(|e| {
            format!(
                "failed to download SHA256SUMS ({sums_url}): {e}\n\
                 installing without a checksum manifest is not allowed; download the \
                 release asset manually and point AIYA_BEETS_BUNDLE \
                 at the local file"
            )
        })?;
    resp.into_reader()
        .take(1 << 20) // 清单就几行，1 MiB 上限防呆
        .read_to_string(&mut sums)
        .map_err(|e| format!("failed to read SHA256SUMS: {e}"))?;

    let expected = sums
        .lines()
        .filter_map(|line| {
            // sha256sum 标准格式：`<64 位十六进制>␣␣<文件名>`（两空格 = 二进制模式）
            let (hash, name) = line.split_once("  ")?;
            let hash = hash.trim();
            (hash.len() == 64 && name.trim() == asset).then(|| hash.to_ascii_lowercase())
        })
        .next()
        .ok_or_else(|| {
            format!("SHA256SUMS has no entry for {asset} (asset and manifest do not match)")
        })?;

    let mut file = std::fs::File::open(path)
        .map_err(|e| format!("failed to open the downloaded file: {e}"))?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file
            .read(&mut buf)
            .map_err(|e| format!("failed to read the downloaded file: {e}"))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    let actual = format!("{:x}", hasher.finalize());
    if actual != expected {
        // 校验不过的东西不能留给下一次安装误用
        let _ = std::fs::remove_file(path);
        return Err(format!(
            "sha256 of {asset} does not match the manifest (got {actual}, expected {expected}) - \
             the download may be truncated or tampered with and was deleted; \
             retry or install from a local file"
        ));
    }
    Ok(())
}

/// 取安装物：环境变量指本地文件优先，否则下载到临时目录。
///
/// 两个来源都过 [`verify_asset`]：本地文件同样可能是半截下载
/// （浏览器中断、解压工具写了一半），而"形状不对"的错误在这里说最清楚。
/// 下载来源额外过 [`verify_checksum`]（本地文件是用户自己指的，没有清单可比）。
fn fetch_asset(
    env_key: &str,
    asset: &str,
    kind: AssetKind,
    emit: &dyn Fn(serde_json::Value),
) -> Result<PathBuf, String> {
    if let Ok(local) = std::env::var(env_key) {
        let p = PathBuf::from(local);
        if p.is_file() {
            emit(serde_json::json!({
                "type": "progress", "stage": "prepare",
                "message": format!("using local asset: {}", p.display()),
            }));
            verify_asset(&p, kind)?;
            return Ok(p);
        }
        return Err(format!(
            "file set in {env_key} does not exist: {}",
            p.display()
        ));
    }

    let base = release_base();
    let url = format!("{base}/{asset}");
    emit(serde_json::json!({
        "type": "progress", "stage": "download",
        "message": format!("downloading {asset}"),
    }));
    let dest = std::env::temp_dir().join(asset);
    download_to(&url, &dest).map_err(|e| {
        format!(
            "{e}\nif the component is not released yet, download it manually and set {env_key} to the file"
        )
    })?;
    verify_asset(&dest, kind)?;
    verify_checksum(&dest, asset, &base)?;
    Ok(dest)
}

/// 安装器整体超时：pip 冷装（解析依赖 + 下载）正常几分钟内，
/// 20 分钟还没完就是卡死了。
const INSTALL_OVERALL_TIMEOUT: Duration = Duration::from_secs(20 * 60);
/// 行间空闲超时：安装器输出连绵不断（pip 也有进度行），
/// 60s 一行都没有 = 子进程卡死，不必等整体超时。
const INSTALL_LINE_IDLE_TIMEOUT: Duration = Duration::from_secs(60);

/// 外置工具下载的两级看门狗。
///
/// 空闲阈值必须**大于单次下载的最长时间**：`bins` 在传输期间不打进度帧
/// （只在开始与结束时 say），87MB 的 ffmpeg 实测 54s，慢网下可能几分钟。
/// 给 10 分钟留足余量，整体 20 分钟兜底。
const BIN_LINE_IDLE_TIMEOUT: Duration = Duration::from_secs(10 * 60);
const BIN_OVERALL_TIMEOUT: Duration = Duration::from_secs(20 * 60);

/// 包内环境管理器（操作桥）：spawn 包内解释器跑
/// `install --plugins <新清单> --previous <旧清单>`，帧逐行透传。
///
/// 这是"换插件"的执行体：pip 装/卸、bins 补外部工具、配置同步全在脚本侧
/// （install.py），宿主只负责拉起进程、转事件、看门狗回收。两级超时同一套
/// （pip 冷装几分钟；行间空闲 60s = 卡死）。
fn run_env_manager(
    host: &dyn BeetsHost,
    entry: &ToolEntry,
    plugins: &[String],
    previous: &[String],
    emit: &dyn Fn(serde_json::Value),
) -> Result<(), String> {
    let mut cmd = std::process::Command::new(&entry.program);
    cmd.args(&entry.args)
        .args(["install", "--plugins", &plugins.join(",")]);
    // 旧清单为空（首次安装/没勾过）就不传：脚本语义是"缺省 = 无差量卸载"。
    if !previous.is_empty() {
        cmd.arg("--previous").arg(previous.join(","));
    }
    cmd.stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .envs(tool_env(host, Profile::Workbench));
    // 要跑几十秒，黑框会一直杵在那儿——比一闪而过更难看
    crate::proc::no_window(&mut cmd);
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("failed to start the environment manager: {e}"))?;

    let stdout = match child.stdout.take() {
        Some(s) => s,
        None => {
            let _ = child.kill();
            let _ = child.wait();
            return Err("environment manager stdout was not captured".into());
        }
    };
    // 句柄唯一所有者是共享槽（AppState.beets_install_child）：本函数此后
    // 经槽 wait/kill，应用退出（RunEvent::Exit）也经槽回收。
    let registry = host.install_registry();
    {
        let mut slot = registry
            .child
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *slot = Some(child);
    }

    // 逐行读 + 两级超时交给 client：同一段骨架 bins 下载器也要用，而且那边
    // 有单行上限 —— `BufRead::lines()` 没有，pip 打一行超长解析链会整行进内存。
    let pump = aiya_beets_client::session::pump_frames(
        stdout,
        INSTALL_LINE_IDLE_TIMEOUT,
        INSTALL_OVERALL_TIMEOUT,
        emit,
    );
    let code = {
        let mut slot = registry
            .child
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match slot.as_mut() {
            Some(c) => {
                if pump.as_ref().map(|p| p.timed_out).unwrap_or(true) {
                    let _ = c.kill();
                }
                let code = c.wait().map_err(|e| {
                    format!("failed to wait for the environment manager to exit: {e}")
                })?;
                slot.take(); // 已回收，退出路径不必再管
                Ok(code)
            }
            None => Err("install was interrupted by host exit".to_string()),
        }
    }?;
    let pump = pump.map_err(|e| format!("failed to read environment manager output: {e}"))?;
    if pump.timed_out {
        return Err(format!(
            "environment manager timed out (line idle >{INSTALL_LINE_IDLE_TIMEOUT:.0?} or overall >{INSTALL_OVERALL_TIMEOUT:.0?}), terminated"
        ));
    }
    if !code.success() || !pump.saw_done {
        return Err(format!(
            "environment manager did not finish successfully (exit code {})",
            code.code().unwrap_or(-1)
        ));
    }
    Ok(())
}

/// 展开套件包到 `{app_data}/beets-tool/bundle`。
fn install_via_bundle(
    host: &dyn BeetsHost,
    zip_path: &std::path::Path,
    emit: &dyn Fn(serde_json::Value),
) -> Result<ToolEntry, String> {
    install_bundle_to(&install_root(host).join("bundle"), zip_path, emit)
}

/// 下载外置工具：`<入口> bins --from-plugins a,b`。
///
/// 为什么放在宿主侧、由安装流程顺带跑：这些东西是"插件能不能用"的前置，
/// 而缺 ffmpeg 会让 beets 的**任何**子命令都起不来（实测
/// `UserError: replaygain initialization failed`）——用户没有修复它的入口，
/// 等于组件坏了。也刻意不给"跳过"选项：不做就没有这个插件。
///
/// 为什么传**插件名**而不是 exe 名：哪个插件要哪个工具是工具侧的 `plugins.json`
/// 说的（`binaries_for`）。宿主自己算就是第二份派生——两边算法一旦分家，
/// 症状是"装了插件却没下工具"，而这条正是让 beets 全线起不来的那种故障。
/// 内嵌表里没有 `binaries` 段，宿主没有能力算，也就不可能算错。
fn run_bins(
    host: &dyn BeetsHost,
    entry: &ToolEntry,
    plugins: &[String],
    emit: &dyn Fn(serde_json::Value),
) -> Result<(), String> {
    let mut cmd = std::process::Command::new(&entry.program);
    cmd.args(&entry.args)
        .arg("bins")
        .arg("--from-plugins")
        .arg(plugins.join(","))
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .envs(tool_env(host, Profile::Workbench));
    crate::proc::no_window(&mut cmd);
    emit(serde_json::json!({
        "type": "progress", "stage": "bin",
        "message": format!("downloading external tools (needed by {})", plugins.join(" / ")),
    }));
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("failed to start the external tool download: {e}"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "external tool download output was not captured".to_string())?;

    // 句柄进共享槽：应用退出时统一回收，不留一个还在下 87MB 的孤儿进程
    let registry = host.install_registry();
    {
        let mut slot = registry
            .child
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        // 槽里可能是上一次安装的僵尸：先清掉，避免把它一直挂着
        if let Some(mut old) = slot.take() {
            let _ = old.wait();
        }
        *slot = Some(child);
    }

    let pump = aiya_beets_client::session::pump_frames(
        stdout,
        BIN_LINE_IDLE_TIMEOUT,
        BIN_OVERALL_TIMEOUT,
        emit,
    );

    // 从槽里取回来 wait：进程可能已经被退出流程回收了，那就没有句柄可等
    let code = {
        let mut slot = registry
            .child
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        slot.take()
            .map(|mut c| c.wait().ok().and_then(|s| s.code()))
    };
    let pump = pump.map_err(|e| format!("failed to read the downloader output: {e}"))?;
    if pump.timed_out {
        return Err(format!(
            "external tool download timed out (>{BIN_LINE_IDLE_TIMEOUT:.0?} without output / overall >{BIN_OVERALL_TIMEOUT:.0?}), terminated"
        ));
    }
    if let Some(msg) = pump.last_error {
        return Err(msg);
    }
    if pump.saw_done {
        emit(serde_json::json!({
            "type": "progress", "stage": "bin", "message": "external tools ready",
        }));
    }
    if let Some(Some(code)) = code {
        if code != 0 {
            return Err(format!("downloader exit code {code}"));
        }
    }
    Ok(())
}

/// 展开 + 校验入口（目标目录由调用方给，便于用假套件包测这一段）。
///
/// 套件包里带 `manifest.json`（`{"entry": "python.exe", "args": ["-m","aiya_beets"]}`），
/// 展开后照着它记入口——**播放器不猜布局**：入口名是 beets 侧构建时写死的，
/// 播放器只负责"按它说的位置看一眼在不在"。
fn install_bundle_to(
    target: &std::path::Path,
    zip_path: &std::path::Path,
    emit: &dyn Fn(serde_json::Value),
) -> Result<ToolEntry, String> {
    if target.exists() {
        std::fs::remove_dir_all(target)
            .map_err(|e| format!("failed to remove the old bundle: {e}"))?;
    }
    std::fs::create_dir_all(target)
        .map_err(|e| format!("failed to create the target directory: {e}"))?;
    emit(serde_json::json!({
        "type": "progress", "stage": "unpack",
        "message": format!("unpacking bundle to {}", target.display()),
    }));

    let file =
        std::fs::File::open(zip_path).map_err(|e| format!("failed to open the bundle: {e}"))?;
    let mut zip =
        zip::ZipArchive::new(file).map_err(|e| format!("bundle is not a valid zip: {e}"))?;
    extract_safely(&mut zip, target)?;

    let manifest = target.join("manifest.json");
    if let Ok(text) = std::fs::read_to_string(&manifest) {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
            if let Some(entry) = v.get("entry").and_then(|x| x.as_str()) {
                let program = target.join(entry);
                if program.is_file() {
                    let args = v
                        .get("args")
                        .and_then(|a| a.as_array())
                        .map(|a| {
                            a.iter()
                                .filter_map(|x| x.as_str().map(str::to_string))
                                .collect()
                        })
                        .unwrap_or_default();
                    return Ok(ToolEntry {
                        program: program.to_string_lossy().into_owned(),
                        args,
                    });
                }
            }
        }
    }

    // manifest 缺失/读不动时的兜底（正常包走不到这里：入口是构建时写死的）。
    // 三个名字覆盖两平台的布局：win 在根（python.exe）或打包产物入口，
    // linux 在 bin/（PBS 解释器，布局与容器构建产物一致）。兜底只保证
    // "能起"，入口契约在 manifest。
    for (name, with_module_arg) in [
        ("aiya-beets.exe", false),
        ("python.exe", true),
        ("bin/python3", true),
    ] {
        let p = target.join(name);
        if p.is_file() {
            let args = if with_module_arg {
                vec!["-m".into(), "aiya_beets".into()]
            } else {
                Vec::new()
            };
            return Ok(ToolEntry {
                program: p.to_string_lossy().into_owned(),
                args,
            });
        }
    }
    Err("no entry point found after unpacking the bundle (manifest.json missing or unreadable, and no known launcher present)".to_string())
}

/// 展开套件包的一件条目该落到哪个相对路径；`None` = 拒绝。
///
/// 三种都要挡：`..`（`zip-slip`，能写到安装目录之外）、盘符（`C:\x` 是
/// 绝对路径）、NTFS 数据流（`a.txt:evil` 会写进另一个文件的隐藏流）。
/// 分隔符两种都切：zip 规范用 `/`，但坏包会用 `\`，而在 Unix 上 `\` 不是
/// 分隔符——只用 `Path` 语义判会漏。
fn safe_relative(name: &str) -> Option<std::path::PathBuf> {
    let mut out = std::path::PathBuf::new();
    for part in name.split(['/', '\\']) {
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." || part.contains(':') {
            return None;
        }
        out.push(part);
    }
    (!out.as_os_str().is_empty()).then_some(out)
}

/// 解压上限：win 包实际约 30MB；linux 轨是完整 Python 运行时，
/// 未压缩 700MB+（libpython 一项 137MB），留一倍余量。不是防自己人，是防
/// "被替换过的包"把磁盘写满——那种故障现场极难归因到一次安装。
const BUNDLE_MAX_BYTES: u64 = 1536 * 1024 * 1024;

/// 展开套件包，**不用 `ZipArchive::extract`**。
///
/// 为什么手写（三条都实测过或写在 crate 通告里）：
/// 1. `extract` 直接把条目名拼到目标目录下。带 `..` 或绝对路径的条目可以
///    写到目标之外。包来源是网络（`AIYA_BEETS_BUNDLE` 还能指到任意本地
///    文件），"反正是自己的包"不是能长期成立的假设。
/// 2. 它会按 unix 权限位建符号链接。Windows 上建不出真链接（失败或退化成
///    复制），失败会让整次安装中断；而"复制"会把链接指向的**包外**内容
///    一起搬进安装目录。
/// 3. 没有总量上限：一个改坏的包能把磁盘写满。
///
/// 展开失败的处置交给调用方：`install_bundle_to` 的错误路径上目标目录会被
/// 下次安装的 `remove_dir_all` 清掉，所以这里不必回滚半成品。
fn extract_safely<R: std::io::Read + std::io::Seek>(
    zip: &mut zip::ZipArchive<R>,
    target: &std::path::Path,
) -> Result<(), String> {
    let mut written = 0u64;
    for i in 0..zip.len() {
        let mut entry = zip
            .by_index(i)
            .map_err(|e| format!("failed to read a bundle entry: {e}"))?;
        // zip 9 起 `name()` 返回 Result：条目名可能是非法编码（CP437 之外的
        // 未标注原始字节）。拿替换后的名字做路径校验会与真实落盘名不符，宁可在这里失败。
        let name = entry
            .name()
            .map_err(|e| format!("failed to read a bundle entry name: {e}"))?
            .to_string();
        let Some(rel) = safe_relative(&name) else {
            return Err(format!(
                "bundle contains an illegal path entry, refusing to unpack: {name}"
            ));
        };
        // 0o170000 = S_IFMT，0o120000 = S_IFLNK
        if entry
            .unix_mode()
            .is_some_and(|mode| mode & 0o170000 == 0o120000)
        {
            return Err(format!(
                "bundle contains a symlink entry, refusing to unpack: {name}"
            ));
        }
        let dest = target.join(&rel);
        if entry.is_dir() {
            std::fs::create_dir_all(&dest)
                .map_err(|e| format!("failed to create directory {}: {e}", dest.display()))?;
            continue;
        }
        written = written.saturating_add(entry.size());
        if written > BUNDLE_MAX_BYTES {
            return Err(format!(
                "bundle unpack exceeded the {} MB limit, aborted (the bundle may be incomplete or replaced)",
                BUNDLE_MAX_BYTES / 1024 / 1024
            ));
        }
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("failed to create directory {}: {e}", parent.display()))?;
        }
        let mut out = std::fs::File::create(&dest)
            .map_err(|e| format!("failed to write {}: {e}", dest.display()))?;
        std::io::copy(&mut entry, &mut out)
            .map_err(|e| format!("failed to unpack {}: {e}", dest.display()))?;
        // unix 位随包还原（linux 轨：bin/python3 没有执行位时 spawn 全线
        // EACCES，组件装完即坏；win 条目没有 unix_mode，跳过）
        #[cfg(unix)]
        if let Some(mode) = entry.unix_mode() {
            use std::os::unix::fs::PermissionsExt as _;
            let _ = std::fs::set_permissions(&dest, std::fs::Permissions::from_mode(mode & 0o7777));
        }
    }
    Ok(())
}

/// 记下入口，下次 `discover` 直接命中（设置优先于 PATH）。
fn remember_entry(host: &dyn BeetsHost, entry: &ToolEntry) -> Result<(), String> {
    let program = entry.program.clone();
    let args = entry.args.join(" ");
    update_settings(host, move |s| {
        s.tool_path = Some(program);
        s.tool_args = Some(args);
    })
}

/// 安装（异步）：立刻返回，进度走 `beets:install`，结束时补一条 finished。
///
/// `plugins` 是用户在 UI 里勾的**启用清单**：安装据此按勾选清单补外置二进制
/// （bins）；pip 依赖不在此步，走"换插件"。勾选清单写进 `settings.plugins`
/// （重装后恢复启用），工具侧渲染 `config.yaml` 时照它启用。
///
/// finished 带**重新探测后的 status**：UI 不用再单独发一次查询，
/// 也不会出现"装完了但状态块还是旧值"这种中间态。
///
/// 并发互斥：同一时刻只允许一条安装线程。安装线程往
/// `beets_install_child` 这个单槽里塞句柄，两条线程并发时第二个会顶掉第一个
/// ——收尾 kill/wait 错对象、误报"被退出中断"，pip 被中途杀还可能留半个
/// venv。占位失败直接返回错误，UI 以 error 事件呈现。
pub fn start_install(
    host: std::sync::Arc<dyn BeetsHost>,
    plugins: Vec<String>,
) -> Result<(), String> {
    if !host.try_set_installing() {
        return Err(
            "a component install is already running; wait for it to finish (or fail) and try again"
                .to_string(),
        );
    }
    std::thread::Builder::new()
        .name("beets-install".into())
        .spawn(move || {
            // 互斥占位在返回/panic 时由 Drop 复位
            let registry = host.install_registry();
            let _guard = InstallGuard::new(&registry.installing);
            let emit = |v: serde_json::Value| {
                host.emit("beets:install", v);
            };
            let result = (|| -> Result<ToolEntry, String> {
                let path =
                    fetch_asset("AIYA_BEETS_BUNDLE", bundle_asset(), AssetKind::Bundle, &emit)?;
                let entry = install_via_bundle(&*host, &path, &emit)?;
                // 外置工具：便携包自带 Essentia 提取器（`exe_path` 在包内
                // `bundle/bin` 里找到它），其余按插件清单现下。清单由**工具侧
                // 按插件名**自己算，已经就位的会跳过，所以"包里带了"不会变成
                // "再下一次 20MB"。为什么非做不可：缺 ffmpeg 时 replaygain 会让
                // beets 整个 CLI 起不来（实测 `UserError: replaygain initialization failed`）。
                if !plugins.is_empty() {
                    run_bins(&*host, &entry, &plugins, &emit).map_err(|e| {
                        format!("component installed, but the external tools failed to download: {e} (click install again to fetch them)")
                    })?;
                }
                Ok(entry)
            })();
            match result {
                Ok(entry) => {
                    // 记不住也不算彻底失败：PATH 入口仍可能命中，
                    // 只是下次启动要多一次探测。如实报给 UI。
                    if let Err(e) = remember_entry(&*host, &entry) {
                        emit(serde_json::json!({ "type": "warning", "message": e }));
                    }
                    // 勾选写进设置——装了不启用等于白装（cfg 渲染 plugins 行读它）
                    if !plugins.is_empty() {
                        let want = plugins.clone();
                        if let Err(e) = update_settings(&*host, move |s| {
                            s.plugins = Some(want);
                        }) {
                            emit(serde_json::json!({ "type": "warning", "message": e }));
                        }
                    }
                }
                Err(message) => emit(serde_json::json!({ "type": "error", "message": message })),
            }
            let status = probe(&*host);
            host.emit(
                "beets:install",
                serde_json::json!({ "type": "finished", "status": status }),
            );
        })
        .map(|_| ())
        .map_err(|e| format!("failed to spawn the install thread: {e}"))
}

/// 换插件（操作桥）：把新清单写进 settings，再 spawn 包内解释器跑
/// `install --plugins <新> --previous <旧>`——pip 装/卸的差量、外部工具补齐、
/// 配置同步全在脚本侧，宿主只拉起进程、转事件、看门狗回收。
///
/// 与安装共用同一把互斥（beets_installing）与同一个事件通道（beets:install）：
/// pip 改 site-packages 与解压套件包都不能并发，UI 也共用同一块进度区。
pub fn start_plugins_set(
    host: std::sync::Arc<dyn BeetsHost>,
    plugins: Vec<String>,
) -> Result<(), String> {
    if !host.try_set_installing() {
        return Err(
            "a component install is already running; wait for it to finish (or fail) and try again"
                .to_string(),
        );
    }
    std::thread::Builder::new()
        .name("beets-plugins-set".into())
        .spawn(move || {
            let registry = host.install_registry();
            let _guard = InstallGuard::new(&registry.installing);
            let emit = |v: serde_json::Value| {
                host.emit("beets:install", v);
            };
            let result = (|| -> Result<(), String> {
                // 旧清单要在改写 settings 之前取——它是差量卸载的依据
                let previous = load_settings(&*host).plugins.clone().unwrap_or_default();
                let want = plugins.clone();
                update_settings(&*host, move |s| {
                    s.plugins = Some(want);
                })?;
                let (entry, _) = discover(&*host).ok_or_else(|| NOT_INSTALLED.to_string())?;
                run_env_manager(&*host, &entry, &plugins, &previous, &emit)
            })();
            match result {
                Ok(()) => {}
                Err(message) => emit(serde_json::json!({ "type": "error", "message": message })),
            }
            let status = probe(&*host);
            host.emit(
                "beets:install",
                serde_json::json!({ "type": "finished", "status": status }),
            );
        })
        .map(|_| ())
        .map_err(|e| format!("failed to spawn the plugin change thread: {e}"))
}

/* 卸载组件 */

/// 统计目录的文件数与字节数（先算账再删——删完就没法报了）。
///
/// 只在**工具侧不可用**时用：正常路径上账目由 `uninstall --all --dry-run`
/// 给出（见 [`uninstall_plan`]），那份还顺带过了四道闸。
fn measure_dir(root: &std::path::Path) -> (u64, u64) {
    let mut files = 0u64;
    let mut bytes = 0u64;
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            // 不跟随符号链接/junction：统计跟着走会数到目标目录去
            let Ok(meta) = entry.metadata() else { continue };
            if meta.is_dir() {
                stack.push(path);
            } else if meta.is_file() {
                files += 1;
                bytes += meta.len();
            }
        }
    }
    (files, bytes)
}

/// 从工具侧的行帧输出里取最后一条 `type == want` 的 JSON。
///
/// 只认 JSON 行：工具侧在协议内容之前可能打过人类可读的提示（迁移日志），
/// 那些行解析不出来，跳过即可——与 `spawn_reader` 的容错行为一致。
fn last_frame(text: &str, want: &str) -> Option<serde_json::Value> {
    text.lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line.trim()).ok())
        .rfind(|v| v.get("type").and_then(|t| t.as_str()) == Some(want))
}

/// 工具侧对"这次卸载删什么"的答复。
enum UninstallPlan {
    /// 过完闸，给出账目
    Ready { files: u64, bytes: u64 },
    /// 没有可用入口 / 跑不起来：宿主自己算账（这次没有工具侧的闸）
    Unavailable,
    /// 工具侧明确拒绝（盘根、系统目录、不像安装目录…）：**不许删**
    Refused(String),
}

/// `uninstall --all --dry-run` 的超时。dry-run 只做目录遍历与账目计算，
/// 本地大目录也在秒级；30 s 还没完 = 子进程卡死（杀软锁文件 / 进程僵死），
/// 与 [`QUERY_TIMEOUT`] 同一量级的保守取值。
const UNINSTALL_PLAN_TIMEOUT: Duration = Duration::from_secs(30);

/// 让工具侧出卸载计划：`uninstall --all --dry-run --target <root>`。
///
/// ## 为什么只借 `--dry-run`，不把删除交出去
///
/// `aiya-beets uninstall` 的执行体就在被删目录里（便携包的 python.exe），而
/// Windows 上运行中的 exe 删不掉（实测 `DeleteFileW` 返回 0、错误码 5
/// access denied；同目录的普通文件返回 1 可删）。让工具侧真删，结果是一半
/// 删掉、一半留着，比不删更难收拾。所以分工是：**工具侧过闸 + 算账，
/// 宿主执行删除**——四道闸照旧生效，删除者不在目录树内。
///
/// ## 为什么 `Refused` 不回落
///
/// 退出码 2 是工具侧的闸门约定。它说"不该删"时宿主若自己删，闸门就成了装饰。
/// 只有"跑不起来"（没有入口、spawn 失败、输出里没有 done 帧）才回落自算，
/// 因为那种情况闸门根本没被执行过，不该让它阻断卸载。
///
/// ## 超时语义
///
/// 返回 `Err` 仅发生在**超时/等待失败**——此时子进程已被 kill 并 wait 回收。
/// 超时按错误上报而不是回落"宿主自算"（[`UninstallPlan::Unavailable`]）：
/// 回落 = 没过四道闸就直接删目录树，比多等几十秒危险得多；"跑不起来回落"
/// 的宽限只属于 spawn 失败那种"闸门根本没机会执行"的情况。
fn uninstall_plan(host: &dyn BeetsHost, root: &std::path::Path) -> Result<UninstallPlan, String> {
    let Some((entry, _source)) = discover(host) else {
        return Ok(UninstallPlan::Unavailable);
    };
    let mut cmd = std::process::Command::new(&entry.program);
    cmd.args(&entry.args)
        .args(["uninstall", "--all", "--dry-run"])
        .arg("--target")
        .arg(root)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .envs(tool_env(host, Profile::Workbench));
    crate::proc::no_window(&mut cmd);
    // 不用 `cmd.output()`：它无限等退出，dry-run 卡住时整个卸载线程永久挂在
    // "有 progress 帧、永远等不到终态"。spawn + 轮询 try_wait + 超时 kill，
    // 所有等待有界（其余子进程交互走 StdioTool / pump_frames 的两级看门狗，
    // 那是行流式场景，单次一问一答不适用）。
    let Ok(mut child) = cmd.spawn() else {
        return Ok(UninstallPlan::Unavailable);
    };
    let mut stdout = child.stdout.take();
    let deadline = Instant::now() + UNINSTALL_PLAN_TIMEOUT;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(20));
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!(
                    "uninstall plan timed out (dry-run {UNINSTALL_PLAN_TIMEOUT:.0?} did not finish), tool process terminated"
                ));
            }
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("failed to wait for the uninstall plan: {e}"));
            }
        }
    };
    // 子进程已退出（写端随之关闭），read_to_end 必然返回；dry-run 输出只有
    // 账目一帧，远小于管道缓冲，轮询期间不存在写满阻塞。
    let mut text = Vec::new();
    if let Some(out) = stdout.as_mut() {
        use std::io::Read;
        let _ = out.read_to_end(&mut text);
    }
    let _ = child.wait();
    Ok(plan_from_output(
        status.code(),
        &String::from_utf8_lossy(&text),
    ))
}

/// 把工具侧的输出翻成 [`UninstallPlan`]（纯函数，好测）。
///
/// 退出码 2 是工具侧的闸门约定。**只有**在退出码 2 且真有 error 帧时才当
/// "拒绝"——否则一次崩溃（segfault 也是非 0）会被误报成"不许卸载"，
/// 用户就再也卸不掉了。
fn plan_from_output(code: Option<i32>, text: &str) -> UninstallPlan {
    if code != Some(0) {
        if code == Some(2) {
            if let Some(msg) = last_frame(text, "error").and_then(|v| extract_message(&v)) {
                return UninstallPlan::Refused(msg);
            }
        }
        return UninstallPlan::Unavailable;
    }
    match last_frame(text, "done") {
        Some(v) => UninstallPlan::Ready {
            files: v
                .get("files")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0),
            bytes: v
                .get("bytes")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0),
        },
        None => UninstallPlan::Unavailable,
    }
}

/// 行帧里的 `message`（没有就是 None）。
fn extract_message(v: &serde_json::Value) -> Option<String> {
    v.get("message")
        .and_then(|m| m.as_str())
        .map(str::to_string)
}

/// 卸载：删 `{data_root}/beets-tool` 整层（bundle 自足解释器 + bin 外置工具 +
/// 下载缓存），清设置里的入口。
///
/// 与安装/换插件共用同一把互斥（`beets_installing`）：两边动的是同一棵目录树，
/// 并发必然互相拆台。占位在起线程**之前**做，线程内的释放走 [`InstallGuard`]。
///
/// 卸载是**安装的反向操作**：安装产物是 bundle 自足解释器（beets 本体、pip
/// 依赖、插件都在包内 site-packages，没有 venv），全在 `beets-tool/` 里，
/// 删它就等于全部撤回。
/// `beets/`（整理家 library.db + ops.jsonl）与 `beets-index/`（分析库）虽然
/// 也在同一个数据根下，但它们是用户资产、不是安装产物，**这里一个字节都不动**
/// ——那些由应用自己的清理入口管；分析结果（1850 首量级）不在卸载范围。
pub fn start_uninstall(host: std::sync::Arc<dyn BeetsHost>) -> Result<(), String> {
    {
        // 与安装/换插件共用同一把互斥（beets_installing）：卸载删的正是安装
        // 写的那个目录树，两边并发 = 一边在 pip 装、一边在 rmtree，半成品谁也
        // 收不了场。占位失败与 start_install 同文案（UI 在同一个进度区报错）。
        if !host.try_set_installing() {
            return Err("a component install is already running; wait for it to finish (or fail) and try again".to_string());
        }
    }
    let thread_host = host.clone();
    let spawned = std::thread::Builder::new()
        .name("beets-uninstall".into())
        .spawn(move || {
            // 互斥占位在返回/panic 时由 Drop 复位
            let registry = thread_host.install_registry();
            let _guard = InstallGuard::new(&registry.installing);
            let emit = |v: serde_json::Value| {
                thread_host.emit("beets:install", v);
            };
            if let Err(e) = uninstall_now(&*thread_host, &emit) {
                emit(serde_json::json!({ "type": "error", "message": e }));
            }
            let status = probe(&*thread_host);
            thread_host.emit(
                "beets:install",
                serde_json::json!({ "type": "finished", "status": status }),
            );
        });
    spawned.map(|_| ()).map_err(|e| {
        // 线程没起来，guard 永远不会执行：占位必须在这里手工释放，
        // 否则之后所有安装/卸载都报 busy
        if true {
            host.install_registry()
                .installing
                .store(false, std::sync::atomic::Ordering::SeqCst);
        }
        format!("failed to spawn the uninstall thread: {e}")
    })
}

fn uninstall_now(host: &dyn BeetsHost, emit: &dyn Fn(serde_json::Value)) -> Result<(), String> {
    let root = install_root(host);
    if !root.exists() {
        // 幂等：没有安装目录就算成功。但要提醒"当前入口可能来自别处"，
        // 否则 PATH/环境变量入口的机器上点卸载会毫无反应、还报成功。
        if let Some((_, source)) = discover(host) {
            emit(serde_json::json!({
                "type": "warning",
                "message": format!(
                    "no install directory to remove; the component comes from {} (PATH/env var), uninstall leaves it alone",
                    source.as_str()
                ),
            }));
        }
        emit(serde_json::json!({
            "type": "done", "stage": "uninstall", "removed": false,
            "message": "no install directory, nothing to uninstall",
        }));
        return Ok(());
    }

    // 先问工具侧：它过四道闸、算账。明确拒绝就停（闸门的意义就在这儿），
    // 只有"跑不起来"才回落到宿主自己算——那时闸门根本没被执行过。
    // 计划超时（Err）直接上抛：见 uninstall_plan 的超时说明。
    let (files, bytes) = match uninstall_plan(host, &root)? {
        UninstallPlan::Ready { files, bytes } => (files, bytes),
        UninstallPlan::Refused(why) => return Err(why),
        UninstallPlan::Unavailable => measure_dir(&root),
    };
    emit(serde_json::json!({
        "type": "progress", "stage": "uninstall",
        "message": format!(
            "will remove {} ({} files, {:.1} MB)", root.display(), files, bytes as f64 / 2f64.powi(20)
        ),
    }));

    // 有会话在跑时 Windows 上删不掉 exe。尽力收尾一次（不等退出——那会让
    // 卸载卡在这里；删不掉就如实报，让用户先停掉索引）。
    let _ = cancel_session();

    std::fs::remove_dir_all(&root).map_err(|e| {
        format!(
            "failed to remove ({e}). If indexing/organizing is running, wait for it to \
             finish or quit the host first; directory: {}",
            root.display()
        )
    })?;

    // 设置里的入口必须清掉：否则 discover 会一直指着一个不存在的 exe，
    // 症状是"卸载了但状态页仍显示组件在、只是起不来"。
    update_settings(host, |s| {
        s.tool_path = None;
        s.tool_args = None;
    })?;

    emit(serde_json::json!({
        "type": "done", "stage": "uninstall", "removed": true,
        "files": files, "bytes": bytes,
        "message": "component uninstalled; Organize library and analysis library untouched",
    }));
    Ok(())
}

/* 导入会话（事件流转发） */

/// 取工具入口并造一个可直接起进程的 tool。
///
/// 找不到入口时的文案要能指导用户下一步（去扫描入库页装组件），
/// 而不是一句 "no beets interpreter available"。
fn tool_or_err(host: &dyn BeetsHost) -> Result<StdioTool, String> {
    let (entry, _source) = discover(host).ok_or_else(|| NOT_INSTALLED.to_string())?;
    Ok(StdioTool::new(launcher_for(
        host,
        &entry,
        Profile::Workbench,
    )))
}

/* 会话槽 + 取消 */

/// 取消当前会话：写 cancel 行 → 关 stdin（对面收尾后 EOF 退出）。
///
/// 实现在 client crate 里（会话槽是进程级静态状态，只能有一份）。这里保留
/// 一个同名入口是为了命令层不用知道 crate 结构。
pub fn cancel_session() -> bool {
    aiya_beets_client::session::cancel()
}

/// 起会话：把请求交给 client，事件转进 `channel`，结束后跑 `on_exit`。
///
/// `channel` 由调用方给：导入链路走 `beets:import`（ImportPanel 在听），
/// 曲库维护走 `beets:maintenance`——两条链路语义不同，混在一个通道里会让
/// "导入进度条"突然收到"整理文件"的事件。
///
/// `busy` 是并发占位失败时的文案：两条链路的用户可见说法不同（导入面板说
/// "导入任务"，维护卡片说"任务"），保留原样免得 UI 提示语义变糊。
fn run_session(
    tool: StdioTool,
    host: Arc<dyn BeetsHost>,
    request: serde_json::Value,
    channel: &'static str,
    busy: &str,
    on_exit: Box<dyn FnOnce() + Send + 'static>,
) -> Result<(), String> {
    let sink: Arc<dyn EventSink> = crate::event_sink(host.clone());
    run_session_with(tool, request, channel, busy, sink, on_exit)
}

/// [`run_session`] 的通用形态：事件出口由调用方给。
///
/// 索引增强需要自定义出口（边转发边把 `enrichItem` 攒起来落库），
/// 其余链路直接用 [`TauriSink`] 转发。
fn run_session_with(
    tool: StdioTool,
    request: serde_json::Value,
    channel: &'static str,
    busy: &str,
    sink: Arc<dyn EventSink>,
    on_exit: Box<dyn FnOnce() + Send + 'static>,
) -> Result<(), String> {
    tool.start_session(request, channel, sink, on_exit)
        .map_err(|e| match e {
            ToolError::Busy => busy.to_string(),
            other => other.to_string(),
        })
}

/// 启动一次 preview_import：一次性子进程，stdout 每行 = 一个事件，
/// 逐行转发到 Tauri 事件 `beets:import`。完成（进程退出）后自动回收。
///
/// 低频操作，不值得养常驻会话；每命令 ~300ms 的启动开销可接受。
/// 返回 Err 表示没找到可用解释器（UI 显示指引）。
pub fn start_preview(
    host: std::sync::Arc<dyn BeetsHost>,
    paths: Vec<String>,
) -> Result<(), String> {
    let tool = tool_or_err(&*host)?;

    // 排除表是**播放器侧事实**（哪些 inbox 源文件已成功导入过），在这里注入
    // 进协议预留的 options 对象：待整理目录固定后，copy 应用的
    // 原件一直留在 inbox，不排除的话下次整理会把同一批文件再导一遍。UI 不用
    // 搬运这份数据，preview 命令签名保持不变。
    let exclude: Vec<String> = load_settings(&*host).inbox_imported;

    let mut body = serde_json::json!({ "id": 1, "cmd": "preview_import", "paths": paths });
    if !exclude.is_empty() {
        body["options"] = serde_json::json!({ "excludePaths": exclude });
    }

    run_session(
        tool,
        host.clone(),
        body,
        "beets:import",
        "a beets import task is already running (cancel it first)",
        Box::new(|| {}),
    )
}

/// v2 幂等键：每次调用生成新值（`{cmd}-{unix 毫秒}-{进程内序号}`）。
///
/// 单实例下宿主不重试，重放是协议层安全网 + 双实例防护；
/// 工具侧的 ops.jsonl 同时是操作审计日志。进程内自增序号保证同一毫秒内
/// 的两次调用也不撞键。
fn new_request_id(cmd: &str) -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};
    static SEQ: AtomicU64 = AtomicU64::new(1);
    let ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    format!("{cmd}-{ms}-{seq:04}")
}

/// 应用导入决策。decisions 形状由契约定义（ApplyDecision），
/// 宿主不解释语义，只透传；完成后自动把工具侧结果对齐自有库（自闭环）。
///
/// `resume`（v2）：中断后续跑——重发同一批 decisions，sidecar 按内容指纹
/// 跳过已完成的（move/copy 中断后源文件已移走，不 resume 的重跑必报错）。
pub fn start_apply(
    host: std::sync::Arc<dyn BeetsHost>,
    decisions: Vec<serde_json::Value>,
    resume: bool,
) -> Result<(), String> {
    let tool = tool_or_err(&*host)?;

    // 整理写入的落点。apply 结束后要按它补一次索引 —— 见下面 on_exit 的说明。在**发命令前**取：落点是被 `apply`
    // 正在使用的那个（`tool_env` 同一个来源），结束后再取可能已经被用户改掉，补索引就会扫错目录。
    let dest = roots_for_tools(&*host).1;

    // on_exit 里拿一份 AppHandle（会话线程要能独立跑，不能借用 &host）
    let exit_host = host.clone();
    run_session(
        tool,
        host.clone(),
        serde_json::json!({
            "id": 1, "cmd": "apply_import",
            "decisions": decisions,
            "requestId": new_request_id("apply_import"),
            "resume": resume,
        }),
        "beets:import",
        "a beets import task is already running (cancel it first)",
        Box::new(move || {
            {
                // 自闭环：工具刚写完库就是最新权威。
                // 走完整刷新管线而不是只 dump+sync —— apply 会写标签改 mtime，
                // 扫描段正好把这些文件的 mtime/size 与标签一起还原。
                host.kick_library_refresh();
            }
            // 上面那条只管**应用曲库**（扫描器是它自己的，unchanged 预筛让整库
            // 增量很廉价）。索引家是**另一个库文件**，apply 写的是整理家，它不会
            // 自己知道有新文件 —— 不补这一步，刚整理好的歌在查询页永远 0 命中。
            //
            // 传 `dest`（落点）而不是整根：整根 96 批 ≈ 82 s，全是进程启动开销；
            // 落点通常 1 批 ≈ 1 s。详见 `start_index_import` 的实测表。
            tracing::info!(
                dest = dest.as_deref().unwrap_or("-"),
                "整理完成：应用曲库交给 refresh(action)，索引家按落点补扫"
            );
            if let Err(e) = start_index_import(exit_host.clone(), dest) {
                // 补索引失败**不影响整理结果**（文件已经搬好了），所以只记日志。
                // 用户仍可手动「重建索引」——那是这条路的兜底，不是主路径。
                tracing::warn!(error = %e, "整理后补索引未能启动（整理结果不受影响）");
            }
        }),
    )
}

/// 曲库维护任务的统一入口（adopt / update / duplicates / move）。
///
/// 四条命令的公共部分完全一样：占会话位 → 起一次性子进程 → 事件流走
/// `beets:maintenance` → 进程退出后按需跑一次读模型对账。差别只有 req 载荷
/// 和"要不要对账"，所以合成一个函数，避免四份几乎相同的代码各自漂移
/// （漏了释放、漏了 ensure_config 之类）。
///
/// `refresh` 为什么只有 adopt/move 给：这两条会改库的**内容与路径**——
/// adopt 把文件登记进库（对账后 `tracks.beets_item_id` 才有值，而后续 move
/// 的路径跟随正是靠这个双键）；move 把文件搬走，路径漂移必须
/// 立刻跟随，不能等用户下次扫描。update 只改标签（路径不变）、duplicates
/// 纯只读，都不需要自动对账。
fn start_maintenance(
    host: std::sync::Arc<dyn BeetsHost>,
    payload: serde_json::Value,
    refresh_after: bool,
) -> Result<(), String> {
    let tool = tool_or_err(&*host)?;

    let _exit_host = host.clone();
    run_session(
        tool,
        host.clone(),
        payload,
        "beets:maintenance",
        "a beets task is already running (cancel it first)",
        Box::new(move || {
            if refresh_after {
                host.kick_library_refresh();
            }
        }),
    )
}

/// 建 beets 库基线：把自有库里的文件按现状登记进 beets 库。
/// `paths` 为空 = 由宿主取自有库全部在线路径（UI 不该为了这个传 1800 个路径）。
pub fn start_adopt(
    host: std::sync::Arc<dyn BeetsHost>,
    paths: Vec<String>,
    refresh_after: bool,
) -> Result<(), String> {
    start_maintenance(
        host,
        serde_json::json!({
            "id": 1, "cmd": "adopt_library", "paths": paths,
            "requestId": new_request_id("adopt_library"),
        }),
        refresh_after,
    )
}

/// 用文件标签刷新 beets 库（`pretend` = 只报 diff 不写库）。
pub fn start_update(host: std::sync::Arc<dyn BeetsHost>, pretend: bool) -> Result<(), String> {
    start_maintenance(
        host,
        serde_json::json!({
            "id": 1, "cmd": "update_library", "pretend": pretend,
            "requestId": new_request_id("update_library"),
        }),
        false,
    )
}

/// 按字段组查重（只读）。
pub fn start_duplicates(
    host: std::sync::Arc<dyn BeetsHost>,
    keys: Vec<String>,
) -> Result<(), String> {
    start_maintenance(
        host,
        serde_json::json!({ "id": 1, "cmd": "find_duplicates", "keys": keys }),
        false,
    )
}

/* 索引增强 */

/// 起一次索引增强会话：事件走 `beets:enrich`，`enrichItem` 由宿主边收边落库。
///
/// 与其它会话的四处不同，都不是风格选择：
/// 1. **跑在索引家**（`Profile::Index`）。这是本模块最危险的一跳：`enrich`
///    会先 `item.remove()` 再 `beet import -R` 重导——跑在用户库上，那一步会
///    按用户的 config 写标签、搬文件。索引家的写盘开关全部钉死，最坏结果是
///    "索引库里多几条记录"。
/// 2. **路径清单由 [`crate::enrichment::work_paths`] 现算**：只有应用侧知道
///    曲库有哪些文件、`size/mtime` 是多少。sidecar 收到的是一份明确清单，
///    不自己挑文件。
/// 3. **事件出口不是纯转发**：`enrichment::Sink` 一边转发给 UI，一边把
///    `enrichItem` 攒起来写**自己的**结论库（`{beets-index}/enrich.db`）。
///    那个库不与应用库共享文件，所以**不需要** `scanning`——分析再久也不会
///    挡住用户的扫描。
/// 4. **不触发读模型对账**：增强只往自己的结论库写，`tracks` 一个字节不变。
pub fn start_enrich(
    host: std::sync::Arc<dyn BeetsHost>,
    source: String,
    force: bool,
) -> Result<(), String> {
    let (entry, _src) = discover(&*host).ok_or_else(|| NOT_INSTALLED.to_string())?;
    let paths = crate::enrichment::work_paths(&*host, &source, force)?;
    if paths.is_empty() {
        return Err("work list is empty: no tracks to analyze".to_string());
    }
    let tool = StdioTool::new(launcher_for(&*host, &entry, enrich_profile()));
    let sink = Arc::new(crate::enrichment::Sink::start(host.clone())?);
    let exit_sink = Arc::clone(&sink);
    run_session_with(
        tool,
        serde_json::json!({
            "id": 1, "cmd": "enrich",
            "paths": paths,
            "want": [source],
            "force": force,
        }),
        crate::enrichment::CHANNEL,
        "a beets task is already running (cancel it first)",
        sink,
        Box::new(move || exit_sink.finish()),
    )
}

/// 分析会话固定跑索引家。
///
/// 抽成函数只为让测试能盯住它：直接写 `Profile::Index` 也能跑，但改成
/// `Workbench`（看起来更简单）不会有任何东西变红——而那个改动的后果是
/// `item.remove()` + 按用户 config 重导，也就是**用户的歌被改**。
pub fn enrich_profile() -> Profile {
    Profile::Index
}

/// 查询页固定跑**索引家**。
///
/// 与 `enrich_profile` 同理抽成函数：这条"改回工作台家"的改动不会有任何东西
/// 变红，而症状是"整理库页永远查不到东西"——因为工作台家是用户自己的 beets 库，
/// 应用从不往里导，空是常态。索引家的 `directory` 已指向曲库根，它才是那个
/// "曲库的只读镜像"。
pub fn query_profile() -> Profile {
    Profile::Index
}

/// 只读入库的事件通道。
///
/// 与 `beets:enrich` 分开：那条流的语义是"逐文件分析结论"（`enrichItem` /
/// `enrichPersisted`，宿主还要落结论库），这条只有"导到哪了"与"库里现在几条"。
/// 混一条通道会让查询页的进度条在两种粒度之间跳。
pub const INDEX_CHANNEL: &str = "beets:index";

/// 只读入库：把 **`root`**（缺省＝曲库根）登记进索引家，不做分析。
///
/// 与 `start_enrich` 的分工很硬：
/// * 这条 → "索引库里有行"，查询页的前提；
/// * 那条 → "每个文件有分析结论"，`xtractor` 未接线时**整批**被拒。
///
/// 入库不是分析的副产品：分析器没接线时 `enrich` 直接拒绝（`E_INVALID_ARG`），
/// 连库都不建——筛选页会永远 0 命中。所以两条路必须分开。
///
/// ## 为什么 `root` 可传子目录（整理后补索引就走这条）
///
/// 索引家与整理家是**两个库文件**，`apply` 只写整理家；不补的话"刚整理完的歌
/// 在查询页搜不到"。补的代价取决于扫多大范围，实测（1846 首曲库、beets 2.14.1）：
///
/// | 扫什么 | 批数 | 耗时 |
/// |---|---|---|
/// | 整根 | 96 | **82 s** |
/// | 一个落点目录（1 首） | 1 | **0.99 s** |
///
/// 82 s 里几乎全是**进程启动开销**（每批 `python -m beets import` 固定 ~0.85 s，
/// 与批内文件数关系很小：实测 1 个已入库文件 0.88 s、20 个 1.25 s），不是解析。
/// 所以"整理完整根重扫"是不能接受的 82 秒，而落点通常只有一两批。
///
/// 传子目录**不会被 `-R` 误跳过**：beets 的增量记忆（`statefile: state.pickle`
/// 的 `taghistory`）记的是「一个导入任务的那批 paths 元组」，新文件组成新元组；
/// 实测新目录只扫该目录 → 索引库 1849 → 1850。
pub fn start_index_import(
    host: std::sync::Arc<dyn BeetsHost>,
    root: Option<String>,
) -> Result<(), String> {
    let (entry, _src) = discover(&*host).ok_or_else(|| NOT_INSTALLED.to_string())?;
    let tool = StdioTool::new(launcher_for(&*host, &entry, Profile::Index));
    let mut body = serde_json::json!({ "id": 1, "cmd": "index_import" });
    if let Some(r) = trimmed(root) {
        body["root"] = serde_json::Value::String(r);
    }
    run_session(
        tool,
        host.clone(),
        body,
        INDEX_CHANNEL,
        "a beets task is already running (cancel it first)",
        Box::new(|| {}),
    )
}

/* 查询页：读 beets 库，转成歌单 */

/// 「组件没装」的统一文案。放在一处：三个调用点各写一句时，改一次要改三处，
/// 而漏改的那处会让 UI 的"安装指引"分支认不出来（`map_beets_start` 靠关键词
/// 分类，文案本身就参与判定）。
pub const NOT_INSTALLED: &str =
    "beets component not installed: install it in Settings, or set the component path";

/// 查询页的短进程超时。查询要做 `COUNT(*)` + 取一页，几万条的库也在秒级；
/// 30 s 是给"库刚被整理过、SQLite 在补索引"留的余量。
const QUERY_TIMEOUT: Duration = Duration::from_secs(30);

/// 读 beets 库（**索引家**）。
///
/// 索引家的 `directory` 指向**曲库根**，它就是"用户曲库的只读镜像"——库里的
/// 每一条 path 都必然在曲库里，分析字段（bpm / 情绪 / 流派）也只在这里有。
/// 反过来说，工作台家是用户自己的 beets 库，应用从不往里面导，为空是常态
/// （查到 0 条不是 bug，是"没配"）。
///
/// **兜底**：工具侧查询出口把 path 拼成绝对（`normpath(join(directory, path))`），
/// 宿主这边照旧只按 path 关联，不假设路径形态。
///
/// `rules` 与 `raw` 互斥（工具侧会拒）：不猜优先级，两个都给等于让调用方
/// 把"我到底想要什么"外包给一个 if。
pub fn query_page(
    host: &dyn BeetsHost,
    rules: Option<serde_json::Value>,
    raw: Option<String>,
    fields: Option<Vec<String>>,
    limit: Option<usize>,
) -> Result<aiya_dto::beets::BeetsQueryPage, String> {
    let (entry, _source) = discover(host).ok_or_else(|| NOT_INSTALLED.to_string())?;
    let launcher = launcher_for(host, &entry, query_profile());
    let data = aiya_beets_client::session::request_data(
        &launcher,
        &serde_json::json!({
            "id": 1, "cmd": "query",
            "rules": rules,
            "raw": raw,
            "fields": fields,
            "limit": limit,
        }),
        QUERY_TIMEOUT,
    )
    .map_err(|e| e.to_string())?;
    serde_json::from_value(data)
        .map_err(|e| format!("query response has an unexpected shape (protocol drift?): {e}"))
}

/// 可查字段清单（含插件写入的 flexible 键）+ 识别源三态。
///
/// 字段清单归工具侧回答（它才知道装了哪些插件）；宿主把它原样转给 UI。
/// **必须和 `query_page` 跑同一个家**：字段清单来自哪个库，查询就得查哪个库，
/// 否则 UI 会拿索引库的字段去查用户库（或反过来），症状是"能选 bpm 但一查就报
/// 未知字段"。
pub fn field_catalog(host: &dyn BeetsHost) -> Result<aiya_dto::beets::BeetsFieldCatalog, String> {
    let (entry, _source) = discover(host).ok_or_else(|| NOT_INSTALLED.to_string())?;
    let launcher = launcher_for(host, &entry, query_profile());
    let data = aiya_beets_client::session::request_data(
        &launcher,
        &serde_json::json!({ "id": 1, "cmd": "fields" }),
        QUERY_TIMEOUT,
    )
    .map_err(|e| e.to_string())?;
    serde_json::from_value(data)
        .map_err(|e| format!("field catalog has an unexpected shape (protocol drift?): {e}"))
}

/* 插件市场 */

/// 插件表的**编译期内嵌**副本（唯一事实源是工具侧那个 JSON）。
///
/// 为什么内嵌而不是"装完组件再问 sidecar"：安装页要在**什么都没装**的时候
/// 就把"有哪些插件、各自要装什么"展示出来让用户勾选。这条路径上没有 serve
/// 进程，只能靠编译期带一份。它和工具侧、打包脚本、安装器读的是同一个文件，
/// 所以不存在"UI 说 A、安装器装 B"的漂移；`build.rs` 里有 rerun-if-changed。
///
/// 双语分支：英文基名 + `plugins-zh_Hans.json` 中文变体。
/// 两份只有 `binaries[].why` / `vendor` 这两个展示字段允许不同，结构字段
/// （name/target/carrier/size/sha256/bundle/probe/license）逐字节一致——
/// 由 `catalog_contract.py` 的奇偶校验与 `plugin_table_variants` 单测共同守住。
/// 人类可读的展示字段随语言走，结构字段只有一份事实。
const PLUGIN_TABLE_JSON: &str =
    include_str!("../../../beets-tool/src/aiya_beets/data/plugins.json");
const PLUGIN_TABLE_JSON_ZH_HANS: &str =
    include_str!("../../../beets-tool/src/aiya_beets/data/plugins-zh_Hans.json");

/// 应用设置里的语言键（值 "zh" / "en"）。
const LANGUAGE_KEY: &str = "language";

/// 语言 → 插件表文本。值非 "zh"（空 / 未设 / 拼错）一律用英文基名——
/// "变体缺失回退基名"的回退就落在这个 match 的默认分支。
fn plugin_table_text(lang: &str) -> &'static str {
    match lang {
        "zh" => PLUGIN_TABLE_JSON_ZH_HANS,
        _ => PLUGIN_TABLE_JSON,
    }
}

/// 读应用设置的语言键（AppState 的 settings 内存镜像；镜像里没有 = 未设 = 英文）。
fn settings_language(host: &dyn BeetsHost) -> String {
    host.setting_str(LANGUAGE_KEY).unwrap_or_default()
}

#[derive(serde::Deserialize)]
struct PluginTableFile {
    schema: u32,
    #[serde(default)]
    profiles: PluginProfiles,
    #[serde(default)]
    plugins: Vec<aiya_dto::beets::BeetsPluginItem>,
}

#[derive(serde::Deserialize, Default)]
struct PluginProfiles {
    #[serde(default)]
    workbench: Vec<String>,
    #[serde(default)]
    index: Vec<String>,
}

/// 解析内嵌表（按设置语言选变体），并把 `profiles` 里的默认启用关系注入每一条。
///
/// 默认启用**不由条目自己声明**（否则又是两份事实）：JSON 里只有
/// `profiles.workbench` / `profiles.index` 两处列表，这里算一次。
pub fn plugin_table(host: &dyn BeetsHost) -> Result<Vec<aiya_dto::beets::BeetsPluginItem>, String> {
    plugin_table_in(&settings_language(host))
}

/// 纯函数版（可测）：语言 → 解析对应内嵌表。
pub fn plugin_table_in(lang: &str) -> Result<Vec<aiya_dto::beets::BeetsPluginItem>, String> {
    let mut doc: PluginTableFile = serde_json::from_str(plugin_table_text(lang)).map_err(|e| {
        format!("failed to parse the embedded plugin table (broken build artifact?): {e}")
    })?;
    if doc.schema != 2 {
        return Err(format!(
            "embedded plugin table schema={} not recognized (this binary only understands 2)",
            doc.schema
        ));
    }
    if doc.plugins.is_empty() {
        return Err("embedded plugin table is empty".into());
    }
    for item in &mut doc.plugins {
        item.default = doc.profiles.workbench.contains(&item.name);
        item.index_default = doc.profiles.index.contains(&item.name);
    }
    Ok(doc.plugins)
}

/// 按 path_formats 整理文件（`pretend` = 只出清单不搬文件）。
/// `directory` 是校验值：与 config.yaml 的 directory 不一致会被 sidecar 拒绝。
pub fn start_move(
    host: std::sync::Arc<dyn BeetsHost>,
    directory: String,
    pretend: bool,
    refresh_after: bool,
) -> Result<(), String> {
    start_maintenance(
        host,
        serde_json::json!({
            "id": 1, "cmd": "move_items",
            "directory": directory, "pretend": pretend,
            "requestId": new_request_id("move_items"),
        }),
        refresh_after,
    )
}

/// 组件当前是否可用（只做发现，不起进程）。
///
/// 刷新管线用它决定"要不要跑 beets 段"。刻意不复用 `probe`：探测要起一次
/// 子进程（约 300 ms），而定时刷新每 30 s 就会问一次 —— 这里只需要回答
/// "有没有入口"，有入口之后让 dump 自己失败（那时候的错误才值得报告）。
///
/// 索引增强卡片也用它：`available=false` 时按钮直接禁用，不必等一次探测。
pub fn available(host: &dyn BeetsHost) -> bool {
    discover(host).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    /* 环境：两家的家必须同步 */

    /// `BEETSDIR` 必须与 `AIYA_BEETS_HOME` 同值：工具侧按后者定位锁/登记簿，
    /// beets 只认前者。两者分家时症状是"索引跑完了却查不到"，所以这条不是
    /// 风格问题，是双家隔离的地基。
    #[test]
    fn env_pins_beetsdir_to_home() {
        let home = std::path::Path::new(r"D:\data\beets-index");
        let env = env_pairs(home, None, None, Profile::Index);
        let get = |k: &str| {
            env.iter()
                .find(|(key, _)| key == k)
                .map(|(_, v)| v.as_str())
                .unwrap_or("")
        };
        assert_eq!(get("AIYA_BEETS_HOME"), r"D:\data\beets-index");
        assert_eq!(get("BEETSDIR"), get("AIYA_BEETS_HOME"));
        // 没有扫描根时不写这个键（工具侧据此省略 directory 行）
        assert!(env.iter().all(|(k, _)| k != "AIYA_BEETS_SCAN_ROOT"));
    }

    /// 曲库根与落点是**两个事实**，必须各走各的键。
    ///
    /// 合成一个变量时，用户把落点改成 `曲库/整理` 会把索引范围一起缩成那一块
    /// ——索引跑完却查不出东西，而配置看上去完全合规。这条测试盯的就是那个键。
    #[test]
    fn env_separates_library_root_from_destination() {
        let home = std::path::Path::new("/tmp/beets");
        let env = env_pairs(
            home,
            Some("/music".to_string()),
            Some("/music/整理".to_string()),
            Profile::Index,
        );
        let get = |k: &str| {
            env.iter()
                .find(|(key, _)| key == k)
                .map(|(_, v)| v.as_str())
                .unwrap_or("")
        };
        assert_eq!(get("AIYA_BEETS_SCAN_ROOT"), "/music");
        assert_eq!(get("AIYA_BEETS_DEST"), "/music/整理");
    }

    /* 落点口径：与 beets_set_target 同一条判据 */

    /// 子目录落点必须**真的生效**，不能静默回退成曲库根。
    ///
    /// 落点下发判据（`roots_for_tools`）与校验判据（`beets_set_target`）分离时，
    /// 一处放宽子目录另一处仍是严格相等：「选了子目录当落点」被静默判为非法、
    /// 回退成曲库根——文件倒进曲库根，用户看不出是哪一步错了。
    /// 夹具按平台拆：落点判据走分隔符边界，盘符形态在 unix 上不是绝对路径。
    #[test]
    fn pick_dest_accepts_subdirectory() {
        let (root, sub) = if cfg!(windows) {
            ("D:\\Music".to_string(), "D:\\Music\\整理".to_string())
        } else {
            ("/music".to_string(), "/music/整理".to_string())
        };
        let roots = vec![root.clone()];
        assert_eq!(pick_dest(Some(sub.clone()), &roots), Some(sub));
        // 等于根也合法
        assert_eq!(pick_dest(Some(root.clone()), &roots), Some(root));
    }

    /// 曲库外的落点不算数 → 回退第一个根。
    /// 假兄弟（`Music2`）不能因为字符串前缀像就被当成子目录。
    #[test]
    fn pick_dest_falls_back_when_outside() {
        let (root, outside, sibling) = if cfg!(windows) {
            (
                "D:\\Music".to_string(),
                "E:\\Other".to_string(),
                "D:\\Music2".to_string(),
            )
        } else {
            (
                "/music".to_string(),
                "/srv/other".to_string(),
                "/music2".to_string(),
            )
        };
        let roots = vec![root.clone()];
        assert_eq!(
            pick_dest(Some(outside.clone()), &roots),
            Some(root.clone()),
            "曲库外回退第一个根"
        );
        assert_eq!(
            pick_dest(Some(sibling.clone()), &roots),
            Some(root.clone()),
            "假兄弟不是子目录"
        );
        // 未设置 → 回退第一个根
        assert_eq!(pick_dest(None, &roots), Some(root.clone()));
        // 没有根 → None（没有可落的地方）
        assert_eq!(pick_dest(Some(root), &[]), None);
    }

    #[test]
    fn env_passes_scan_root() {
        let home = std::path::Path::new("/tmp/beets");
        let env = env_pairs(home, Some("/music".to_string()), None, Profile::Workbench);
        let get = |k: &str| {
            env.iter()
                .find(|(key, _)| key == k)
                .map(|(_, v)| v.as_str())
                .unwrap_or("")
        };
        assert_eq!(get("AIYA_BEETS_SCAN_ROOT"), "/music");
    }

    /// profile 必须由宿主显式下发。
    ///
    /// `enrich` 若走工作台家（用户真库），`-R` 重导前的 `item.remove()` 会把
    /// 用户的条目删掉再按用户的 config 重导——"点一下分析"= 改用户的歌。
    /// 这条测试盯着那句环境变量有没有被写上。
    #[test]
    fn env_always_states_profile() {
        for (profile, want) in [(Profile::Workbench, "workbench"), (Profile::Index, "index")] {
            let env = env_pairs(std::path::Path::new("/tmp/x"), None, None, profile);
            let got = env
                .iter()
                .find(|(k, _)| k == "AIYA_BEETS_PROFILE")
                .map(|(_, v)| v.as_str());
            assert_eq!(got, Some(want), "profile 必须显式下发，不能靠工具侧默认");
        }
    }

    /// 分析必须跑在索引家（配套测试：`env_always_states_profile`）。
    #[test]
    fn enrich_runs_in_index_home() {
        assert_eq!(
            enrich_profile(),
            Profile::Index,
            "分析改回工作台家 = 改用户的歌"
        );
    }

    /// 查询/字段清单必须与分析同家。
    ///
    /// 分开看只是"少几行"，连起来看才是账单：字段清单来自索引库、查询打到工作台
    /// 库时，UI 会列出 bpm 这个字段而工作台库里根本没有它，用户看到的是
    /// "字段能选、一查就报未知字段"。
    #[test]
    fn query_runs_in_same_home_as_enrich() {
        assert_eq!(query_profile(), enrich_profile(), "查询与分析必须同家");
    }

    /// 两个 crate 各自有一份"路径 → path_lower 键"的实现：`aiya-beets-store`
    /// 用它写结论库的行键，`aiya-data::repo` 用它回曲库换 `track_id`。
    ///
    /// 它们**不能互相依赖**（依赖正是边界要断的东西），所以只能靠断言锁在
    /// 一起：一旦一边改了归一规则，症状不是报错，而是"增强结果一条都对不上
    /// 曲目"——那种 bug 只有对着真库才会显形。这条测试把两边拉到同一批样本上。
    #[test]
    fn path_key_agrees_across_crates() {
        let samples = [
            r"D:\Music\A\01 - Song.flac",
            "d:/music/a/01 - song.flac",
            r"E:\库\周杰伦\叶惠美\03.mp3",
            "E:/库/周杰伦/叶惠美/03.mp3",
            r"\\nas\share\Album\Track.flac",
            "/posix/style/path.flac",
        ];
        for s in samples {
            assert_eq!(
                aiya_beets_store::path_key(s),
                aiya_data::repo::path_key(s),
                "同一路径在两处归一结果不同：{s}"
            );
        }
    }

    /* 模块自有配置 */
    fn tmp_settings_dir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("aiya-beets-cfg-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// 内嵌插件表必须能解析，且默认启用关系由 `profiles` 注入。
    ///
    /// 守的是"二进制里的表 = 仓库里的 JSON"：`include_str!` 是编译期嵌入，
    /// JSON 写坏时**编译不会报错**，只有运行时解析才失败——而那时用户已经
    /// 在安装页点按钮了。
    #[test]
    fn embedded_plugin_table_parses_and_marks_defaults() {
        let rows = plugin_table_in("en").expect("内嵌插件表解析失败");
        assert!(rows.len() >= 10, "插件表条目太少：{}", rows.len());
        assert!(
            rows.iter().any(|r| r.name == "lyrics" && r.default),
            "lyrics 应当在 profiles.workbench 里"
        );
        assert!(
            rows.iter().any(|r| r.name == "xtractor" && r.index_default),
            "xtractor 应当在 profiles.index 里"
        );
        // 索引家只默认开一个（多开就等于把联网/外部二进制拖进索引步骤）
        assert_eq!(
            rows.iter().filter(|r| r.index_default).count(),
            1,
            "索引家默认启用的插件数变了，确认是否有意"
        );
        // 免配置包预置的插件：带外部命令的，那个命令必须**也随包发**（要带
        // 就带全）——否则装完的包里 xtractor 会被渲染侧剔掉（缺工具），用户
        // 看到的是"预置了但用不了"。
        let bundle_bins = bundle_binary_names();
        for r in rows.iter().filter(|r| r.bundle) {
            for dep in &r.exe {
                assert!(
                    bundle_bins.iter().any(|b| b == dep),
                    "{} 标了 bundle，但要的 {dep} 没随包发布（binaries 里缺 bundle: true）",
                    r.name
                );
            }
        }
        // 默认启用的插件同样不许依赖"用户机现下"的外部二进制：
        // 否则"默认安装"就等于强迫每个用户多下 87MB 的 ffmpeg，而绝大多数人不需要。
        for r in rows.iter().filter(|r| r.default) {
            assert!(r.exe.is_empty(), "{} 默认启用却需要外部命令", r.name);
        }
    }

    /// 内嵌表里 `binaries[].bundle` 的名字（与 `plugin_table` 读同一份 JSON）。
    ///
    /// 走 `Value` 而不是给 `PluginTableFile` 加字段：这一段**只有单测要**。
    /// 运行时不需要它——哪个插件要哪个工具由工具侧按插件名自己算
    /// （`bins --from-plugins`），宿主算就是第二份派生。加字段就得挂
    /// `#[allow(dead_code)]`，那正是"运行时不用但为测试留着"的味道。
    fn bundle_binary_names() -> Vec<String> {
        let doc: serde_json::Value =
            serde_json::from_str(PLUGIN_TABLE_JSON).expect("内嵌表解析失败");
        doc.get("binaries")
            .and_then(|b| b.as_array())
            .map(|rows| {
                rows.iter()
                    .filter(|r| r.get("bundle").and_then(serde_json::Value::as_bool) == Some(true))
                    .filter_map(|r| r.get("name").and_then(|v| v.as_str()).map(str::to_string))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// 双语奇偶：zh 变体与英文基名的**结构字段**必须逐字段
    /// 一致，只有 `binaries[].why` / `vendor` 允许不同。Python 侧
    /// `catalog_contract.py` 守仓库文件，这里守 Rust 内嵌的两份——两边都红
    /// 才能同时防住"改了仓库忘了重编"与"只改了一份"。
    #[test]
    fn plugin_table_variants_match_structurally() {
        // 变体缺失回退基名：任何语言值（含空 / 未设 / 乱写）都必须能解析
        for lang in ["", "en", "zh", "fr"] {
            let rows = plugin_table_in(lang).expect("任何语言都应落到可解析的表");
            assert!(rows.len() >= 10, "语言 {lang:?} 解析出的条目过少");
        }
        let en: serde_json::Value =
            serde_json::from_str(plugin_table_text("en")).expect("基名应可解析");
        let zh: serde_json::Value =
            serde_json::from_str(plugin_table_text("zh")).expect("zh 变体应可解析");
        assert_eq!(en["schema"], zh["schema"], "schema 两份必须一致");
        assert_eq!(en["beets"], zh["beets"], "beets 段两份必须一致");
        assert_eq!(en["profiles"], zh["profiles"], "profiles 两份必须一致");
        assert_eq!(
            en["plugins"], zh["plugins"],
            "plugins 段没有双语字段，必须逐字节一致"
        );
        let (en_b, zh_b) = (
            en["binaries"].as_array().expect("binaries 数组"),
            zh["binaries"].as_array().expect("binaries 数组"),
        );
        assert_eq!(en_b.len(), zh_b.len(), "binaries 条数必须一致");
        for (e, z) in en_b.iter().zip(zh_b.iter()) {
            let name = e["name"].as_str().unwrap_or("?");
            for key in [
                "name", "target", "size", "sha256", "bundle", "license", "probe",
            ] {
                assert_eq!(e[key], z[key], "{name}: binaries[].{key} 两份必须一致");
            }
            for key in ["kind", "url", "size", "sha256", "member"] {
                assert_eq!(
                    e["carrier"][key], z["carrier"][key],
                    "{name}: carrier.{key} 两份必须一致"
                );
            }
            for key in ["why", "vendor"] {
                assert!(
                    !e[key].as_str().unwrap_or_default().is_empty(),
                    "{name}: 基名 {key} 非空"
                );
                assert!(
                    !z[key].as_str().unwrap_or_default().is_empty(),
                    "{name}: zh 变体 {key} 非空"
                );
            }
        }
    }

    #[test]
    fn settings_roundtrip_and_corrupt_falls_back_to_default() {
        let dir = tmp_settings_dir("roundtrip");
        let path = dir.join("settings.json");
        let cfg = Settings {
            tool_path: Some("C:/x/aiya-beets.exe".into()),
            tool_args: Some("-m aiya_beets".into()),
            target_root: Some("D:/Music".into()),
            inbox_path: Some("D:/Inbox".into()),
            inbox_imported: vec!["D:/Inbox/a.flac".into()],
            plugins: Some(vec!["lyrics".into(), "fetchart".into()]),
        };
        write_at(&path, &cfg).unwrap();
        let back = load_at(&path);
        assert_eq!(back.tool_path, cfg.tool_path);
        assert_eq!(back.inbox_imported, cfg.inbox_imported);
        assert_eq!(back.plugins, cfg.plugins);
        // 缺 plugins 键的 settings.json 必须能读（否则升级后所有设置
        // 一起回落默认值，用户看到的是"我的配置没了"）
        let old = r#"{"toolPath":"C:/x/aiya-beets.exe","inboxImported":[]}"#;
        std::fs::write(&path, old).unwrap();
        let migrated = load_at(&path);
        assert_eq!(migrated.tool_path.as_deref(), Some("C:/x/aiya-beets.exe"));
        assert!(migrated.plugins.is_none());

        // 坏 JSON 不能让整个模块不可用：回落默认值，用户重填一次就好
        std::fs::write(&path, b"{ not json").unwrap();
        let broken = load_at(&path);
        assert!(broken.tool_path.is_none() && broken.inbox_imported.is_empty());
        // 不存在的文件同理
        assert!(load_at(&dir.join("nope.json")).tool_path.is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn config_view_falls_back_to_first_root() {
        let roots = vec!["D:/Music".to_string(), "E:/Audio".to_string()];
        let cfg = Settings {
            target_root: Some("D:/Music".into()),
            inbox_path: Some("  ".into()),
            ..Settings::default()
        };
        // 勾的目录仍在 → 生效值就是它；空串的 inbox 归一成 None
        let v = config_view_of(&cfg, &roots);
        assert_eq!(v.target_root.as_deref(), Some("D:/Music"));
        assert_eq!(v.effective_target.as_deref(), Some("D:/Music"));
        assert!(v.inbox.is_none(), "空串必须归一成 None");

        // 勾的目录被移除了 → 生效值回退第一个根，但**不清用户的选择**
        let gone = Settings {
            target_root: Some("F:/Gone".into()),
            ..Settings::default()
        };
        let v = config_view_of(&gone, &roots);
        assert_eq!(v.effective_target.as_deref(), Some("D:/Music"));
        assert_eq!(
            v.target_root.as_deref(),
            Some("F:/Gone"),
            "用户勾的目录不该被静默清掉——加回来时它自己恢复"
        );
    }

    /// 子目录落点必须**显示成生效值**，否则界面说"落到曲库根"、实际写到子目录。
    ///
    /// 显示判据与下发判据（`tool_env` 的 `AIYA_BEETS_DEST`）不是同一条时，
    /// 界面显示的就是回退后的曲库根，而工具拿到的落点是子目录。
    #[test]
    fn config_view_shows_subdirectory_target() {
        let roots = vec!["D:/Music".to_string()];
        let cfg = Settings {
            target_root: Some("D:/Music/整理".into()),
            ..Settings::default()
        };
        let v = config_view_of(&cfg, &roots);
        assert_eq!(v.target_root.as_deref(), Some("D:/Music/整理"));
        assert_eq!(
            v.effective_target.as_deref(),
            Some("D:/Music/整理"),
            "子目录落点必须原样显示，不能回退成曲库根"
        );
    }

    /* 安装 */

    /// 造一个最小 zip（内存里拼，不落盘）。
    fn make_zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
        use std::io::Write;
        let mut buf = std::io::Cursor::new(Vec::new());
        {
            let mut w = zip::ZipWriter::new(&mut buf);
            let opts = zip::write::SimpleFileOptions::default();
            for (name, data) in entries {
                w.start_file(*name, opts).unwrap();
                w.write_all(data).unwrap();
            }
            w.finish().unwrap();
        }
        buf.into_inner()
    }

    /// 只回一次响应的迷你 HTTP 服务（模拟 release 的静态文件）。
    ///
    /// 为什么不用现成的测试框架：这条路径要验的是"下载回来的字节是否原样
    /// 落盘"，一个 accept + write 就够了；引入 http 服务器 crate 只为一次
    /// GET 不值。
    fn serve_once(body: Vec<u8>) -> (String, std::thread::JoinHandle<()>) {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = std::thread::spawn(move || {
            if let Ok((mut sock, _)) = listener.accept() {
                let mut buf = [0u8; 2048];
                let _ = sock.read(&mut buf); // 请求内容不关心，读完就回
                let head = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = sock.write_all(head.as_bytes());
                let _ = sock.write_all(&body);
                let _ = sock.flush();
            }
        });
        (format!("http://{addr}/asset.zip"), handle)
    }

    /// 真实下载路径：字节原样落盘 + 形状校验放行。
    #[test]
    fn download_writes_bytes_and_verify_accepts_zip() {
        let body = make_zip(&[("manifest.json", br#"{"entry":"python.exe","args":[]}"#)]);
        let (url, handle) = serve_once(body.clone());
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("asset.zip");

        let n = download_to(&url, &dest).expect("下载应成功");
        handle.join().unwrap();
        assert_eq!(n as usize, body.len());
        assert_eq!(std::fs::read(&dest).unwrap(), body, "字节必须原样落盘");
        assert_eq!(
            verify_asset(&dest, AssetKind::Bundle).unwrap(),
            1,
            "1 个条目"
        );
    }

    /// 最常见的真实故障：release 没发布时下到的是一个 HTML 错误页。
    /// 这条要保证错误文案指向"下到了错误页"，而不是后面解压时报的"不是 zip"。
    #[test]
    fn verify_rejects_html_error_page() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("bundle.zip");
        std::fs::write(&p, b"<!DOCTYPE html><title>Not Found</title>").unwrap();
        let err = verify_asset(&p, AssetKind::Bundle).unwrap_err();
        assert!(err.contains("not a valid archive"), "{err}");
        assert!(err.contains("error page"), "要指得到原因：{err}");
    }

    /// 空文件是另一种常见状态（下载被中断、写到一半）。
    #[test]
    fn verify_rejects_empty_file() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("bundle.zip");
        std::fs::write(&p, b"").unwrap();
        let err = verify_asset(&p, AssetKind::Bundle).unwrap_err();
        assert!(err.contains("empty file"), "{err}");
    }

    /// 套件包的展开 + 入口校验：入口由 manifest 说了算（播放器不猜布局）。
    #[test]
    fn bundle_unpack_follows_manifest_entry() {
        let zip_bytes = make_zip(&[
            (
                "manifest.json",
                br#"{"entry":"python.exe","args":["-m","aiya_beets"]}"#,
            ),
            ("python.exe", b"fake interpreter"),
            ("Lib/site-packages/aiya_beets/__init__.py", b""),
        ]);
        let dir = tempfile::tempdir().unwrap();
        let zip_path = dir.path().join("bundle.zip");
        std::fs::write(&zip_path, &zip_bytes).unwrap();
        let target = dir.path().join("bundle");

        let entry = install_bundle_to(&target, &zip_path, &|_| {}).expect("展开应成功");
        assert!(entry.program.ends_with("python.exe"), "{}", entry.program);
        assert_eq!(entry.args, vec!["-m".to_string(), "aiya_beets".to_string()]);
        assert!(
            target.join("Lib/site-packages/aiya_beets").is_dir(),
            "包必须展开到库目录"
        );
    }

    /// manifest 缺失时按约定找入口（老套件包 / 手搓包）。
    #[test]
    fn bundle_unpack_falls_back_to_conventional_entry() {
        let zip_bytes = make_zip(&[("aiya-beets.exe", b"fake")]);
        let dir = tempfile::tempdir().unwrap();
        let zip_path = dir.path().join("bundle.zip");
        std::fs::write(&zip_path, &zip_bytes).unwrap();
        let target = dir.path().join("bundle");

        let entry = install_bundle_to(&target, &zip_path, &|_| {}).expect("展开应成功");
        assert!(
            entry.program.ends_with("aiya-beets.exe"),
            "{}",
            entry.program
        );
        assert!(entry.args.is_empty(), "单文件入口不该带参数");
    }

    /// 一个入口都没有 → 明确失败，不能留下"装好了但跑不起来"的目录。
    #[test]
    fn bundle_without_entry_fails_loudly() {
        let zip_bytes = make_zip(&[("readme.txt", b"nothing here")]);
        let dir = tempfile::tempdir().unwrap();
        let zip_path = dir.path().join("bundle.zip");
        std::fs::write(&zip_path, &zip_bytes).unwrap();
        let target = dir.path().join("bundle");

        let err = install_bundle_to(&target, &zip_path, &|_| {}).unwrap_err();
        assert!(err.contains("no entry point found"), "{err}");
    }

    /// 形状锚点：BeetsStatus 是设置页数据源，必须 camelCase 进 IPC。
    #[test]
    fn beets_status_serializes_camel_case() {
        // BeetsStatus 只有 Deserialize；用 JSON 反序列化锚定输入形状
        let v = serde_json::json!({
            "found": true, "interpreter": "py", "scriptPath": "-m aiya_beets",
            "entrySource": "settings",
            "protocolVersion": 1, "beetsVersion": "2.14.1", "pythonVersion": "3.14.7",
            "libraryPath": "l.db", "libraryExists": true,
            "libraryDirectory": "D:/Music",
            "plugins": ["musicbrainz"],
            "pluginHealth": [
                { "name": "lyrics", "state": "missing_deps", "enabled": true,
                  "missing": ["bs4"], "note": "歌词抓取" }
            ],
            "pluginSummary": { "ready": 4, "missingDeps": 2, "unavailable": 0 },
            "configDir": "c", "dataHome": "d",
            "schemaMatched": true, "libraryLock": "free", "libraryLockHolder": null,
            "errorCode": null, "errorMessage": null
        });
        let st: BeetsStatus = serde_json::from_value(v).expect("camelCase 形状必须可解析");
        assert_eq!(st.beets_version.as_deref(), Some("2.14.1"));
        assert_eq!(st.library_exists, Some(true));
        assert_eq!(st.plugins, Some(vec!["musicbrainz".to_string()]));
        assert_eq!(st.entry_source.as_deref(), Some("settings"));
        // 装后状态卡按探测结果出能力面：入口形状推导的结果要能过 IPC
        assert_eq!(st.data_home.as_deref(), Some("d"));
        // v2：schema 比对结论 + 库锁三态原样透传
        assert!(st.schema_matched);
        assert_eq!(st.library_lock.as_deref(), Some("free"));
        assert!(st.library_lock_holder.is_none());
        // 能力三态：不能只报"配置里开着"，要报"能不能干活"
        let health = st.plugin_health.expect("pluginHealth 必须解析");
        assert_eq!(health[0].state, "missing_deps");
        assert_eq!(health[0].missing, vec!["bs4".to_string()]);
        assert_eq!(st.plugin_summary.expect("pluginSummary").missing_deps, 2);
    }

    /// 未装工具时给的是**可解释的状态**，不是崩溃、也不是空对象。
    ///
    /// "未装 beets 时核心功能全通"有两半：依赖隔离由 `tools/check-boundary.mjs`
    /// 守住（核心 crate 连依赖都不许有）；beets 自己的命令面在未装时必须回一个
    /// 带错误码的状态，让 UI 能说清"没装"而不是"出错了"。
    #[test]
    fn absent_status_carries_code_and_entry() {
        let entry = ToolEntry {
            program: "C:/x/aiya-beets.exe".into(),
            args: vec!["serve".into()],
        };
        let st = absent(
            "E_TOOL_NOT_FOUND",
            Some("找不到".into()),
            Some(&entry),
            Some(EntrySource::Path),
        );
        assert!(!st.found);
        assert_eq!(st.error_code.as_deref(), Some("E_TOOL_NOT_FOUND"));
        assert_eq!(st.error_message.as_deref(), Some("找不到"));
        // 用户排障要知道"试着用了哪个程序、从哪找的"
        assert_eq!(st.interpreter.as_deref(), Some("C:/x/aiya-beets.exe"));
        assert_eq!(st.script_path.as_deref(), Some("serve"));
        assert_eq!(st.entry_source.as_deref(), Some("path"));
        // 失败态不许残留任何"看起来像成功"的字段
        assert!(
            st.beets_version.is_none() && st.plugin_health.is_none() && st.library_path.is_none()
        );
    }

    /// 忙碌（会话槽被占）**不是**不可用：`found` 必须留 true。
    ///
    /// 把 Busy 当不可达（`found=false`）的话，整理库页会自行跳到"未安装"的
    /// 安装页——用户刚点的预览还在跑，看起来像组件凭空消失。`busy_status`
    /// 用 `E_BEETS_BUSY` 堵住这条误判。
    #[test]
    fn busy_status_stays_found() {
        let entry = ToolEntry {
            program: "C:/x/aiya-beets.exe".into(),
            args: vec!["serve".into()],
        };
        let st = busy_status(&entry, EntrySource::Path);
        assert!(st.found, "忙碌时必须仍报 found，否则 UI 会当成未安装");
        assert_eq!(st.error_code.as_deref(), Some("E_BEETS_BUSY"));
        assert_eq!(st.interpreter.as_deref(), Some("C:/x/aiya-beets.exe"));
        // 没拿到报告就谈不上契约匹配，写操作应当被 UI 拦住
        assert!(!st.schema_matched);
    }

    /// 协议版本不符 → **工具存在但不可用**，且必须说清升哪一边。
    ///
    /// "不做自动协商"：两侧独立发布，版本不匹配是**常态**，不能表现为
    /// "功能时好时坏"，只能表现为一句能照做的话。
    #[test]
    fn protocol_mismatch_marks_tool_unusable() {
        let entry = ToolEntry {
            program: "aiya-beets".into(),
            args: vec![],
        };
        let mut report = StatusReport {
            protocol_version: Some(99),
            ..Default::default()
        };
        report.tool_version = Some("9.9.9".into());

        let st = status_from(&entry, EntrySource::Path, report.clone());
        assert!(!st.found, "版本不符不能算可用");
        assert_eq!(st.error_code.as_deref(), Some("E_PROTOCOL_MISMATCH"));
        let msg = st.error_message.clone().unwrap_or_default();
        assert!(
            msg.contains("update the player"),
            "组件太新时应提示升级播放器：{msg}"
        );
        // 版本/入口照样显示：用户要能看到"我装的是什么"
        assert_eq!(st.protocol_version, Some(99));
        assert_eq!(st.beets_version.as_deref(), Some("9.9.9"));

        // 反向：组件太老 → 提示升级组件
        let old = status_from(
            &entry,
            EntrySource::Path,
            StatusReport {
                protocol_version: Some(0),
                ..Default::default()
            },
        );
        assert!(!old.found);
        assert!(old
            .error_message
            .unwrap_or_default()
            .contains("update the component"));

        // 不报版本（老工具/被裁剪的输出）也要拒，不能默认信任
        let silent = status_from(&entry, EntrySource::Path, StatusReport::default());
        assert!(!silent.found);
        assert_eq!(silent.error_code.as_deref(), Some("E_PROTOCOL_MISMATCH"));
    }

    /// 用指定入口跑一次 status（测试用；生产走 `probe`，那里入口来自 `discover`）。
    ///
    /// `AIYA_BEETS_HOME` 不设 = 走 beets 自己的默认位置，不碰应用的 app_data。
    /// `PYTHONPATH` 指向仓库源码树，这样源码树里的工具能被 import。
    ///
    /// 一次性查询与会话共用进程级会话槽（client 侧修复）：两个真进程 probe
    /// 并行跑会有一方拿到 Busy。生产里 probe 是用户触发的低频操作，测试里
    /// 用这把锁串行化即可。
    static REAL_PROCESS_PROBE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn status_via(program: &str, args: &[&str]) -> BeetsStatus {
        let _probe_lock = REAL_PROCESS_PROBE_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let src = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../beets-tool/src");
        let launcher = Launcher::new(program)
            .args(args.iter().copied())
            .env("PYTHONIOENCODING", "utf-8")
            .env("PYTHONUTF8", "1")
            .env("PYTHONPATH", src.to_string_lossy());
        let entry = ToolEntry {
            program: program.into(),
            args: args.iter().map(|s| s.to_string()).collect(),
        };
        let tool = StdioTool::new(launcher);
        match tool.status(Duration::from_secs(20)) {
            Ok(report) => status_from(&entry, EntrySource::Env, report),
            Err(e) => absent(
                "E_SPAWN",
                Some(e.to_string()),
                Some(&entry),
                Some(EntrySource::Env),
            ),
        }
    }

    /// 套件包（自带 embeddable 解释器 + 包在 Lib/site-packages）能命中工具。
    ///
    /// 这条守的是分发的核心前提：套件包**不需要系统装 Python**，
    /// 解压到任意路径都能跑。产物由 `beets-tool/tools/build_bundle.py` 生成，
    /// 没生成就跳过（clean checkout / CI 上默认不跑这条）。
    ///
    /// 注意这里刻意**不**靠 `PYTHONPATH` 传包：embeddable 带 `python3xx._pth`
    /// 时 Python 会忽略 `PYTHONPATH`，包只能落在 `Lib/site-packages`。
    /// 换句话说本测试同时验证了「套件包是自洽的」——外部环境给不了一点帮助。
    #[cfg(debug_assertions)]
    #[test]
    fn probe_finds_bundle_runtime() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../beets-tool/dist/bundle");
        let py = root.join("python.exe");
        if !py.exists() {
            return; // 没构建套件包（clean checkout / CI）就跳过
        }
        let manifest: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(root.join("manifest.json")).expect("manifest"),
        )
        .expect("manifest json");
        assert_eq!(
            manifest["entry"], "python.exe",
            "manifest 的 entry 必须与实测入口一致"
        );
        let status = status_via(&py.to_string_lossy(), &["-m", "aiya_beets"]);
        assert!(
            status.found,
            "套件包应命中工具，但被拒了（协议 v{:?}，播放器要 v{}）：{:?}\n  \
             → 多半是 dist/bundle 还是上次构建的旧版，跑 npm run beets:bundle 重新打包",
            status.protocol_version,
            aiya_beets_client::PROTOCOL_VERSION,
            status.error_message,
        );
        assert_eq!(
            status.protocol_version,
            Some(aiya_beets_client::PROTOCOL_VERSION),
            "套件包报的协议版本与播放器不一致"
        );
        assert!(
            status
                .beets_version
                .as_deref()
                .unwrap_or_default()
                .starts_with("2.14"),
            "套件包自报的 beets 版本: {:?}",
            status.beets_version
        );
        // 自带解释器 ⇒ 运行时版本必须自报
        assert!(status.python_version.is_some(), "自带解释器应自报版本");
    }

    /// 端到端：真 sidecar dump_items（dev venv + dev 隔离库，冒烟脚本留下的 3 条）
    /// → Rust 逐行解析（协议形状锚点）。
    /// dump_items 保留为协议命令（诊断/未来显式流程用），宿主没有常规调用方。
    #[cfg(debug_assertions)]
    #[test]
    fn dump_parse_end_to_end() {
        let venv = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../beets-tool/.venv/Scripts/python.exe");
        let beets_home = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../beets-tool/tests/fixtures/beets-home");
        if !venv.exists() || !beets_home.exists() {
            return; // CI/其他机器没有 dev 环境就跳过
        }
        let mut child = std::process::Command::new(&venv)
            .args(["-m", "aiya_beets"])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            // 与生产路径同一套环境（编码钉 UTF-8 + 数据根），再覆盖隔离库
            .env("PYTHONIOENCODING", "utf-8")
            .env("PYTHONUTF8", "1")
            .env(
                "PYTHONPATH",
                PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../beets-tool/src"),
            )
            .env("BEETSDIR", &beets_home)
            .spawn()
            .expect("spawn sidecar");
        use std::io::Write;
        writeln!(
            child.stdin.take().expect("stdin"),
            r#"{{"id":1,"cmd":"dump_items"}}"#
        )
        .unwrap();

        let mut stdout = child.stdout.take().expect("stdout");
        let mut items: Vec<serde_json::Value> = Vec::new();
        {
            use std::io::BufRead;
            for line in std::io::BufReader::new(&mut stdout).lines() {
                let Ok(line) = line else { break };
                let Ok(v) = serde_json::from_str::<serde_json::Value>(line.trim()) else {
                    continue;
                };
                if v.get("type").and_then(|t| t.as_str()) == Some("item") {
                    items.push(v["item"].clone());
                }
            }
        }
        let _ = child.wait();
        assert!(
            items.len() >= 3,
            "dev 库应有 B2 冒烟写入的 3 条，实际 {}",
            items.len()
        );
        for it in &items {
            assert!(it["id"].as_i64().unwrap_or(0) > 0, "item id 缺失：{it}");
            assert!(
                !it["path"].as_str().unwrap_or_default().is_empty(),
                "item path 缺失：{it}"
            );
            assert!(it["title"].is_string(), "item title 形状：{it}");
        }
    }

    /* 卸载：计划解析 */

    /// `last_frame` 取**最后一条**匹配的帧。
    ///
    /// 工具侧可能在协议内容前打人类可读提示（迁移日志），一次输出里也可能
    /// 有多条 `done`。取错就会拿旧账目去删——那是"删了 A 报了 B"。
    #[test]
    fn last_frame_takes_the_last_match_and_skips_noise() {
        let text = "迁移日志：建表\n\
                    {\"type\":\"progress\",\"message\":\"a\"}\n\
                    {\"type\":\"done\",\"files\":7,\"bytes\":99}\n\
                    {\"type\":\"done\",\"files\":2,\"bytes\":10}\n\
                    {\"type\":\"error\",\"message\":\"boom\"}\n";
        let done = last_frame(text, "done").expect("应当找到 done");
        assert_eq!(done["files"], 2, "必须取最后一条 done");
        assert_eq!(done["bytes"], 10);
        let err = last_frame(text, "error").expect("应当找到 error");
        assert_eq!(extract_message(&err).as_deref(), Some("boom"));
        assert!(last_frame(text, "warning").is_none());
        assert!(last_frame("", "done").is_none(), "空输出不该凭空造帧");
    }

    /// 退出码与帧的组合决定"删不删"：**只有** `2 + error 帧` 才是闸门拒绝。
    ///
    /// 两侧都错不得：把崩溃误判成拒绝，用户就永远卸不掉；把拒绝当崩溃回落，
    /// 闸门就成了装饰（那是唯一会 rmtree 的地方）。
    #[test]
    fn uninstall_plan_only_refuses_on_exit_2_with_error_frame() {
        let ok = "{\"type\":\"done\",\"files\":3,\"bytes\":42}\n";
        match plan_from_output(Some(0), ok) {
            UninstallPlan::Ready { files, bytes } => assert_eq!((files, bytes), (3, 42)),
            _ => panic!("exit 0 + done 应当是 Ready"),
        }
        let refused = "{\"type\":\"error\",\"message\":\"拒绝卸载：目标是盘根\"}\n";
        match plan_from_output(Some(2), refused) {
            UninstallPlan::Refused(msg) => assert!(msg.contains("盘根"), "{msg}"),
            _ => panic!("exit 2 + error 应当是 Refused"),
        }
        // exit 2 但没有 error 帧 = 工具侧自己崩了，不能当成"不许删"
        assert!(matches!(
            plan_from_output(Some(2), "Traceback...\n"),
            UninstallPlan::Unavailable
        ));
        // 非闸门退出码带 error 帧也不拦：那是执行失败，不是拒绝
        assert!(matches!(
            plan_from_output(Some(1), refused),
            UninstallPlan::Unavailable
        ));
        assert!(matches!(
            plan_from_output(None, ""),
            UninstallPlan::Unavailable
        ));
        // exit 0 却没有 done 帧 = 没跑完，账目不可信
        assert!(matches!(
            plan_from_output(Some(0), "{\"type\":\"progress\"}\n"),
            UninstallPlan::Unavailable
        ));
    }
}
