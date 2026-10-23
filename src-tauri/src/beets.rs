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

    // 读响应：单独线程 + 超时，避免 beets 卡住拖死宿主。
    // 容错非 JSON 行（beets 2.14 坑⑤：新库首写会往 stdout 打迁移提示）：
    // 循环读到第一条可解析 JSON 行才算响应。
    let (tx, rx) = std::sync::mpsc::channel();
    let mut stdout = child.stdout.take().expect("stdout piped");
    std::thread::spawn(move || {
        use std::io::BufRead;
        let reader = std::io::BufReader::new(&mut stdout);
        for line in reader.lines() {
            let Ok(line) = line else { break };
            if serde_json::from_str::<serde_json::Value>(line.trim()).is_ok() {
                let _ = tx.send(line);
                break;
            }
        }
    });
    let line = match rx.recv_timeout(timeout) {
        Ok(l) => l,
        Err(_) => {
            // 超时收尸（P2：此前直接 return，卡死的 sidecar 进程泄漏）
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!("sidecar 响应超时（{timeout:.0?}）"));
        }
    };
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

/// 读模型刷新参数（lib.rs 注入 AppState 的克隆）。
pub struct RefreshParams {
    pub db_path: PathBuf,
    /// 与 library_scan 共用的互斥标志（避免同时写同一 DB）
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

/* ---------------- B3：会话槽 + 取消 ---------------- */

/// 活跃导入会话的 stdin（preview/apply 互斥 + cancel 写入口）。
///
/// 所有权流转：start_* 先占 session_flag 再 spawn → spawn 完成后 stdin 放入
/// 槽 → reader 见终止事件（done/applyDone/cancelled/sessionFailed）或进程
/// 退出时取走 drop → cancel 命令取走、写 cancel 行再 drop。任一时刻至多
/// 一个持有者。
type SessionStdin = std::process::ChildStdin;

fn session_slot() -> &'static std::sync::Mutex<Option<SessionStdin>> {
    static SLOT: std::sync::OnceLock<std::sync::Mutex<Option<SessionStdin>>> =
        std::sync::OnceLock::new();
    SLOT.get_or_init(|| std::sync::Mutex::new(None))
}

/// 会话占用标志（spawn 前置占位）。P1：此前槽位在 spawn **之后**才 acquire，
/// 并发 preview/apply 会双双 spawn sidecar——败者的请求行已进 sidecar 队列
/// 会照常执行（写标签/动 beets 库），且败者没有 reader → 事件不发、进程泄漏。
fn session_flag() -> &'static std::sync::atomic::AtomicBool {
    static FLAG: std::sync::OnceLock<std::sync::atomic::AtomicBool> =
        std::sync::OnceLock::new();
    FLAG.get_or_init(|| std::sync::atomic::AtomicBool::new(false))
}

fn reserve_session() -> bool {
    use std::sync::atomic::Ordering;
    session_flag()
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_ok()
}

/// 占用会话槽（stdin 保管）。flag 已由 reserve_session 占住，理论上不会失败。
fn acquire_session(stdin: SessionStdin) -> Result<(), String> {
    let mut guard = session_slot().lock().map_err(|_| "session slot poisoned")?;
    if guard.is_some() {
        return Err("已有 beets 导入任务在执行（可先取消）".into());
    }
    *guard = Some(stdin);
    Ok(())
}

/// 释放会话槽 + 占用标志（reader/cancel/失败路径调用）。
fn release_session() -> Option<SessionStdin> {
    use std::sync::atomic::Ordering;
    session_flag().store(false, Ordering::SeqCst);
    session_slot().lock().ok().and_then(|mut g| g.take())
}

/// 取消当前导入：写 cancel 行（sidecar 在下个检查点收尾并发 cancelled 事件）
/// 随后关闭 stdin（工作收尾后 EOF 退出）。无活跃会话返回 false。
pub fn cancel_session() -> bool {
    let Some(mut stdin) = release_session() else {
        return false;
    };
    {
        use std::io::Write;
        let _ = writeln!(stdin, r#"{{"id":0,"cmd":"cancel"}}"#);
    }
    drop(stdin); // EOF：sidecar 工作收尾后主循环退出
    true
}

/// 起一次性 sidecar 子进程并写入请求；stdin 交还调用方（保活以支持取消）。
fn spawn_session(
    python: &str,
    script: &std::path::Path,
    request: &serde_json::Value,
) -> Result<(std::process::Child, SessionStdin), String> {
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
    Ok((child, stdin))
}

/// 转发 stdout 行事件到 `beets:import`；见终止事件（done/applyDone/
/// cancelled/sessionFailed）后关 stdin 让 sidecar EOF 退出，进程退出后收尾清槽。
/// 内置空闲看门狗：10 分钟无任何 stdout 输出判卡死（匹配是网络操作，但正常
/// 路径每个专辑都有 progress 事件），kill 子进程并发 sessionFailed——
/// 此前导入会话无任何超时保护，卡死即槽位永久占死。
const SESSION_IDLE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(600);

fn spawn_reader(
    mut child: std::process::Child,
    app: tauri::AppHandle,
    on_exit: impl FnOnce(&tauri::AppHandle) + Send + 'static,
) -> Result<(), String> {
    let mut stdout = child.stdout.take().expect("stdout piped");
    // 管道线程：阻塞读原始行进 channel（超时判断在会话线程做）
    let (line_tx, line_rx) = std::sync::mpsc::channel::<String>();
    let pipe_thread = std::thread::Builder::new()
        .name("beets-pipe-recv".into())
        .spawn(move || {
            use std::io::BufRead;
            for line in std::io::BufReader::new(&mut stdout).lines() {
                match line {
                    Ok(l) => {
                        if line_tx.send(l).is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
        });
    if let Err(e) = pipe_thread {
        let _ = child.kill();
        let _ = child.wait();
        return Err(format!("起 pipe 线程: {e}"));
    }

    let session_thread = {
        // Arc<Mutex<Child>>：会话线程持有（kill/wait），外层失败路径也要收尸
        let child = std::sync::Arc::new(std::sync::Mutex::new(child));
        let child_for_fail = std::sync::Arc::clone(&child);
        let spawned = std::thread::Builder::new()
            .name("beets-session-reader".into())
            .spawn(move || {
                let mut idle = SESSION_IDLE_TIMEOUT;
                loop {
                    match line_rx.recv_timeout(idle) {
                        Ok(line) => {
                            idle = SESSION_IDLE_TIMEOUT; // 有输出就续命
                            let trimmed = line.trim();
                            if trimmed.is_empty() {
                                continue;
                            }
                            match serde_json::from_str::<serde_json::Value>(trimmed) {
                                Ok(evt) => {
                                    let terminal = evt.get("type").and_then(|t| t.as_str())
                                        .is_some_and(|t| matches!(
                                            t, "done" | "applyDone" | "cancelled" | "sessionFailed"));
                                    let _ = app.emit("beets:import", evt);
                                    if terminal {
                                        break;
                                    }
                                }
                                Err(e) => {
                                    tracing::warn!(%e, %trimmed, "beets sidecar 非法事件行，忽略");
                                }
                            }
                        }
                        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                            tracing::error!("beets 会话空闲超时（10 分钟无输出），看门狗终止");
                            {
                                let mut c = child.lock().unwrap();
                                let _ = c.kill();
                            }
                            let _ = app.emit(
                                "beets:import",
                                serde_json::json!({
                                    "type": "sessionFailed",
                                    "error": {
                                        "code": "E_SESSION_TIMEOUT", "stage": "watchdog",
                                        "beets_message": "会话 10 分钟无输出，已被看门狗终止",
                                    },
                                }),
                            );
                            break;
                        }
                        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break, // sidecar 退出
                    }
                }
                // 关 stdin → sidecar 主循环 EOF 退出；收尸防僵尸
                drop(release_session());
                {
                    let mut c = child.lock().unwrap();
                    let _ = c.wait();
                }
                on_exit(&app);
                let _ = app.emit("beets:import", serde_json::json!({ "type": "sessionEnd" }));
            });
        spawned.map_err(|e| {
            let mut c = child_for_fail.lock().unwrap();
            let _ = c.kill();
            let _ = c.wait();
            drop(release_session());
            format!("起 reader 线程: {e}")
        })
    };
    session_thread?;
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
    // P1：先占会话标志再 spawn，失败路径释放（并发请求第二个直接被拒，
    // 不会像以前那样 spawn 出第二个照常执行的 sidecar）
    if !reserve_session() {
        return Err("已有 beets 导入任务在执行（可先取消）".into());
    }
    let result = (|| {
        let (child, stdin) = spawn_session(&python, &script, &req)?;
        acquire_session(stdin)?;
        spawn_reader(child, app, |_| {})
    })();
    if result.is_err() {
        release_session();
    }
    result
}

/// B2：应用导入决策。decisions 形状由契约定义（ApplyDecision），
/// 宿主不解释语义，只透传给 sidecar；完成后自动 beets 对齐读模型（自闭环）。
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
    // P1：同 start_preview，先占位再 spawn
    if !reserve_session() {
        return Err("已有 beets 导入任务在执行（可先取消）".into());
    }
    let result = (|| {
        let (child, stdin) = spawn_session(&python, &script, &req)?;
        acquire_session(stdin)?;
        spawn_reader(child, app, move |app| {
            if let Some(params) = refresh {
                // 自闭环（docs/08 §5）：beets 刚写完库就是最新权威，
                // 直接全量对齐自有 DB，替代旧"重扫受影响目录"路径。
                run_sync(app, params);
            }
        })
    })();
    if result.is_err() {
        release_session();
    }
    result
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

/// 从 sidecar 收集 dump_items 全量（一次性进程，调用线程同步读）。
fn collect_items(
    python: &str,
    script: &std::path::Path,
) -> Result<Vec<aiya_data::beets_sync::BeetsItem>, String> {
    let (mut child, stdin) = spawn_session(
        python,
        script,
        &serde_json::json!({ "id": 1, "cmd": "dump_items" }),
    )?;
    // dump 是一次性收集：不留 stdin 挂着，sidecar 处理完 EOF 退出
    drop(stdin);
    let mut stdout = child.stdout.take().expect("stdout piped");
    let mut items = Vec::new();
    {
        use std::io::BufRead;
        let reader = std::io::BufReader::new(&mut stdout);
        for line in reader.lines() {
            let Ok(line) = line else { break };
            let Ok(v) = serde_json::from_str::<serde_json::Value>(line.trim()) else {
                continue;
            };
            if v.get("type").and_then(|t| t.as_str()) == Some("item") {
                if let Some(item) = v.get("item") {
                    match serde_json::from_value(item.clone()) {
                        Ok(it) => items.push(it),
                        Err(e) => tracing::warn!(%e, "beets item 解析失败，跳过"),
                    }
                }
            }
        }
    }
    let _ = child.wait();
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
        let python = pick_interpreter(&script_str)
            .ok_or_else(|| "no beets interpreter available".to_string())?;
        let items = collect_items(&python, &script)?;
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
