//! SQLite 连接所有权与迁移。
//!
//! 为什么专用线程：`rusqlite::Connection` 不是 `Sync`，且 docs/02 §5.1 要求
//! 只有宿主进程打开 SQLite。调用方拿到的是 `DbHandle`（Clone），
//! 每次调用生成一个 reply channel，DB 线程串行执行闭包 —— 天然无锁。

use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread;

use rusqlite::Connection;

pub type DbResult<T> = Result<T, DbError>;

#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[error("sqlite: {0}")]
    Sql(#[from] rusqlite::Error),
    #[error("db thread closed")]
    Closed,
    #[error("{0}")]
    Custom(String),
}

// 迁移链：v1 压平基线（项目未发布无存量库，0001+0002+0003 曾是三个编号，
// 已合并进 0001，见 migrations/0001_init.sql 头注）；0002 起是真增量。
const MIGRATIONS: &[&str] = &[
    include_str!("../migrations/0001_init.sql"),
    include_str!("../migrations/0002_playlist_order.sql"),
    include_str!("../migrations/0003_smart_playlist.sql"),
    include_str!("../migrations/0004_history_retention.sql"),
    include_str!("../migrations/0005_facet_norm_indexes.sql"),
    include_str!("../migrations/0006_entity_artwork.sql"),
    include_str!("../migrations/0007_dynamic_materialized.sql"),
    include_str!("../migrations/0008_track_enrichment.sql"),
    include_str!("../migrations/0009_drop_track_enrichment.sql"),
    include_str!("../migrations/0010_queue_transient.sql"),
];

type Job = Box<dyn FnOnce(&mut Connection) + Send>;

/// 打开（或创建）数据库，执行 PRAGMA 与迁移，并启动 DB 线程。
pub fn open(path: &Path) -> DbResult<DbHandle> {
    let conn = open_checked(path)?;
    let (tx, rx): (SyncSender<Job>, Receiver<Job>) = mpsc::sync_channel(256);
    let worker = thread::Builder::new()
        .name("aiya-db".into())
        .spawn(move || run_worker(conn, rx))
        .map_err(|e| DbError::Custom(e.to_string()))?;
    Ok(DbHandle {
        tx: Some(tx),
        worker: Arc::new(Mutex::new(Some(worker))),
        senders: Arc::new(AtomicUsize::new(1)),
    })
}

/// 内存库（测试用）：同样的 PRAGMA + 迁移，但不开线程 —— 测试直接拿连接跑。
pub fn open_in_memory() -> DbResult<Connection> {
    let conn = Connection::open_in_memory()?;
    configure(&conn)?;
    migrate(&conn)?;
    Ok(conn)
}

/// 独立连接（扫描线程用）：与主 DB 线程并行，WAL 下读不阻塞写。
/// 只配 PRAGMA 与迁移，不开线程 —— 所有权归调用方。
pub fn open_conn(path: &Path) -> DbResult<Connection> {
    open_checked(path)
}

/// 打开 + 配置 + 版本体检 + 迁移，带损坏降级。
///
/// 两类不可恢复的打开失败都走"留底重建"（此前只处理第一类，损坏库会让
/// App 直接起不来且没有任何恢复手段——用户只能自己去 AppData 里找文件删）：
///
/// 1. **版本超前**（库比应用新，降级安装）：把库文件改名留底，新建空库。
/// 2. **库文件损坏**（SQLITE_NOTADB / SQLITE_CORRUPT，或头部短读导致的
///    同类失败）：重试没有意义（每次打开都在同一个坏字节上摔倒），
///    走同一套留底重建。旧文件一个字节不丢，用户想找回随时可以拿备份
///    覆盖回去；代价只是"要重扫/重新对齐一次"。
fn open_checked(path: &Path) -> DbResult<Connection> {
    match open_and_migrate(path) {
        Ok(conn) => Ok(conn),
        Err(e) if is_db_corruption(&e) => {
            // 出错路径上连接已随 open_and_migrate 的局部变量 drop（Windows
            // 上打开着的文件不能改名），可以安全挪走
            set_aside_files(path, "corrupt")?;
            tracing::warn!(
                from = %path.display(),
                error = %e,
                supported = MIGRATIONS.len(),
                "数据库文件损坏（非版本超前），已留底改名并新建空库，需要重新扫描/对齐"
            );
            let fresh = Connection::open(path)?;
            configure(&fresh)?;
            migrate(&fresh)?;
            Ok(fresh)
        }
        Err(e) => Err(e),
    }
}

/// 打开 + 配置 + 版本体检 + 迁移（无降级路径的裸流程）。
fn open_and_migrate(path: &Path) -> DbResult<Connection> {
    let conn = Connection::open(path)?;
    configure(&conn)?;
    let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    if version > MIGRATIONS.len() as i64 {
        // Windows 上打开着的文件不能改名，必须先关连接
        drop(conn);
        quarantine(path, version)?;
        let fresh = Connection::open(path)?;
        configure(&fresh)?;
        migrate(&fresh)?;
        return Ok(fresh);
    }
    migrate(&conn)?;
    Ok(conn)
}

/// 库文件级损坏（非版本超前）：NOTADB / CORRUPT。
/// 这两类错误重试无意义——同一个坏字节每次都摔倒，必须留底重建。
/// 刻意**不含**普通 IO 错误（磁盘满、U 盘拔出等是瞬态，留底重建会
/// 把可恢复的故障放大成数据丢失）。
fn is_db_corruption(e: &DbError) -> bool {
    use rusqlite::ffi::ErrorCode as EC;
    matches!(
        e,
        DbError::Sql(rusqlite::Error::SqliteFailure(f, _))
            if matches!(f.code, EC::NotADatabase | EC::DatabaseCorrupt)
    )
}

/// 把库文件（连同 -wal/-shm）改名留底，原路径留给新库。
///
/// 必须一起挪 WAL 附属文件：只挪主库的话，新建的空库会套用旧 WAL 里残留的
/// 页帧，等于把旧 schema 的脏页灌进新库（比版本检查本身更危险）。
/// `tag` 进后缀区分留底原因（`v{N}` = 版本超前，`corrupt` = 损坏）。
fn set_aside_files(path: &Path, tag: &str) -> DbResult<()> {
    let file_name = path
        .file_name()
        .ok_or_else(|| DbError::Custom(format!("非法数据库路径: {}", path.display())))?
        .to_string_lossy()
        .into_owned();
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let backup = path.with_file_name(format!("{file_name}.{tag}-{stamp}.bak"));
    std::fs::rename(path, &backup)
        .map_err(|e| DbError::Custom(format!("数据库留底改名失败（{e}）：{}", path.display())))?;
    for suffix in ["-wal", "-shm"] {
        let side = path.with_file_name(format!("{file_name}{suffix}"));
        if side.exists() {
            let _ = std::fs::rename(
                &side,
                path.with_file_name(format!("{file_name}{suffix}.bak")),
            );
        }
    }
    Ok(())
}

/// 版本超前的留底 + 留痕（损坏路径见 open_checked 的同名流程）。
fn quarantine(path: &Path, version: i64) -> DbResult<()> {
    set_aside_files(path, &format!("v{version}"))?;
    tracing::warn!(
        from = %path.display(),
        version,
        supported = MIGRATIONS.len(),
        "数据库版本超前（降级启动？），已留底改名并新建空库，需要重新扫描/对齐"
    );
    Ok(())
}

fn configure(conn: &Connection) -> DbResult<()> {
    // docs/02 §5.1 的启动 PRAGMA；WAL 让读不阻塞写，NORMAL 在掉电时最多丢最后事务
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    conn.pragma_update(None, "busy_timeout", 5_000)?;
    Ok(())
}

fn migrate(conn: &Connection) -> DbResult<()> {
    let mut version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    // 版本超前的库不会走到这里（open_checked 已把它留底改名后重建空库）；
    // 迁移按序补跑：每个迁移连同 PRAGMA user_version 一起包在一个事务里
    // （CREATE...ALTER 原子）。user_version 必须与 DDL 同事务写入 —— 否则
    // 两步之间崩溃会导致重放迁移报 already exists。BEGIN IMMEDIATE 拿写锁，
    // 防止两个连接同时首跑迁移的竞态。
    for (i, sql) in MIGRATIONS.iter().enumerate() {
        let v = (i + 1) as i64;
        if version < v {
            conn.execute_batch(&format!(
                "BEGIN IMMEDIATE;\n{sql}\nPRAGMA user_version = {v};\nCOMMIT;"
            ))?;
            version = v;
        }
    }
    Ok(())
}

fn run_worker(mut conn: Connection, rx: Receiver<Job>) {
    while let Ok(job) = rx.recv() {
        // panic 隔离：单个闭包 panic 不允许杀死 worker 线程
        //（否则后续所有 send 永久 Closed）。错误信封在 send() 的闭包内
        // 通过 catch_unwind 构造；这里再兜一层防御未捕获路径。
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| job(&mut conn)));
    }
    // channel 关闭（所有 handle drop）：正常退出路径
}

/// 克隆型句柄。Drop 全部句柄后 DB 线程退出。
///
/// 每个克隆持有**独立**的 SyncSender（channel 语义：所有发送端消失 worker
/// 的 recv 才返回 Err），`close()` 想等 worker 退出就必须知道还剩几个发送端
/// ——否则任一克隆存活时 close 会**永久阻塞**。`senders` 就是这个计数：
/// Clone 时 +1，Drop 时 -1，close 只在"只剩自己"时才 join。
pub struct DbHandle {
    tx: Option<SyncSender<Job>>,
    worker: Arc<Mutex<Option<thread::JoinHandle<()>>>>,
    /// 存活的发送端数量（含自己）。Clone/Drop 手动维护。
    senders: Arc<AtomicUsize>,
}

impl Clone for DbHandle {
    fn clone(&self) -> Self {
        self.senders.fetch_add(1, Ordering::SeqCst);
        DbHandle {
            tx: self.tx.clone(),
            worker: Arc::clone(&self.worker),
            senders: Arc::clone(&self.senders),
        }
    }
}

impl Drop for DbHandle {
    fn drop(&mut self) {
        // 字段（含 tx）在本函数之后才真正析构；这里只维护计数
        self.senders.fetch_sub(1, Ordering::SeqCst);
    }
}

/// close() 等其它克隆释放的上限。超时不代表失败——worker 会在剩余克隆
/// 全部 drop 后自然退出，只是调用方等不到 WAL 落盘的确认了。
const CLONE_WAIT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

impl DbHandle {
    /// 提交一个闭包到 DB 线程执行。闭包内部通过 `reply` 把结果发回去。
    pub fn send<T: Send + 'static>(
        &self,
        f: impl FnOnce(&mut Connection) -> DbResult<T> + Send + 'static,
    ) -> DbResult<T> {
        let (reply_tx, reply_rx) = mpsc::channel();
        let job: Job = Box::new(move |conn| {
            // panic 隔离：用户闭包 panic 时构造错误信封回发（否则接收端
            // recv 挂死/得到 Closed），worker 线程继续存活服务后续任务。
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(conn)))
                .unwrap_or_else(|_| Err(DbError::Custom("db job panicked".into())));
            // 接收方已放弃等待（panic/超时）时丢弃结果
            let _ = reply_tx.send(result);
        });
        self.tx
            .as_ref()
            .ok_or(DbError::Closed)?
            .send(job)
            .map_err(|_| DbError::Closed)?;
        reply_rx.recv().map_err(|_| DbError::Closed)?
    }

    /// 关闭 DB 线程并等待退出（进程退出前调用，保证 WAL 落盘）。
    ///
    /// 先等其它克隆全部释放（发送端计数只剩自己，上限 [`CLONE_WAIT_TIMEOUT`]；
    /// 超时则放弃 join 并记 warn——worker 会在剩余克隆 drop 后自然退出），
    /// 再丢弃自己的发送端、join worker。
    pub fn close(&mut self) {
        let deadline = std::time::Instant::now() + CLONE_WAIT_TIMEOUT;
        while self.senders.load(Ordering::SeqCst) > 1 {
            if std::time::Instant::now() >= deadline {
                let left = self.senders.load(Ordering::SeqCst) - 1;
                tracing::warn!(
                    clones = left,
                    "DbHandle::close: 仍有 {left} 个克隆句柄存活，放弃等待 DB 线程退出（它将在最后一个句柄 drop 后自然结束）"
                );
                self.tx = None;
                return;
            }
            thread::sleep(std::time::Duration::from_millis(10));
        }
        self.tx = None; // drop 最后一个 SyncSender
        if let Ok(mut guard) = self.worker.lock() {
            if let Some(worker) = guard.take() {
                let _ = worker.join();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrate_creates_schema() {
        let conn = open_in_memory().unwrap();
        let n: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(n >= 9, "expect >= 9 tables, got {n}");
        let q: i64 = conn
            .query_row("SELECT COUNT(*) FROM queue_state", [], |r| r.get(0))
            .unwrap();
        assert_eq!(q, 1, "queue_state singleton row must exist");
        let uv: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(uv, MIGRATIONS.len() as i64, "all migrations applied");
        // artworks 表与 tracks.artwork_id 列存在
        let aw: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE name='artworks'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(aw, 1);
        let has_col: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('tracks') WHERE name='artwork_id'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(has_col, 1);
        // 0005：facet 排序列有索引（没有时每页 ORDER BY 都要 temp B-tree）
        for (table, col) in [
            ("artists", "idx_artists_name_norm"),
            ("albums", "idx_albums_title_norm"),
        ] {
            let idx: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type='index' AND name = ?1",
                    [col],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(idx, 1, "{table} 的 {col} 缺失");
        }
        // 0009 的解耦防线：**应用库里不许出现 beets 的表**。
        //
        // 这条断言比它看起来重要。批 3 把 `track_enrichment` 建在这里，于是
        // 应用的数据模型里有一条"因为装了 beets 才有"的表——备份、回滚、第三方
        // 读库都得理解一条与它无关的记录。结论库现在在 `{beets-index}/enrich.db`
        // （`aiya-beets-store` 自己建表）。谁哪天想"顺手在这儿建一张"，会先撞上它。
        let beets_tables: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name LIKE '%beets%'
                    OR type='table' AND name LIKE '%enrich%'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            beets_tables, 0,
            "应用库里出现了 beets 模块的表：它应该住在 {{beets-index}}/enrich.db"
        );
    }

    #[test]
    fn handle_roundtrip_and_close() {
        // 每进程唯一文件名：避免上次运行的半迁移库残留影响
        let path = std::env::temp_dir().join(format!("aiya-test-handle-{}.db", std::process::id()));
        let mut db = open(&path).unwrap();
        let answer: i64 = db
            .send(|conn| Ok(conn.query_row("SELECT 40 + 2", [], |r| r.get(0))?))
            .unwrap();
        assert_eq!(answer, 42);
        db.close();
    }

    #[test]
    fn worker_survives_job_panic() {
        let path = std::env::temp_dir().join(format!("aiya-test-panic-{}.db", std::process::id()));
        let mut db = open(&path).unwrap();
        // 闭包 panic → 回发错误信封，worker 存活
        let r: DbResult<i64> = db.send(|_conn| panic!("boom"));
        assert!(r.is_err(), "panic 转成错误信封");
        let ok: i64 = db
            .send(|conn| Ok(conn.query_row("SELECT 1", [], |r| r.get(0))?))
            .expect("worker must still be alive after a panicking job");
        assert_eq!(ok, 1);
        db.close();
    }

    /// 降级启动（库比应用新）不能直接起不来：必须留底改名 + 建新库。
    #[test]
    fn future_version_db_is_quarantined_and_recreated() {
        let path = std::env::temp_dir().join(format!("aiya-test-future-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        {
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch("CREATE TABLE legacy_only(x); PRAGMA user_version = 99;")
                .unwrap();
        }

        let mut db = open(&path).expect("超前版本不能直接报错起不来");
        let (uv, legacy): (i64, i64) = db
            .send(|conn| {
                Ok((
                    conn.query_row("PRAGMA user_version", [], |r| r.get(0))?,
                    conn.query_row(
                        "SELECT COUNT(*) FROM sqlite_master WHERE name='legacy_only'",
                        [],
                        |r| r.get(0),
                    )?,
                ))
            })
            .unwrap();
        assert_eq!(uv, MIGRATIONS.len() as i64, "新库迁移全部跑完");
        assert_eq!(legacy, 0, "旧结构不会残留在新库里");

        // 备份留底：同目录下应有 xxx.db.v99-<stamp>.bak
        let dir = path.parent().unwrap();
        let prefix = format!("{}.v99-", path.file_name().unwrap().to_string_lossy());
        let backups: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.starts_with(&prefix) && n.ends_with(".bak"))
            .collect();
        assert_eq!(backups.len(), 1, "旧库必须有一个留底文件: {backups:?}");
        db.close();

        // 清理现场（含备份），避免污染后续运行
        let _ = std::fs::remove_file(&path);
        for name in backups {
            let _ = std::fs::remove_file(dir.join(name));
        }
    }

    /// P2 回归：库文件损坏（写满垃圾字节）时 open 不能直接失败——
    /// 走同一套 quarantine：留底改名 + 从零重建，open 成功且是全新库。
    #[test]
    fn corrupt_db_file_is_quarantined_and_recreated() {
        let path =
            std::env::temp_dir().join(format!("aiya-test-corrupt-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        // 非 SQLite 魔数的一堆垃圾字节 → SQLITE_NOTADB
        std::fs::write(
            &path,
            b"definitely not a sqlite database, just garbage bytes!!",
        )
        .unwrap();

        let mut db = open(&path).expect("损坏库不能让 App 起不来");
        let (uv, tracks, settings): (i64, i64, i64) = db
            .send(|conn| {
                Ok((
                    conn.query_row("PRAGMA user_version", [], |r| r.get(0))?,
                    conn.query_row("SELECT COUNT(*) FROM tracks", [], |r| r.get(0))?,
                    conn.query_row("SELECT COUNT(*) FROM settings", [], |r| r.get(0))?,
                ))
            })
            .unwrap();
        assert_eq!(uv, MIGRATIONS.len() as i64, "全新库迁移全部跑完");
        assert_eq!(
            (tracks, settings),
            (0, 1),
            "从零重建（queue_state/settings 单行在）"
        );

        // 留底：同目录应有 xxx.db.corrupt-<stamp>.bak
        let dir = path.parent().unwrap();
        let prefix = format!("{}.corrupt-", path.file_name().unwrap().to_string_lossy());
        let backups: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.starts_with(&prefix) && n.ends_with(".bak"))
            .collect();
        assert_eq!(backups.len(), 1, "损坏库必须留底: {backups:?}");
        db.close();

        let _ = std::fs::remove_file(&path);
        for name in backups {
            let _ = std::fs::remove_file(dir.join(name));
        }
    }

    /// close() 的克隆等待：计数随 Clone/Drop 增减；克隆释放后 close 正常返回。
    #[test]
    fn close_waits_for_sender_count_to_settle() {
        let path = std::env::temp_dir().join(format!("aiya-test-clones-{}.db", std::process::id()));
        let mut db = open(&path).unwrap();
        assert_eq!(db.senders.load(Ordering::SeqCst), 1);
        {
            let c1 = db.clone();
            let _c2 = db.clone();
            assert_eq!(db.senders.load(Ordering::SeqCst), 3, "Clone 必须 +1");
            drop(c1);
            assert_eq!(db.senders.load(Ordering::SeqCst), 2, "Drop 必须 -1");
        }
        assert_eq!(db.senders.load(Ordering::SeqCst), 1, "只剩自己");
        db.close();
        assert!(
            db.send::<i64>(|_| Ok(1)).is_err(),
            "close 后 send 报 Closed"
        );
        let _ = std::fs::remove_file(&path);
    }

    /// close() 时克隆被其它线程持有：不能永久阻塞——克隆在超时内释放则
    /// close 正常返回（超时 5s，这里 200ms 释放，用 4s 上限断言没有走超时路径）。
    #[test]
    fn close_unblocks_when_late_clone_is_dropped() {
        let path = std::env::temp_dir().join(format!("aiya-test-late-{}.db", std::process::id()));
        let mut db = open(&path).unwrap();
        let holder = db.clone();
        let releaser = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(200));
            drop(holder);
        });
        let started = std::time::Instant::now();
        db.close();
        assert!(
            started.elapsed() < std::time::Duration::from_secs(4),
            "close 在克隆释放后应尽快返回，实际 {:?}",
            started.elapsed()
        );
        releaser.join().unwrap();
        let _ = std::fs::remove_file(&path);
    }
}
