// 扫描管线（server 形态）：一次性/定时增量扫描 + 取消 + 进度事件 + 诊断快照。
// 桌面壳 refresh.rs 的窗口聚焦/唤醒机制在这里没有对应物——浏览器端"焦点"
// 语义不同，定时器恒跑（增量扫描对 unchanged 库是廉价的）。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use aiya_dto::RefreshSnapshot;

use crate::state::{db_lock, read_paused_roots, read_roots, ServerState};

pub const DEFAULT_INTERVAL_SECS: u64 = 30;

pub fn root_key(path: &str) -> std::path::PathBuf {
    #[cfg(windows)]
    let path = path.replace('/', "\\").to_lowercase();
    std::path::PathBuf::from(path)
}

fn active_roots(roots: Vec<String>, paused: Vec<String>) -> Vec<std::path::PathBuf> {
    let paused: Vec<_> = paused.iter().map(|p| root_key(p)).collect();
    roots
        .into_iter()
        .filter(|r| !paused.iter().any(|p| root_key(r).starts_with(p)))
        .map(std::path::PathBuf::from)
        .collect()
}

fn scan_roots(
    st: &ServerState,
    roots: &[std::path::PathBuf],
    report_progress: bool,
) -> aiya_data::DbResult<aiya_data::scanner::ScanResult> {
    let mut conn = aiya_data::open_conn(&st.db_path)?;
    aiya_data::scanner::scan_roots_ext(
        &mut conn,
        roots,
        Some(&st.artwork_dir),
        || st.scan.cancel.load(Ordering::SeqCst),
        |p| {
            if report_progress {
                st.events.emit(
                    "scan:progress",
                    &serde_json::to_value(p).unwrap_or_default(),
                );
            }
        },
    )
}

pub struct ScanCtl {
    pub scanning: AtomicBool,
    pub cancel: AtomicBool,
    pub last: Mutex<Option<RefreshSnapshot>>,
}

impl ScanCtl {
    pub fn new() -> Self {
        Self {
            scanning: AtomicBool::new(false),
            cancel: AtomicBool::new(false),
            last: Mutex::new(None),
        }
    }
}

/// 起一次后台扫描（timer / action）。已在扫描中则跳过（与桌面管线的合并语义
/// 一致：正在跑的增量扫描会看到最新的盘面，不需要排队第二次）。
pub fn spawn_scan(st: &Arc<ServerState>, trigger: &'static str) {
    let Some(guard) = ScanFlagGuard::acquire(st) else {
        return;
    };
    st.scan.cancel.store(false, Ordering::SeqCst);
    let st = Arc::clone(st);
    let trigger = trigger.to_string();
    std::thread::Builder::new()
        .name("aiya-server-scan".into())
        .spawn(move || {
            // guard 进闭包：run_scan 的任何早退/panic 都经 Drop 复位；
            // spawn 本身失败时闭包被丢弃，Drop 同样触发
            let _guard = guard;
            run_scan(&st, &trigger);
        })
        .expect("spawn scan thread");
}

/// 扫描标志守卫（桌面壳 refresh.rs 的 FlagGuard 同型）：构造即置位、Drop
/// 复位。审查复盘：run_scan 的早退分支（DB 读失败/根集为空）曾只 finish 不
/// 复位——无根 server 的第一个 timer tick 就能把标志永久卡死（所有后续
/// 扫描静默跳过、library_scan 恒 busy），这类路径必须共用同一套兜底。
struct ScanFlagGuard {
    st: Arc<ServerState>,
}

impl ScanFlagGuard {
    /// 置位成功返回守卫；已在扫描中返回 None（调用方跳过本轮）。
    fn acquire(st: &Arc<ServerState>) -> Option<Self> {
        if st.scan.scanning.swap(true, Ordering::SeqCst) {
            return None;
        }
        Some(Self { st: Arc::clone(st) })
    }
}

impl Drop for ScanFlagGuard {
    fn drop(&mut self) {
        self.st.scan.scanning.store(false, Ordering::SeqCst);
    }
}

/// 定时器线程：按 refreshSeconds 间隔自动增量扫描（0 = 只保留手动/action 触发）。
pub fn spawn_timer(st: Arc<ServerState>) {
    std::thread::Builder::new()
        .name("aiya-server-refresh-timer".into())
        .spawn(move || loop {
            let secs = {
                let v = crate::state::settings_lock(&st)
                    .get("refreshSeconds")
                    .and_then(serde_json::Value::as_u64);
                match v {
                    Some(0) => 0,
                    Some(n) => n.clamp(10, 3600),
                    None => DEFAULT_INTERVAL_SECS,
                }
            };
            if secs == 0 {
                std::thread::sleep(std::time::Duration::from_secs(DEFAULT_INTERVAL_SECS));
                continue;
            }
            std::thread::sleep(std::time::Duration::from_secs(secs));
            spawn_scan(&st, "timer");
        })
        .expect("spawn refresh timer");
}

fn run_scan(st: &Arc<ServerState>, trigger: &str) {
    let started = Instant::now();
    let roots: Vec<std::path::PathBuf> = match db_lock(st)
        .send(move |conn| Ok((read_roots(conn), read_paused_roots(conn))))
        .map_err(|e| e.to_string())
    {
        Ok((roots, paused)) => active_roots(roots, paused),
        Err(e) => {
            finish(st, trigger, RefreshSnapshot::skipped(trigger, 0, Some(e)));
            return;
        }
    };
    if roots.is_empty() {
        finish(st, trigger, RefreshSnapshot::skipped(trigger, 0, None));
        return;
    }

    let result = scan_roots(st, &roots, trigger == "action");
    let elapsed = started.elapsed().as_millis() as u64;
    match result {
        Ok(r) => {
            finish(st, trigger, snapshot(trigger, &r));
        }
        Err(e) => finish(
            st,
            trigger,
            RefreshSnapshot::skipped(trigger, elapsed, Some(e.to_string())),
        ),
    }
    // 标志复位由闭包里的 ScanFlagGuard Drop 负责（早退/panic 全路径兜底），
    // 这里不得手动 store(false)：guard 仍存活时误复位会放行下一个扫描、
    // 随后 guard Drop 又把它误杀
}

fn snapshot(trigger: &str, result: &aiya_data::scanner::ScanResult) -> RefreshSnapshot {
    RefreshSnapshot {
        trigger: trigger.into(),
        at_ms: now_ms(),
        changed: result.imported > 0 || result.updated > 0 || result.missing > 0,
        scanned: result.scanned,
        imported: result.imported,
        updated: result.updated,
        missing: result.missing,
        failed: result.failed,
        elapsed_ms: result.elapsed_ms,
        error: None,
    }
}

fn finish(st: &Arc<ServerState>, trigger: &str, snap: RefreshSnapshot) {
    tracing::info!(
        trigger,
        scanned = snap.scanned,
        imported = snap.imported,
        missing = snap.missing,
        elapsed = snap.elapsed_ms,
        "scan finished"
    );
    *st.scan
        .last
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(snap.clone());
    if let Some(error) = &snap.error {
        st.events.emit(
            "beets:import",
            &serde_json::json!({"type":"libraryRefreshFailed","message":error}),
        );
        if trigger == "action" {
            st.events.emit(
                "scan:progress",
                &serde_json::json!({"phase":"failed","message":error}),
            );
        }
    } else if snap.changed && trigger != "action" {
        let mut event = serde_json::to_value(&snap).unwrap_or_default();
        event["type"] = "libraryRefreshed".into();
        st.events.emit("beets:import", &event);
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// 同步扫描（library_scan 命令体）：自持 busy 闸（已在扫描中返回 busy 错误），
/// 复位交给 ScanFlagGuard Drop；返回完整结果（进度事件与 spawn_scan 共用一条路径）。
pub fn run_scan_inline(
    st: &Arc<ServerState>,
    trigger: &'static str,
) -> Result<aiya_data::scanner::ScanResult, String> {
    let started = Instant::now();
    let _guard =
        ScanFlagGuard::acquire(st).ok_or_else(|| "a scan is already in progress".to_string())?;
    st.scan.cancel.store(false, Ordering::SeqCst);
    let roots: Vec<std::path::PathBuf> = match db_lock(st)
        .send(|conn| Ok((read_roots(conn), read_paused_roots(conn))))
        .map_err(|e| e.to_string())
    {
        Ok((roots, paused)) => active_roots(roots, paused),
        Err(e) => {
            finish(
                st,
                trigger,
                RefreshSnapshot::skipped(trigger, 0, Some(e.clone())),
            );
            return Err(e);
        }
    };
    if roots.is_empty() {
        let error = "no scan folders configured".to_string();
        finish(
            st,
            trigger,
            RefreshSnapshot::skipped(trigger, 0, Some(error.clone())),
        );
        return Err(error);
    }
    let result = scan_roots(st, &roots, true);
    match &result {
        Ok(result) => finish(st, trigger, snapshot(trigger, result)),
        Err(error) => finish(
            st,
            trigger,
            RefreshSnapshot::skipped(
                trigger,
                started.elapsed().as_millis() as u64,
                Some(error.to_string()),
            ),
        ),
    }
    result.map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paused_roots_compare_path_components() {
        let result = active_roots(
            vec![
                "/music/A".into(),
                "/music/A/disc".into(),
                "/music/Albums".into(),
            ],
            vec!["/music/A".into()],
        );
        assert_eq!(result, vec![std::path::PathBuf::from("/music/Albums")]);
    }

    #[cfg(not(windows))]
    #[test]
    fn linux_roots_keep_case_and_backslashes() {
        let result = active_roots(
            vec![
                "/music/a".into(),
                "/music/A".into(),
                "/music/A\\disc".into(),
            ],
            vec!["/music/A".into()],
        );
        assert_eq!(
            result,
            vec![
                std::path::PathBuf::from("/music/a"),
                std::path::PathBuf::from("/music/A\\disc")
            ]
        );
    }

    #[test]
    fn scan_uses_its_own_connection_without_waiting_on_the_db_worker() {
        let dir = tempfile::tempdir().unwrap();
        let music = dir.path().join("music");
        std::fs::create_dir(&music).unwrap();
        let st = crate::state::test_state(&dir.path().join("library.sqlite"));
        let _locked = db_lock(&st);
        let result = scan_roots(&st, &[music], true).unwrap();
        assert_eq!(result.scanned, 0);
    }

    #[test]
    fn automatic_scan_is_silent_and_manual_completion_has_a_phase() {
        let dir = tempfile::tempdir().unwrap();
        let music = dir.path().join("music");
        std::fs::create_dir(&music).unwrap();
        let st = crate::state::test_state(&dir.path().join("library.sqlite"));
        db_lock(&st)
            .send(move |conn| {
                crate::state::setting_set(conn, "library.roots", &serde_json::json!([music]))?;
                Ok(())
            })
            .unwrap();
        let mut events = st.events.subscribe();
        run_scan(&st, "timer");
        assert!(events.try_recv().is_err());
        assert_eq!(
            st.scan.last.lock().unwrap().as_ref().unwrap().trigger,
            "timer"
        );
        run_scan_inline(&st, "action").unwrap();
        let mut phases = Vec::new();
        while let Ok((channel, data)) = events.try_recv() {
            assert_eq!(channel, "scan:progress");
            let event: serde_json::Value = serde_json::from_str(&data).unwrap();
            phases.push(event["phase"].as_str().unwrap().to_owned());
        }
        assert_eq!(phases.last().map(String::as_str), Some("done"));
        assert_eq!(
            st.scan.last.lock().unwrap().as_ref().unwrap().trigger,
            "action"
        );
        assert!(!st.scan.scanning.load(Ordering::SeqCst));
    }

    #[test]
    fn automatic_changes_and_errors_use_refresh_events() {
        let dir = tempfile::tempdir().unwrap();
        let st = crate::state::test_state(&dir.path().join("library.sqlite"));
        let mut events = st.events.subscribe();
        let mut changed = RefreshSnapshot::skipped("timer", 10, None);
        changed.changed = true;
        changed.imported = 1;
        finish(&st, "timer", changed);
        let (channel, data) = events.try_recv().unwrap();
        assert_eq!(channel, "beets:import");
        let event: serde_json::Value = serde_json::from_str(&data).unwrap();
        assert_eq!(event["type"], "libraryRefreshed");
        finish(
            &st,
            "timer",
            RefreshSnapshot::skipped("timer", 10, Some("db error".into())),
        );
        let (channel, data) = events.try_recv().unwrap();
        assert_eq!(channel, "beets:import");
        let event: serde_json::Value = serde_json::from_str(&data).unwrap();
        assert_eq!(event["type"], "libraryRefreshFailed");
        assert!(events.try_recv().is_err());
    }

    #[test]
    fn failed_scan_releases_busy_and_does_not_reset_an_existing_cancel() {
        let dir = tempfile::tempdir().unwrap();
        let st = crate::state::test_state(&dir.path().join("library.sqlite"));
        assert!(run_scan_inline(&st, "action").is_err());
        assert!(!st.scan.scanning.load(Ordering::SeqCst));
        let _guard = ScanFlagGuard::acquire(&st).unwrap();
        st.scan.cancel.store(true, Ordering::SeqCst);
        assert!(run_scan_inline(&st, "action")
            .unwrap_err()
            .contains("already"));
        assert!(st.scan.cancel.load(Ordering::SeqCst));
    }
}
