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
    Dev,
}

impl EntrySource {
    pub fn as_str(self) -> &'static str {
        match self {
            EntrySource::Env => "env",
            EntrySource::Settings => "settings",
            EntrySource::Path => "path",
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
    app.path()
        .app_data_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("beets")
}

/// 用户的第一个扫描根 → 工具的整理目标根。
///
/// 这是**用户事实**，不是 beets 知识：播放器本来就知道用户的曲库在哪。
/// 工具拿它写进自己的配置，播放器不碰配置格式。
fn scan_root(app: &tauri::AppHandle) -> Option<String> {
    app.try_state::<crate::AppState>()
        .and_then(|st| {
            st.db()
                .send(|conn| Ok(crate::setting_get(conn, "library.roots")))
                .ok()
                .flatten()
        })
        .and_then(|v| {
            v.as_array()?
                .first()
                .and_then(|x| x.as_str())
                .map(str::to_string)
        })
}

fn setting_str(app: &tauri::AppHandle, key: &'static str) -> Option<String> {
    app.try_state::<crate::AppState>()
        .and_then(|st| st.db().send(|conn| Ok(crate::setting_get(conn, key))).ok().flatten())
        .and_then(|v| v.as_str().map(str::to_string))
        .filter(|s| !s.trim().is_empty())
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

    if let Some(program) = setting_str(app, "beets.toolPath") {
        let args = setting_str(app, "beets.toolArgs").map(|s| split_args(&s)).unwrap_or_default();
        if PathBuf::from(&program).is_file() {
            return Some((ToolEntry { program, args }, EntrySource::Settings));
        }
        tracing::warn!(path = %program, "设置里的 beets.toolPath 已不存在，继续找下一个来源");
    }

    if let Some(found) = find_on_path("aiya-beets") {
        return Some((
            ToolEntry { program: found.to_string_lossy().into_owned(), args: Vec::new() },
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
/// * `AIYA_BEETS_SCAN_ROOT`：用户第一个扫描根（播放器给，工具拿去写整理目标根）
/// * `PYTHONIOENCODING` / `PYTHONUTF8`：管道编码钉 UTF-8。工具内部也 reconfigure
///   了一次，这里是双保险（工具被替换/被单独调用时仍正确）。这两个是 Python
///   通用变量，不是 beets 知识，留着不亏。
fn tool_env(app: &tauri::AppHandle) -> Vec<(String, String)> {
    let mut env = vec![
        ("PYTHONIOENCODING".to_string(), "utf-8".to_string()),
        ("PYTHONUTF8".to_string(), "1".to_string()),
        ("AIYA_BEETS_HOME".to_string(), data_home(app).to_string_lossy().into_owned()),
    ];
    if let Some(root) = scan_root(app) {
        env.push(("AIYA_BEETS_SCAN_ROOT".to_string(), root));
    }
    // 源码树形态需要 PYTHONPATH 才 import 得到包
    #[cfg(debug_assertions)]
    {
        let src = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../beets-tool/src");
        if src.is_dir() {
            env.push(("PYTHONPATH".to_string(), src.to_string_lossy().into_owned()));
        }
    }
    env
}

/// 入口 + 环境 → 可直接起进程的描述。
fn launcher_for(app: &tauri::AppHandle, entry: &ToolEntry) -> Launcher {
    Launcher::new(&entry.program).args(entry.args.clone()).envs(tool_env(app))
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

/// 协议报告 → UI 契约。
///
/// 两边字段名不同是刻意的：crate 用中性词（`tool_version` / `runtime_version` /
/// `library_root`，因为它不知道对面是什么），UI 沿用用户看得懂的说法。
/// 转换只在这一处，改协议时只要改这个函数。
/// 协议报告 → UI 契约。
///
/// 两边字段名不同是刻意的：crate 用中性词（`tool_version` / `runtime_version` /
/// `library_root`，因为它不知道对面是什么），UI 沿用用户看得懂的说法。
/// 转换只在这一处，改协议时只要改这个函数。
fn status_from(entry: &ToolEntry, source: EntrySource, report: StatusReport) -> BeetsStatus {
    BeetsStatus {
        found: true,
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
        plugin_health: report.capability_health.and_then(|v| serde_json::from_value(v).ok()),
        plugin_summary: report.capability_summary.and_then(|v| serde_json::from_value(v).ok()),
        config_dir: report.config_dir,
        data_home: report.data_home,
        error_code: None,
        error_message: None,
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
) -> Result<BeetsStatus, String> {
    let tool = StdioTool::new(launcher_for(app, entry));
    match tool.status(timeout) {
        Ok(report) => Ok(status_from(entry, source, report)),
        Err(ToolError::Rejected { code, stage, message }) => Ok(absent(
            &code,
            Some(format!("[{stage}] {message}")),
            Some(entry),
            Some(source),
        )),
        Err(e) => Err(e.to_string()),
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
        Err(e) => absent("E_BEETS_UNREACHABLE", Some(e), Some(&entry), Some(source)),
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

/// 发布物位置。M4 接真实 release 后，这里换成 manifest 拉取（可带版本号与校验和）。
///
/// 现在指向占位地址：下载失败会给"还没发布"的明确错误，而不是静默卡住。
/// 开发/离线时用 `AIYA_BEETS_INSTALLER` / `AIYA_BEETS_BUNDLE` 指向本地文件。
const RELEASE_BASE: &str = "https://github.com/aiya-music/aiya-beets/releases/latest/download";
const INSTALLER_ASSET: &str = "aiya-beets-installer.pyz";
const BUNDLE_ASSET: &str = "aiya-beets-bundle-win-x64.zip";

fn install_root(app: &tauri::AppHandle) -> PathBuf {
    app.path()
        .app_data_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("beets-tool")
}

/// 取安装物：环境变量指本地文件优先，否则下载到临时目录。
fn fetch_asset(env_key: &str, asset: &str, emit: &dyn Fn(serde_json::Value)) -> Result<PathBuf, String> {
    if let Ok(local) = std::env::var(env_key) {
        let p = PathBuf::from(local);
        if p.is_file() {
            emit(serde_json::json!({
                "type": "progress", "stage": "prepare",
                "message": format!("使用本地安装物：{}", p.display()),
            }));
            return Ok(p);
        }
        return Err(format!("{env_key} 指向的文件不存在：{}", p.display()));
    }

    let url = format!("{RELEASE_BASE}/{asset}");
    emit(serde_json::json!({
        "type": "progress", "stage": "download",
        "message": format!("下载 {asset}"),
    }));
    let dest = std::env::temp_dir().join(asset);
    let resp = ureq::get(&url)
        .timeout(std::time::Duration::from_secs(120))
        .call()
        .map_err(|e| format!("下载失败（{url}）：{e}\n——若组件尚未发布，可手动下载后设置 {env_key} 指向该文件"))?;
    let mut out = std::fs::File::create(&dest).map_err(|e| format!("写临时文件失败：{e}"))?;
    std::io::copy(&mut resp.into_reader(), &mut out).map_err(|e| format!("下载写入失败：{e}"))?;
    Ok(dest)
}

/// A 轨道：用本机 Python 跑安装器。
///
/// 找 Python 用 `python`/`python3` 试 `-V`——这是**宿主的便利探测**，不是
/// beets 知识；装什么、装到哪由安装器自己决定（它只接受 `--target`）。
fn find_system_python() -> Option<String> {
    for cand in ["python", "python3"] {
        let ok = std::process::Command::new(cand)
            .arg("-V")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if ok {
            return Some(cand.into());
        }
    }
    None
}

fn install_via_installer(
    app: &tauri::AppHandle,
    pyz: &std::path::Path,
    emit: &dyn Fn(serde_json::Value),
) -> Result<ToolEntry, String> {
    let python = find_system_python().ok_or_else(|| {
        "本机没有可用的 Python。改用「免安装套件包」这条轨道，或先安装 Python 3.10+".to_string()
    })?;
    let target = install_root(app).join("venv");
    emit(serde_json::json!({
        "type": "progress", "stage": "install",
        "message": format!("用 {python} 安装组件到 {}", target.display()),
    }));

    let mut child = std::process::Command::new(&python)
        .arg(pyz)
        .arg("install")
        .arg("--target")
        .arg(&target)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .envs(tool_env(app))
        .spawn()
        .map_err(|e| format!("启动安装器失败：{e}"))?;

    let mut stdout = child.stdout.take().ok_or("安装器 stdout 未捕获")?;
    use std::io::BufRead;
    let reader = std::io::BufReader::new(&mut stdout);
    let mut done = false;
    for line in reader.lines().map_while(Result::ok) {
        let s = line.trim();
        if s.is_empty() {
            continue;
        }
        // 安装器输出 JSON 行；万一有裸行（pip 进度条）也包成 progress，不丢信息
        let v: serde_json::Value = serde_json::from_str(s).unwrap_or_else(|_| {
            serde_json::json!({ "type": "progress", "stage": "pip", "message": s })
        });
        if v.get("type").and_then(|t| t.as_str()) == Some("done") {
            done = true;
        }
        emit(v);
    }
    let code = child.wait().map_err(|e| format!("等待安装器退出失败：{e}"))?;
    if !code.success() || !done {
        return Err(format!("安装器未成功完成（退出码 {}）", code.code().unwrap_or(-1)));
    }

    // 入口：venv 里的 aiya-beets 脚本。
    // 这是**安装器的产物约定**，不是探测：安装器把它装在这个位置，
    // 我们就记这个位置（见 docs/33 §6.2 A 轨道）。
    let exe = target.join("Scripts").join("aiya-beets.exe");
    if exe.is_file() {
        return Ok(ToolEntry { program: exe.to_string_lossy().into_owned(), args: Vec::new() });
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

/// B 轨道：展开套件包。
///
/// 套件包里带 `manifest.json`（`{"entry": "aiya-beets.exe", "args": []}` 或
/// 解释器形态），展开后照着它记入口。没有 manifest 就按约定找两个常见入口。
fn install_via_bundle(
    app: &tauri::AppHandle,
    zip_path: &std::path::Path,
    emit: &dyn Fn(serde_json::Value),
) -> Result<ToolEntry, String> {
    let target = install_root(app).join("bundle");
    if target.exists() {
        std::fs::remove_dir_all(&target).map_err(|e| format!("清理旧套件包失败：{e}"))?;
    }
    std::fs::create_dir_all(&target).map_err(|e| format!("创建目标目录失败：{e}"))?;
    emit(serde_json::json!({
        "type": "progress", "stage": "unpack",
        "message": format!("展开套件包到 {}", target.display()),
    }));

    let file = std::fs::File::open(zip_path).map_err(|e| format!("打开套件包失败：{e}"))?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| format!("套件包不是有效 zip：{e}"))?;
    zip.extract(&target).map_err(|e| format!("展开失败：{e}"))?;

    let manifest = target.join("manifest.json");
    if let Ok(text) = std::fs::read_to_string(&manifest) {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
            if let Some(entry) = v.get("entry").and_then(|x| x.as_str()) {
                let program = target.join(entry);
                if program.is_file() {
                    let args = v
                        .get("args")
                        .and_then(|a| a.as_array())
                        .map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect())
                        .unwrap_or_default();
                    return Ok(ToolEntry { program: program.to_string_lossy().into_owned(), args });
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
            return Ok(ToolEntry { program: p.to_string_lossy().into_owned(), args });
        }
    }
    Err("套件包展开后找不到入口（缺 manifest.json 且没有 aiya-beets.exe）".to_string())
}

/// 记下入口，下次 `discover` 直接命中（设置优先于 PATH 与源码树）。
fn remember_entry(app: &tauri::AppHandle, entry: &ToolEntry) -> Result<(), String> {
    let program = serde_json::json!(entry.program);
    let args = serde_json::json!(entry.args);
    app.try_state::<crate::AppState>()
        .ok_or_else(|| "AppState 未就绪".to_string())?
        .db()
        .send(move |conn| {
            crate::setting_set(conn, "beets.toolPath", &program).map_err(aiya_data::DbError::Sql)?;
            crate::setting_set(conn, "beets.toolArgs", &args).map_err(aiya_data::DbError::Sql)?;
            Ok(())
        })
        .map_err(|e| e.to_string())
}

/// 安装（异步）：立刻返回，进度走 `beets:install`，结束时补一条 finished。
///
/// finished 带**重新探测后的 status**：UI 不用再单独发一次查询，
/// 也不会出现"装完了但状态块还是旧值"这种中间态。
pub fn start_install(app: tauri::AppHandle, track: String) -> Result<(), String> {
    let track = InstallTrack::parse(&track)?;
    std::thread::Builder::new()
        .name("beets-install".into())
        .spawn(move || {
            let emit = |v: serde_json::Value| {
                let _ = app.emit("beets:install", v);
            };
            let result = (|| -> Result<ToolEntry, String> {
                let (env_key, asset) = match track {
                    InstallTrack::Installer => ("AIYA_BEETS_INSTALLER", INSTALLER_ASSET),
                    InstallTrack::Bundle => ("AIYA_BEETS_BUNDLE", BUNDLE_ASSET),
                };
                let path = fetch_asset(env_key, asset, &emit)?;
                match track {
                    InstallTrack::Installer => install_via_installer(&app, &path, &emit),
                    InstallTrack::Bundle => install_via_bundle(&app, &path, &emit),
                }
            })();
            match result {
                Ok(entry) => match remember_entry(&app, &entry) {
                    // 记不住也不算彻底失败：PATH 与源码树仍可能命中，
                    // 只是下次启动要多一次探测。如实报给 UI。
                    Err(e) => emit(serde_json::json!({ "type": "warning", "message": e })),
                    Ok(()) => {}
                },
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

/* ---------------- B1/B2：导入会话（事件流转发） ---------------- */

/// 读模型刷新参数（lib.rs 注入 AppState 的克隆）。
pub struct RefreshParams {
    pub db_path: PathBuf,
    /// 与 library_scan 共用的互斥标志（避免同时写同一 DB）
    pub scanning: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

/// 取工具入口并造一个可直接起进程的 tool。
///
/// 找不到入口时的文案要能指导用户下一步（去扫描入库页装组件），
/// 而不是一句 "no beets interpreter available"。
fn tool_or_err(app: &tauri::AppHandle) -> Result<StdioTool, String> {
    let (entry, _source) = discover(app).ok_or_else(|| {
        "未安装 beets 组件：请到「扫描入库 → 曲库维护」安装，或先设置 beets.toolPath".to_string()
    })?;
    Ok(StdioTool::new(launcher_for(app, &entry)))
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

    run_session(
        tool,
        &app,
        serde_json::json!({ "id": 1, "cmd": "preview_import", "paths": paths }),
        "beets:import",
        "已有 beets 导入任务在执行（可先取消）",
        Box::new(|| {}),
    )
}

/// B2：应用导入决策。decisions 形状由契约定义（ApplyDecision），
/// 宿主不解释语义，只透传；完成后自动把工具侧结果对齐自有库（自闭环）。
pub fn start_apply(
    app: tauri::AppHandle,
    decisions: Vec<serde_json::Value>,
    refresh: Option<RefreshParams>,
) -> Result<(), String> {
    let tool = tool_or_err(&app)?;

    // on_exit 里拿一份 AppHandle（会话线程要能独立跑，不能借用 &app）
    let exit_app = app.clone();
    run_session(
        tool,
        &app,
        serde_json::json!({ "id": 1, "cmd": "apply_import", "decisions": decisions }),
        "beets:import",
        "已有 beets 导入任务在执行（可先取消）",
        Box::new(move || {
            if let Some(params) = refresh {
                // 自闭环（docs/08 §5）：工具刚写完库就是最新权威，
                // 直接全量对齐自有 DB，替代旧"重扫受影响目录"路径。
                run_sync(&exit_app, params);
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
    refresh: Option<RefreshParams>,
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
                run_sync(&exit_app, params);
            }
        }),
    )
}

/// 建 beets 库基线：把自有库里的文件按现状登记进 beets 库。
/// `paths` 为空 = 由宿主取自有库全部在线路径（UI 不该为了这个传 1800 个路径）。
pub fn start_adopt(
    app: tauri::AppHandle,
    paths: Vec<String>,
    refresh: Option<RefreshParams>,
) -> Result<(), String> {
    start_maintenance(
        app,
        serde_json::json!({ "id": 1, "cmd": "adopt_library", "paths": paths }),
        refresh,
    )
}

/// 用文件标签刷新 beets 库（`pretend` = 只报 diff 不写库）。
pub fn start_update(app: tauri::AppHandle, pretend: bool) -> Result<(), String> {
    start_maintenance(
        app,
        serde_json::json!({ "id": 1, "cmd": "update_library", "pretend": pretend }),
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

/// 按 path_formats 整理文件（`pretend` = 只出清单不搬文件）。
/// `directory` 是校验值：与 config.yaml 的 directory 不一致会被 sidecar 拒绝。
pub fn start_move(
    app: tauri::AppHandle,
    directory: String,
    pretend: bool,
    refresh: Option<RefreshParams>,
) -> Result<(), String> {
    start_maintenance(
        app,
        serde_json::json!({
            "id": 1, "cmd": "move_items",
            "directory": directory, "pretend": pretend,
        }),
        refresh,
    )
}

/// 手动触发一次读模型对齐（设置页/导入面板按钮；B4 的自动同步基座）。
pub fn sync_now(app: tauri::AppHandle, params: RefreshParams) {
    std::thread::Builder::new()
        .name("beets-sync".into())
        .spawn(move || run_sync(&app, params))
        .map(|_| ())
        .map_err(|e| tracing::warn!(%e, "起 sync 线程失败"))
        .ok();
}

/// dump 全量收集的整体超时：本地库读取，几万条也在秒级；超过这个数就是卡死。
const DUMP_TIMEOUT: Duration = Duration::from_secs(180);

/// 从 sidecar 收集 dump_items 全量（一次性进程，调用线程同步读）。
///
/// 两条硬要求（此前都没有，导致过静默的数据损坏风险）：
/// 1. **必须收到 `dumpDone` 才算成功**。只看"进程退出"的话，dump 中途崩/被
///    kill 会返回半份结果，而 `sync_items` 把"自有有、beets 没有"的行计入
///    orphaned —— 半份 dump 会让一大批曲目被误判成 orphan，后续清理逻辑
///    会拿它当依据。
/// 2. **整体超时要收尸**。卡死的 sidecar 会一直占着 stdout 读取线程，
///    sync 线程永远等下去（用户侧表现：手动同步按钮按了没反应）。
fn collect_items(app: &tauri::AppHandle) -> Result<Vec<aiya_data::beets_sync::BeetsItem>, String> {
    let (entry, _source) = discover(app).ok_or_else(|| "未安装 beets 组件".to_string())?;
    let launcher = launcher_for(app, &entry);
    let (child, stdin) = aiya_beets_client::session::spawn_session(
        &launcher,
        &serde_json::json!({ "id": 1, "cmd": "dump_items" }),
    )
    .map_err(|e| e.to_string())?;
    // dump 是一次性收集：不留 stdin 挂着，对面处理完 EOF 退出
    drop(stdin);
    let child = std::sync::Arc::new(std::sync::Mutex::new(child));
    let stdout = {
        let mut c = child.lock().map_err(|_| "child lock poisoned")?;
        c.stdout.take().expect("stdout piped")
    };

    // 读线程：逐行带编码检查地送进 channel（超时/收尸在主线程）。
    // 编码处理与事件链路共用同一份实现，避免两处各自漂移。
    let (tx, rx) = std::sync::mpsc::channel::<String>();
    if let Err(e) = aiya_beets_client::session::spawn_line_reader(stdout, tx, "beets-dump-recv") {
        let mut c = child.lock().map_err(|_| "child lock poisoned")?;
        let _ = c.kill();
        let _ = c.wait();
        return Err(format!("起 dump 读线程: {e}"));
    }

    let mut items = Vec::new();
    let mut done = false;
    loop {
        match rx.recv_timeout(DUMP_TIMEOUT) {
            Ok(line) => {
                let Ok(v) = serde_json::from_str::<serde_json::Value>(&line) else {
                    continue; // 非 JSON 行（首次初始化时的迁移提示等）
                };
                match v.get("type").and_then(|t| t.as_str()) {
                    Some("item") => {
                        if let Some(item) = v.get("item") {
                            match serde_json::from_value(item.clone()) {
                                Ok(it) => items.push(it),
                                Err(e) => tracing::warn!(%e, "beets item 解析失败，跳过"),
                            }
                        }
                    }
                    Some("dumpDone") => {
                        done = true;
                        break;
                    }
                    Some("sessionFailed") => {
                        let msg = v
                            .get("error")
                            .and_then(|e| e.get("beets_message"))
                            .and_then(|m| m.as_str())
                            .unwrap_or("sidecar 报告失败");
                        let mut c = child.lock().map_err(|_| "child lock poisoned")?;
                        let _ = c.kill();
                        let _ = c.wait();
                        return Err(format!("dump 失败: {msg}"));
                    }
                    _ => {}
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                let mut c = child.lock().map_err(|_| "child lock poisoned")?;
                let _ = c.kill();
                let _ = c.wait();
                return Err(format!("dump 超时（{DUMP_TIMEOUT:.0?} 无输出）"));
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break, // 进程退出
        }
    }
    {
        let mut c = child.lock().map_err(|_| "child lock poisoned")?;
        let _ = c.kill();
        let _ = c.wait();
    }
    if !done {
        return Err(format!(
            "dump 未收到 dumpDone（已收到 {} 条 item，进程可能中途退出）",
            items.len()
        ));
    }
    Ok(items)
}

/// 对齐执行体：dump 全量 → sync_items → 发 `librarySynced` 汇总。
/// 与 library_scan 共用 scanning 互斥；正扫全库时让位（libraryRefreshSkipped）。
fn run_sync(app: &tauri::AppHandle, params: RefreshParams) {
    use std::sync::atomic::Ordering;
    let emit = |v: serde_json::Value| {
        let _ = app.emit("beets:import", v);
    };

    if discover(app).is_none() {
        emit(serde_json::json!({
            "type": "librarySyncFailed",
            "message": "未安装 beets 组件",
        }));
        return;
    }
    if params
        .scanning
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        emit(serde_json::json!({
            "type": "libraryRefreshSkipped",
            "reason": "scanBusy",
        }));
        return;
    }

    // P2：scanning 标志的 panic 保护——sync 链路里任一 panic 曾让标志永真，
    // 之后所有扫描 E_SCAN_BUSY。Drop 兜底保证退出路径（含 unwind）必复位。
    struct ScanGuard(std::sync::Arc<std::sync::atomic::AtomicBool>);
    impl Drop for ScanGuard {
        fn drop(&mut self) {
            self.0.store(false, Ordering::SeqCst);
        }
    }
    let _guard = ScanGuard(std::sync::Arc::clone(&params.scanning));

    let result = (|| -> Result<aiya_data::beets_sync::SyncStats, String> {
        let items = collect_items(app)?;
        let mut conn = aiya_data::open_conn(&params.db_path)
            .map_err(|e| format!("open db: {e}"))?;
        let stats = aiya_data::beets_sync::sync_items(&mut conn, &items, now_secs())
            .map_err(|e| format!("sync: {e}"))?;
        let _ = conn.close();
        Ok(stats)
    })();

    match result {
        Ok(stats) => match serde_json::to_value(&stats) {
            Ok(mut v) => {
                if let Some(obj) = v.as_object_mut() {
                    obj.insert("type".into(), serde_json::json!("librarySynced"));
                }
                emit(v);
            }
            Err(e) => emit(serde_json::json!({ "type": "librarySyncFailed", "message": e.to_string() })),
        },
        Err(e) => emit(serde_json::json!({ "type": "librarySyncFailed", "message": e })),
    }
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 形状锚点：BeetsStatus 是设置页数据源，必须 camelCase 进 IPC。
    #[test]
    fn beets_status_serializes_camel_case() {
        // BeetsStatus 只有 Deserialize；用 JSON 反序列化锚定输入形状
        let v = serde_json::json!({
            "found": true, "interpreter": "py", "scriptPath": "-m aiya_beets",
            "entrySource": "settings",
            "protocolVersion": 3, "beetsVersion": "2.14.1", "pythonVersion": "3.14.7",
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
            "errorCode": null, "errorMessage": null
        });
        let st: BeetsStatus = serde_json::from_value(v).expect("camelCase 形状必须可解析");
        assert_eq!(st.beets_version.as_deref(), Some("2.14.1"));
        assert_eq!(st.library_exists, Some(true));
        assert_eq!(st.plugins, Some(vec!["musicbrainz".to_string()]));
        assert_eq!(st.entry_source.as_deref(), Some("settings"));
        assert_eq!(st.python_supported, Some(true));
        assert_eq!(st.data_home.as_deref(), Some("d"));
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
        let entry = ToolEntry { program: "C:/x/aiya-beets.exe".into(), args: vec!["serve".into()] };
        let st = absent("E_TOOL_NOT_FOUND", Some("找不到".into()), Some(&entry), Some(EntrySource::Path));
        assert!(!st.found);
        assert_eq!(st.error_code.as_deref(), Some("E_TOOL_NOT_FOUND"));
        assert_eq!(st.error_message.as_deref(), Some("找不到"));
        // 用户排障要知道"我们试着用了哪个程序、从哪找的"
        assert_eq!(st.interpreter.as_deref(), Some("C:/x/aiya-beets.exe"));
        assert_eq!(st.script_path.as_deref(), Some("serve"));
        assert_eq!(st.entry_source.as_deref(), Some("path"));
        // 失败态不许残留任何"看起来像成功"的字段
        assert!(st.beets_version.is_none() && st.plugin_health.is_none() && st.library_path.is_none());
    }

    /// dev venv 存在时（本仓库），probe 能找到 beets 2.14.x。
    #[cfg(debug_assertions)]
    #[test]
    fn probe_finds_dev_venv_beets() {
        let venv = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../beets-tool/.venv/Scripts/python.exe");
        if !venv.exists() {
            return; // CI/其他机器没有 venv 就跳过
        }
        let status = status_via(&venv.to_string_lossy(), &["-m", "aiya_beets"]);
        assert!(status.found, "dev 环境应命中工具: {:?}", status.error_message);
        assert!(status.beets_version.as_deref().unwrap_or_default().starts_with("2.14"));
        assert!(status.plugins.is_some());
        // 能力三态必须报上来（这是 M2 的验收点之一：lyrics 缺 bs4 要能看见）
        let health = status.plugin_health.expect("工具必须自报插件三态");
        assert!(health.iter().any(|p| p.state == "ready"), "至少有一个 ready 插件");
        assert!(health.iter().any(|p| p.name == "lyrics"), "lyrics 应在清单里");
    }

    /// 用指定入口跑一次 status（测试用；生产走 `probe`，那里入口来自 `discover`）。
///
/// `AIYA_BEETS_HOME` 不设 = 走 beets 自己的默认位置，不碰应用的 app_data。
/// `PYTHONPATH` 指向仓库源码树，这样源码树里的工具能被 import。
fn status_via(program: &str, args: &[&str]) -> BeetsStatus {
    let src = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../beets-tool/src");
    let launcher = Launcher::new(program)
        .args(args.iter().copied())
        .env("PYTHONIOENCODING", "utf-8")
        .env("PYTHONUTF8", "1")
        .env("PYTHONPATH", src.to_string_lossy());
    let entry = ToolEntry { program: program.into(), args: args.iter().map(|s| s.to_string()).collect() };
    let tool = StdioTool::new(launcher);
    match tool.status(Duration::from_secs(20)) {
        Ok(report) => status_from(&entry, EntrySource::Dev, report),
        Err(e) => absent("E_SPAWN", Some(e.to_string()), Some(&entry), Some(EntrySource::Dev)),
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
        let manifest: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(root.join("manifest.json")).expect("manifest"))
                .expect("manifest json");
        assert_eq!(manifest["entry"], "python.exe", "manifest 的 entry 必须与实测入口一致");
        let status = status_via(&py.to_string_lossy(), &["-m", "aiya_beets"]);
        assert!(status.found, "套件包也应命中工具: {:?}", status.error_message);
        assert!(
            status.beets_version.as_deref().unwrap_or_default().starts_with("2.14"),
            "套件包自报的 beets 版本: {:?}",
            status.beets_version
        );
        // 自带解释器 ⇒ 运行时版本必须自报，且被判定为支持
        assert!(status.python_version.is_some(), "自带解释器应自报版本");
        assert_eq!(status.python_supported, Some(true), "自带解释器应判定为支持");
    }

    /// 端到端：真 sidecar dump_items（dev venv + dev 隔离库，B2 冒烟留下的 3 条）
    /// → Rust 解析 → beets_sync 对齐进临时 DB。
    /// 不复用 collect_items（那要 AppHandle 落脚本 + set_var 污染并行测试），
    /// 事件形状按同一契约独立构造。
    #[cfg(debug_assertions)]
    #[test]
    fn dump_parse_and_sync_end_to_end() {
        let venv = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../beets-tool/.venv/Scripts/python.exe");
        let beets_home = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../beets-tool/tests/fixtures/beets-home");
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
            .env("PYTHONPATH", PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../beets-tool/src"))
            .env("BEETSDIR", &beets_home)
            .spawn()
            .expect("spawn sidecar");
        use std::io::Write;
        writeln!(child.stdin.take().expect("stdin"), r#"{{"id":1,"cmd":"dump_items"}}"#).unwrap();

        let mut stdout = child.stdout.take().expect("stdout");
        let mut items: Vec<aiya_data::beets_sync::BeetsItem> = Vec::new();
        {
            use std::io::BufRead;
            for line in std::io::BufReader::new(&mut stdout).lines() {
                let Ok(line) = line else { break };
                let Ok(v) = serde_json::from_str::<serde_json::Value>(line.trim()) else { continue };
                if v.get("type").and_then(|t| t.as_str()) == Some("item") {
                    items.push(serde_json::from_value(v["item"].clone()).expect("item 形状"));
                }
            }
        }
        let _ = child.wait();
        assert!(items.len() >= 3, "dev 库应有 B2 冒烟写入的 3 条，实际 {}", items.len());
        assert!(items.iter().all(|it| it.id > 0 && !it.path.is_empty()));

        let mut conn = aiya_data::db::open_in_memory().unwrap();
        let stats = aiya_data::beets_sync::sync_items(&mut conn, &items, 0).unwrap();
        assert_eq!(stats.imported, items.len());
        // 再对齐一次 → 全部 unchanged（幂等）
        let stats2 = aiya_data::beets_sync::sync_items(&mut conn, &items, 1).unwrap();
        assert_eq!((stats2.matched, stats2.updated, stats2.unchanged), (items.len(), 0, items.len()));
        // beets_item_id 真的写进去了
        let with_id: i64 = conn
            .query_row("SELECT COUNT(*) FROM tracks WHERE beets_item_id IS NOT NULL", [], |r| r.get(0))
            .unwrap();
        assert_eq!(with_id as usize, items.len());
    }

}
