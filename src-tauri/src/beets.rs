//! beets sidecar 宿主侧：解释器探测 + status 查询（docs/08-beets.md B0）。
//!
//! 探测顺序：AIYA_BEETS_PYTHON env → dev venv（仅 debug 构建）→ 系统 python。
//! sidecar 脚本 include_str! 进二进制，写到 app_data/sidecar/（dev/release 行为一致）。
//! v1 不捆绑 Python/beets：没装就返回 not_found，设置页给安装指引，播放功能不受影响。

use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::{Emitter, Manager};

/// 设置页 beets 状态区数据源。serde 进 IPC：camelCase 契约（铁律）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BeetsStatus {
    pub found: bool,
    pub interpreter: Option<String>,
    pub script_path: Option<String>,
    pub protocol_version: Option<u32>,
    pub beets_version: Option<String>,
    pub python_version: Option<String>,
    pub library_path: Option<String>,
    pub library_exists: Option<bool>,
    pub plugins: Option<Vec<String>>,
    pub config_dir: Option<String>,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
}

const SIDECAR_SCRIPT: &str = include_str!("../../sidecar/beets_sidecar.py");

/// sidecar 脚本落盘位置：{app_data}/sidecar/beets_sidecar.py。
/// 内容与内嵌不同才重写（升级友好）。
fn ensure_script(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("app_data_dir: {e}"))?
        .join("sidecar");
    std::fs::create_dir_all(&dir).map_err(|e| format!("create_dir: {e}"))?;
    let path = dir.join("beets_sidecar.py");
    let mismatch = std::fs::read_to_string(&path).map(|old| old != SIDECAR_SCRIPT).unwrap_or(true);
    if mismatch {
        std::fs::write(&path, SIDECAR_SCRIPT).map_err(|e| format!("write sidecar: {e}"))?;
    }
    Ok(path)
}

/// 候选解释器列表（按优先级）。
fn interpreter_candidates() -> Vec<String> {
    let mut out = Vec::new();
    if let Ok(p) = std::env::var("AIYA_BEETS_PYTHON") {
        out.push(p);
    }
    // dev venv：release 构建不存在，自然跳过
    let dev_venv = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../tools/beets-venv/Scripts/python.exe");
    out.push(dev_venv.to_string_lossy().into_owned());
    out.push("python".into());
    out
}

/// 向解释器发一条 status/ping 请求并等响应（行帧 JSON，超时保护）。
fn query_sidecar(
    python: &str,
    script: &str,
    timeout: Duration,
) -> Result<BeetsStatus, String> {
    let mut child = std::process::Command::new(python)
        .arg(script)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("spawn {python}: {e}"))?;

    // 写请求（drop stdin 让读循环可以结束——本命令是一次性查询）
    if let Some(mut stdin) = child.stdin.take() {
        use std::io::Write;
        let _ = writeln!(stdin, r#"{{"id":1,"cmd":"status"}}"#);
    }

    // 读响应：单独线程 + 超时，避免 beets 卡住拖死宿主
    let (tx, rx) = std::sync::mpsc::channel();
    let mut stdout = child.stdout.take().expect("stdout piped");
    std::thread::spawn(move || {
        use std::io::BufRead;
        let reader = std::io::BufReader::new(&mut stdout);
        for line in reader.lines() {
            let _ = tx.send(line.unwrap_or_default());
            break; // status 是单问单答
        }
    });
    let line = rx
        .recv_timeout(timeout)
        .map_err(|_| format!("sidecar 响应超时（{timeout:.0?}）"))?;
    let _ = child.kill();
    let _ = child.wait();

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Envelope {
        ok: bool,
        data: Option<serde_json::Value>,
        error: Option<SidecarError>,
    }
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct SidecarError {
        code: String,
        stage: String,
        beets_message: String,
    }

    let env: Envelope =
        serde_json::from_str(line.trim()).map_err(|e| format!("解析 sidecar 响应: {e}"))?;
    if !env.ok {
        let err = env.error.unwrap_or(SidecarError {
            code: "E_UNKNOWN".into(),
            stage: "unknown".into(),
            beets_message: "no error detail".into(),
        });
        return Ok(BeetsStatus {
            found: false,
            interpreter: Some(python.into()),
            script_path: Some(script.into()),
            protocol_version: None,
            beets_version: None,
            python_version: None,
            library_path: None,
            library_exists: None,
            plugins: None,
            config_dir: None,
            error_code: Some(err.code),
            error_message: Some(format!("[{}] {}", err.stage, err.beets_message)),
        });
    }
    let data = env.data.unwrap_or(serde_json::Value::Null);
    Ok(BeetsStatus {
        found: true,
        interpreter: Some(python.into()),
        script_path: Some(script.into()),
        protocol_version: data.get("protocolVersion").and_then(|v| v.as_u64()).map(|v| v as u32),
        beets_version: data.get("beetsVersion").and_then(|v| v.as_str()).map(Into::into),
        python_version: data.get("pythonVersion").and_then(|v| v.as_str()).map(Into::into),
        library_path: data.get("libraryPath").and_then(|v| v.as_str()).map(Into::into),
        library_exists: data.get("libraryExists").and_then(|v| v.as_bool()),
        plugins: data
            .get("plugins")
            .and_then(|v| v.as_array())
            .map(|a| a.iter().filter_map(|x| x.as_str().map(Into::into)).collect()),
        config_dir: data.get("configDir").and_then(|v| v.as_str()).map(Into::into),
        error_code: None,
        error_message: None,
    })
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
                plugins: None,
                config_dir: None,
                error_code: Some("E_SCRIPT".into()),
                error_message: Some(e),
            };
        }
    };
    let script_str = script.to_string_lossy().into_owned();
    let mut last_err: Option<String> = None;
    for python in interpreter_candidates() {
        match query_sidecar(&python, &script_str, Duration::from_secs(10)) {
            Ok(status) => {
                if status.found {
                    return status;
                }
                // 解释器可用但没装 beets：继续试下一个候选（dev venv 优先命中）
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
        plugins: None,
        config_dir: None,
        error_code: Some("E_BEETS_NOT_FOUND".into()),
        error_message: last_err,
    }
}

/* ---------------- B1/B2：导入会话（事件流转发） ---------------- */

/// apply 完成后宿主刷新读模型所需参数（lib.rs 注入 AppState 的克隆）。
pub struct RefreshParams {
    pub db_path: PathBuf,
    pub artwork_dir: PathBuf,
    /// 与 library_scan 共用的互斥标志（避免同时扫描写同一 DB）
    pub scanning: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

/// 第一个能答 status 且装了 beets 的解释器（预览/应用共用）。
fn pick_interpreter(script: &str) -> Option<String> {
    for cand in interpreter_candidates() {
        if let Ok(s) = query_sidecar(&cand, script, Duration::from_secs(10)) {
            if s.found {
                return Some(cand);
            }
        }
    }
    None
}

/// 起一次性 sidecar 子进程并写入请求（不 drop stdin 之前先写完）。
fn spawn_session(
    python: &str,
    script: &PathBuf,
    request: &serde_json::Value,
) -> Result<std::process::Child, String> {
    let mut child = std::process::Command::new(python)
        .arg(script)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        // AIYA_BEETS_DIR 设置时映射成 BEETSDIR（dev 隔离库）；否则用用户真实环境
        .envs(
            std::env::var("AIYA_BEETS_DIR")
                .ok()
                .map(|d| ("BEETSDIR".to_string(), d))
                .into_iter(),
        )
        .spawn()
        .map_err(|e| format!("spawn beets sidecar: {e}"))?;

    use std::io::Write;
    let mut stdin = child.stdin.take().expect("stdin piped");
    writeln!(stdin, "{request}").map_err(|e| format!("写请求: {e}"))?;
    // 不留 stdin 挂着：sidecar 处理完请求后事件流结束、在下一行 read 上阻塞，
    // 显式 drop 让其 stdin EOF 后循环结束退出
    drop(stdin);
    Ok(child)
}

/// 转发 stdout 行事件到 `beets:import`，进程退出后收尾。
/// 返回 reader 线程句柄（start_preview / start_apply 共用骨架）。
fn spawn_reader(
    mut child: std::process::Child,
    app: tauri::AppHandle,
    on_exit: impl FnOnce(&tauri::AppHandle) + Send + 'static,
) -> Result<(), String> {
    let mut stdout = child.stdout.take().expect("stdout piped");
    std::thread::Builder::new()
        .name("beets-session-reader".into())
        .spawn(move || {
            use std::io::BufRead;
            let reader = std::io::BufReader::new(&mut stdout);
            for line in reader.lines() {
                let Ok(line) = line else { break };
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }
                match serde_json::from_str::<serde_json::Value>(trimmed) {
                    Ok(evt) => {
                        let _ = app.emit("beets:import", evt);
                    }
                    Err(e) => {
                        tracing::warn!(%e, %trimmed, "beets sidecar 非法事件行，忽略");
                    }
                }
            }
            // stdin 已关，sidecar 处理完请求后 EOF 退出；收尸防僵尸
            let _ = child.wait();
            on_exit(&app);
            let _ = app.emit("beets:import", serde_json::json!({ "type": "sessionEnd" }));
        })
        .map_err(|e| format!("起 reader 线程: {e}"))?;
    Ok(())
}

/// 启动一次 preview_import：一次性子进程，stdout 每行 = 一个事件，
/// 逐行转发到 Tauri 事件 `beets:import`。完成（进程退出）后自动回收。
///
/// 低频操作，不值得养常驻会话；每命令 ~300ms 的 Python 启动开销可接受。
/// 返回 ok=false 表示没找到可用解释器（UI 显示指引）。
pub fn start_preview(app: tauri::AppHandle, paths: Vec<String>) -> Result<(), String> {
    let script = ensure_script(&app)?;
    let script_str = script.to_string_lossy().into_owned();

    // 快速探测：无 beets 的解释器对 preview 同样没用，要跳过
    let python = pick_interpreter(&script_str)
        .ok_or_else(|| "no beets interpreter available".to_string())?;

    let req = serde_json::json!({ "id": 1, "cmd": "preview_import", "paths": paths });
    let child = spawn_session(&python, &script, &req)?;
    spawn_reader(child, app, |_| {})
}

/// B2：应用导入决策。decisions 形状由契约定义（ApplyDecision），
/// 宿主不解释语义，只透传给 sidecar；完成后刷新受影响目录的读模型。
pub fn start_apply(
    app: tauri::AppHandle,
    decisions: Vec<serde_json::Value>,
    refresh: Option<RefreshParams>,
) -> Result<(), String> {
    let script = ensure_script(&app)?;
    let script_str = script.to_string_lossy().into_owned();
    let python = pick_interpreter(&script_str)
        .ok_or_else(|| "no beets interpreter available".to_string())?;

    let req = serde_json::json!({ "id": 1, "cmd": "apply_import", "decisions": decisions });
    let child = spawn_session(&python, &script, &req)?;

    let refresh_dirs = affected_dirs(&decisions);
    spawn_reader(child, app, move |app| {
        if let Some(params) = refresh {
            refresh_affected(app, params, &refresh_dirs);
        }
    })
}

/// 决策里 apply 动作的文件 → 去重后的父目录（刷新只扫这些目录，不全库重扫）。
fn affected_dirs(decisions: &[serde_json::Value]) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    for d in decisions {
        if d.get("action").and_then(|a| a.as_str()) != Some("apply") {
            continue;
        }
        if let Some(paths) = d.get("paths").and_then(|p| p.as_array()) {
            for p in paths {
                if let Some(s) = p.as_str() {
                    let dir = PathBuf::from(s);
                    let dir = if dir.is_dir() { dir } else { match dir.parent() {
                        Some(parent) => parent.to_path_buf(),
                        None => continue,
                    } };
                    if !out.contains(&dir) {
                        out.push(dir);
                    }
                }
            }
        }
    }
    out
}

/// apply 完成后的读模型刷新：对受影响目录逐个 scan_directory（增量，快），
/// 进度走既有 `scan:progress` 事件，汇总走 `libraryRefreshed`。
fn refresh_affected(app: &tauri::AppHandle, params: RefreshParams, dirs: &[PathBuf]) {
    use std::sync::atomic::Ordering;
    let emit = |v: serde_json::Value| {
        let _ = app.emit("beets:import", v);
    };
    // 与 library_scan 互斥；正扫全库时本次刷新让位（UI 可手动再扫）
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

    let mut scanned = 0usize;
    let mut imported = 0usize;
    let mut updated = 0usize;
    let mut unchanged = 0usize;
    let mut failed = 0usize;
    for dir in dirs {
        if !dir.is_dir() {
            continue;
        }
        let Ok(mut conn) = aiya_data::open_conn(&params.db_path) else {
            failed += 1;
            continue;
        };
        let app2 = app.clone();
        match aiya_data::scanner::scan_directory(&mut conn, dir, Some(&params.artwork_dir), move |p| {
            if let Ok(v) = serde_json::to_value(&p) {
                let _ = app2.emit("scan:progress", v);
            }
        }) {
            Ok(r) => {
                scanned += r.scanned;
                imported += r.imported;
                updated += r.updated;
                unchanged += r.unchanged;
                failed += r.failed;
            }
            Err(_) => failed += 1,
        }
        let _ = conn.close();
    }
    params.scanning.store(false, Ordering::SeqCst);
    emit(serde_json::json!({
        "type": "libraryRefreshed",
        "scanned": scanned,
        "imported": imported,
        "updated": updated,
        "unchanged": unchanged,
        "failed": failed,
    }));
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
    fn probe_with(python: &str) -> BeetsStatus {
        // 复用 query_sidecar 的完整逻辑，脚本用仓库源（不依赖 app_data）
        match query_sidecar(python, &script_path_for_test(), Duration::from_secs(10)) {
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
                plugins: None,
                config_dir: None,
                error_code: Some("E_SPAWN".into()),
                error_message: Some(e),
            },
        }
    }

    /// 决策里 apply 动作的文件 → 去重后的父目录；skip 动作不参与刷新。
    #[test]
    fn affected_dirs_dedupes_and_skips() {
        let decisions = serde_json::json!([
            { "album": "a", "action": "apply", "paths": ["C:/m/one/1.flac", "C:/m/one/2.flac"] },
            { "album": "b", "action": "skip", "paths": ["C:/m/two/1.mp3"] },
            { "album": "c", "action": "apply", "paths": ["C:/m/one/3.flac", "C:/m/two/x.wav"] }
        ]);
        let dirs = affected_dirs(decisions.as_array().unwrap());
        assert_eq!(dirs, vec![
            PathBuf::from("C:/m/one"),
            PathBuf::from("C:/m/two"),
        ]);
    }

    fn script_path_for_test() -> String {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../sidecar/beets_sidecar.py")
            .to_string_lossy()
            .into_owned()
    }
}
