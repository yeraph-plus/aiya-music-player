//! 曲库刷新管线：增量扫描 → **单** bump（docs/43 §8-E 终态）。
//!
//! 这是"用户在外面改了东西，播放器 30 秒内自己发现"那条线。它不属于 beets
//! 侧（beets 不知道谁在消费它的数据），也不属于 UI（UI 只知道"该重拉了"）：
//! 播放器知道自己的 DB 长什么样，所以编排在这里。
//!
//! ## 本管线与 beets 的关系：**零接触**
//!
//! docs/43 §8-E 拍板：beets 是纯手动增强工具，曲库的成员资格 100% 归播放器
//! 自己的扫描器。旧设计的"扫描后 dump→sync 对账"整体取消——它的前提
//! （beets 是写权威、曲库是读模型）已不成立，且在"输出根可不在扫描根内"
//! （纯整理器模式）下会把 beets 库里扫描根外的文件注进曲库。取代它的：
//! - beets 写进文件的标签/移动，由扫描器在下一轮扫描时从**文件本身**读到；
//! - 与 beets 的一切交互都走显式命令（preview/apply/维护四件套/配置），见 beets.rs。
//!
//! 后续已知欠账（扫描器侧，与 beets 无关）：改名/移动跟随的启发式对
//! "先写标签再移动"（mtime 已变）的文件接不住；replaygain 的 gain_db 尚无
//! 来源（扫描器不读 RG 标签）。见 docs/43 §10。
//!
//! ## 三个触发点（不上 FS watcher）
//!
//! | 触发 | 谁推 | 覆盖什么 |
//! |---|---|---|
//! | [`Trigger::Action`] | beets 命令终态（apply/adopt/move） | 刚写完就得看见 |
//! | [`Trigger::Timer`] | 30 s（窗口失焦暂停） | 用户在别的工具里改了标签 |
//! | [`Trigger::Focus`] | `WindowEvent::Focused(true)` | 刚在资源管理器/mp3tag 改完回来 |
//!
//! watcher 能带来"即时"，代价是去抖、忽略自身 move 造成的回环（"先搬走再清
//! 目录"会触发一串事件）、每个平台一套路径规范化坑。这里 30 s 内发现已经够用，
//! 而重扫 1846 首只要 60 ms。
//!
//! ## 为什么"没有变化就不 bump"
//!
//! 定时刷新每 30 s 一次。如果每次都发事件，UI 就会每 30 s 重拉一次当前页 ——
//! 用户正在滚动时列表跳一下，而实际上什么都没变。判据见 [`Outcome::changed`]：
//! 只有真的写了行（导入/重写/missing）才算。

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use tauri::{Emitter, Manager};

use crate::dto::refresh::RefreshSnapshot;

/// 定时刷新的默认间隔。设置项 `refreshSeconds` 可覆盖（0 = 关掉定时）。
pub const DEFAULT_INTERVAL_SECS: u64 = 30;

/// 刷新管线需要的宿主依赖（`AppState` 里存一份 Arc，线程间共享）。
///
/// 为什么不是一个 `&AppState`：管线跑在独立线程里，而 `State<'_, T>` 的生命期
/// 绑在命令调用栈上。把需要的东西拷出来，线程就不必持有任何借用。
pub struct RefreshParams {
    pub db_path: PathBuf,
    pub artwork_dir: PathBuf,
    /// 写库互斥。与 `library_scan` 共用**同一个** Arc：同一时刻只该有一个
    /// 批任务在写 DB，否则 SQLite 的单写者会互相 BUSY。
    pub scanning: Arc<AtomicBool>,
    pub scan_cancel: Arc<AtomicBool>,
}

/// 是谁要刷新。它出现在事件与诊断页里——"这次为什么刷"是排障第一个问题。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trigger {
    /// 定时器到点（最安静的那种）
    Timer,
    /// 窗口重新获得焦点
    Focus,
    /// 某条 beets 命令刚结束（apply / adopt / move）：只触发扫描段
    Action,
    /// 用户手点的"立即刷新"
    Manual,
}

impl Trigger {
    pub fn as_str(self) -> &'static str {
        match self {
            Trigger::Timer => "timer",
            Trigger::Focus => "focus",
            Trigger::Action => "action",
            Trigger::Manual => "manual",
        }
    }

    /// 自动触发（定时/聚焦）。自动触发撞上忙时**静默**放弃：用户没在等，
    /// 下一轮还会来。手动触发才需要告诉他"没跑成"。
    fn is_auto(self) -> bool {
        matches!(self, Trigger::Timer | Trigger::Focus)
    }
}

/// 扫描互斥的 panic 保护。
///
/// 历史上 scan 链路里任一 panic 让 `scanning` 永真，之后所有扫描报 `E_SCAN_BUSY`，
/// 用户只能重启应用。Drop 兜底保证退出路径（含 unwind）必复位。
struct FlagGuard(Arc<AtomicBool>);

impl Drop for FlagGuard {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

/// 一次刷新的结果。
struct Outcome {
    scan: aiya_data::scanner::ScanResult,
}

impl Outcome {
    /// 有没有真的写出可见变化（决定 bump 不 bump）。
    ///
    /// `failed` 不算变化：解析失败的文件下次还是失败，每 30 s 让列表重拉一次
    /// 只会让"坏文件"变成"列表一直在动"。它进快照与日志，但不触发重绘。
    fn changed(&self) -> bool {
        self.scan.imported > 0 || self.scan.updated > 0 || self.scan.missing > 0
    }
}

enum Fail {
    /// 没到该跑的时候（没配曲库根、被取消）。安静地记一笔。
    Skipped(&'static str),
    /// 真的出错了（DB 打不开之类）
    Failed(String),
}

/// 起一个线程跑一次刷新（不阻塞调用方）。
///
/// `Action` 触发来自 beets 会话的 `on_exit`，那里已经在一条一次性线程上，
/// 但仍然另起线程：刷新要 1 s 量级，不该拖住会话收尾。
pub fn spawn(app: &tauri::AppHandle, params: Arc<RefreshParams>, trigger: Trigger) {
    let app = app.clone();
    let name = format!("library-refresh-{}", trigger.as_str());
    if let Err(e) = std::thread::Builder::new().name(name).spawn(move || {
        run(&app, &params, trigger);
    }) {
        tracing::warn!(%e, trigger = trigger.as_str(), "起刷新线程失败");
    }
}

/// 一次完整刷新（同步执行到结束）。
pub fn run(app: &tauri::AppHandle, params: &RefreshParams, trigger: Trigger) {
    let started = Instant::now();

    // 单飞行。撞上说明有别的批任务在写库（手动扫描 / 上一次刷新没完）。
    if params
        .scanning
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        if !trigger.is_auto() {
            emit(
                app,
                serde_json::json!({
                    "type": "libraryRefreshSkipped",
                    "reason": "busy",
                    "trigger": trigger.as_str(),
                }),
            );
        }
        tracing::debug!(trigger = trigger.as_str(), "刷新让位：已有写库任务在跑");
        return;
    }
    let _guard = FlagGuard(Arc::clone(&params.scanning));

    // 清掉上一次手动扫描留下的取消请求：scan_cancel 只在手动扫描开头清零，
    // 这里是自动刷新唯一会碰它的入口 —— 用户取消过一次手动扫描后，残留的
    // true 会让此后每次自动刷新在扫描第一步就静默 Skipped("cancelled")。
    params.scan_cancel.store(false, Ordering::SeqCst);

    match execute(app, params) {
        Ok(outcome) => {
            let changed = outcome.changed();
            let elapsed_ms = started.elapsed().as_millis() as u64;
            let snapshot = RefreshSnapshot {
                trigger: trigger.as_str().to_string(),
                at_ms: now_ms(),
                changed,
                scanned: outcome.scan.scanned,
                imported: outcome.scan.imported,
                updated: outcome.scan.updated,
                missing: outcome.scan.missing,
                failed: outcome.scan.failed,
                elapsed_ms,
                error: None,
            };
            store_last(app, snapshot.clone());

            tracing::info!(
                trigger = trigger.as_str(),
                scanned = outcome.scan.scanned,
                imported = outcome.scan.imported,
                updated = outcome.scan.updated,
                missing = outcome.scan.missing,
                changed,
                elapsed_ms,
                "library refresh"
            );

            // 无变化不发 bump；手动触发例外（用户点了按钮，必须看到回应）
            if changed || trigger == Trigger::Manual {
                match serde_json::to_value(&snapshot) {
                    Ok(mut v) => {
                        if let Some(obj) = v.as_object_mut() {
                            obj.insert("type".into(), serde_json::json!("libraryRefreshed"));
                        }
                        emit(app, v);
                    }
                    Err(e) => tracing::warn!(%e, "刷新快照序列化失败"),
                }
            }
        }
        Err(Fail::Skipped(reason)) => {
            store_last(
                app,
                RefreshSnapshot::skipped(
                    trigger.as_str(),
                    started.elapsed().as_millis() as u64,
                    None,
                ),
            );
            if !trigger.is_auto() {
                emit(
                    app,
                    serde_json::json!({
                        "type": "libraryRefreshSkipped",
                        "reason": reason,
                        "trigger": trigger.as_str(),
                    }),
                );
            }
            tracing::debug!(trigger = trigger.as_str(), reason, "刷新跳过");
        }
        Err(Fail::Failed(message)) => {
            tracing::warn!(trigger = trigger.as_str(), %message, "刷新失败");
            store_last(
                app,
                RefreshSnapshot::skipped(
                    trigger.as_str(),
                    started.elapsed().as_millis() as u64,
                    Some(message.clone()),
                ),
            );
            emit(
                app,
                serde_json::json!({
                    "type": "libraryRefreshFailed",
                    "trigger": trigger.as_str(),
                    "message": message,
                }),
            );
        }
    }
}

fn execute(app: &tauri::AppHandle, params: &RefreshParams) -> Result<Outcome, Fail> {
    /* 增量扫描（唯一的一段） */
    let mut conn =
        aiya_data::open_conn(&params.db_path).map_err(|e| Fail::Failed(format!("open db: {e}")))?;

    let roots: Vec<PathBuf> = roots_from_settings(&conn)
        .into_iter()
        .filter(|r| r.is_dir())
        .collect();
    if roots.is_empty() {
        let _ = conn.close();
        return Err(Fail::Skipped("noRoots"));
    }

    let scan = aiya_data::scanner::scan_roots_ext(
        &mut conn,
        &roots,
        Some(&params.artwork_dir),
        || params.scan_cancel.load(Ordering::Relaxed),
        |_p| {
            // 刻意**不**转发 scan:progress：自动刷新不该弹出进度条，
            // 而一次增量扫描只有几十毫秒，进度对用户没有信息量。
        },
    )
    .map_err(|e| Fail::Failed(format!("scan: {e}")))?;
    let _ = conn.close();

    if scan.cancelled {
        return Err(Fail::Skipped("cancelled"));
    }
    let _ = app; // app 仅用于事件；execute 本体不再发事件（终态在 run 统一发）

    Ok(Outcome { scan })
}

/* ---------------- 曲库根 ---------------- */

/// 从设置读曲库根（多目录并集单库，docs/29 批 A）。
///
/// 优先级：`library.roots`（数组，**键存在即权威**）> `library.root`（旧单串，
/// 仅键缺失时兜底）> 系统 Music。
/// 抽成公开函数是为了让 `library_scan` 复用同一份判定——两处各写一遍的话，
/// "手动扫描扫的目录"与"自动刷新扫的目录"迟早会不一样。
pub fn roots_from_settings(conn: &rusqlite::Connection) -> Vec<PathBuf> {
    // 新键 `library.roots`（数组）**存在即权威，空数组就是空**——用户在
    // textarea 里删光了目录，自动刷新就不该再去扫任何地方。
    //
    // 曾经的行为是"空数组回落旧键"：用户把列表删空后，自动刷新按旧键
    // /系统 Music 把目录"死而复生"地扫回来，症状是"已添加的目录移除不掉"
    // （2026-10-28 实测修复）。旧键兜底只保留给**从未写过新键**的老库。
    //
    // 这里**不能用 `?` 串起来**：`library.roots` 不存在时 `?` 会直接把整个
    // 解析判成"没配"，旧键分支永远走不到 —— 老用户的库根全在 `library.root`
    // 里，症状是"自动刷新和手动扫描都说未配置任何扫描目录"，而界面看着一切正常。
    let from_roots = crate::setting_has(conn, "library.roots").then(|| {
        crate::setting_get(conn, "library.roots")
            .and_then(|v| v.as_array().cloned())
            .map(|a| {
                a.iter()
                    .filter_map(|s| s.as_str().map(str::to_string))
                    .filter(|s| !s.trim().is_empty())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
    });
    let list = from_roots.or_else(|| {
        crate::setting_get(conn, "library.root")
            .and_then(|v| v.as_str().map(str::to_string))
            .filter(|s| !s.trim().is_empty())
            .map(|s| vec![s])
    });
    match list {
        Some(list) => list.into_iter().map(PathBuf::from).collect(),
        // 完全没有配置：回落系统 Music 目录（保持旧行为；不存在时由调用方过滤）
        None => vec![std::env::var("USERPROFILE")
            .ok()
            .map(|h| PathBuf::from(h).join("Music"))
            .unwrap_or_default()],
    }
}

/* ---------------- 定时器 + 焦点 ---------------- */

/// 定时线程收到的唤醒。
pub enum Wake {
    /// 窗口重新聚焦：立刻刷新（覆盖"刚在 mp3tag 改完回来"）
    Focus,
    /// 设置变了（间隔/开关）：只是醒来重读，不刷新
    SettingsChanged,
}

/// 记下"当前有没有焦点"。失焦时定时器空转（不扫），回到前台再继续。
///
/// 默认 true：启动瞬间窗口是可见的，若默认 false 要等第一次 Focused(true)
/// 才会开始定时，而某些平台启动时不发这个事件。
static FOCUSED: AtomicBool = AtomicBool::new(true);

/// 当前窗口是否在前台（诊断页展示用）。
pub fn focused() -> bool {
    FOCUSED.load(Ordering::Acquire)
}

/// 起定时线程（应用启动时调一次）。
///
/// 单线程串行执行：定时与聚焦触发都在这条线程上排队，天然不会出现
/// "两次刷新同时写库"。刷新本身 1 s 量级，排队不会积压。
pub fn start_scheduler(app: tauri::AppHandle, params: Arc<RefreshParams>) {
    let (tx, rx) = mpsc::sync_channel::<Wake>(1);
    if let Some(state) = app.try_state::<crate::AppState>() {
        *state
            .refresh_wake
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(tx);
    }

    let builder = std::thread::Builder::new().name("library-refresh-timer".into());
    if let Err(e) = builder.spawn(move || {
        // 启动后补一次：上次退出到现在，用户可能在别的工具里改过标签。
        // 延迟 3 s 是给 boot 的首屏查询让路 —— 同时发两个查询会让首屏闪一下，
        // 而这次刷新本来也不急（它是"兜底"，不是"首屏数据源"）。
        std::thread::sleep(Duration::from_secs(3));
        if FOCUSED.load(Ordering::Acquire) {
            run(&app, &params, Trigger::Timer);
        }
        loop {
            let secs = interval_secs(&app);
            if secs == 0 {
                // 定时被关掉：不空转，1 s 一次重读设置即可（只读内存镜像，
                // 不碰 DB）。仍要能被聚焦/设置变更唤醒。
                match rx.recv_timeout(Duration::from_secs(1)) {
                    Ok(_) | Err(mpsc::RecvTimeoutError::Timeout) => continue,
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                }
            }
            match rx.recv_timeout(Duration::from_secs(secs)) {
                Ok(Wake::Focus) => {
                    // 只看"现在有没有焦点"：失焦时不该为了一个刚到的事件扫全库
                    if FOCUSED.load(Ordering::Acquire) {
                        run(&app, &params, Trigger::Focus);
                    }
                }
                Ok(Wake::SettingsChanged) => {}
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    if FOCUSED.load(Ordering::Acquire) {
                        run(&app, &params, Trigger::Timer);
                    }
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
    }) {
        tracing::warn!(%e, "起刷新定时线程失败");
    }
}

/// 窗口焦点变化（`WindowEvent::Focused` 转发进来）。
pub fn set_focused(app: &tauri::AppHandle, focused: bool) {
    FOCUSED.store(focused, Ordering::Release);
    if focused {
        wake(app, Wake::Focus);
    }
}

/// 设置变更后叫醒定时线程（重读间隔 / 从关闭状态恢复）。
pub fn wake_settings(app: &tauri::AppHandle) {
    wake(app, Wake::SettingsChanged);
}

fn wake(app: &tauri::AppHandle, msg: Wake) {
    if let Some(state) = app.try_state::<crate::AppState>() {
        if let Some(tx) = state
            .refresh_wake
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_ref()
        {
            // 容量 1：定时线程正在忙时丢掉这条也没关系（马上就会再醒一次）
            let _ = tx.try_send(msg);
        }
    }
}

/// 定时间隔（秒，0 = 关闭）。读设置内存镜像，不碰 DB。
fn interval_secs(app: &tauri::AppHandle) -> u64 {
    app.try_state::<crate::AppState>()
        .map(|state| {
            let v = state
                .settings
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .get("refreshSeconds")
                .and_then(serde_json::Value::as_u64);
            match v {
                // 显式 0 = 关掉定时（聚焦触发仍然有效）
                Some(0) => 0,
                // 下限 10 s：再短就是拿电量换"几乎即时"，而 watcher 才是解法；
                // 上限 1 h：坏配置写个天文数字会把定时器钉死在一个无意义的间隔上
                Some(n) => n.clamp(10, 3600),
                None => DEFAULT_INTERVAL_SECS,
            }
        })
        .unwrap_or(DEFAULT_INTERVAL_SECS)
}

/* ---------------- 快照读写 ---------------- */

fn store_last(app: &tauri::AppHandle, snapshot: RefreshSnapshot) {
    if let Some(state) = app.try_state::<crate::AppState>() {
        *state
            .last_refresh
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(snapshot);
    }
}

/* ---------------- 事件 ---------------- */

fn emit(app: &tauri::AppHandle, v: serde_json::Value) {
    // 通道与 beets 导入链路共用：UI 侧本来就在听 `beets:import`
    // （各类进度从这里来），刷新终态跟着走同一个通道，少一个订阅点。
    let _ = app.emit("beets:import", v);
}

/* ---------------- 时间 ---------------- */

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    /* ---------------- 曲库根解析（新旧两个键） ---------------- */

    fn temp_conn() -> (tempfile::TempDir, rusqlite::Connection) {
        let dir = tempfile::tempdir().unwrap();
        let conn = aiya_data::open_conn(&dir.path().join("t.sqlite")).unwrap();
        (dir, conn)
    }

    /// 老用户的库根在 `library.root`（单串）。这条曾经真红过一次：
    /// 用 `?` 串解析时 `library.roots` 缺失会直接放弃，永远读不到旧键 ——
    /// 症状是"手动扫描与自动刷新都说未配置扫描目录"，界面却看不出问题。
    #[test]
    fn roots_read_legacy_single_key() {
        let (_dir, conn) = temp_conn();
        crate::setting_set(&conn, "library.root", &serde_json::json!("D:/Music/Old")).unwrap();
        assert_eq!(
            roots_from_settings(&conn),
            vec![PathBuf::from("D:/Music/Old")]
        );
    }

    /// 新键优先：两个都在时以数组为准（旧键只是兼容残留）。
    #[test]
    fn roots_prefer_array_over_legacy() {
        let (_dir, conn) = temp_conn();
        crate::setting_set(&conn, "library.root", &serde_json::json!("D:/Music/Old")).unwrap();
        crate::setting_set(&conn, "library.roots", &serde_json::json!(["D:/A", "D:/B"])).unwrap();
        assert_eq!(
            roots_from_settings(&conn),
            vec![PathBuf::from("D:/A"), PathBuf::from("D:/B")]
        );
    }

    /// 空数组是**权威**的：用户在 textarea 里删光了目录，就不该回落旧键
    /// 把目录"死而复生"地扫回来（2026-10-28 实测缺陷修复）。
    /// 旧键兜底只属于"从未写过新键"的库（见 `roots_read_legacy_single_key`）。
    #[test]
    fn empty_array_is_authoritative() {
        let (_dir, conn) = temp_conn();
        crate::setting_set(&conn, "library.root", &serde_json::json!("D:/Music/Old")).unwrap();
        crate::setting_set(&conn, "library.roots", &serde_json::json!([])).unwrap();
        assert!(roots_from_settings(&conn).is_empty());
    }

    /// 两个键都没有 → 回落系统 Music（目录不存在时由调用方过滤掉）。
    #[test]
    fn roots_default_to_system_music() {
        let (_dir, conn) = temp_conn();
        let roots = roots_from_settings(&conn);
        assert_eq!(roots.len(), 1);
        assert!(roots[0].ends_with("Music"), "{:?}", roots[0]);
    }

    /// 自动触发撞忙要静默（用户没在等），手动触发要报。
    #[test]
    fn only_manual_and_action_report_skips() {
        assert!(Trigger::Timer.is_auto());
        assert!(Trigger::Focus.is_auto());
        assert!(!Trigger::Action.is_auto());
        assert!(!Trigger::Manual.is_auto());
    }

    fn scan(imported: usize, updated: usize, missing: usize) -> aiya_data::scanner::ScanResult {
        aiya_data::scanner::ScanResult {
            scanned: 10,
            imported,
            updated,
            unchanged: 10 - imported - updated,
            failed: 0,
            missing,
            walk_errors: 0,
            cancelled: false,
            errors: Vec::new(),
            elapsed_ms: 1,
        }
    }

    /// bump 判据：只有"真的写了行"才算变化。失败不算 —— 否则一个坏文件
    /// 会让列表每 30 s 重拉一次，看起来像在抽风。
    #[test]
    fn change_detection_ignores_failures() {
        let quiet = Outcome {
            scan: scan(0, 0, 0),
        };
        assert!(!quiet.changed());

        let failed_only = Outcome {
            scan: aiya_data::scanner::ScanResult {
                failed: 5,
                ..scan(0, 0, 0)
            },
        };
        assert!(!failed_only.changed(), "解析失败不是变化");

        assert!(
            Outcome {
                scan: scan(1, 0, 0)
            }
            .changed(),
            "新导入"
        );
        assert!(
            Outcome {
                scan: scan(0, 2, 0)
            }
            .changed(),
            "重写"
        );
        assert!(
            Outcome {
                scan: scan(0, 0, 1)
            }
            .changed(),
            "文件消失"
        );
    }
}
