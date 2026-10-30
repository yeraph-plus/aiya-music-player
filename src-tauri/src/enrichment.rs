//! 索引增强的宿主侧：算清单、收事件、边收边落库（docs/45 §5.2–§5.4）。
//!
//! ## 三条不可让渡的分工
//!
//! 1. **算清单归宿主**：只有它知道扫描根、`size_bytes`/`mtime`、上次分析时间。
//!    sidecar 收到的是一份明确的路径清单，不自己挑文件。
//! 2. **落库归宿主**：sidecar 只发 `enrichItem` 事件（见 `cmds/enrich.py` 头注
//!    的三条理由：边界门禁、`scanning` 是进程内状态、外部写行会变成隐形迁移）。
//! 3. **`scanning` 只在写事务期间持有**：整场持有会让 30 s 定时刷新全部静默
//!    跳过、手动扫描报 `E_SCAN_BUSY`——用户看到的是"点扫描没反应"。
//!
//! ## 为什么要有独立的落库线程
//!
//! 事件是在会话线程上按行读出来的。如果在那里同步落库，sidecar 的 stdout
//! 就会因为磁盘 IO 停顿而被反压（管道只有 64 KB），进而在对面的
//! `SESSION_IDLE_TIMEOUT` 里表现为"工具卡死"。所以职责拆开：事件线程只往
//! 内存队列里 push（微秒级），落库线程按行数/时间攒批。

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use aiya_beets_client::EventSink;
use aiya_data::enrichment as data;
use aiya_data::DbHandle;
use tauri::{Emitter, Manager};

use crate::dto::enrichment::{EnrichmentGc, EnrichmentMatch, EnrichmentWorklist};

/// 事件通道。索引增强有自己的一条：混进 `beets:import` 会让导入面板的进度条
/// 突然开始跳"分析第 N 首"。
pub const CHANNEL: &str = "beets:enrich";

/// 默认源。宿主只知道"数值特征来自这个源"，不知道它怎么装、开没开
/// ——那是工具侧 `fields` 命令的回答。
pub const DEFAULT_SOURCE: &str = "xtractor";

/// 攒够这么多行落一次库。200 行的单事务在实测规模（1472 首）下是毫秒级，
/// 同时保证"进程被杀最多丢 200 条结论"。
const FLUSH_ROWS: usize = 200;

/// 攒不满也要落：进度数字要跟着动，否则用户看着一条不动的进度条会以为卡死。
const FLUSH_INTERVAL: Duration = Duration::from_millis(400);

/// 抢 `scanning` 的最长等待。刷新/扫描可能跑几十秒，不能干等——拿不到就
/// 把这几行退回缓冲，下一轮再试（工作清单不会因此变"已算过"）。
const LOCK_WAIT: Duration = Duration::from_secs(10);

/// 清单预览条数上限。
const PREVIEW: usize = 20;

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

/// 源名的唯一校验点：只拒绝空串。**不在这里维护源白名单**——有哪些源、
/// 哪个还没接线，是工具侧 `fields` 命令的事实；宿主再抄一份就等着两边漂移。
pub fn normalize_source(raw: Option<String>) -> Result<String, String> {
    let s = raw.unwrap_or_default();
    let s = s.trim();
    if s.is_empty() {
        return Ok(DEFAULT_SOURCE.to_string());
    }
    if s.len() > 64 || !s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
        return Err(format!("增强源名字不合法：{s}"));
    }
    Ok(s.to_string())
}

/* ---------------- 读：清单与统计 ---------------- */

fn db_of(app: &tauri::AppHandle) -> Result<DbHandle, String> {
    app.try_state::<crate::AppState>()
        .ok_or_else(|| "AppState 未就绪".to_string())
        .map(|st| st.db().clone())
}

/// 一张卡需要的全部数字 + 清单预览。
pub fn worklist(
    app: &tauri::AppHandle,
    source: &str,
    include_empty: bool,
) -> Result<EnrichmentWorklist, String> {
    let db = db_of(app)?;
    let src = source.to_string();
    let (stats, preview) = db
        .send(move |conn| {
            let stats = data::stats(conn, &src)?;
            let rows = data::worklist(conn, &src, include_empty, PREVIEW)?;
            Ok((stats, rows.into_iter().map(|w| w.path).collect::<Vec<_>>()))
        })
        .map_err(|e| e.to_string())?;

    let available = crate::beets::available(app);
    let note = (!available).then(|| {
        "未找到 aiya-beets 组件：先安装组件，或检查设置里的组件路径".to_string()
    });
    Ok(EnrichmentWorklist {
        source: source.to_string(),
        available,
        note,
        total_tracks: stats.total as u32,
        analyzed: stats.analyzed as u32,
        empty: stats.empty as u32,
        missing: stats.missing as u32,
        stale: stats.stale as u32,
        orphan: stats.orphan as u32,
        work: stats.work(include_empty) as u32,
        include_empty,
        preview,
    })
}

/// 起会话要用的完整路径清单（不做预览截断）。
pub fn work_paths(
    app: &tauri::AppHandle,
    source: &str,
    include_empty: bool,
) -> Result<Vec<String>, String> {
    let db = db_of(app)?;
    let src = source.to_string();
    db.send(move |conn| {
        Ok(data::worklist(conn, &src, include_empty, usize::MAX)?
            .into_iter()
            .map(|w| w.path)
            .collect::<Vec<_>>())
    })
    .map_err(|e| e.to_string())
}

/// 按维度查询（曲库页/智能歌单）。
pub fn find(
    app: &tauri::AppHandle,
    filter: data::Filter,
) -> Result<Vec<EnrichmentMatch>, String> {
    let db = db_of(app)?;
    db.send(move |conn| {
        Ok(data::find(conn, &filter)?
            .into_iter()
            .map(|m| EnrichmentMatch {
                track_id: m.track_id,
                mood_label: m.mood_label,
                mood_score: m.mood_score,
                bpm: m.bpm,
                danceability: m.danceability,
                loudness: m.loudness,
                analyzed_at_ms: m.analyzed_at * 1000,
            })
            .collect::<Vec<_>>())
    })
    .map_err(|e| e.to_string())
}

/// 清理残留行（曲库里已经没有对应曲目且 N 天没更新过）。
pub fn gc(app: &tauri::AppHandle, source: &str, keep_days: u32) -> Result<EnrichmentGc, String> {
    let db = db_of(app)?;
    let src = source.to_string();
    let removed = db
        .send(move |conn| data::gc(conn, &src, now_secs(), keep_days))
        .map_err(|e| e.to_string())?;
    Ok(EnrichmentGc { removed: removed as u32, keep_days })
}

/* ---------------- 写：会话期间的行缓冲与落库 ---------------- */

/// 会话期间的累计计数（落库线程写、收尾时读一次）。
#[derive(Debug, Default)]
struct Tally {
    written: AtomicUsize,
    /// 状态是 failed/skipped，宿主不许写行（写了等于永久放弃这个文件）
    skipped: AtomicUsize,
    /// 曲库里没有这个路径（扫描器还没收录 / 路径已漂移）
    not_in_library: AtomicUsize,
    /// 没能落库（抢不到写锁超时 / DB 出错）。下一次分析会重算它们。
    deferred: AtomicUsize,
    batches: AtomicUsize,
}

struct Shared {
    pending: Mutex<Vec<data::Item>>,
    closed: AtomicBool,
    tally: Tally,
}

/// `scanning` 的释放保护：panic 也不能把标志留成 true（否则此后所有扫描/刷新
/// 都报忙，用户只能重启）。与 `refresh::FlagGuard` 同一个思路，各自持有
/// 自己的 Arc。
struct Gate(Arc<AtomicBool>);

impl Drop for Gate {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

#[derive(Debug)]
enum WriteFail {
    /// 有人正持着 `scanning`（扫描/刷新），等一会儿再来
    Busy,
    Db(String),
}

/// 把一批行写进关联表，**只在这里持 `scanning`**。
///
/// 失败时行还留在调用方手里（这里只借不改），调用方自己决定重试还是计入
/// `deferred` —— 那条决定描述的是"这批结论要不要现在放弃"，属于会话策略，
/// 不该藏进写函数里。
fn write_batch(
    db: &DbHandle,
    scanning: &Arc<AtomicBool>,
    rows: &[data::Item],
) -> Result<data::Saved, WriteFail> {
    if scanning
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return Err(WriteFail::Busy);
    }
    let _gate = Gate(Arc::clone(scanning));
    let owned = rows.to_vec();
    db.send(move |conn| data::save(conn, now_secs(), &owned))
        .map_err(|e| WriteFail::Db(e.to_string()))
}

/// 会话内的落库器：既是 `EventSink`（转事件），也是行的接收端。
pub struct Sink {
    app: tauri::AppHandle,
    shared: Arc<Shared>,
    flusher: Mutex<Option<std::thread::JoinHandle<()>>>,
}

impl Sink {
    /// 起一个落库线程，返回可以直接交给会话的 sink。
    pub fn start(app: tauri::AppHandle) -> Result<Self, String> {
        let state = app
            .try_state::<crate::AppState>()
            .ok_or_else(|| "AppState 未就绪".to_string())?;
        let db = state.db().clone();
        let scanning = Arc::clone(&state.refresh.scanning);

        let shared = Arc::new(Shared {
            pending: Mutex::new(Vec::new()),
            closed: AtomicBool::new(false),
            tally: Tally::default(),
        });
        let worker = Arc::clone(&shared);
        let emit_app = app.clone();
        let flusher = std::thread::Builder::new()
            .name("aiya-enrich-writer".into())
            .spawn(move || flush_loop(&worker, &db, &scanning, &emit_app))
            .map_err(|e| format!("起落库线程失败：{e}"))?;
        Ok(Sink { app, shared, flusher: Mutex::new(Some(flusher)) })
    }

    /// 会话结束：等落库线程把余量写完，报一条带计数的终态事件。
    ///
    /// 为什么由这里报而不是 sidecar 报：`enrichDone` 说的是"工具跑了什么"，
    /// 这条说的是"库里真落了什么"。两者不同（跳过/库外/写锁超时都会让它们
    /// 不等），排障时必须能分开看。
    pub fn finish(&self) {
        self.shared.closed.store(true, Ordering::SeqCst);
        if let Some(handle) = self
            .flusher
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
        {
            let _ = handle.join();
        }
        let t = &self.shared.tally;
        let _ = self.app.emit(
            CHANNEL,
            serde_json::json!({
                "type": "enrichPersisted",
                "written": t.written.load(Ordering::SeqCst),
                "skipped": t.skipped.load(Ordering::SeqCst),
                "notInLibrary": t.not_in_library.load(Ordering::SeqCst),
                "deferred": t.deferred.load(Ordering::SeqCst),
                "batches": t.batches.load(Ordering::SeqCst),
            }),
        );
    }
}

impl EventSink for Sink {
    fn emit(&self, channel: &'static str, payload: serde_json::Value) {
        // 先转发：UI 的进度不能等落库（落库是批量的，转发是逐条的）
        let _ = self.app.emit(channel, payload.clone());
        if payload.get("type").and_then(|t| t.as_str()) != Some("enrichItem") {
            return;
        }
        let Some(item) = item_from_event(&payload) else {
            tracing::warn!(?payload, "enrichItem 形状不认识，已忽略（协议漂移？）");
            return;
        };
        if let Ok(mut q) = self.shared.pending.lock() {
            q.push(item);
        }
    }
}

/// `enrichItem` 事件 → 落库行。缺 `path`/`source` 的直接丢弃（记 warn）：
/// 宁可少一条，也不许把空路径写成一行 `''`。
fn item_from_event(v: &serde_json::Value) -> Option<data::Item> {
    let path = v.get("path").and_then(|x| x.as_str())?.trim().to_string();
    let source = v.get("source").and_then(|x| x.as_str())?.trim().to_string();
    if path.is_empty() || source.is_empty() {
        return None;
    }
    Some(data::Item {
        path,
        source,
        status: v
            .get("status")
            .and_then(|x| x.as_str())
            .unwrap_or("failed")
            .to_string(),
        fields: v.get("fields").cloned().unwrap_or_else(|| serde_json::json!({})),
    })
}

fn flush_loop(
    shared: &Arc<Shared>,
    db: &DbHandle,
    scanning: &Arc<AtomicBool>,
    app: &tauri::AppHandle,
) {
    loop {
        let batch = take_batch(&shared.pending, FLUSH_ROWS);
        if batch.is_empty() {
            if shared.closed.load(Ordering::SeqCst) {
                break;
            }
            std::thread::sleep(FLUSH_INTERVAL);
            continue;
        }
        let rows = batch;
        let deadline = Instant::now() + LOCK_WAIT;
        loop {
            match write_batch(db, scanning, &rows) {
                Ok(saved) => {
                    let t = &shared.tally;
                    t.written.fetch_add(saved.written, Ordering::SeqCst);
                    t.skipped.fetch_add(saved.skipped, Ordering::SeqCst);
                    t.not_in_library
                        .fetch_add(saved.not_in_library, Ordering::SeqCst);
                    t.batches.fetch_add(1, Ordering::SeqCst);
                    let _ = app.emit(
                        CHANNEL,
                        serde_json::json!({
                            "type": "enrichPersist",
                            "written": t.written.load(Ordering::SeqCst),
                            "skipped": saved.skipped,
                            "notInLibrary": saved.not_in_library,
                        }),
                    );
                    break;
                }
                // 抢不到写锁：扫描/刷新在跑。等一会儿再来——不能直接丢，
                // 丢了这些文件的结论就白算了（工作清单还会再点它们一次）。
                Err(WriteFail::Busy) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(120));
                }
                Err(WriteFail::Busy) => {
                    tracing::warn!(rows = rows.len(), "写库互斥一直被占用，这批结论留待下次重算");
                    shared
                        .tally
                        .deferred
                        .fetch_add(rows.len(), Ordering::SeqCst);
                    break;
                }
                Err(WriteFail::Db(e)) => {
                    // DB 出错（含应用正在退出）不重试：写不进去的那些同上，
                    // 下次分析自然重算。
                    tracing::warn!(error = %e, "索引增强落库失败，这批行留待下次重算");
                    shared
                        .tally
                        .deferred
                        .fetch_add(rows.len(), Ordering::SeqCst);
                    break;
                }
            }
        }
    }
}

fn take_batch(pending: &Mutex<Vec<data::Item>>, max: usize) -> Vec<data::Item> {
    let Ok(mut q) = pending.lock() else {
        return Vec::new();
    };
    let n = q.len().min(max);
    q.drain(..n).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_defaults_and_rejects_junk() {
        assert_eq!(normalize_source(None).unwrap(), DEFAULT_SOURCE);
        assert_eq!(normalize_source(Some("  ".into())).unwrap(), DEFAULT_SOURCE);
        assert_eq!(normalize_source(Some(" probe ".into())).unwrap(), "probe");
        assert!(normalize_source(Some("a/b".into())).is_err(), "路径分隔符会毁掉事件识别");
        assert!(normalize_source(Some("x y".into())).is_err());
    }

    #[test]
    fn event_maps_to_row_and_rejects_empty_path() {
        let good = serde_json::json!({
            "type": "enrichItem", "path": "D:/m/a.flac", "source": "xtractor",
            "status": "ok", "fields": {"bpm": 120.0}, "message": null,
        });
        let it = item_from_event(&good).expect("正常事件必须能落行");
        assert_eq!(it.path, "D:/m/a.flac");
        assert_eq!(it.status, "ok");
        assert_eq!(it.fields["bpm"], 120.0);

        // 缺 path / 空 path → 丢弃（宁少一条，不写空键）
        assert!(item_from_event(&serde_json::json!({"source": "x"})).is_none());
        assert!(item_from_event(&serde_json::json!({"path": "  ", "source": "x"})).is_none());
        // 缺 status → failed（落库层会跳过它，不会静默写成 ok）
        let no_status = serde_json::json!({"path": "/m/a", "source": "x"});
        assert_eq!(item_from_event(&no_status).unwrap().status, "failed");
        // 缺 fields → 空对象（不是 null：落库要序列化它）
        assert!(item_from_event(&no_status).unwrap().fields.is_object());
    }

    #[test]
    fn take_batch_drains_in_order_and_caps() {
        let q = Mutex::new(vec![
            data::Item { path: "a".into(), source: "s".into(), status: "ok".into(), fields: serde_json::json!({}) },
            data::Item { path: "b".into(), source: "s".into(), status: "ok".into(), fields: serde_json::json!({}) },
            data::Item { path: "c".into(), source: "s".into(), status: "ok".into(), fields: serde_json::json!({}) },
        ]);
        let first = take_batch(&q, 2);
        assert_eq!(first.iter().map(|i| i.path.as_str()).collect::<Vec<_>>(), vec!["a", "b"]);
        let second = take_batch(&q, 2);
        assert_eq!(second.len(), 1, "剩下的要被下一批取走");
        assert!(take_batch(&q, 2).is_empty());
    }

    /// 写锁被占时不许抢写：`write_batch` 必须立刻回 Busy 而不是等。
    ///
    /// 这条守的是 docs/45 §5.3 的"锁粒度"：落库线程一旦在这里阻塞，
    /// 事件线程就不会阻塞（两线程分离），但整个落库会停摆——所以要让
    /// 调用方看到 Busy 并自己决定重试节奏。
    #[test]
    fn write_batch_refuses_when_gate_is_taken() {
        let path = std::env::temp_dir().join(format!("aiya-enrich-gate-{}.db", std::process::id()));
        let mut db = aiya_data::open(&path).unwrap();
        let scanning = Arc::new(AtomicBool::new(true)); // 假装扫描在跑
        let rows = vec![data::Item {
            path: "/m/a.mp3".into(),
            source: "xtractor".into(),
            status: "ok".into(),
            fields: serde_json::json!({}),
        }];
        match write_batch(&db, &scanning, &rows) {
            Err(WriteFail::Busy) => {}
            other => panic!("应为 Busy，实际 {:?}", other.map(|s| s.written)),
        }
        assert!(scanning.load(Ordering::SeqCst), "别人的锁不许被我们放开");
        db.close();
        let _ = std::fs::remove_file(&path);
    }

    /// 正常路径：拿到锁 → 落库 → 锁必须放开（否则下一次刷新永远被跳过）。
    #[test]
    fn write_batch_writes_and_releases_gate() {
        let path = std::env::temp_dir().join(format!("aiya-enrich-ok-{}.db", std::process::id()));
        let mut db = aiya_data::open(&path).unwrap();
        // 曲目行的 path_lower 必须与落库侧同一个口径（path_key），否则
        // 这条测试会因为"找不到曲目"而被跳过——那是测试自己造错了数据
        let raw = r"D:\m\a.mp3";
        let key = data::path_key(raw);
        db.send(move |conn| {
            conn.execute(
                "INSERT INTO tracks (id, title, title_norm, added_at, path, path_lower, size_bytes, mtime)
                 VALUES ('t1','t1','t1',0,?1,?2,10,20)",
                rusqlite::params![raw, key],
            )?;
            Ok(())
        })
        .unwrap();
        let scanning = Arc::new(AtomicBool::new(false));
        let rows = vec![data::Item {
            path: raw.to_string(),
            source: "xtractor".into(),
            status: "ok".into(),
            fields: serde_json::json!({"bpm": 99.0}),
        }];
        let saved = write_batch(&db, &scanning, &rows).expect("应能写");
        assert_eq!(saved.written, 1);
        assert!(!scanning.load(Ordering::SeqCst), "写完必须放开锁");
        let bpm: f64 = db
            .send(|conn| Ok(conn.query_row("SELECT bpm FROM track_enrichment", [], |r| r.get(0))?))
            .unwrap();
        assert_eq!(bpm, 99.0);
        db.close();
        let _ = std::fs::remove_file(&path);
    }

    /// 端到端：**真 sidecar** 发的 `enrichItem` → 宿主解析 → 落关联表。
    ///
    /// 为什么非要有这条（而不是只测 `item_from_event`）：字段名/形状的漂移
    /// 只有对着对面真发的东西才暴露。手写 `json!({"path": ...})` 的单测永远
    /// 绿——它测的是自己写的那个形状（docs/45 §11.4 的教训：解析失败静默变
    /// `ok()` 会让 UI"什么都不显示"，而单测发现不了）。
    ///
    /// 假源（`AIYA_BEETS_ENRICH_PROBE=1`）在这台机器上就能跑，不依赖 Essentia。
    /// venv / 测试音源缺一个就跳过（clean checkout、CI 上的常态）。
    #[cfg(debug_assertions)]
    #[test]
    fn real_sidecar_events_land_in_the_table() {
        use std::io::{BufRead, Write};

        let repo = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
        let venv = repo.join("beets-tool/.venv/Scripts/python.exe");
        let src = repo.join("beets-tool/src");
        let music = std::env::var("AIYA_TEST_MUSIC")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|_| repo.join("../test-music"));
        if !venv.exists() || !src.is_dir() || !music.is_dir() {
            return;
        }
        let wavs: Vec<std::path::PathBuf> = std::fs::read_dir(&music)
            .map(|d| {
                d.filter_map(|e| e.ok())
                    .map(|e| e.path())
                    .filter(|p| p.extension().is_some_and(|x| x == "wav"))
                    .take(2)
                    .collect()
            })
            .unwrap_or_default();
        if wavs.is_empty() {
            return;
        }

        let root = std::env::temp_dir().join(format!("aiya-enrich-e2e-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let home = root.join("home");
        let mus = root.join("music");
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(&mus).unwrap();
        let paths: Vec<std::path::PathBuf> = wavs
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let dst = mus.join(format!("{i}.wav"));
                std::fs::copy(p, &dst).unwrap();
                dst
            })
            .collect();

        let mut child = std::process::Command::new(&venv)
            .args(["-m", "aiya_beets"])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            // 与生产同一套环境（生产由 beets::tool_env 下发）
            .env("PYTHONIOENCODING", "utf-8")
            .env("PYTHONUTF8", "1")
            .env("PYTHONPATH", &src)
            .env("AIYA_BEETS_HOME", &home)
            .env("BEETSDIR", &home)
            .env("AIYA_BEETS_PROFILE", "index")
            .env("AIYA_BEETS_ENRICH_PROBE", "1")
            .spawn()
            .expect("spawn sidecar");
        {
            let req = serde_json::json!({
                "id": 1, "cmd": "enrich",
                "paths": paths.iter().map(|p| p.to_string_lossy()).collect::<Vec<_>>(),
                "want": ["probe"],
            });
            let mut stdin = child.stdin.take().expect("stdin");
            writeln!(stdin, "{req}").unwrap();
        }

        // 事件 → 行（与生产同一个解析入口）
        let mut rows: Vec<data::Item> = Vec::new();
        let mut saw_done = false;
        let stdout = child.stdout.take().expect("stdout");
        for line in std::io::BufReader::new(stdout).lines() {
            let Ok(line) = line else { break };
            let Ok(v) = serde_json::from_str::<serde_json::Value>(line.trim()) else {
                continue;
            };
            match v.get("type").and_then(|t| t.as_str()) {
                Some("enrichItem") => {
                    if let Some(item) = item_from_event(&v) {
                        rows.push(item);
                    }
                }
                Some("enrichDone") => saw_done = true,
                _ => {}
            }
        }
        let _ = child.wait();
        assert!(saw_done, "没有 enrichDone：会话没走到终态");
        assert_eq!(rows.len(), paths.len(), "每个文件都该有一条结论：{rows:?}");

        // 落库：文件事实由宿主从 tracks 现查（这里按真实文件造行）
        let db_path = root.join("t.db");
        let mut db = aiya_data::open(&db_path).unwrap();
        db.send(move |conn| {
            for (i, p) in paths.iter().enumerate() {                let meta = std::fs::metadata(p).unwrap();
                let mtime = meta
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_secs() as i64)
                    .unwrap_or(0);
                conn.execute(
                    "INSERT INTO tracks (id, title, title_norm, added_at, path, path_lower,
                                         size_bytes, mtime)
                     VALUES (?1, ?1, ?1, 0, ?2, ?3, ?4, ?5)",
                    rusqlite::params![
                        format!("t{i}"),
                        p.to_string_lossy(),
                        p.to_string_lossy().to_lowercase(),
                        meta.len() as i64,
                        mtime,
                    ],
                )?;
            }
            Ok(())
        })
        .unwrap();

        let saved = db
            .send({
                let rows = rows.clone();
                move |conn| data::save(conn, 1, &rows)
            })
            .unwrap();
        assert_eq!(saved.written, 2, "{saved:?}");
        assert_eq!(saved.not_in_library, 0, "path_lower 口径必须与 scanner 一致");

        let (n, with_mood, with_bpm): (i64, i64, i64) = db
            .send(|conn| {
                Ok(conn.query_row(
                    "SELECT COUNT(*),
                            SUM(CASE WHEN mood_label IS NOT NULL THEN 1 ELSE 0 END),
                            SUM(CASE WHEN bpm IS NOT NULL THEN 1 ELSE 0 END)
                     FROM track_enrichment",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )?)
            })
            .unwrap();
        assert_eq!((n, with_mood, with_bpm), (2, 2, 2), "提升列没填上");
        // 情绪标签必须是 7 类之一（不是空串、不是原始 JSON 片段）
        let label: String = db
            .send(|conn| Ok(conn.query_row("SELECT mood_label FROM track_enrichment", [], |r| r.get(0))?))
            .unwrap();
        assert!(
            data::MOOD_CLASSES.contains(&label.as_str()),
            "mood_label 不是已知情绪类：{label}"
        );

        db.close();
        let _ = std::fs::remove_dir_all(&root);
    }
}
