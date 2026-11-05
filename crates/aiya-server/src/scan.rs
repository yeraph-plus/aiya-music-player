// 扫描管线（server 形态）：一次性/定时增量扫描 + 取消 + 进度事件 + 诊断快照。
// 桌面壳 refresh.rs 的窗口聚焦/唤醒机制在这里没有对应物——浏览器端"焦点"
// 语义不同，定时器恒跑（增量扫描对 unchanged 库是廉价的）。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use aiya_dto::RefreshSnapshot;

use crate::state::{db_lock, read_paused_roots, read_roots, ServerState};

pub const DEFAULT_INTERVAL_SECS: u64 = 30;

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
    if st.scan.scanning.swap(true, Ordering::SeqCst) {
        return;
    }
    st.scan.cancel.store(false, Ordering::SeqCst);
    let st = Arc::clone(st);
    let trigger = trigger.to_string();
    std::thread::Builder::new()
        .name("aiya-server-scan".into())
        .spawn(move || run_scan(&st, &trigger))
        .expect("spawn scan thread");
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
    let artwork_dir = st.artwork_dir.clone();
    let roots: Vec<std::path::PathBuf> = match db_lock(st)
        .send(move |conn| Ok((read_roots(conn), read_paused_roots(conn), artwork_dir)))
        .map_err(|e| e.to_string())
    {
        Ok((roots, paused, _art)) => roots
            .into_iter()
            .map(std::path::PathBuf::from)
            .filter(|p| {
                let s = p.to_string_lossy();
                !paused
                    .iter()
                    .any(|p| s.to_lowercase().starts_with(&p.to_lowercase()))
            })
            .collect(),
        Err(e) => {
            finish(st, trigger, RefreshSnapshot::skipped(trigger, 0, Some(e)));
            return;
        }
    };
    if roots.is_empty() {
        finish(st, trigger, RefreshSnapshot::skipped(trigger, 0, None));
        return;
    }

    let art = st.artwork_dir.clone();
    let events = st.events.clone();
    let st2 = Arc::clone(st);
    let result = db_lock(st).send(move |conn| {
        aiya_data::scanner::scan_roots_ext(
            conn,
            &roots,
            Some(&art),
            || st2.scan.cancel.load(Ordering::SeqCst),
            |p| {
                // 进度事件：扫描器回调粒度较粗，无需额外节流
                events.emit(
                    "scan:progress",
                    &serde_json::to_value(p).unwrap_or_default(),
                );
            },
        )
    });
    let elapsed = started.elapsed().as_millis() as u64;
    match result {
        Ok(r) => {
            let changed = r.imported > 0 || r.updated > 0 || r.missing > 0;
            let cancelled = st.scan.cancel.load(Ordering::SeqCst);
            let snap = if cancelled {
                RefreshSnapshot::skipped(trigger, elapsed, None)
            } else {
                RefreshSnapshot {
                    trigger: trigger.to_string(),
                    at_ms: now_ms(),
                    changed,
                    scanned: r.scanned,
                    imported: r.imported,
                    updated: r.updated,
                    missing: r.missing,
                    failed: r.failed,
                    elapsed_ms: elapsed,
                    error: None,
                }
            };
            finish(st, trigger, snap);
        }
        Err(e) => finish(
            st,
            trigger,
            RefreshSnapshot::skipped(trigger, elapsed, Some(e.to_string())),
        ),
    }
    st.scan.scanning.store(false, Ordering::SeqCst);
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
    st.events.emit(
        "scan:progress",
        &serde_json::to_value(&snap).unwrap_or_default(),
    );
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// 同步扫描（library_scan 命令体）：busy 闸由调用方置位，本函数负责复位并返回
/// 完整结果（进度事件与 spawn_scan 共用一条路径）。
pub fn run_scan_inline(
    st: &Arc<ServerState>,
    _trigger: &'static str,
) -> Result<aiya_data::scanner::ScanResult, String> {
    let _started = Instant::now();
    let roots: Vec<std::path::PathBuf> = match db_lock(st)
        .send(|conn| Ok((read_roots(conn), read_paused_roots(conn))))
        .map_err(|e| e.to_string())
    {
        Ok((roots, paused)) => roots
            .into_iter()
            .map(std::path::PathBuf::from)
            .filter(|p| {
                let s = p.to_string_lossy();
                !paused
                    .iter()
                    .any(|p| s.to_lowercase().starts_with(&p.to_lowercase()))
            })
            .collect(),
        Err(e) => {
            st.scan.scanning.store(false, Ordering::SeqCst);
            return Err(e);
        }
    };
    if roots.is_empty() {
        st.scan.scanning.store(false, Ordering::SeqCst);
        return Err("no scan folders configured".to_string());
    }
    let art = st.artwork_dir.clone();
    let events = st.events.clone();
    let st2 = Arc::clone(st);
    let result = db_lock(st).send(move |conn| {
        aiya_data::scanner::scan_roots_ext(
            conn,
            &roots,
            Some(&art),
            || st2.scan.cancel.load(Ordering::SeqCst),
            |p| {
                events.emit(
                    "scan:progress",
                    &serde_json::to_value(p).unwrap_or_default(),
                )
            },
        )
    });
    st.scan.scanning.store(false, Ordering::SeqCst);
    result.map_err(|e| e.to_string())
}
