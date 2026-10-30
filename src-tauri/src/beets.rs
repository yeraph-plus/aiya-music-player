//! beets 侧工具（`aiya-beets`）的宿主侧：**发现入口 + 结果落地**。
//!
//! 这个文件里**不再有 beets 的环境知识**（docs/33 M2）：
//! 没有 include_str!、没有解释器探测、没有 config.yaml 拼装、没有插件清单。
//! 播放器只做两件事：
//!   ① 找到工具的入口（四个来源，见 `discover`），带上数据目录起进程；
//!   ② 把工具报回来的结果落地（协议形状 → UI DTO、dump → 自有库对账）。
//!
//! 工具怎么装、装到哪、配置怎么写、开哪些插件，全归 `beets-tool/` 那边。
//! 唯一共享的东西是数据目录约定 `{app_data}/beets`（docs/33 §3.1）。
//!
//! **未安装时核心播放功能必须全通**——beets 是增强能力，不是依赖。

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use aiya_beets_client::{BeetsTool, EventSink, Launcher, StatusReport, StdioTool, ToolError};
use tauri::{Emitter, Manager};

/// 设置页 beets 状态区数据源。形状定义搬去 `dto::beets`（契约归 dto），
/// 这里 `pub use` 回来，本文件其余调用点不用改。
pub use crate::dto::beets::BeetsStatus;

/* ---------------- 发现（docs/33 §6.1） ---------------- */

/// 工具入口：程序 + 参数。
///
/// 为什么是「程序 + 参数」而不是单个可执行路径：源码树里跑是
/// `<venv>/python.exe -m aiya_beets`，装好的包是 `aiya-beets.exe`。
/// 统一成这个形状后，三种形态共用同一套代码。
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
    /// 仓库源码树（仅 debug 构建）
    #[cfg(debug_assertions)]
    Dev,
}

impl EntrySource {
    pub fn as_str(self) -> &'static str {
        match self {
            EntrySource::Env => "env",
            EntrySource::Settings => "settings",
            EntrySource::Path => "path",
            #[cfg(debug_assertions)]
            EntrySource::Dev => "dev",
        }
    }
}

/// 数据目录约定：`{app_data}/beets`。
///
/// 这是播放器与工具**唯一共享的约定**（docs/33 §3.1）：播放器决定"数据放哪"
/// （应用数据目录归宿主管），工具决定"里面怎么摆"。不传的话，工具会去
/// `%APPDATA%\beets`，与用户自己的 beets 抢同一个库。
pub fn data_home(app: &tauri::AppHandle) -> PathBuf {
    crate::paths::data_root(app).join("beets")
}

/// beets 的"双家"（docs/45 §4）：工作台家与索引家。
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

    fn home(self, app: &tauri::AppHandle) -> PathBuf {
        match self {
            Profile::Workbench => data_home(app),
            Profile::Index => index_home(app),
        }
    }
}

/// 索引家的数据目录：`{app_data}/beets-index`。
///
/// 与工作台家并列，**不是**它的子目录：beets 会往 home 里放 `config.yaml`、
/// `library.db`、`state.pickle`，两个家共用父目录只会让"手滑清了工作台"
/// 顺带清掉索引。
pub fn index_home(app: &tauri::AppHandle) -> PathBuf {
    crate::paths::data_root(app).join("beets-index")
}

/// beets 模块自有的派生数据（索引增强结论）。
///
/// 放在索引家里而不是应用库里：见 `crates/aiya-beets-store` 的头注。一句话，
/// 它是 beets 的中间产物，应用的数据模型里不该有它。
pub fn enrich_db(app: &tauri::AppHandle) -> PathBuf {
    index_home(app).join("enrich.db")
}

/* ---------------- 模块自有配置：不在应用 settings 表里 ---------------- */

/// beets 模块自己的配置，落在 `{data_home}/settings.json`。
///
/// ## 为什么搬出应用的 settings 表
///
/// 这些键（`beets.toolPath` / `beets.targetRoot` / `beets.inboxImported` …）
/// 曾经存在应用的 `settings` 表里。它们不是应用设置：应用从不读它们来做任何
/// 决定，只有 beets 模块自己读。放在应用表里有两个真实代价：
///
/// 1. **数据模型耦合**：应用的 settings 里出现"因为装了 beets 才有"的键，
///    备份、迁移、"导出我的设置"都会被它们污染。
/// 2. **写路径耦合**：`clear_target_if_invalid` 让**应用**在曲库根变化时去改
///    beets 的配置——方向反了。现在改成读时校验：target 不在扫描根里就当没设，
///    用户把目录加回来时它自己恢复，比"被悄悄清掉"更好。
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

pub fn settings_path(app: &tauri::AppHandle) -> PathBuf {
    data_home(app).join("settings.json")
}

/// 读配置。任何读取失败（不存在、坏 JSON、权限）都回落默认值：
/// 一份坏掉的配置文件不该让 beets 模块整体不可用——用户重填一次就好。
pub fn load_settings(app: &tauri::AppHandle) -> Settings {
    load_at(&settings_path(app))
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
fn write_settings(app: &tauri::AppHandle, s: &Settings) -> Result<(), String> {
    write_at(&settings_path(app), s)
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
    app: &tauri::AppHandle,
    f: impl FnOnce(&mut Settings) -> R,
) -> Result<R, String> {
    let _guard = SETTINGS_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut s = load_settings(app);
    let out = f(&mut s);
    write_settings(app, &s)?;
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
pub fn remember_imported(app: &tauri::AppHandle, paths: Vec<String>) -> Result<(), String> {
    update_settings(app, move |s| {
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
pub fn config_view(app: &tauri::AppHandle, roots: &[String]) -> crate::dto::beets::BeetsConfig {
    config_view_of(&load_settings(app), roots)
}

/// 纯函数版（可测）：配置 + 曲库目录 → 视图。
///
/// 落点合法性走 [`pick_dest`]，与 `beets_set_target` 的校验、`tool_env` 的
/// 下发**同一套判据**。这三处曾经各写一遍，放宽子目录时漏掉其中一处就会
/// 出现"界面显示 A、实际写到 B"——2026-10-30 真的发生了（`config_view_of`
/// 还留着严格相等，UI 显示曲库根而 `tool_env` 已经下发子目录）。
fn config_view_of(s: &Settings, roots: &[String]) -> crate::dto::beets::BeetsConfig {
    let target = trimmed(s.target_root.clone());
    let effective = pick_dest(target.clone(), roots);
    crate::dto::beets::BeetsConfig {
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
pub fn adopt_legacy_settings(app: &tauri::AppHandle, legacy: Settings) -> bool {
    let path = settings_path(app);
    if path.exists() {
        return false;
    }
    write_settings(app, &legacy).is_ok()
}

/// 一次取两个事实：**曲库根**（索引家的分析对象）与**整理落点**（整理家的
/// `directory`，copy/move 的目标）。
///
/// 为什么必须分开（docs/49 §3.2）：落点常常只是曲库里一个子目录（`曲库/整理`）。
/// 两者合成一个变量时，用户改落点会把索引范围一起缩成那一小块——症状是
/// "索引跑完了，可查询里只有几条"，而配置看起来完全合规。
///
/// 优先级：
/// * 曲库根 = **第一个扫描根**，永远不取勾选的落点；
/// * 落点 = 勾选值（若它仍是某个扫描根）否则退回第一个扫描根——与"勾选框出现
///   之前"的行为一致（docs/40 十三轮），老配置零迁移。
fn roots_for_tools(app: &tauri::AppHandle) -> (Option<String>, Option<String>) {
    // 勾选值来自**本模块自己的配置**，扫描根来自**应用**（用户事实）。
    // 两者分开取：配置和应用库各有各的写者，混在一次读里会让"谁的目录算数"
    // 这个问题有两个答案。
    let target = trimmed(load_settings(app).target_root);
    app.try_state::<crate::AppState>()
        .and_then(|st| st.db().send(|conn| Ok(crate::read_roots(conn))).ok())
        .map(|roots| (roots.first().cloned(), pick_dest(target, &roots)))
        .unwrap_or((None, None))
}

/// 落点取哪个：勾选值合法就用它，否则回退第一个扫描根。
///
/// 合法性判据与 `beets_set_target` **必须同一条**：落点可以是曲库目录本身，
/// 也可以是它的子目录（docs/49 §3.4）。
///
/// 曾经这里是 `canon_path(r) == canon_path(t)`（严格相等），而校验已经放宽到
/// 子目录 —— 后果是「选了子目录当落点」被静默判为非法、回退成曲库根：设置里
/// 明明写着 `D:\Music\整理`，实际整理却把文件倒进 `D:\Music`。两条判据分离
/// 就会这样，所以复用同一个函数，并由 `pick_dest_*` 的单测钉住。
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

/// 按优先级找工具入口（docs/33 §6.1）。
///
/// 顺序理由：显式指定永远第一 → 安装器写下的路径最可靠 → PATH 说明用户
/// 自己装过（pip install）→ 源码树只在开发机上存在。
///
/// **刻意没有"猜 venv 布局"这一条**：A 轨道的入口是安装器**写进设置**的，
/// 不是探测出来的。播放器不知道 venv 长什么样（P1 禁止），它只记住"入口在哪"。
pub fn discover(app: &tauri::AppHandle) -> Option<(ToolEntry, EntrySource)> {
    if let Ok(program) = std::env::var("AIYA_BEETS_BIN") {
        if !program.trim().is_empty() {
            let args = std::env::var("AIYA_BEETS_BIN_ARGS")
                .map(|s| split_args(&s))
                .unwrap_or_default();
            return Some((ToolEntry { program, args }, EntrySource::Env));
        }
    }

    let cfg = load_settings(app);
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

    // 开发便利：仓库里就有源码 + dev 环境。仅 debug 构建，release 不编译这段。
    #[cfg(debug_assertions)]
    {
        let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
        let py = repo.join("beets-tool/.venv/Scripts/python.exe");
        if py.is_file() && repo.join("beets-tool/src/aiya_beets").is_dir() {
            return Some((
                ToolEntry {
                    program: py.to_string_lossy().into_owned(),
                    args: vec!["-m".into(), "aiya_beets".into()],
                },
                EntrySource::Dev,
            ));
        }
    }

    None
}

/// 起进程时的环境。只给**事实**，不给 beets 知识。
///
/// * `AIYA_BEETS_HOME`：数据根（播放器给）
/// * `BEETSDIR`：**必须显式下发且与上面一致**（docs/45 §6.3）
/// * `AIYA_BEETS_SCAN_ROOT`：**曲库根**（索引家的分析对象，永不取落点）
/// * `AIYA_BEETS_DEST`：**整理落点**（整理家写进 `directory`，copy/move 的目标）
/// * `PYTHONIOENCODING` / `PYTHONUTF8`：管道编码钉 UTF-8。工具内部也 reconfigure
///   了一次，这里是双保险（工具被替换/被单独调用时仍正确）。这两个是 Python
///   通用变量，不是 beets 知识，留着不亏。
///
/// ## 为什么要显式下发 `BEETSDIR`
///
/// 工具侧的家（`config.yaml` / 锁 / ops 登记簿 / 断点）全按 `AIYA_BEETS_HOME`
/// 定位，而 beets 自己只认 `BEETSDIR`。以前靠工具"翻译"这一个方向，且遇到
/// **外部已存在的** `BEETSDIR` 就直接让步——dev shell 里 export 过一个，
/// 于是出现"beets 写 A 家、锁建在 B 家"：双家隔离静默失效，症状是"索引跑完了
/// 却查不到"。现在两侧都钉死：宿主显式覆盖（`env()` 优先于继承值），
/// 工具侧再在两者不等时 fail-fast。
fn tool_env(app: &tauri::AppHandle, profile: Profile) -> Vec<(String, String)> {
    // 源码树形态需要 PYTHONPATH 才 import 得到包
    #[cfg(debug_assertions)]
    let dev_src = {
        let src = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../beets-tool/src");
        if src.is_dir() {
            Some(src)
        } else {
            None
        }
    };
    #[cfg(not(debug_assertions))]
    let dev_src: Option<PathBuf> = None;

    let (root, dest) = roots_for_tools(app);
    env_pairs(&profile.home(app), root, dest, dev_src, profile)
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
    dev_src: Option<PathBuf>,
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
    if let Some(src) = dev_src {
        env.push(("PYTHONPATH".to_string(), src.to_string_lossy().into_owned()));
    }
    env
}

/// 入口 + 环境 → 可直接起进程的描述。
fn launcher_for(app: &tauri::AppHandle, entry: &ToolEntry, profile: Profile) -> Launcher {
    Launcher::new(&entry.program)
        .args(entry.args.clone())
        .envs(tool_env(app, profile))
}

/// 事件出口：把 client 抽象出来的通道接到 Tauri 事件上。
///
/// 这层适配只有一行实质代码，但它把「crate 不依赖 Tauri」变成硬约束——
/// client 里再也写不出 `app.emit(...)`，宿主形态也就不会渗回协议层。
struct TauriSink(tauri::AppHandle);

impl EventSink for TauriSink {
    fn emit(&self, channel: &'static str, payload: serde_json::Value) {
        let _ = self.0.emit(channel, payload);
    }
}

/// 协议版本闸门：`None` = 兼容；`Some(文案)` = 拒用，并在文案里说清升哪一边。
///
/// 为什么不做自动协商（docs/33 §4.4）：协商会产生"一半功能能用一半不能"的
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
            "beets 组件协议 v{v}，播放器需要 v{want}：请升级组件",
        )),
        Some(v) => Some(format!(
            "组件协议 v{v} 比播放器支持的 v{want} 新：请升级播放器",
        )),
        None => Some(format!("组件没有报协议版本（应为 v{want}）：请重装组件",)),
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
        python_supported: report.runtime_supported,
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
/// v2 接线 schemaHash 运行时比对（docs/35 P1-4 闭合）：工具自报的
/// `schemaHash` 与它一比就知道"装的是不是同一版契约"——版本号相同但
/// schema 字节不同（装错版本、手改过文件）从此骗不过宿主。meta 是提交
/// 进仓的合法 JSON，include_str 把"文件不存在"变成编译错误。
fn expected_schema_sha256() -> &'static str {
    static HASH: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    HASH.get_or_init(|| {
        serde_json::from_str::<serde_json::Value>(include_str!(
            "../../protocol/aiya-beets-v2.meta.json"
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
/// "我们试着用了哪个程序"，否则报错只有一句"不可用"。
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
        python_supported: None,
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
    app: &tauri::AppHandle,
    entry: &ToolEntry,
    source: EntrySource,
    timeout: Duration,
) -> Result<BeetsStatus, ToolError> {
    let tool = StdioTool::new(launcher_for(app, entry, Profile::Workbench));
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
/// 并把用户踹到安装页——而真相是"你刚点的预览/整理还在跑"。
/// 2026-10-30 真机撞到的就是这个：预览在联网匹配（MusicBrainz 单请求 6 s，
/// 一张专辑要发很多次），期间页面自己跳去了安装页，看起来像组件凭空没了。
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
        python_supported: None,
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
        library_lock_holder: Some("本应用正在执行 beets 任务".into()),
        error_code: Some("E_BEETS_BUSY".into()),
        error_message: Some("组件正在执行其它任务（完成后再看状态）".into()),
    }
}

/// 当前状态。找不到入口 → not_installed（UI 显示安装指引，播放功能不受影响）。
pub fn probe(app: &tauri::AppHandle) -> BeetsStatus {
    let Some((entry, source)) = discover(app) else {
        return absent(
            "E_BEETS_NOT_INSTALLED",
            Some("未找到 aiya-beets 组件。可在下方安装，或指定已有路径。".into()),
            None,
            None,
        );
    };
    match query_sidecar(app, &entry, source, Duration::from_secs(10)) {
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

/* ---------------- 安装：两条轨道（docs/33 §6.2） ----------------
 *
 * 播放器只做四件事：下载 → 展开 → 记下入口 → 跑一次 status 验证。
 * 它**不建 venv、不装 pip 包、不碰 Python 工具链**——那些全在工具侧的
 * 安装器里（A 轨道），或干脆不需要（B 轨道自带解释器）。
 *
 * 成功判据刻意不是"退出码 0"，而是"装完 probe 得到 found"。退出码可能骗人：
 * pip 装到一半也能 0，套件包解压成功也可能缺 DLL。
 */

/// 安装轨道。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallTrack {
    /// A：本机已有 Python → 跑 `.pyz` 安装器建 venv
    Installer,
    /// B：免安装套件包（自带解释器，可重定位）
    Bundle,
}

impl InstallTrack {
    pub fn parse(raw: &str) -> Result<Self, String> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "" | "auto" | "installer" | "a" => Ok(InstallTrack::Installer),
            "bundle" | "b" => Ok(InstallTrack::Bundle),
            other => Err(format!("未知安装轨道：{other}（可用：installer / bundle）")),
        }
    }
}

/// 发布物的默认位置。
///
/// 优先级见 [`release_base`]：环境变量 > 设置 > 这里。做成可配置而不是硬编码，
/// 是因为"发布到哪"是**发布流程**的事，不该逼着播放器改代码再发版；
/// 也让两条轨道的下载路径能在本地用 `python -m http.server` 真实走一遍
/// （`AIYA_BEETS_RELEASE_BASE=http://127.0.0.1:8000`）。
///
/// 指向**浮动 tag `beets-latest`**（docs/51 §3.4）。
///
/// 为什么不再用 `releases/latest/download`：那是"最近创建的 release"，而 beets
/// 与播放器是两个独立发布节奏。beets 单独发一版，latest 就指过去；之后发播放器
/// release（不带 beets 资产），这里又 404。两边互相踩。
///
/// `release-beets.yml` 每次发布都会把同样两个资产 `--clobber` 到
/// `beets-latest` 这个 prerelease 上，所以这里的 URL 永久稳定，与播放器发布
/// 完全解耦。用浮动 tag 而不是查 GitHub API：零限流、零 JSON 解析、出问题时
/// `git ls-remote --tags` 就能看见。
///
/// 下载失败会给"下到了什么"的明确错误（见 `verify_asset`），而不是静默卡住。
const RELEASE_BASE: &str =
    "https://github.com/yeraph-plus/aiya-music-player/releases/download/beets-latest";
const INSTALLER_ASSET: &str = "aiya-beets-installer.pyz";
const BUNDLE_ASSET: &str = "aiya-beets-bundle-win-x64.zip";

/// 发布物根地址：`AIYA_BEETS_RELEASE_BASE` > 默认常量。
///
/// 曾经还有一层「设置 `beets.releaseBase`」，2026-10-27 删掉了：那个键在设置页、
/// schema、mock 里都没有入口，也就是**写了没人能设**——三层优先级实际只有两层
/// 生效，剩下一层是死分支。真要支持镜像源，顺序是先给入口再留分支，反过来等于
/// 没做。眼下环境变量这一层够用：CI 与本地演练都用它指过去。
///
/// 尾斜杠统一去掉：`{base}/{asset}` 拼出来不能出现 `//`（有些静态服务器会 404）。
fn release_base() -> String {
    let raw = std::env::var("AIYA_BEETS_RELEASE_BASE")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| RELEASE_BASE.to_string());
    raw.trim().trim_end_matches('/').to_string()
}

fn install_root(app: &tauri::AppHandle) -> PathBuf {
    crate::paths::data_root(app).join("beets-tool")
}

/// 发布物的形状。两条轨道都是 zip 容器（`.pyz` 就是换了扩展名的 zip）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AssetKind {
    /// A 轨道：`aiya-beets-installer.pyz`
    Installer,
    /// B 轨道：`aiya-beets-bundle-win-x64.zip`
    Bundle,
}

impl AssetKind {
    fn label(self) -> &'static str {
        match self {
            AssetKind::Installer => "安装器（.pyz）",
            AssetKind::Bundle => "套件包（.zip）",
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
        .map_err(|e| format!("下载失败（{url}）：{e}"))?;
    if let Some(len) = resp
        .header("content-length")
        .and_then(|v| v.trim().parse::<u64>().ok())
    {
        if len > ASSET_MAX_BYTES {
            return Err(format!(
                "下载物声明大小 {} MB，超过 {} MB 上限（地址指错了？）：{url}",
                len / 1024 / 1024,
                ASSET_MAX_BYTES / 1024 / 1024
            ));
        }
    }
    let mut out = std::fs::File::create(dest).map_err(|e| format!("写临时文件失败：{e}"))?;
    // take(上限+1)：多读 1 字节用来区分"正好在上限"与"超了"
    let mut body = resp.into_reader().take(ASSET_MAX_BYTES + 1);
    let n = std::io::copy(&mut body, &mut out).map_err(|e| format!("下载写入失败：{e}"))?;
    if n > ASSET_MAX_BYTES {
        drop(out);
        let _ = std::fs::remove_file(dest);
        return Err(format!(
            "下载物超过 {} MB 上限，已中止（地址指错了？）：{url}",
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
        return Err(format!("{} 是空文件（下载没成功）", kind.label()));
    }
    let file = std::fs::File::open(path).map_err(|e| format!("打开{}失败：{e}", kind.label()))?;
    match zip::ZipArchive::new(file) {
        Ok(z) if !z.is_empty() => Ok(z.len()),
        Ok(_) => Err(format!("{} 是空压缩包（构建有问题）", kind.label())),
        Err(_) => Err(format!(
            "{} 不是有效的压缩包（{} 字节）——多半是下到了错误页，\
             或这个版本的发布物还没传上去",
            kind.label(),
            size
        )),
    }
}

/// 取安装物：环境变量指本地文件优先，否则下载到临时目录。
///
/// 两个来源都过 [`verify_asset`]：本地文件同样可能是半截下载
/// （浏览器中断、解压工具写了一半），而"形状不对"的错误在这里说最清楚。
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
                "message": format!("使用本地安装物：{}", p.display()),
            }));
            verify_asset(&p, kind)?;
            return Ok(p);
        }
        return Err(format!("{env_key} 指向的文件不存在：{}", p.display()));
    }

    let base = release_base();
    let url = format!("{base}/{asset}");
    emit(serde_json::json!({
        "type": "progress", "stage": "download",
        "message": format!("下载 {asset}"),
    }));
    let dest = std::env::temp_dir().join(asset);
    download_to(&url, &dest)
        .map_err(|e| format!("{e}\n——若组件尚未发布，可手动下载后设置 {env_key} 指向该文件"))?;
    verify_asset(&dest, kind)?;
    Ok(dest)
}

/// 一个可用的本机 Python。
#[derive(Debug, Clone)]
pub struct PythonInfo {
    /// 绝对路径（`py -0p` 给的就是绝对路径；PATH 命中的也换成绝对路径）
    pub program: String,
    /// `3.12.4`
    pub version: String,
    /// `py-launcher` | `python` | `python3`
    pub source: &'static str,
}

/// 工具侧的支持窗口（`beets-tool/src/aiya_beets/env.py` 的 PYTHON_MIN/MAX）。
/// 只用于"能不能装"的判断与提示；装完到底行不行，安装器自己会验。
const PYTHON_MIN: (u32, u32) = (3, 10);
const PYTHON_MAX_EXCLUSIVE: (u32, u32) = (3, 15);

fn version_ok(v: &str) -> bool {
    let mut it = v.split('.').filter_map(|x| x.parse::<u32>().ok());
    match (it.next(), it.next()) {
        (Some(maj), Some(min)) => (maj, min) >= PYTHON_MIN && (maj, min) < PYTHON_MAX_EXCLUSIVE,
        _ => false,
    }
}

/// 问一个解释器：版本 + 真实可执行路径。失败返回 None。
fn ask_python(program: &str) -> Option<(String, String)> {
    let mut cmd = std::process::Command::new(program);
    // 一行一个值，省得解析多行输出（sys.executable 在 Windows 上可能含空格）
    cmd.arg("-c")
        .arg("import sys;print(sys.version.split()[0]);print(sys.executable)")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null());
    // Python 是控制台程序：不加这个，点「安装组件」时先闪一个黑框
    crate::proc::no_window(&mut cmd);
    let out = cmd.output().ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let mut lines = text.lines().map(str::trim).filter(|s| !s.is_empty());
    let version = lines.next()?.to_string();
    let path = lines.next()?.to_string();
    if version.is_empty() || path.is_empty() {
        return None;
    }
    Some((version, path))
}

/// Windows 的 py launcher 列出的解释器（`py -0p`）。
///
/// 为什么要单独试它：Windows 上 python.org 的安装包默认**不写 PATH**，只装
/// `py.exe`。只试 `python`/`python3` 会把"装了 Python"的机器判成"没有 Python"
/// （本机实测：`py -0p` 能列出 `3.14 * C:\Python314\python.exe`，而 PATH 上的
/// `python` 是另一个 3.13）。
fn py_launcher_paths() -> Vec<String> {
    if !cfg!(windows) {
        return Vec::new();
    }
    let mut cmd = std::process::Command::new("py");
    cmd.arg("-0p")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null());
    crate::proc::no_window(&mut cmd);
    let Ok(out) = cmd.output() else {
        return Vec::new();
    };
    if !out.status.success() {
        return Vec::new();
    }
    let text = String::from_utf8_lossy(&out.stdout);
    // 每行形如：` -V:3.14 *        C:\Python314\python.exe`
    text.lines()
        .filter_map(|line| {
            let path = line.split_whitespace().last()?;
            let lower = path.to_ascii_lowercase();
            if lower.ends_with("python.exe") || lower.ends_with("pythonw.exe") {
                Some(path.to_string())
            } else {
                None
            }
        })
        .collect()
}

/// 探测本机 Python：py launcher 优先，再 PATH。
///
/// 返回值里 `source` 只用于 UI 说明（"我们是从 py 启动器找到的"），
/// 不参与决策。**优先支持窗口内**的版本：装了 3.14 与 3.7 两台解释器时，
/// 取 3.7 会让安装器跑一半报语法错。
pub fn probe_python() -> Option<PythonInfo> {
    let mut fallback: Option<PythonInfo> = None;
    let candidates: Vec<(String, &'static str)> = py_launcher_paths()
        .into_iter()
        .map(|p| (p, "py-launcher"))
        .chain(["python", "python3"].iter().map(|p| ((*p).to_string(), *p)))
        .collect();
    for (program, source) in candidates {
        let Some((version, path)) = ask_python(&program) else {
            continue;
        };
        let info = PythonInfo {
            program: path,
            version,
            source,
        };
        if version_ok(&info.version) {
            return Some(info);
        }
        if fallback.is_none() {
            fallback = Some(info);
        }
    }
    fallback
}

/// 给 UI 的探测结果（不吞细节：找不到要说清试过什么）。
pub fn python_probe_payload() -> crate::dto::beets::BeetsPythonProbe {
    use crate::dto::beets::BeetsPythonProbe;
    match probe_python() {
        Some(info) => BeetsPythonProbe {
            found: true,
            supported: version_ok(&info.version),
            path: Some(info.program),
            version: Some(info.version),
            source: Some(info.source.to_string()),
            reason: None,
        },
        None => BeetsPythonProbe {
            found: false,
            path: None,
            version: None,
            supported: false,
            source: None,
            reason: Some(if cfg!(windows) {
                "没有找到 Python（试过 py 启动器、python、python3）。\
                 可以不装 Python，直接用「免配置包」这条路径"
                    .into()
            } else {
                "没有找到 python/python3".into()
            }),
        },
    }
}

/// 安装器整体超时：pip 冷装（解析依赖 + 下载）正常几分钟内，
/// 20 分钟还没完就是卡死了。
const INSTALL_OVERALL_TIMEOUT: Duration = Duration::from_secs(20 * 60);
/// 行间空闲超时：安装器输出连绵不断（pip 也有进度行），
/// 60s 一行都没有 = 子进程卡死，不必等整体超时。
const INSTALL_LINE_IDLE_TIMEOUT: Duration = Duration::from_secs(60);

fn install_via_installer(
    app: &tauri::AppHandle,
    pyz: &std::path::Path,
    plugins: &[String],
    emit: &dyn Fn(serde_json::Value),
) -> Result<ToolEntry, String> {
    let python = probe_python().ok_or_else(|| {
        "本机没有可用的 Python。改用「免配置包」这条路径，或先安装 Python 3.10+".to_string()
    })?;
    if !version_ok(&python.version) {
        emit(serde_json::json!({
            "type": "warning",
            "message": format!(
                "本机 Python {} 不在支持窗口（>=3.10 <3.15），仍尝试安装", python.version
            ),
        }));
    }
    let target = install_root(app).join("venv");
    emit(serde_json::json!({
        "type": "progress", "stage": "install",
        "message": format!("用 Python {}（{}）安装组件到 {}",
                           python.version, python.program, target.display()),
    }));

    let mut cmd = std::process::Command::new(&python.program);
    cmd.arg(pyz).arg("install").arg("--target").arg(&target);
    // 勾选为空就不传这个参数：安装器的语义是"省略 = 只装 beets 基础"，
    // 传空串会让它解析出一个空名字。
    if !plugins.is_empty() {
        cmd.arg("--plugins").arg(plugins.join(","));
    }
    cmd.stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .envs(tool_env(app, Profile::Workbench));
    // 安装要跑几十秒，黑框会一直杵在那儿——比一闪而过更难看
    crate::proc::no_window(&mut cmd);
    let mut child = cmd.spawn().map_err(|e| format!("启动安装器失败：{e}"))?;

    let stdout = match child.stdout.take() {
        Some(s) => s,
        None => {
            let _ = child.kill();
            let _ = child.wait();
            return Err("安装器 stdout 未捕获".into());
        }
    };
    // 句柄唯一所有者是共享槽（AppState.beets_install_child）：本函数此后
    // 经槽 wait/kill，应用退出（RunEvent::Exit）也经槽收尸 —— 此前安装
    // 线程逐行阻塞读、无超时，主进程退出时安装器跟着变孤儿进程。
    let state = app
        .try_state::<crate::AppState>()
        .ok_or_else(|| "AppState 未就绪".to_string())?;
    {
        let mut slot = state
            .beets_install_child
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *slot = Some(child);
    }

    // 读线程：逐行送 channel。安装线程 recv_timeout 做两级超时
    // （行间空闲 / 整体），超时 kill+wait 收尸并报错 —— 阻塞读永远等不到
    // "安装器卡死"这一天结束。
    let (tx, rx) = std::sync::mpsc::channel::<String>();
    let reader = std::thread::Builder::new()
        .name("beets-install-recv".into())
        .spawn(move || {
            use std::io::BufRead;
            for line in std::io::BufReader::new(stdout)
                .lines()
                .map_while(Result::ok)
            {
                if tx.send(line).is_err() {
                    break; // 主线程已返回/收尸
                }
            }
        })
        .map_err(|e| {
            // 读线程没起来：安装器没人读输出会卡死在管道缓冲上，直接收尸
            let mut slot = state
                .beets_install_child
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if let Some(mut c) = slot.take() {
                let _ = c.kill();
                let _ = c.wait();
            }
            format!("起安装器读线程失败：{e}")
        })?;

    let started = std::time::Instant::now();
    let mut done = false;
    let mut timed_out = false;
    loop {
        if started.elapsed() > INSTALL_OVERALL_TIMEOUT {
            timed_out = true;
            break;
        }
        match rx.recv_timeout(INSTALL_LINE_IDLE_TIMEOUT) {
            Ok(line) => {
                let s = line.trim();
                if s.is_empty() {
                    continue;
                }
                // 安装器输出 JSON 行；万一有裸行（pip 进度条）也包成 progress，不丢信息
                let v: serde_json::Value = serde_json::from_str(s).unwrap_or_else(
                    |_| serde_json::json!({ "type": "progress", "stage": "pip", "message": s }),
                );
                if v.get("type").and_then(|t| t.as_str()) == Some("done") {
                    done = true;
                }
                emit(v);
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                timed_out = true;
                break;
            }
            // stdout 关闭 = 进程要退出了（此后 wait 很快）
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    // 收尾统一经槽，**先 kill 再 join**：超时路径上读线程还挂在 stdout 上，
    // 不先杀进程 join 永远等不到。槽被 Exit 收尾拿走（take 后为 None）
    // = 应用正在退出，子进程已被 kill+wait，安装就此放弃。
    let code = {
        let mut slot = state
            .beets_install_child
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match slot.as_mut() {
            Some(c) => {
                if timed_out {
                    let _ = c.kill();
                }
                let code = c.wait().map_err(|e| format!("等待安装器退出失败：{e}"))?;
                slot.take(); // 已收尸，退出路径不必再管
                Ok(code)
            }
            None => Err("安装被应用退出中断".to_string()),
        }
    }?;
    // 子进程已死，管道关闭，读线程必已退出；join 只为不留悬线程
    let _ = reader.join();
    if timed_out {
        return Err(format!(
            "安装器超时未完成（行间空闲 >{INSTALL_LINE_IDLE_TIMEOUT:.0?} 或整体 >{INSTALL_OVERALL_TIMEOUT:.0?}），已终止"
        ));
    }
    if !code.success() || !done {
        return Err(format!(
            "安装器未成功完成（退出码 {}）",
            code.code().unwrap_or(-1)
        ));
    }

    // 入口：venv 里的 aiya-beets 脚本。
    // 这是**安装器的产物约定**，不是探测：安装器把它装在这个位置，
    // 我们就记这个位置（见 docs/33 §6.2 A 轨道）。
    let exe = target.join("Scripts").join("aiya-beets.exe");
    if exe.is_file() {
        return Ok(ToolEntry {
            program: exe.to_string_lossy().into_owned(),
            args: Vec::new(),
        });
    }
    // 退一步：用 venv 解释器跑模块（Scripts 里没有 console script 的旧式安装）
    let py = target.join("Scripts").join("python.exe");
    if py.is_file() {
        return Ok(ToolEntry {
            program: py.to_string_lossy().into_owned(),
            args: vec!["-m".into(), "aiya_beets".into()],
        });
    }
    Err(format!("安装完成但找不到入口（{}）", target.display()))
}

/// B 轨道：展开套件包到 `{app_data}/beets-tool/bundle`。
fn install_via_bundle(
    app: &tauri::AppHandle,
    zip_path: &std::path::Path,
    emit: &dyn Fn(serde_json::Value),
) -> Result<ToolEntry, String> {
    install_bundle_to(&install_root(app).join("bundle"), zip_path, emit)
}

/// 这批勾选的插件需要哪些外置工具（`plugins.json` 的 `exe` 字段，docs/53）。
///
/// 用编译期内嵌的那张表算，不再问 sidecar：安装时 sidecar 可能还不存在。
fn needed_binaries(plugins: &[String]) -> Vec<String> {
    let Ok(rows) = plugin_table() else {
        return Vec::new();
    };
    let mut need: Vec<String> = Vec::new();
    for name in plugins {
        let Some(row) = rows.iter().find(|r| &r.name == name) else {
            continue;
        };
        for dep in &row.exe {
            if !need.contains(dep) {
                need.push(dep.clone());
            }
        }
    }
    need
}

/// 下载外置工具：`<入口> bins --only a,b`（docs/53）。
///
/// 为什么放在宿主侧、由安装流程顺带跑：这些东西是"插件能不能用"的前置，
/// 而缺 ffmpeg 会让 beets 的**任何**子命令都起不来（实测
/// `UserError: replaygain initialization failed`）——用户没有修复它的入口，
/// 等于组件坏了。也刻意不给"跳过"选项：不做就没有这个插件。
///
/// 子进程的超时不在这里设：`bins` 自己对每次请求有 120s 超时与总大小上限，
/// 卡死会自己退出来，比在父进程再叠一层"多久没输出就杀"更准。
fn run_bins(
    app: &tauri::AppHandle,
    entry: &ToolEntry,
    only: &[String],
    emit: &dyn Fn(serde_json::Value),
) -> Result<(), String> {
    use std::io::BufRead as _;

    let mut cmd = std::process::Command::new(&entry.program);
    cmd.args(&entry.args)
        .arg("bins")
        .arg("--only")
        .arg(only.join(","))
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .envs(tool_env(app, Profile::Workbench));
    crate::proc::no_window(&mut cmd);
    emit(serde_json::json!({
        "type": "progress", "stage": "bin",
        "message": format!("下载外置工具：{}", only.join(" / ")),
    }));
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("启动外置工具下载失败：{e}"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "外置工具下载的输出没捕获到".to_string())?;

    // 句柄进共享槽：应用退出时统一收尸，别留下一个还在下 87MB 的孤儿进程
    let guard = app.try_state::<crate::AppState>();
    if let Some(state) = guard.as_ref() {
        let mut slot = state
            .beets_install_child
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        // 槽里可能是上一次安装的僵尸：先清掉，避免把它一直挂着
        if let Some(mut old) = slot.take() {
            let _ = old.wait();
        }
        *slot = Some(child);
    }

    let mut failure: Option<String> = None;
    for line in std::io::BufReader::new(stdout)
        .lines()
        .map_while(Result::ok)
    {
        let Ok(obj) = serde_json::from_str::<serde_json::Value>(&line) else {
            continue;
        };
        match obj.get("type").and_then(|t| t.as_str()) {
            Some("progress") => emit(obj),
            Some("error") => {
                failure = obj
                    .get("message")
                    .and_then(|m| m.as_str())
                    .map(str::to_string);
            }
            Some("done") => {
                emit(serde_json::json!({
                    "type": "progress", "stage": "bin", "message": "外置工具就绪",
                }));
            }
            _ => {}
        }
    }

    // 从槽里取回来 wait：进程可能已经被退出流程收尸了，那就没有句柄可等
    let code = match guard.as_ref() {
        Some(state) => {
            let mut slot = state
                .beets_install_child
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            slot.take()
                .map(|mut c| c.wait().ok().and_then(|s| s.code()))
        }
        None => None,
    };
    if let Some(msg) = failure {
        return Err(msg);
    }
    if let Some(Some(code)) = code {
        if code != 0 {
            return Err(format!("下载器退出码 {code}"));
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
        std::fs::remove_dir_all(target).map_err(|e| format!("清理旧套件包失败：{e}"))?;
    }
    std::fs::create_dir_all(target).map_err(|e| format!("创建目标目录失败：{e}"))?;
    emit(serde_json::json!({
        "type": "progress", "stage": "unpack",
        "message": format!("展开套件包到 {}", target.display()),
    }));

    let file = std::fs::File::open(zip_path).map_err(|e| format!("打开套件包失败：{e}"))?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| format!("套件包不是有效 zip：{e}"))?;
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

    for name in ["aiya-beets.exe", "python.exe"] {
        let p = target.join(name);
        if p.is_file() {
            let args = if name == "python.exe" {
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
    Err("套件包展开后找不到入口（缺 manifest.json 且没有 aiya-beets.exe）".to_string())
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

/// 解压上限：套件包实际约 30MB，留约 16 倍余量。不是防自己人，是防
/// "被替换过的包"把磁盘写满——那种故障现场极难归因到一次安装。
const BUNDLE_MAX_BYTES: u64 = 512 * 1024 * 1024;

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
            .map_err(|e| format!("读取套件包条目失败：{e}"))?;
        // zip 9 起 `name()` 返回 Result：条目名可能是非法编码（CP437 之外的
        // 未标注原始字节），旧版默默替换成 lossy 字符串——那会让路径校验
        // 对着"替换后的名字"做判断，与真实落盘名不符。宁可在这里失败。
        let name = entry
            .name()
            .map_err(|e| format!("读取套件包条目名失败：{e}"))?
            .to_string();
        let Some(rel) = safe_relative(&name) else {
            return Err(format!("套件包含非法路径条目，拒绝展开：{name}"));
        };
        // 0o170000 = S_IFMT，0o120000 = S_IFLNK
        if entry
            .unix_mode()
            .is_some_and(|mode| mode & 0o170000 == 0o120000)
        {
            return Err(format!("套件包含符号链接条目，拒绝展开：{name}"));
        }
        let dest = target.join(&rel);
        if entry.is_dir() {
            std::fs::create_dir_all(&dest)
                .map_err(|e| format!("创建目录 {} 失败：{e}", dest.display()))?;
            continue;
        }
        written = written.saturating_add(entry.size());
        if written > BUNDLE_MAX_BYTES {
            return Err(format!(
                "套件包展开超过 {} MB 上限，已中止（包可能不完整或被替换）",
                BUNDLE_MAX_BYTES / 1024 / 1024
            ));
        }
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("创建目录 {} 失败：{e}", parent.display()))?;
        }
        let mut out = std::fs::File::create(&dest)
            .map_err(|e| format!("写入 {} 失败：{e}", dest.display()))?;
        std::io::copy(&mut entry, &mut out)
            .map_err(|e| format!("展开 {} 失败：{e}", dest.display()))?;
    }
    Ok(())
}

/// 记下入口，下次 `discover` 直接命中（设置优先于 PATH 与源码树）。
fn remember_entry(app: &tauri::AppHandle, entry: &ToolEntry) -> Result<(), String> {
    let program = entry.program.clone();
    let args = entry.args.join(" ");
    update_settings(app, move |s| {
        s.tool_path = Some(program);
        s.tool_args = Some(args);
    })
}

/// 安装（异步）：立刻返回，进度走 `beets:install`，结束时补一条 finished。
///
/// `plugins` 是用户在 UI 里勾的**启用清单**：安装器据此装额外依赖，
/// 装成功后写进 `settings.plugins`，工具侧渲染 `config.yaml` 时照它启用。
/// 免配置包（Bundle）轨道自带预置插件，勾选对它没有意义——传了也忽略，
/// 但会发一条 warning 说清"这次勾选没生效"，而不是静默吞掉。
///
/// finished 带**重新探测后的 status**：UI 不用再单独发一次查询，
/// 也不会出现"装完了但状态块还是旧值"这种中间态。
pub fn start_install(
    app: tauri::AppHandle,
    track: String,
    plugins: Vec<String>,
) -> Result<(), String> {
    let track = InstallTrack::parse(&track)?;
    std::thread::Builder::new()
        .name("beets-install".into())
        .spawn(move || {
            let emit = |v: serde_json::Value| {
                let _ = app.emit("beets:install", v);
            };
            let result = (|| -> Result<ToolEntry, String> {
                let (env_key, asset, kind) = match track {
                    InstallTrack::Installer => (
                        "AIYA_BEETS_INSTALLER",
                        INSTALLER_ASSET,
                        AssetKind::Installer,
                    ),
                    InstallTrack::Bundle => ("AIYA_BEETS_BUNDLE", BUNDLE_ASSET, AssetKind::Bundle),
                };
                let path = fetch_asset(env_key, asset, kind, &emit)?;
                let entry = match track {
                    InstallTrack::Installer => install_via_installer(&app, &path, &plugins, &emit)?,
                    InstallTrack::Bundle => {
                        if !plugins.is_empty() {
                            emit(serde_json::json!({
                                "type": "warning",
                                "message": "免配置包已预置插件，这次的勾选不生效",
                            }));
                        }
                        install_via_bundle(&app, &path, &emit)?
                    }
                };
                // 外置工具（docs/53）：**两个轨道都要**。免配置包里刻意不带
                // ffmpeg/Essentia（GPL/AGPL 与体积），而缺 ffmpeg 时 replaygain
                // 会让 beets 整个 CLI 起不来 —— 勾了就顺手带上，不留半装状态。
                let need = needed_binaries(&plugins);
                if !need.is_empty() {
                    run_bins(&app, &entry, &need, &emit).map_err(|e| {
                        format!("组件已装好，但外置工具没下下来：{e}（再点一次安装即可补齐）")
                    })?;
                }
                Ok(entry)
            })();
            match result {
                Ok(entry) => {
                    // 记不住也不算彻底失败：PATH 与源码树仍可能命中，
                    // 只是下次启动要多一次探测。如实报给 UI。
                    if let Err(e) = remember_entry(&app, &entry) {
                        emit(serde_json::json!({ "type": "warning", "message": e }));
                    }
                    // 勾选写进设置——装了不启用等于白装（cfg 渲染 plugins 行读它）
                    if !plugins.is_empty() {
                        let want = plugins.clone();
                        if let Err(e) = update_settings(&app, move |s| {
                            s.plugins = Some(want);
                        }) {
                            emit(serde_json::json!({ "type": "warning", "message": e }));
                        }
                    }
                }
                Err(message) => emit(serde_json::json!({ "type": "error", "message": message })),
            }
            let status = probe(&app);
            let _ = app.emit(
                "beets:install",
                serde_json::json!({ "type": "finished", "status": status }),
            );
        })
        .map(|_| ())
        .map_err(|e| format!("起安装线程失败：{e}"))
}

/* ---------------- 卸载组件（docs/51 §2.2） ---------------- */

/// 统计目录的文件数与字节数（先算账再删——删完就没法报了）。
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

/// 卸载：删 `{data_root}/beets-tool`（venv / bundle / 下载缓存），清设置里的入口。
///
/// ## 为什么只删这一层（2026-10-30 拍板）
///
/// 卸载是**安装的反向操作**：安装往里放了 venv 解释器、beets 本体、pip 依赖、
/// 插件，全在 `beets-tool/` 里，删它就等于全部撤回。
/// `beets/`（整理家 library.db + ops.jsonl）与 `beets-index/`（分析库）虽然
/// 也在同一个数据根下，但它们是用户资产、不是安装产物，**这里一个字节都不动**
/// ——那些由应用自己的清理入口管。顺手删掉 1850 首的分析结果，是用户没让你做的事。
pub fn start_uninstall(app: tauri::AppHandle) -> Result<(), String> {
    std::thread::Builder::new()
        .name("beets-uninstall".into())
        .spawn(move || {
            let emit = |v: serde_json::Value| {
                let _ = app.emit("beets:install", v);
            };
            if let Err(e) = uninstall_now(&app, &emit) {
                emit(serde_json::json!({ "type": "error", "message": e }));
            }
            let status = probe(&app);
            let _ = app.emit(
                "beets:install",
                serde_json::json!({ "type": "finished", "status": status }),
            );
        })
        .map(|_| ())
        .map_err(|e| format!("起卸载线程失败：{e}"))
}

fn uninstall_now(app: &tauri::AppHandle, emit: &dyn Fn(serde_json::Value)) -> Result<(), String> {
    let root = install_root(app);
    if !root.exists() {
        // 幂等：没有安装目录就算成功。但要提醒"当前入口可能来自别处"，
        // 否则开发机上点卸载会毫无反应、还报成功。
        if let Some((_, source)) = discover(app) {
            emit(serde_json::json!({
                "type": "warning",
                "message": format!(
                    "没有安装目录可删；当前组件来自 {}（源码树/环境变量），卸载不动它",
                    source.as_str()
                ),
            }));
        }
        emit(serde_json::json!({
            "type": "done", "stage": "uninstall", "removed": false,
            "message": "没有安装目录，无需卸载",
        }));
        return Ok(());
    }

    let (files, bytes) = measure_dir(&root);
    emit(serde_json::json!({
        "type": "progress", "stage": "uninstall",
        "message": format!(
            "将删除 {}（{} 个文件，{:.1} MB）", root.display(), files, bytes as f64 / 2f64.powi(20)
        ),
    }));

    // 有会话在跑时 Windows 上删不掉 exe。尽力收尾一次（不等退出——那会让
    // 卸载卡在这里；删不掉就如实报，让用户先停掉索引）。
    let _ = cancel_session();

    std::fs::remove_dir_all(&root).map_err(|e| {
        format!(
            "删除失败（{e}）。如果正在索引/整理，等它结束或先退出应用再试；\
             目录：{}",
            root.display()
        )
    })?;

    // 设置里的入口必须清掉：否则 discover 会一直指着一个不存在的 exe，
    // 症状是"卸载了但状态页仍显示组件在、只是起不来"。
    update_settings(app, |s| {
        s.tool_path = None;
        s.tool_args = None;
    })?;

    emit(serde_json::json!({
        "type": "done", "stage": "uninstall", "removed": true,
        "files": files, "bytes": bytes,
        "message": "组件已卸载；整理库与分析库未改动",
    }));
    Ok(())
}

/* ---------------- B1/B2：导入会话（事件流转发） ---------------- */

/// 取工具入口并造一个可直接起进程的 tool。
///
/// 找不到入口时的文案要能指导用户下一步（去扫描入库页装组件），
/// 而不是一句 "no beets interpreter available"。
fn tool_or_err(app: &tauri::AppHandle) -> Result<StdioTool, String> {
    let (entry, _source) = discover(app).ok_or_else(|| NOT_INSTALLED.to_string())?;
    Ok(StdioTool::new(launcher_for(
        app,
        &entry,
        Profile::Workbench,
    )))
}

/* ---------------- B3：会话槽 + 取消 ---------------- */

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
    app: &tauri::AppHandle,
    request: serde_json::Value,
    channel: &'static str,
    busy: &str,
    on_exit: Box<dyn FnOnce() + Send + 'static>,
) -> Result<(), String> {
    let sink: Arc<dyn EventSink> = Arc::new(TauriSink(app.clone()));
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
pub fn start_preview(app: tauri::AppHandle, paths: Vec<String>) -> Result<(), String> {
    let tool = tool_or_err(&app)?;

    // 排除表是**播放器侧事实**（哪些 inbox 源文件已成功导入过），在这里注入
    // 进协议预留的 options 对象（docs/40 十三轮）：待整理目录固定后，copy 应用的
    // 原件一直留在 inbox，不排除的话下次整理会把同一批文件再导一遍。UI 不用
    // 搬运这份数据，preview 命令签名保持不变。
    let exclude: Vec<String> = load_settings(&app).inbox_imported;

    let mut body = serde_json::json!({ "id": 1, "cmd": "preview_import", "paths": paths });
    if !exclude.is_empty() {
        body["options"] = serde_json::json!({ "excludePaths": exclude });
    }

    run_session(
        tool,
        &app,
        body,
        "beets:import",
        "已有 beets 导入任务在执行（可先取消）",
        Box::new(|| {}),
    )
}

/// v2 幂等键：每次调用生成新值（`{cmd}-{unix 毫秒}-{进程内序号}`）。
///
/// 单实例下宿主不重试，重放是协议层安全网 + 双实例防护（docs/41 §3.2）；
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

/// B2：应用导入决策。decisions 形状由契约定义（ApplyDecision），
/// 宿主不解释语义，只透传；完成后自动把工具侧结果对齐自有库（自闭环）。
///
/// `resume`（v2）：中断后续跑——重发同一批 decisions，sidecar 按内容指纹
/// 跳过已完成的（move/copy 中断后源文件已移走，不 resume 的重跑必报错）。
pub fn start_apply(
    app: tauri::AppHandle,
    decisions: Vec<serde_json::Value>,
    resume: bool,
    refresh: Option<Arc<crate::refresh::RefreshParams>>,
) -> Result<(), String> {
    let tool = tool_or_err(&app)?;

    // 整理写入的落点。apply 结束后要按它补一次索引 —— 见下面 on_exit 的说明。
    // 在**发命令前**取：落点是被 `apply` 正在使用的那个（`tool_env` 同一个来源），
    // 结束后再取可能已经被用户改掉，补索引就会扫错目录。
    let dest = roots_for_tools(&app).1;

    // on_exit 里拿一份 AppHandle（会话线程要能独立跑，不能借用 &app）
    let exit_app = app.clone();
    run_session(
        tool,
        &app,
        serde_json::json!({
            "id": 1, "cmd": "apply_import",
            "decisions": decisions,
            "requestId": new_request_id("apply_import"),
            "resume": resume,
        }),
        "beets:import",
        "已有 beets 导入任务在执行（可先取消）",
        Box::new(move || {
            if let Some(params) = refresh {
                // 自闭环（docs/08 §5）：工具刚写完库就是最新权威。
                // 走完整刷新管线而不是只 dump+sync —— apply 会写标签改 mtime，
                // 扫描段正好把这些文件的 mtime/size 与标签一起还原（docs/33 §7）。
                crate::refresh::spawn(&exit_app, params, crate::refresh::Trigger::Action);
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
            if let Err(e) = start_index_import(exit_app.clone(), dest) {
                // 补索引失败**不影响整理结果**（文件已经搬好了），所以只记日志。
                // 用户仍可手动「重建索引」——那是这条路的兜底，不是主路径。
                tracing::warn!(error = %e, "整理后补索引未能启动（整理结果不受影响）");
            }
        }),
    )
}

/// 批 D 曲库维护任务的统一入口（adopt / update / duplicates / move）。
///
/// 四条命令的公共部分完全一样：占会话位 → 起一次性子进程 → 事件流走
/// `beets:maintenance` → 进程退出后按需跑一次读模型对账。差别只有 req 载荷
/// 和"要不要对账"，所以合成一个函数，避免四份几乎相同的代码各自漂移
/// （漏了释放、漏了 ensure_config 之类）。
///
/// `refresh` 为什么只有 adopt/move 给：这两条会改库的**内容与路径**——
/// adopt 把文件登记进库（对账后 `tracks.beets_item_id` 才有值，而后续 move
/// 的路径跟随正是靠这个双键，docs/29 §12）；move 把文件搬走，路径漂移必须
/// 立刻跟随，不能等用户下次扫描。update 只改标签（路径不变）、duplicates
/// 纯只读，都不需要自动对账。
fn start_maintenance(
    app: tauri::AppHandle,
    payload: serde_json::Value,
    refresh: Option<Arc<crate::refresh::RefreshParams>>,
) -> Result<(), String> {
    let tool = tool_or_err(&app)?;

    let exit_app = app.clone();
    run_session(
        tool,
        &app,
        payload,
        "beets:maintenance",
        "已有 beets 任务在执行（可先取消）",
        Box::new(move || {
            if let Some(params) = refresh {
                crate::refresh::spawn(&exit_app, params, crate::refresh::Trigger::Action);
            }
        }),
    )
}

/// 建 beets 库基线：把自有库里的文件按现状登记进 beets 库。
/// `paths` 为空 = 由宿主取自有库全部在线路径（UI 不该为了这个传 1800 个路径）。
pub fn start_adopt(
    app: tauri::AppHandle,
    paths: Vec<String>,
    refresh: Option<Arc<crate::refresh::RefreshParams>>,
) -> Result<(), String> {
    start_maintenance(
        app,
        serde_json::json!({
            "id": 1, "cmd": "adopt_library", "paths": paths,
            "requestId": new_request_id("adopt_library"),
        }),
        refresh,
    )
}

/// 用文件标签刷新 beets 库（`pretend` = 只报 diff 不写库）。
pub fn start_update(app: tauri::AppHandle, pretend: bool) -> Result<(), String> {
    start_maintenance(
        app,
        serde_json::json!({
            "id": 1, "cmd": "update_library", "pretend": pretend,
            "requestId": new_request_id("update_library"),
        }),
        None,
    )
}

/// 按字段组查重（只读）。
pub fn start_duplicates(app: tauri::AppHandle, keys: Vec<String>) -> Result<(), String> {
    start_maintenance(
        app,
        serde_json::json!({ "id": 1, "cmd": "find_duplicates", "keys": keys }),
        None,
    )
}

/* ---------------- 索引增强（docs/45 §5.4） ---------------- */

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
pub fn start_enrich(app: tauri::AppHandle, source: String, force: bool) -> Result<(), String> {
    let (entry, _src) = discover(&app).ok_or_else(|| NOT_INSTALLED.to_string())?;
    let paths = crate::enrichment::work_paths(&app, &source, force)?;
    if paths.is_empty() {
        return Err("工作清单为空：没有需要分析的曲目".to_string());
    }
    let tool = StdioTool::new(launcher_for(&app, &entry, enrich_profile()));
    let sink = Arc::new(crate::enrichment::Sink::start(app.clone())?);
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
        "已有 beets 任务在执行（可先取消）",
        sink,
        Box::new(move || exit_sink.finish()),
    )
}

/// 分析会话固定跑索引家。
///
/// 抽成函数只为让测试能盯住它：直接写 `Profile::Index` 也能跑，但"哪天有人
/// 顺手改成 `Workbench`（看起来更简单）"时，没有任何东西会红——而那个改动的
/// 后果是 `item.remove()` + 按用户 config 重导，也就是**用户的歌被改**。
pub fn enrich_profile() -> Profile {
    Profile::Index
}

/// 查询页固定跑**索引家**（docs/49 §3.7）。
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

/// 只读入库：把 **`root`**（缺省＝曲库根）登记进索引家，不做分析（docs/49 §3.8）。
///
/// 与 `start_enrich` 的分工很硬：
/// * 这条 → "索引库里有行"，查询页的前提；
/// * 那条 → "每个文件有分析结论"，`xtractor` 未接线时**整批**被拒。
///
/// 曾经以为入库是分析的副产品，实测发现分析器没接好时连库都不建（`enrich`
/// 直接 E_INVALID_ARG），于是筛选页永远 0 命中。所以两条路必须分开。
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
/// 所以"整理完顺手整根重扫"是不能接受的 82 秒，而落点通常只有一两批。
///
/// 传子目录**不会被 `-R` 误跳过**：beets 的增量记忆（`statefile: state.pickle`
/// 的 `taghistory`）记的是「一个导入任务的那批 paths 元组」，新文件组成新元组；
/// 实测新目录只扫该目录 → 索引库 1849 → 1850。
pub fn start_index_import(app: tauri::AppHandle, root: Option<String>) -> Result<(), String> {
    let (entry, _src) = discover(&app).ok_or_else(|| NOT_INSTALLED.to_string())?;
    let tool = StdioTool::new(launcher_for(&app, &entry, Profile::Index));
    let mut body = serde_json::json!({ "id": 1, "cmd": "index_import" });
    if let Some(r) = trimmed(root) {
        body["root"] = serde_json::Value::String(r);
    }
    run_session(
        tool,
        &app,
        body,
        INDEX_CHANNEL,
        "已有 beets 任务在执行（可先取消）",
        Box::new(|| {}),
    )
}

/* ---------------- 查询页（docs/47）：读 beets 库，转成歌单 ---------------- */

/// 「组件没装」的统一文案。放在一处：三个调用点各写一句时，改一次要改三处，
/// 而漏改的那处会让 UI 的"安装指引"分支认不出来（`map_beets_start` 靠关键词
/// 分类，文案本身就参与判定）。
pub const NOT_INSTALLED: &str = "未安装 beets 组件：请先在设置里安装，或指定组件路径";

/// 查询页的短进程超时。查询要做 `COUNT(*)` + 取一页，几万条的库也在秒级；
/// 30 s 是给"库刚被整理过、SQLite 在补索引"留的余量。
const QUERY_TIMEOUT: Duration = Duration::from_secs(30);

/// 读 beets 库（**索引家**，docs/49 §3.7 改了归属）。
///
/// 为什么现在查索引家：索引家的 `directory` 已经指向**曲库根**（docs/49 §3.1），
/// 它就是"用户曲库的只读镜像"——库里的每一条 path 都必然在曲库里，分析字段
/// （bpm / 情绪 / 流派）也只在这里有。反过来说，工作台家是用户自己的 beets 库，
/// 应用从不往里面导，为空是常态（查到 0 条不是 bug，是"没配"）。
///
/// 旧立论是"索引家是应用独占的只读副本、路径大概率不在曲库"，那是 `directory`
/// 还是占位 `scratch` 时的情况，已经不成立。**兜底**：工具侧查询出口把 path
/// 拼成绝对（`normpath(join(directory, path))`），宿主这边照旧只按 path 关联，
/// 不假设路径形态。
///
/// `rules` 与 `raw` 互斥（工具侧会拒）：不猜优先级，两个都给等于让调用方
/// 把"我到底想要什么"外包给一个 if。
pub fn query_page(
    app: &tauri::AppHandle,
    rules: Option<serde_json::Value>,
    raw: Option<String>,
    fields: Option<Vec<String>>,
    limit: Option<usize>,
) -> Result<crate::dto::beets::BeetsQueryPage, String> {
    let (entry, _source) = discover(app).ok_or_else(|| NOT_INSTALLED.to_string())?;
    let launcher = launcher_for(app, &entry, query_profile());
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
    serde_json::from_value(data).map_err(|e| format!("查询应答形状不对（协议漂移？）: {e}"))
}

/// 可查字段清单（含插件写入的 flexible 键）+ 识别源三态。
///
/// 字段清单归工具侧回答（它才知道装了哪些插件）；宿主把它原样转给 UI。
/// **必须和 `query_page` 跑同一个家**：字段清单来自哪个库，查询就得查哪个库，
/// 否则 UI 会拿索引库的字段去查用户库（或反过来），症状是"能选 bpm 但一查就报
/// 未知字段"。
pub fn field_catalog(
    app: &tauri::AppHandle,
) -> Result<crate::dto::beets::BeetsFieldCatalog, String> {
    let (entry, _source) = discover(app).ok_or_else(|| NOT_INSTALLED.to_string())?;
    let launcher = launcher_for(app, &entry, query_profile());
    let data = aiya_beets_client::session::request_data(
        &launcher,
        &serde_json::json!({ "id": 1, "cmd": "fields" }),
        QUERY_TIMEOUT,
    )
    .map_err(|e| e.to_string())?;
    serde_json::from_value(data).map_err(|e| format!("字段清单形状不对（协议漂移？）: {e}"))
}

/* ---------------- 插件市场（docs/40 §3 白名单版） ---------------- */

/// 插件表的**编译期内嵌**副本（唯一事实源是工具侧那个 JSON，docs/51 §1）。
///
/// 为什么内嵌而不是"装完组件再问 sidecar"：安装页要在**什么都没装**的时候
/// 就把"有哪些插件、各自要装什么"展示出来让用户勾选。这条路径上没有 serve
/// 进程，只能靠编译期带一份。它和工具侧、打包脚本、安装器读的是同一个文件，
/// 所以不存在"UI 说 A、安装器装 B"的漂移；`build.rs` 里有 rerun-if-changed。
const PLUGIN_TABLE_JSON: &str = include_str!("../../beets-tool/src/aiya_beets/data/plugins.json");

#[derive(serde::Deserialize)]
struct PluginTableFile {
    schema: u32,
    #[serde(default)]
    profiles: PluginProfiles,
    #[serde(default)]
    plugins: Vec<crate::dto::beets::BeetsPluginItem>,
}

#[derive(serde::Deserialize, Default)]
struct PluginProfiles {
    #[serde(default)]
    workbench: Vec<String>,
    #[serde(default)]
    index: Vec<String>,
}

/// 解析内嵌表，并把 `profiles` 里的默认启用关系注入每一条。
///
/// 默认启用**不由条目自己声明**（否则又是两份事实）：JSON 里只有
/// `profiles.workbench` / `profiles.index` 两处列表，这里算一次。
pub fn plugin_table() -> Result<Vec<crate::dto::beets::BeetsPluginItem>, String> {
    let mut doc: PluginTableFile = serde_json::from_str(PLUGIN_TABLE_JSON)
        .map_err(|e| format!("内嵌插件表解析失败（构建产物坏了？）：{e}"))?;
    if doc.schema != 2 {
        return Err(format!(
            "内嵌插件表 schema={} 不认识（本二进制只懂 2：docs/53 加了 binaries 段）",
            doc.schema
        ));
    }
    if doc.plugins.is_empty() {
        return Err("内嵌插件表是空的".into());
    }
    for item in &mut doc.plugins {
        item.default = doc.profiles.workbench.contains(&item.name);
        item.index_default = doc.profiles.index.contains(&item.name);
    }
    Ok(doc.plugins)
}

/// 插件市场目录：一次性短进程查询（与 status 探测同款开销，~1s）。
///
/// 目录 = 工具侧白名单 + 三态复检。UI 只在 beets 可用时才展示这一区，
/// 所以这里拿不到入口时直接报错由命令层分类即可。
pub fn plugin_catalog(
    app: &tauri::AppHandle,
) -> Result<Vec<crate::dto::beets::BeetsMarketItem>, String> {
    let (entry, _source) = discover(app).ok_or_else(|| NOT_INSTALLED.to_string())?;
    let launcher = launcher_for(app, &entry, Profile::Workbench);
    let rows = aiya_beets_client::session::query_plugin_catalog(&launcher, Duration::from_secs(20))
        .map_err(|e| e.to_string())?;
    rows.into_iter()
        .map(|row| {
            serde_json::from_value(row).map_err(|e| format!("目录项形状不对（协议漂移？）: {e}"))
        })
        .collect()
}

/// 安装一个白名单插件。事件流走 `beets:maintenance`（pluginProgress →
/// pluginDone 终态），**复用维护会话槽**：装插件和整理库操作都会在
/// serve 进程里跑，同一时刻只能有一个，分开两个槽会让两个 pip 并发
/// 写同一个 site-packages。装完不触发读模型对账（插件不改自有库）。
pub fn start_plugin_install(app: tauri::AppHandle, name: String) -> Result<(), String> {
    start_maintenance(
        app,
        serde_json::json!({ "id": 1, "cmd": "plugin_install", "name": name }),
        None,
    )
}

/// 按 path_formats 整理文件（`pretend` = 只出清单不搬文件）。
/// `directory` 是校验值：与 config.yaml 的 directory 不一致会被 sidecar 拒绝。
pub fn start_move(
    app: tauri::AppHandle,
    directory: String,
    pretend: bool,
    refresh: Option<Arc<crate::refresh::RefreshParams>>,
) -> Result<(), String> {
    start_maintenance(
        app,
        serde_json::json!({
            "id": 1, "cmd": "move_items",
            "directory": directory, "pretend": pretend,
            "requestId": new_request_id("move_items"),
        }),
        refresh,
    )
}

/// 组件当前是否可用（只做发现，不起进程）。
///
/// 刷新管线用它决定"要不要跑 beets 段"。刻意不复用 `probe`：探测要起一次
/// 子进程（约 300 ms），而定时刷新每 30 s 就会问一次 —— 这里只需要回答
/// "有没有入口"，有入口之后让 dump 自己失败（那时候的错误才值得报告）。
///
/// 索引增强卡片也用它：`available=false` 时按钮直接禁用，不必等一次探测。
pub fn available(app: &tauri::AppHandle) -> bool {
    discover(app).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    /* ---------------- 环境：两家的家必须同步（docs/45 §6.3） ---------------- */

    /// `BEETSDIR` 必须与 `AIYA_BEETS_HOME` 同值：工具侧按后者定位锁/登记簿，
    /// beets 只认前者。两者分家时症状是"索引跑完了却查不到"，所以这条不是
    /// 风格问题，是双家隔离的地基。
    #[test]
    fn env_pins_beetsdir_to_home() {
        let home = std::path::Path::new(r"D:\data\beets-index");
        let env = env_pairs(home, None, None, None, Profile::Index);
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

    /// 曲库根与落点是**两个事实**，必须各走各的键（docs/49 §3.2）。
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
            None,
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

    /* ---------------- 落点口径：与 beets_set_target 同一条判据 ---------------- */

    /// 子目录落点必须**真的生效**，不能静默回退成曲库根。
    ///
    /// 这条是回归测试：`beets_set_target` 已经放宽到接受子目录，而
    /// `roots_for_tools` 里曾经还写着严格相等，于是"设置里存着 D:\Music\整理"
    /// 会被判非法、实际用 D:\Music —— 文件倒进曲库根，用户看不出是哪一步错了。
    #[test]
    fn pick_dest_accepts_subdirectory() {
        let roots = vec!["D:\\Music".to_string()];
        assert_eq!(
            pick_dest(Some("D:\\Music\\整理".to_string()), &roots),
            Some("D:\\Music\\整理".to_string())
        );
        // 等于根也合法
        assert_eq!(
            pick_dest(Some("D:\\Music".to_string()), &roots),
            Some("D:\\Music".to_string())
        );
    }

    /// 曲库外的落点不算数 → 回退第一个根。
    /// 假兄弟（`D:\Music2`）不能因为字符串前缀像就被当成子目录。
    #[test]
    fn pick_dest_falls_back_when_outside() {
        let roots = vec!["D:\\Music".to_string()];
        assert_eq!(
            pick_dest(Some("E:\\Other".to_string()), &roots),
            Some("D:\\Music".to_string())
        );
        assert_eq!(
            pick_dest(Some("D:\\Music2".to_string()), &roots),
            Some("D:\\Music".to_string()),
            "D:\\Music2 是 D:\\Music 的假兄弟，不是子目录"
        );
        // 未设置 → 回退第一个根
        assert_eq!(pick_dest(None, &roots), Some("D:\\Music".to_string()));
        // 没有根 → None（没有可落的地方）
        assert_eq!(pick_dest(Some("D:\\Music".to_string()), &[]), None);
    }

    #[test]
    fn env_passes_scan_root_and_dev_src() {
        let home = std::path::Path::new("/tmp/beets");
        let env = env_pairs(
            home,
            Some("/music".to_string()),
            None,
            Some(PathBuf::from("/repo/beets-tool/src")),
            Profile::Workbench,
        );
        let get = |k: &str| {
            env.iter()
                .find(|(key, _)| key == k)
                .map(|(_, v)| v.as_str())
                .unwrap_or("")
        };
        assert_eq!(get("AIYA_BEETS_SCAN_ROOT"), "/music");
        assert_eq!(get("PYTHONPATH"), "/repo/beets-tool/src");
    }

    /// **P0 的回归防线**：profile 必须由宿主显式下发。
    ///
    /// 这条测试存在的理由是它曾经不存在：`enrich` 走的是工作台家（用户真库），
    /// 而 `-R` 重导前的 `item.remove()` 会把用户的条目删掉再按用户的 config
    /// 重导——"点一下分析"= 改用户的歌。缺的不是代码，是**没有一条测试看着
    /// 那句环境变量有没有被写上**。
    #[test]
    fn env_always_states_profile() {
        for (profile, want) in [(Profile::Workbench, "workbench"), (Profile::Index, "index")] {
            let env = env_pairs(std::path::Path::new("/tmp/x"), None, None, None, profile);
            let got = env
                .iter()
                .find(|(k, _)| k == "AIYA_BEETS_PROFILE")
                .map(|(_, v)| v.as_str());
            assert_eq!(got, Some(want), "profile 必须显式下发，不能靠工具侧默认");
        }
    }

    /// 分析必须跑在索引家（P0 的第二条防线；第一条看 `env_always_states_profile`）。
    #[test]
    fn enrich_runs_in_index_home() {
        assert_eq!(
            enrich_profile(),
            Profile::Index,
            "分析改回工作台家 = 改用户的歌"
        );
    }

    /// 查询/字段清单必须与分析同家（docs/49 §3.7）。
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
    /// 它们**不能互相依赖**（那正是这次解耦要断的东西），所以只能靠断言锁在
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

    /* ---------------- 模块自有配置（docs/47） ---------------- */
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
        let rows = plugin_table().expect("内嵌插件表解析失败");
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
        // 免配置包预置的插件不应该依赖外部二进制（预置了也还是 missing_deps）
        for r in rows.iter().filter(|r| r.bundle) {
            assert!(r.exe.is_empty(), "{} 依赖外部命令却标了 bundle", r.name);
        }
        // 默认启用的插件同样不许依赖外部二进制（docs/53 拍板）：否则"默认安装"
        // 就等于强迫每个用户多下 87MB 的 ffmpeg，而绝大多数人不需要 gain_db。
        for r in rows.iter().filter(|r| r.default) {
            assert!(r.exe.is_empty(), "{} 默认启用却需要外部命令", r.name);
        }
    }

    /// Python 探测的形状自洽：找到就必须有路径与版本，找不到就必须有人话。
    /// 不在测试里断言"一定找得到"——CI 与用户机器不一样，那种断言只会变成
    /// 环境依赖的假红。
    #[test]
    fn python_probe_shape_is_consistent() {
        let p = python_probe_payload();
        if p.found {
            assert!(p.path.is_some() && p.version.is_some());
            assert!(p.reason.is_none());
            assert_eq!(version_ok(p.version.as_deref().unwrap_or("")), p.supported);
        } else {
            assert!(p.reason.is_some(), "找不到 Python 时必须给一句人话");
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
        // 旧版 settings.json 没有 plugins 键：必须能读（否则升级后所有设置
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
    /// 2026-10-30 的真实事故：`beets_set_target` 放宽到子目录、`roots_for_tools`
    /// 也改了，但 `config_view_of` 还留着严格相等 —— UI 显示 `D:\CkMsr\Music`，
    /// 而工具拿到的 `AIYA_BEETS_DEST` 是 `D:\CkMsr\Music\_整理测试`。
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

    /* ---------------- 安装轨道（docs/33 §6.2） ---------------- */

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
        assert!(err.contains("不是有效的压缩包"), "{err}");
        assert!(err.contains("错误页"), "要指得到原因：{err}");
    }

    /// 空文件是另一种常见状态（下载被中断、写到一半）。
    #[test]
    fn verify_rejects_empty_file() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("installer.pyz");
        std::fs::write(&p, b"").unwrap();
        let err = verify_asset(&p, AssetKind::Installer).unwrap_err();
        assert!(err.contains("空文件"), "{err}");
    }

    /// B 轨道的展开 + 入口校验：入口由 manifest 说了算（播放器不猜布局）。
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
        assert!(err.contains("找不到入口"), "{err}");
    }

    /// 形状锚点：BeetsStatus 是设置页数据源，必须 camelCase 进 IPC。
    #[test]
    fn beets_status_serializes_camel_case() {
        // BeetsStatus 只有 Deserialize；用 JSON 反序列化锚定输入形状
        let v = serde_json::json!({
            "found": true, "interpreter": "py", "scriptPath": "-m aiya_beets",
            "entrySource": "settings",
            "protocolVersion": 1, "beetsVersion": "2.14.1", "pythonVersion": "3.14.7",
            "pythonSupported": true,
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
        assert_eq!(st.python_supported, Some(true));
        assert_eq!(st.data_home.as_deref(), Some("d"));
        // v2：schema 比对结论 + 库锁三态原样透传
        assert!(st.schema_matched);
        assert_eq!(st.library_lock.as_deref(), Some("free"));
        assert!(st.library_lock_holder.is_none());
        // 三态是 docs/31 P0-1 的修法：不能只报"配置里开着"，要报"能不能干活"
        let health = st.plugin_health.expect("pluginHealth 必须解析");
        assert_eq!(health[0].state, "missing_deps");
        assert_eq!(health[0].missing, vec!["bs4".to_string()]);
        assert_eq!(st.plugin_summary.expect("pluginSummary").missing_deps, 2);
    }

    /// 未装工具时给的是**可解释的状态**，不是崩溃、也不是空对象。
    ///
    /// docs/33 §3.1 的硬原则要求"未装 beets 时核心功能全通"；依赖隔离那条
    /// 已由 `tools/check-boundary.mjs` 守住（核心 crate 连依赖都不许有）。
    /// 这里补的是另一半：beets 自己的命令面在未装时必须回一个带错误码的
    /// 状态，让 UI 能说清"没装"而不是"出错了"。
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
        // 用户排障要知道"我们试着用了哪个程序、从哪找的"
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
    /// 守的是一个真机 bug（2026-10-30）：预览在联网匹配时，`beets_status`
    /// 撞上会话槽返回 Busy，`probe` 把它统一翻成 `E_BEETS_UNREACHABLE`，
    /// 于是 `found=false` → 整理库页自行跳到"未安装"的安装页。用户看到的
    /// 是"组件凭空消失"，实际只是他自己刚点的预览还在跑。
    #[test]
    fn busy_status_stays_found() {
        let entry = ToolEntry {
            program: "C:/x/aiya-beets.exe".into(),
            args: vec!["serve".into()],
        };
        let st = busy_status(&entry, EntrySource::Dev);
        assert!(st.found, "忙碌时必须仍报 found，否则 UI 会当成未安装");
        assert_eq!(st.error_code.as_deref(), Some("E_BEETS_BUSY"));
        assert_eq!(st.interpreter.as_deref(), Some("C:/x/aiya-beets.exe"));
        // 没拿到报告就谈不上契约匹配，写操作应当被 UI 拦住
        assert!(!st.schema_matched);
    }

    /// 协议版本不符 → **工具存在但不可用**，且必须说清升哪一边。
    ///
    /// 这条守的是 docs/33 §4.4 的"不做自动协商"：两侧独立发布之后，
    /// 版本不匹配是**常态**，不能表现为"功能时好时坏"，只能表现为
    /// 一句能照做的话。
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
            msg.contains("升级播放器"),
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
        assert!(old.error_message.unwrap_or_default().contains("升级组件"));

        // 不报版本（老工具/被裁剪的输出）也要拒，不能默认信任
        let silent = status_from(&entry, EntrySource::Path, StatusReport::default());
        assert!(!silent.found);
        assert_eq!(silent.error_code.as_deref(), Some("E_PROTOCOL_MISMATCH"));
    }

    /// dev venv 存在时（本仓库），probe 能找到 beets 2.14.x。
    #[cfg(debug_assertions)]
    #[test]
    fn probe_finds_dev_venv_beets() {
        let venv = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../beets-tool/.venv/Scripts/python.exe");
        if !venv.exists() {
            return; // CI/其他机器没有 venv 就跳过
        }
        let status = status_via(&venv.to_string_lossy(), &["-m", "aiya_beets"]);
        assert!(
            status.found,
            "dev 环境应命中工具: {:?}",
            status.error_message
        );
        assert!(status
            .beets_version
            .as_deref()
            .unwrap_or_default()
            .starts_with("2.14"));
        assert!(status.plugins.is_some());
        // 能力三态必须报上来（这是 M2 的验收点之一：lyrics 缺 bs4 要能看见）
        let health = status.plugin_health.expect("工具必须自报插件三态");
        assert!(
            health.iter().any(|p| p.state == "ready"),
            "至少有一个 ready 插件"
        );
        assert!(
            health.iter().any(|p| p.name == "lyrics"),
            "lyrics 应在清单里"
        );
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
            Ok(report) => status_from(&entry, EntrySource::Dev, report),
            Err(e) => absent(
                "E_SPAWN",
                Some(e.to_string()),
                Some(&entry),
                Some(EntrySource::Dev),
            ),
        }
    }

    /// B 轨道套件包（自带 embeddable 解释器 + 包在 Lib/site-packages）能命中工具。
    ///
    /// 这条守的是 B 轨道的核心前提：套件包**不需要系统装 Python**，
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
        // 自带解释器 ⇒ 运行时版本必须自报，且被判定为支持
        assert!(status.python_version.is_some(), "自带解释器应自报版本");
        assert_eq!(
            status.python_supported,
            Some(true),
            "自带解释器应判定为支持"
        );
    }

    /// 端到端：真 sidecar dump_items（dev venv + dev 隔离库，B2 冒烟留下的 3 条）
    /// → Rust 逐行解析（协议形状锚点）。
    /// 对账已随 docs/43 §8-E 整体移除；dump_items 保留为协议命令
    /// （诊断/未来显式流程用），宿主没有常规调用方。
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
}
