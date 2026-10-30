//! 索引增强的宿主侧：算清单、收事件、边收边落库（docs/45 §5.2–§5.4）。
//!
//! ## 与批 3 的关键差别：结论库不再在应用库里
//!
//! 批 3 把结论写进应用库的 `track_enrichment`，代价是三条耦合：
//! 写它要抢 `scanning`（挡住用户的扫描）、应用库 schema 里多一张与它无关的
//! 表、结论跟着应用库的备份/迁移/卸载一起走。现在结论落在
//! `{beets-index}/enrich.db`（见 `aiya_beets_store`），**本模块不碰 `scanning`、
//! 不碰刷新管线**。
//!
//! ## 仍然读应用库，但只读一处
//!
//! 工作清单要知道"曲库里有哪些文件、`size/mtime` 是多少"。这条信息只有应用
//! 知道，所以 [`arena_of`] 读一次 `tracks`（`repo::online_files`，只读、单条
//! SQL、一次会话一次）。反方向没有：应用主体不 import 本模块，也不 import
//! `aiya_beets_store`。
//!
//! ## 为什么要有独立的落库线程
//!
//! 事件是在会话线程上按行读出来的。如果在那里同步落库，sidecar 的 stdout
//! 就会因为磁盘 IO 停顿而被反压（管道只有 64 KB），进而在对面的
//! `SESSION_IDLE_TIMEOUT` 里表现为"工具卡死"。所以职责拆开：事件线程只往
//! 内存队列里 push（微秒级），落库线程按行数/时间攒批。

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use aiya_beets_client::EventSink;
use aiya_beets_store::{Arena, Item, Store};
use tauri::{Emitter, Manager};

use crate::dto::enrichment::{EnrichmentGc, EnrichmentMatch, EnrichmentWorklist};

/// 事件通道。索引增强有自己的一条：混进 `beets:import` 会让导入面板的进度条
/// 突然开始跳"分析第 N 首"。
pub const CHANNEL: &str = "beets:enrich";

/// 默认源。宿主只知道"数值特征来自这个源"，不知道它怎么装、开没开
/// ——那是工具侧 `fields` 命令的回答。
pub const DEFAULT_SOURCE: &str = aiya_beets_store::SOURCE_XTRACTOR;

/// 攒够这么多行落一次库。200 行的单事务在实测规模（1472 首）下是毫秒级，
/// 同时保证"进程被杀最多丢 200 条结论"。
const FLUSH_ROWS: usize = 200;

/// 攒不满也要落：进度数字要跟着动，否则用户看着一条不动的进度条会以为卡死。
const FLUSH_INTERVAL: Duration = Duration::from_millis(400);

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
    if s.len() > 64
        || !s
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return Err(format!("增强源名字不合法：{s}"));
    }
    Ok(s.to_string())
}

/* ---------------- 存储与快照 ---------------- */

fn store_of(app: &tauri::AppHandle) -> Result<Arc<Store>, String> {
    app.try_state::<crate::AppState>()
        .ok_or_else(|| "AppState 未就绪".to_string())?
        .enrich_store(app)
}

/// 曲库当前在线文件的快照（`path_lower → path/size/mtime`）。
///
/// 一次会话只取一次：会话中途曲库变了，宁可这一轮少算几首（下一轮自然补），
/// 也不要让同一批里的行忽快忽慢地失效。
fn arena_of(app: &tauri::AppHandle) -> Result<Arena, String> {
    let state = app
        .try_state::<crate::AppState>()
        .ok_or_else(|| "AppState 未就绪".to_string())?;
    let files = state
        .db()
        .send(|conn| aiya_data::repo::online_files(conn))
        .map_err(|e| e.to_string())?;
    Ok(Arena::from_rows(
        files
            .into_iter()
            .map(|f| (f.path, f.path_lower, f.size_bytes, f.mtime)),
    ))
}

/* ---------------- 读：清单与统计 ---------------- */

/// 一张卡需要的全部数字 + 清单预览。
pub fn worklist(
    app: &tauri::AppHandle,
    source: &str,
    include_empty: bool,
) -> Result<EnrichmentWorklist, String> {
    let (stats, preview) = {
        let store = store_of(app)?;
        let arena = arena_of(app)?;
        let stats = store.stats(source, &arena).map_err(|e| e.to_string())?;
        let preview = store
            .worklist(source, &arena, include_empty, PREVIEW)
            .map_err(|e| e.to_string())?;
        (stats, preview)
    };

    let available = crate::beets::available(app);
    let note = (!available)
        .then(|| "未找到 aiya-beets 组件：先安装组件，或检查设置里的组件路径".to_string());
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
///
/// `limit` 传 `i64::MAX` 而不是 `usize::MAX`：后者 `as i64` 会变成 `-1`，
/// 而 SQLite 恰好把 `LIMIT -1` 当"不限"——那是个会让"清单只剩一条"的巧合。
pub fn work_paths(
    app: &tauri::AppHandle,
    source: &str,
    include_empty: bool,
) -> Result<Vec<String>, String> {
    let store = store_of(app)?;
    let arena = arena_of(app)?;
    store
        .worklist(source, &arena, include_empty, i64::MAX as usize)
        .map_err(|e| e.to_string())
}

/// 按维度查询（曲库页/智能歌单）。
///
/// 结论库不认识 `track_id`，所以这里要回曲库换一次。没换到的行**直接丢**：
/// 结论还在（等 GC 收），但"点开已经没有的曲目"出现在结果里只是噪音。
pub fn find(
    app: &tauri::AppHandle,
    filter: aiya_beets_store::Filter,
) -> Result<Vec<EnrichmentMatch>, String> {
    let store = store_of(app)?;
    let rows = store.find(&filter).map_err(|e| e.to_string())?;
    if rows.is_empty() {
        return Ok(Vec::new());
    }
    let keys: Vec<String> = rows.iter().map(|r| r.path_lower.clone()).collect();
    let state = app
        .try_state::<crate::AppState>()
        .ok_or_else(|| "AppState 未就绪".to_string())?;
    let ids = state
        .db()
        .send(move |conn| aiya_data::repo::track_ids_by_paths(conn, &keys))
        .map_err(|e| e.to_string())?;
    Ok(rows
        .into_iter()
        .filter_map(|r| {
            let track_id = ids.get(&r.path_lower)?.clone();
            Some(EnrichmentMatch {
                track_id,
                mood_label: r.mood_label,
                mood_score: r.mood_score,
                bpm: r.bpm,
                danceability: r.danceability,
                loudness: r.loudness,
                analyzed_at_ms: r.analyzed_at * 1000,
            })
        })
        .collect())
}

/// 清理残留行（曲库里已经没有对应曲目且 N 天没更新过）。
pub fn gc(app: &tauri::AppHandle, source: &str, keep_days: u32) -> Result<EnrichmentGc, String> {
    let store = store_of(app)?;
    let arena = arena_of(app)?;
    let removed = store
        .gc(source, now_secs(), keep_days, &arena)
        .map_err(|e| e.to_string())?;
    Ok(EnrichmentGc {
        removed: removed as u32,
        keep_days,
    })
}

/* ---------------- 写：会话期间的行缓冲与落库 ---------------- */

/// 会话期间的累计计数（落库线程写、收尾时读一次）。
#[derive(Debug, Default)]
struct Tally {
    written: AtomicUsize,
    /// 状态是 failed/skipped，宿主不许写行（写了等于永久放弃这个文件）
    skipped: AtomicUsize,
    /// 曲库快照里没有这个路径（扫描器还没收录 / 路径已漂移）
    not_in_library: AtomicUsize,
    /// 没能落库（DB 出错）。下一次分析会重算它们。
    deferred: AtomicUsize,
    batches: AtomicUsize,
}

struct Shared {
    pending: Mutex<Vec<Item>>,
    closed: AtomicBool,
    tally: Tally,
}

/// 会话内的落库器：既是 `EventSink`（转事件），也是行的接收端。
///
/// ## 为什么必须有 `Drop`
///
/// `beets::start_enrich` 先 `Sink::start`（起落库线程），再 `run_session_with`。
/// 后者会失败（已有任务在执行 → `Busy`、进程起不来 → `Spawn`），而在那条
/// 路径上 `on_exit` **不会执行**。没有 `Drop` 的话，每点一次失败的分析就漏
/// 一个线程，它带着 `Arc<Store>` 永久 sleep——用户看不到任何症状，直到点了几
/// 十次之后才发现"内存一直涨"。
///
/// `finish()` 幂等（`finished` 标志）：正常路径调一次、`Drop` 再兜一次，
/// `enrichPersisted` 只会发一条。
pub struct Sink {
    app: tauri::AppHandle,
    shared: Arc<Shared>,
    flusher: Mutex<Option<std::thread::JoinHandle<()>>>,
    finished: AtomicBool,
}

impl Sink {
    /// 起一个落库线程，返回可以直接交给会话的 sink。
    pub fn start(app: tauri::AppHandle) -> Result<Self, String> {
        let store = store_of(&app)?;
        let alive = Arc::new(arena_of(&app)?);

        let shared = Arc::new(Shared {
            pending: Mutex::new(Vec::new()),
            closed: AtomicBool::new(false),
            tally: Tally::default(),
        });
        let worker = Arc::clone(&shared);
        let store_ref = Arc::clone(&store);
        let alive_ref = Arc::clone(&alive);
        let emit_app = app.clone();
        let flusher = std::thread::Builder::new()
            .name("aiya-enrich-writer".into())
            .spawn(move || flush_loop(&worker, &store_ref, &alive_ref, &emit_app))
            .map_err(|e| format!("起落库线程失败：{e}"))?;
        Ok(Sink {
            app,
            shared,
            flusher: Mutex::new(Some(flusher)),
            finished: AtomicBool::new(false),
        })
    }

    /// 会话结束：等落库线程把余量写完，报一条带计数的终态事件。
    ///
    /// 为什么由这里报而不是 sidecar 报：`enrichDone` 说的是"工具跑了什么"，
    /// 这条说的是"库里真落了什么"。两者不同（跳过/库外/写失败都会让它们
    /// 不等），排障时必须能分开看。
    pub fn finish(&self) {
        if self.finished.swap(true, Ordering::SeqCst) {
            return;
        }
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

impl Drop for Sink {
    fn drop(&mut self) {
        // 会话正常结束时 `finish()` 已经跑过，这里是那条路径的兜底：
        // 线程不能因为"启动失败了"就永远挂着。
        self.finish();
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
fn item_from_event(v: &serde_json::Value) -> Option<Item> {
    let path = v.get("path").and_then(|x| x.as_str())?.trim().to_string();
    let source = v.get("source").and_then(|x| x.as_str())?.trim().to_string();
    if path.is_empty() || source.is_empty() {
        return None;
    }
    Some(Item {
        path,
        source,
        status: v
            .get("status")
            .and_then(|x| x.as_str())
            .unwrap_or("failed")
            .to_string(),
        fields: v
            .get("fields")
            .cloned()
            .unwrap_or_else(|| serde_json::json!({})),
    })
}

fn flush_loop(
    shared: &Arc<Shared>,
    store: &Arc<Store>,
    alive: &Arc<Arena>,
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
        match store.save(now_secs(), &batch, alive) {
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
            }
            Err(e) => {
                // 自己的库里写失败没有"等一会儿再来"这一说（没有第二个写者），
                // 只能是磁盘/权限问题。这批行留待下次重算。
                tracing::warn!(error = %e, rows = batch.len(), "索引增强落库失败，这批行留待下次重算");
                shared
                    .tally
                    .deferred
                    .fetch_add(batch.len(), Ordering::SeqCst);
            }
        }
    }
}

fn take_batch(pending: &Mutex<Vec<Item>>, max: usize) -> Vec<Item> {
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
        assert!(
            normalize_source(Some("a/b".into())).is_err(),
            "路径分隔符会毁掉事件识别"
        );
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
        let pending = Mutex::new(vec![
            Item {
                path: "a".into(),
                source: "s".into(),
                status: "ok".into(),
                fields: serde_json::json!({}),
            },
            Item {
                path: "b".into(),
                source: "s".into(),
                status: "ok".into(),
                fields: serde_json::json!({}),
            },
            Item {
                path: "c".into(),
                source: "s".into(),
                status: "ok".into(),
                fields: serde_json::json!({}),
            },
        ]);
        let first = take_batch(&pending, 2);
        assert_eq!(
            first.iter().map(|i| i.path.as_str()).collect::<Vec<_>>(),
            vec!["a", "b"]
        );
        let rest = take_batch(&pending, 2);
        assert_eq!(rest.len(), 1, "余量留给下一批，不能丢");
        assert!(take_batch(&pending, 2).is_empty());
    }

    /// 端到端：**真 sidecar** 发的 `enrichItem` → 宿主解析 → 落 beets 自己的结论库。
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
    fn real_sidecar_events_land_in_the_store() {
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
        let home = root.join("beets-index");
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
            // 与生产同一套环境（生产由 `beets::env_pairs` 下发）
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
        let mut rows: Vec<Item> = Vec::new();
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

        // 快照：按真实文件造 Arena（生产里这份数据来自 `repo::online_files`）
        let arena = Arena::from_rows(paths.iter().map(|p| {
            let meta = std::fs::metadata(p).unwrap();
            let mtime = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            (
                p.to_string_lossy().into_owned(),
                p.to_string_lossy().to_lowercase(),
                meta.len() as i64,
                mtime,
            )
        }));

        let store = Store::open(&home.join("enrich.db")).unwrap();
        let saved = store.save(1_000, &rows, &arena).unwrap();
        assert_eq!(saved.written, 2, "{saved:?}");
        assert_eq!(saved.not_in_library, 0, "path_key 口径必须与曲库一致");

        let hit = store
            .find(&aiya_beets_store::Filter {
                source: Some("probe".into()),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(hit.len(), 2, "提升列没填上：{hit:?}");
        for row in &hit {
            let label = row.mood_label.as_deref().unwrap_or("");
            assert!(
                aiya_beets_store::MOOD_CLASSES.contains(&label),
                "mood_label 不是已知情绪类：{label}"
            );
            assert!(row.bpm.is_some(), "bpm 没提升成列");
        }

        let _ = std::fs::remove_dir_all(&root);
    }
}
