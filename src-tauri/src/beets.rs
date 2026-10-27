//! beets sidecar 宿主侧：解释器探测 + status 查询（docs/08-beets.md B0）。
//!
//! 探测顺序：AIYA_BEETS_PYTHON env → dev venv（仅 debug 构建）→ 系统 python。
//! sidecar 脚本 include_str! 进二进制，写到 app_data/sidecar/（dev/release 行为一致）。
//! v1 不捆绑 Python/beets：没装就返回 not_found，设置页给安装指引，播放功能不受影响。
//!
//! **进程与协议不在这里**（docs/33 M1）：起进程、写 NDJSON、超时收尸、会话
//! 互斥、取消全部在 `aiya-beets-client`。本文件只做两件事——
//! ① 环境侧决策（找哪个入口、带什么环境变量、配置放哪）；
//! ② 结果侧落地（协议形状 → UI DTO、dump 结果 → 自有库对账）。
//! M2 会把 ① 整体搬到工具侧，届时这里只剩 ②。

use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use aiya_beets_client::{BeetsTool, EventSink, Launcher, StatusReport, StdioTool, ToolError};
use tauri::{Emitter, Manager};

/// 设置页 beets 状态区数据源。形状定义搬去 `dto::beets`（契约归 dto），
/// 这里 `pub use` 回来，本文件其余调用点不用改。
pub use crate::dto::beets::BeetsStatus;

const SIDECAR_SCRIPT: &str = include_str!("../../sidecar/beets_sidecar.py");
/// 运行时安装脚本（docs/29 批 C）。与 sidecar 一起落盘 —— 它要读同目录的
/// requirements，两个文件放一起最简单。
const INSTALL_SCRIPT: &str = include_str!("../../sidecar/beets_install.py");
/// 依赖清单：`tools/beets-requirements.lock` 是**唯一真源**，
/// 构建期（pack-beets-runtime.mjs）与运行期（install.py）共用这一份。
const REQUIREMENTS: &str = include_str!("../../tools/beets-requirements.lock");

/// 内容不同才写（升级友好；也让"脚本被手改过"能被发现并纠正）。
fn write_if_changed(path: &std::path::Path, content: &str) -> Result<(), String> {
    let mismatch = std::fs::read_to_string(path).map(|old| old != content).unwrap_or(true);
    if mismatch {
        std::fs::write(path, content).map_err(|e| format!("write {}: {e}", path.display()))?;
    }
    Ok(())
}

/// sidecar 脚本落盘位置：{app_data}/sidecar/beets_sidecar.py。
/// 同时落安装脚本与依赖清单（install.py 从自己所在目录读 requirements）。
fn ensure_script(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("app_data_dir: {e}"))?
        .join("sidecar");
    std::fs::create_dir_all(&dir).map_err(|e| format!("create_dir: {e}"))?;
    let path = dir.join("beets_sidecar.py");
    write_if_changed(&path, SIDECAR_SCRIPT)?;
    write_if_changed(&dir.join("beets_install.py"), INSTALL_SCRIPT)?;
    write_if_changed(&dir.join("beets_requirements.txt"), REQUIREMENTS)?;
    Ok(path)
}

/// beets 相关路径。每次现算（几次 stat），不做缓存 —— 也就不会有
/// "缓存了旧位置"这类只在重装后才发现的问题。
struct BeetsPaths {
    /// `{app_data}/beets` —— BEETSDIR。配置与 beets 库都放这儿，
    /// 不与用户系统 beets（`%APPDATA%\beets`）抢同一个库。
    home: PathBuf,
    /// 用户装到应用数据目录的 venv（`beets_install` 的产物）
    user_venv: PathBuf,
    /// 随包运行时（embeddable Python + site-packages），存在才用
    bundled: Option<PathBuf>,
}

fn beets_paths(app: &tauri::AppHandle) -> BeetsPaths {
    let data = app
        .path()
        .app_data_dir()
        .unwrap_or_else(|_| PathBuf::from("."));
    // 随包运行时：安装后在 resource_dir/beets-runtime；开发时在仓库 tools/
    let mut bundled = None;
    if let Ok(res) = app.path().resource_dir() {
        let p = res.join("beets-runtime");
        if p.join("python.exe").is_file() {
            bundled = Some(p);
        }
    }
    if bundled.is_none() {
        let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tools/beets-runtime");
        if p.join("python.exe").is_file() {
            bundled = Some(p);
        }
    }
    BeetsPaths { home: data.join("beets"), user_venv: data.join("beets-venv"), bundled }
}

fn venv_python(venv: &std::path::Path) -> String {
    venv.join("Scripts").join("python.exe").to_string_lossy().into_owned()
}

/// 每次 spawn sidecar 都要带的环境（三处 spawn 共用，别再各写一份）。
///
/// * `PYTHONIOENCODING` / `PYTHONUTF8`：把子进程的管道编码钉成 UTF-8。
///   Windows 上 Python 默认按本地代码页（简中 = cp936）编码 stdout，带中文的
///   路径/标题写出来就是 GBK 字节，宿主按 UTF-8 解析直接失配（旧实现的
///   `lines()` 遇到非法字节会静默 break，整个会话无声死掉）。脚本内部也
///   reconfigure 了一次，这里是双保险（脚本被替换/被单独调用时仍正确）。
/// * `BEETSDIR`：**恒定指向 `{app_data}/beets`**（docs/29 §5 拍板）。
///   不设的话 beets 用 `%APPDATA%\beets`，与用户自己的 beets 共用一个库；
///   而本应用的语义是"beets 是本应用的写权威"，库该归应用管、可随应用卸载。
///   `AIYA_BEETS_DIR` 仍可覆盖（dev 冒烟要隔离另一份库）。
/// * `VIRTUAL_ENV`：解释器是 venv 时告诉子进程。beets 本身不读它，但
///   有些工具链（pkg_resources / 插件里的 `sys.prefix` 推断）依赖它。
///
/// `beets_home`：None = 不设 BEETSDIR（测试/探测走 beets 自己的默认位置）。
fn sidecar_env(beets_home: Option<&std::path::Path>, python: &str) -> Vec<(String, String)> {
    let mut env = vec![
        ("PYTHONIOENCODING".to_string(), "utf-8".to_string()),
        ("PYTHONUTF8".to_string(), "1".to_string()),
    ];
    // AIYA_BEETS_DIR 永远优先（dev 冒烟要把库隔离到别处）
    let dir = match std::env::var("AIYA_BEETS_DIR") {
        Ok(dir) => Some(PathBuf::from(dir)),
        Err(_) => beets_home.map(|p| p.to_path_buf()),
    };
    if let Some(dir) = dir {
        let _ = std::fs::create_dir_all(&dir);
        env.push(("BEETSDIR".to_string(), dir.to_string_lossy().into_owned()));
    }
    // venv 解释器形如 <venv>/Scripts/python.exe：pyvenv.cfg 在 <venv> 根上
    if let Some(root) = std::path::Path::new(python).parent().and_then(|p| p.parent()) {
        if root.join("pyvenv.cfg").is_file() {
            env.push(("VIRTUAL_ENV".to_string(), root.to_string_lossy().into_owned()));
        }
    }
    env
}

/// 首发档插件（docs/29 §8 拍板）。全部是 beets **内置**插件，不需要额外 pip 包，
/// 因此写进 config 不会让运行时体积或离线可用性变差。
const FIRST_WAVE_PLUGINS: &[&str] = &[
    "replaygain",   // gain_db 列已在引擎链路消费
    "lyrics",       // LRC 规划内能力，lyrics 字段可直供
    "fromfilename", // 无标签文件导入兜底
    "duplicates",   // 完整性维护
    "scrub",        // 导入时清脏标签
    "smartplaylist", // beets 查询 → m3u，与"猜你喜欢"衔接
];

/// YAML 单引号字符串：里面的反斜杠是字面量，Windows 路径不用再转义
/// （双引号里 `\M` 会被当成转义序列，是这类配置文件最经典的坑）。
fn yaml_path(p: &std::path::Path) -> String {
    format!("'{}'", p.to_string_lossy().replace('\'', "''"))
}

/// 生成 config.yaml 的正文（纯函数，单测直接验文本）。
///
/// `scan_root` = None 时**省略 directory 这一行**：beets 会用它自己的默认。
/// 瞎猜一个目录比不写更糟 —— 猜错等于把用户的文件 move 到他没预期的地方。
fn render_config(home: &std::path::Path, scan_root: Option<&str>) -> String {
    let mut text = String::new();
    text.push_str("# AIYA Music 首次启动生成；改过之后应用不会再覆盖\n");
    text.push_str(&format!("library: {}\n", yaml_path(&home.join("library.db"))));
    if let Some(root) = scan_root {
        text.push_str(&format!("directory: {}\n", yaml_path(std::path::Path::new(root))));
    }
    text.push_str(&format!("plugins: {}\n", FIRST_WAVE_PLUGINS.join(" ")));
    text
}

/// 默认 config.yaml。
///
/// 为什么必须有：BEETSDIR 指向 `{app_data}/beets` 之后那里是空的，beets 会用
/// 内置默认 —— plugins 一个都不开，首发档能力全部落空。
///
/// **已存在就不动**：用户手改过的配置不能被应用每次启动覆盖。要恢复默认就
/// 删掉这个文件。
fn ensure_config(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let home = match std::env::var("AIYA_BEETS_DIR") {
        Ok(dir) => PathBuf::from(dir),
        Err(_) => beets_paths(app).home,
    };
    std::fs::create_dir_all(&home).map_err(|e| format!("create beets home: {e}"))?;
    let cfg = home.join("config.yaml");
    if cfg.is_file() {
        return Ok(cfg);
    }

    // directory = 第一个扫描根：导入后 move/copy 的目的地应该落在用户的库上。
    let scan_root = {
        use tauri::Manager;
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
    };

    std::fs::write(&cfg, render_config(&home, scan_root.as_deref()))
        .map_err(|e| format!("write config.yaml: {e}"))?;
    tracing::info!(config = %cfg.display(), "已生成 beets 默认配置");
    Ok(cfg)
}

/// 候选解释器（按优先级）。
///
/// 顺序理由：
///   1. `AIYA_BEETS_PYTHON` —— 排障/开发时显式指定，永远第一
///   2. 用户自己装的 venv —— 他主动装过，就是要用这个
///   3. 随包运行时 —— 构建期自检过，绝大多数用户的路径
///   4. dev venv —— 只有开发机上存在
///   5. 系统 python —— 兜底（用户机器上自己装过 beets 的情况）
fn interpreter_candidates(app: &tauri::AppHandle) -> Vec<String> {
    let paths = beets_paths(app);
    let mut out = Vec::new();
    if let Ok(p) = std::env::var("AIYA_BEETS_PYTHON") {
        out.push(p);
    }
    out.push(venv_python(&paths.user_venv));
    if let Some(b) = &paths.bundled {
        out.push(b.join("python.exe").to_string_lossy().into_owned());
    }
    out.push(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../tools/beets-venv/Scripts/python.exe")
            .to_string_lossy()
            .into_owned(),
    );
    out.push("python".into());
    out
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

/// 「怎么起这个工具」的完整描述：入口 + 参数 + 环境。
///
/// 这三样就是 docs/33 说的**环境知识**，也是 M2 要整体搬去工具侧的部分。
/// 集中在一个函数里的好处是：将来换发现机制只改这里，所有调用点一行不动。
fn launcher_for(python: &str, script: &str, beets_home: Option<&std::path::Path>) -> Launcher {
    Launcher::new(python).arg(script).envs(sidecar_env(beets_home, python))
}

/// 协议报告 → UI 契约。
///
/// 两边字段名不同是刻意的：crate 用中性词（`tool_version` / `runtime_version` /
/// `library_root`，因为它不知道对面是什么），UI 沿用用户看得懂的说法。
/// 转换只在这一处，改协议时只要改这个函数。
fn status_from_report(python: &str, script: &str, report: StatusReport) -> BeetsStatus {
    BeetsStatus {
        found: true,
        interpreter: Some(python.into()),
        script_path: Some(script.into()),
        protocol_version: report.protocol_version,
        beets_version: report.tool_version,
        python_version: report.runtime_version,
        library_path: report.library_path,
        library_exists: report.library_exists,
        library_directory: report.library_root,
        plugins: report.plugins,
        config_dir: report.config_dir,
        error_code: None,
        error_message: None,
    }
}

/// 一次 status 查询，结果翻成 UI 形状。
///
/// 两种失败必须分开（探测循环靠这个区分"跳下一个"和"记诊断"）：
/// - 工具**明确说不行**（`Rejected`）→ 返回 `Ok(found:false)`：解释器活着但
///   没装工具，是继续试下一个候选的**正常信号**。
/// - 起不来 / 超时 / 协议乱 → `Err`，循环把它当最后一条诊断记下。
fn query_sidecar(
    python: &str,
    script: &str,
    timeout: Duration,
    beets_home: Option<&std::path::Path>,
) -> Result<BeetsStatus, String> {
    let tool = StdioTool::new(launcher_for(python, script, beets_home));
    match tool.status(timeout) {
        Ok(report) => Ok(status_from_report(python, script, report)),
        Err(ToolError::Rejected { code, stage, message }) => Ok(BeetsStatus {
            found: false,
            interpreter: Some(python.into()),
            script_path: Some(script.into()),
            protocol_version: None,
            beets_version: None,
            python_version: None,
            library_path: None,
            library_exists: None,
            library_directory: None,
            plugins: None,
            config_dir: None,
            error_code: Some(code),
            error_message: Some(format!("[{stage}] {message}")),
        }),
        Err(e) => Err(e.to_string()),
    }
}

/// 完整探测：按候选解释器逐个试，第一个能给出 status 的胜出。
/// 全部失败 → not_found（有 error_message 供设置页显示）。
pub fn probe(app: &tauri::AppHandle) -> BeetsStatus {
    let script = match ensure_script(app) {
        Ok(p) => p,
        Err(e) => {
            return BeetsStatus {
                found: false,
                interpreter: None,
                script_path: None,
                protocol_version: None,
                beets_version: None,
                python_version: None,
                library_path: None,
                library_exists: None,
                library_directory: None,
                plugins: None,
                config_dir: None,
                error_code: Some("E_SCRIPT".into()),
                error_message: Some(e),
            };
        }
    };
    let script_str = script.to_string_lossy().into_owned();
    // 首次探测就是"准备环境"的时机：BEETSDIR 下没有 config.yaml 就写一份默认的
    // （首发档插件在那里）。失败不阻断探测 —— 没配置时 beets 用内置默认也能跑，
    // 只是插件全关，比"beets 完全不可用"好得多。
    let home = beets_paths(app).home;
    if let Err(e) = ensure_config(app) {
        tracing::warn!(error = %e, "生成 beets 默认配置失败（继续探测）");
    }
    let mut last_err: Option<String> = None;
    for python in interpreter_candidates(app) {
        match query_sidecar(&python, &script_str, Duration::from_secs(10), Some(&home)) {
            Ok(status) => {
                if status.found {
                    return status;
                }
                // 解释器可用但没装 beets：继续试下一个候选（随包运行时优先命中）
                last_err = status.error_message;
            }
            Err(e) => last_err = Some(e),
        }
    }
    BeetsStatus {
        found: false,
        interpreter: None,
        script_path: Some(script_str),
        protocol_version: None,
        beets_version: None,
        python_version: None,
        library_path: None,
        library_exists: None,
        library_directory: None,
        plugins: None,
        config_dir: None,
        error_code: Some("E_BEETS_NOT_FOUND".into()),
        error_message: last_err,
    }
}

/* ---------------- 批 C：运行时安装 ---------------- */

/// PATH 里的系统 Python。
///
/// **不能用随包的 embeddable**：它没有 pip、也没有 ensurepip，建不了 venv。
/// 这条路径只在"随包运行时不可用 + 用户机器装了 Python"时才走。
fn system_python() -> Option<String> {
    for cand in ["python", "python3"] {
        let ok = std::process::Command::new(cand)
            .arg("-V")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if ok {
            return Some(cand.into());
        }
    }
    None
}

/// 安装 beets 运行时到 `{app_data}/beets-venv`（用系统 Python 建 venv）。
///
/// 异步：立刻返回 JobStarted，进度走 `beets:install` 事件，全部结束后再推一条
/// `{"type":"finished","status": BeetsStatus}` —— UI 不用再单独发一次 status 查询。
pub fn start_install(app: tauri::AppHandle) -> Result<(), String> {
    let sidecar = ensure_script(&app)?;
    let install_py = sidecar.with_file_name("beets_install.py");
    let python = system_python().ok_or_else(|| {
        "没有可用的系统 Python。随包运行时应随安装包一起分发；若它缺失，请安装 Python 3.11+ 后重试"
            .to_string()
    })?;
    let target = beets_paths(&app).user_venv;

    let mut child = std::process::Command::new(&python)
        .arg(&install_py)
        .arg("--target")
        .arg(&target)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .envs(sidecar_env(None, &python))
        .spawn()
        .map_err(|e| format!("spawn {python}: {e}"))?;

    let mut stdout = child.stdout.take().ok_or("stdout 未捕获")?;
    std::thread::Builder::new()
        .name("beets-install".into())
        .spawn(move || {
            use std::io::BufRead;
            let reader = std::io::BufReader::new(&mut stdout);
            for line in reader.lines().map_while(Result::ok) {
                let s = line.trim();
                if s.is_empty() {
                    continue;
                }
                // install.py 输出的是 JSON 行；万一有裸行（pip 的进度条被
                // 某些终端环境刷出来）也包成 progress，不丢信息也不让前端
                // 解析失败。
                let v: serde_json::Value = serde_json::from_str(s).unwrap_or_else(|_| {
                    serde_json::json!({ "type": "progress", "stage": "pip", "message": s })
                });
                let _ = app.emit("beets:install", v);
            }
            let _ = child.wait();
            // 收尾再探测一次：装完能不能用由 probe 说了算，不由退出码说了算
            let status = probe(&app);
            let _ = app.emit(
                "beets:install",
                serde_json::json!({ "type": "finished", "status": status }),
            );
        })
        .map_err(|e| format!("起安装线程失败：{e}"))?;
    Ok(())
}

/* ---------------- B1/B2：导入会话（事件流转发） ---------------- */

/// 读模型刷新参数（lib.rs 注入 AppState 的克隆）。
pub struct RefreshParams {
    pub db_path: PathBuf,
    /// 与 library_scan 共用的互斥标志（避免同时写同一 DB）
    pub scanning: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

/// 第一个能答 status 且装了 beets 的解释器（预览/应用共用）。
fn pick_interpreter(app: &tauri::AppHandle, script: &str) -> Option<String> {
    let home = beets_paths(app).home;
    for cand in interpreter_candidates(app) {
        if let Ok(s) = query_sidecar(&cand, script, Duration::from_secs(10), Some(&home)) {
            if s.found {
                return Some(cand);
            }
        }
    }
    None
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
    let script = ensure_script(&app)?;
    let script_str = script.to_string_lossy().into_owned();

    // 快速探测：没有这个工具的解释器对 preview 同样没用，要跳过
    let python = pick_interpreter(&app, &script_str)
        .ok_or_else(|| "no beets interpreter available".to_string())?;
    let home = beets_paths(&app).home;
    let tool = StdioTool::new(launcher_for(&python, &script_str, Some(&home)));

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
    let script = ensure_script(&app)?;
    let script_str = script.to_string_lossy().into_owned();
    let python = pick_interpreter(&app, &script_str)
        .ok_or_else(|| "no beets interpreter available".to_string())?;
    let home = beets_paths(&app).home;
    let tool = StdioTool::new(launcher_for(&python, &script_str, Some(&home)));

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
    let script = ensure_script(&app)?;
    let script_str = script.to_string_lossy().into_owned();
    let python = pick_interpreter(&app, &script_str)
        .ok_or_else(|| "no beets interpreter available".to_string())?;
    // move 的目标根只认配置里显式写的 directory（工具侧有硬校验），
    // 确保它存在后再起会话；这里失败不算致命——真缺的话对面会给出
    // "没有 directory"的明确错误
    if let Err(e) = ensure_config(&app) {
        tracing::warn!(error = %e, "维护任务前生成 beets config 失败");
    }
    let home = beets_paths(&app).home;
    let tool = StdioTool::new(launcher_for(&python, &script_str, Some(&home)));

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

/// beets Web 外链（docs/29 §12 拍板 6）：高级用户出口，应用内不做 GUI。
///
/// 复用 probe 找到的解释器起 `python -m beets web`（**分离进程**：不进会话
/// 通道、stdout/stderr 丢弃，关掉应用也不影响 web 服务——它就是用户自己的
/// beets 环境）；1.5s 后用系统默认浏览器打开 `http://localhost:8337`
/// （beets web 默认端口）。返回打开的 URL。
///
/// 端口被占/插件配置报错属于用户 beets 环境问题：进程 spawn 成功就算成功，
/// 浏览器里看不到服务是用户侧排障（页面有 beets 状态块可对照）。
pub fn web_open(app: &tauri::AppHandle) -> Result<String, String> {
    const WEB_URL: &str = "http://localhost:8337";
    let interpreter = probe(app)
        .interpreter
        .ok_or_else(|| "未找到 beets（页面里看安装指引）".to_string())?;
    // 必须带同一套 env：不设 BEETSDIR 的话 `beet web` 会去 %APPDATA%\beets
    // 找库，而 sidecar 用的是 {app_data}/beets —— 浏览器里看到的是一份空库，
    // 用户只会以为"同步坏了"。
    let home = beets_paths(app).home;
    std::process::Command::new(&interpreter)
        .args(["-m", "beets", "web"])
        .envs(sidecar_env(Some(&home), &interpreter))
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| format!("启动 beets web 失败：{e}"))?;
    // 给 web 服务一点启动时间再拉浏览器；失败静默——用户可以自己开 URL
    std::thread::Builder::new()
        .name("beets-web-open".into())
        .spawn(move || {
            std::thread::sleep(Duration::from_millis(1500));
            let _ = std::process::Command::new("explorer").arg(WEB_URL).spawn();
        })
        .map_err(|e| format!("起浏览器线程失败：{e}"))?;
    Ok(WEB_URL.to_string())
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
fn collect_items(
    python: &str,
    script: &std::path::Path,
    beets_home: Option<&std::path::Path>,
) -> Result<Vec<aiya_data::beets_sync::BeetsItem>, String> {
    let launcher = launcher_for(python, &script.to_string_lossy(), beets_home);
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

    let script = match ensure_script(app) {
        Ok(p) => p,
        Err(e) => {
            emit(serde_json::json!({ "type": "librarySyncFailed", "message": e }));
            return;
        }
    };
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
        let script_str = script.to_string_lossy().into_owned();
        let home = beets_paths(app).home;
        let python = pick_interpreter(app, &script_str)
            .ok_or_else(|| "no beets interpreter available".to_string())?;
        let items = collect_items(&python, &script, Some(&home))?;
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
            "found": true, "interpreter": "py", "scriptPath": "/s.py",
            "protocolVersion": 1, "beetsVersion": "2.14.1", "pythonVersion": "3.14.7",
            "libraryPath": "l.db", "libraryExists": true,
            "plugins": ["musicbrainz"], "configDir": "c",
            "errorCode": null, "errorMessage": null
        });
        let st: BeetsStatus = serde_json::from_value(v).expect("camelCase 形状必须可解析");
        assert_eq!(st.beets_version.as_deref(), Some("2.14.1"));
        assert_eq!(st.library_exists, Some(true));
        assert_eq!(st.plugins, Some(vec!["musicbrainz".to_string()]));
    }

    /// dev venv 存在时（本仓库），probe 能找到 beets 2.14.x。
    #[cfg(debug_assertions)]
    #[test]
    fn probe_finds_dev_venv_beets() {
        let venv = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tools/beets-venv/Scripts/python.exe");
        if !venv.exists() {
            return; // CI/其他机器没有 venv 就跳过
        }
        let status = probe_with(&venv.to_string_lossy());
        assert!(status.found, "dev venv 应命中 beets: {:?}", status.error_message);
        assert!(status.beets_version.as_deref().unwrap_or_default().starts_with("2.14"));
        assert!(status.plugins.is_some());
    }

    /// probe 的可注入版本：probe() 就是用候选列表循环 probe_with。
    /// `beets_home = None`：测试走 env 里的 AIYA_BEETS_DIR / beets 默认位置，
    /// 不碰应用的 app_data。
    fn probe_with(python: &str) -> BeetsStatus {
        // 复用 query_sidecar 的完整逻辑，脚本用仓库源（不依赖 app_data）
        match query_sidecar(python, &script_path_for_test(), Duration::from_secs(10), None) {
            Ok(s) => s,
            Err(e) => BeetsStatus {
                found: false,
                interpreter: Some(python.into()),
                script_path: None,
                protocol_version: None,
                beets_version: None,
                python_version: None,
                library_path: None,
                library_exists: None,
                library_directory: None,
                plugins: None,
                config_dir: None,
                error_code: Some("E_SPAWN".into()),
                error_message: Some(e),
            },
        }
    }

    /// 默认 config.yaml 的正文：首发档插件、library/plugins 必备，directory 按需。
    ///
    /// 这三条都是"错了不会报错、只会静默不工作"的地方：少写 plugins 等于插件全关，
    /// 写成双引号路径会被 YAML 当转义序列吃掉，多写一个猜出来的 directory 会把
    /// 用户的文件 move 到他没预期的地方。
    #[test]
    fn render_config_covers_plugins_and_paths() {
        let home = PathBuf::from(r"D:\appdata\beets");
        let text = render_config(&home, None);
        assert!(text.contains("plugins: replaygain"), "首发档插件必须写上：{text}");
        assert!(text.contains("fromfilename"));
        assert!(text.contains("duplicates"));
        assert!(text.contains("scrub"));
        assert!(text.contains("smartplaylist"));
        // 单引号包住 Windows 路径，反斜杠才是字面量（双引号里 \a 会被当转义）
        assert!(
            text.contains(r"library: 'D:\appdata\beets\library.db'"),
            "路径必须单引号包裹：{text}"
        );
        assert!(!text.contains("directory:"), "没有扫描根时不得猜一个 directory");

        let with_root = render_config(&home, Some(r"D:\CkMsr\Music"));
        assert!(
            with_root.contains(r"directory: 'D:\CkMsr\Music'"),
            "有扫描根时必须写进 directory：{with_root}"
        );
    }

    /// 随包运行时（embeddable Python + site-packages）存在时，probe 能命中 beets。
    ///
    /// 这条守的是批 C 的核心前提：随包运行时**不需要系统装 Python**。
    /// 构建产物由 `node tools/pack-beets-runtime.mjs` 生成，没生成就跳过
    /// （CI 上默认不跑这条）。
    #[cfg(debug_assertions)]
    #[test]
    fn probe_finds_bundled_runtime_beets() {
        let py = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tools/beets-runtime/python.exe");
        if !py.exists() {
            return; // 没构建运行时（clean checkout / CI）就跳过
        }
        let status = probe_with(&py.to_string_lossy());
        assert!(status.found, "随包运行时应命中 beets: {:?}", status.error_message);
        assert!(status.beets_version.as_deref().unwrap_or_default().starts_with("2.14"));
    }

    /// 端到端：真 sidecar dump_items（dev venv + dev 隔离库，B2 冒烟留下的 3 条）
    /// → Rust 解析 → beets_sync 对齐进临时 DB。
    /// 不复用 collect_items（那要 AppHandle 落脚本 + set_var 污染并行测试），
    /// 事件形状按同一契约独立构造。
    #[cfg(debug_assertions)]
    #[test]
    fn dump_parse_and_sync_end_to_end() {
        let venv = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../tools/beets-venv/Scripts/python.exe");
        let beets_home = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tools/beets-home");
        if !venv.exists() || !beets_home.exists() {
            return; // CI/其他机器没有 dev 环境就跳过
        }
        let script = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../sidecar/beets_sidecar.py");

        let mut child = std::process::Command::new(&venv)
            .arg(&script)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            // 与生产路径同一套环境（编码钉 UTF-8），再覆盖隔离库
            .envs(sidecar_env(None, &venv.to_string_lossy()))
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

    fn script_path_for_test() -> String {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../sidecar/beets_sidecar.py")
            .to_string_lossy()
            .into_owned()
    }
}
