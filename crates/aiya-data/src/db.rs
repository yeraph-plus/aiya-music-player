//! SQLite 连接所有权与迁移。
//!
//! 为什么专用线程：`rusqlite::Connection` 不是 `Sync`，且 docs/02 §5.1 要求
//! 只有宿主进程打开 SQLite。调用方拿到的是 `DbHandle`（Clone），
//! 每次调用生成一个 reply channel，DB 线程串行执行闭包 —— 天然无锁。

use std::path::Path;
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

// 单一迁移：schema v1 压平基线（项目未发布无存量库，0001+0002+0003 已合并，
// 见 migrations/0001_init.sql 头注）。后续破坏性演进继续往前追加编号即可。
const MIGRATIONS: &[&str] = &[include_str!("../migrations/0001_init.sql")];

type Job = Box<dyn FnOnce(&mut Connection) + Send>;

/// 打开（或创建）数据库，执行 PRAGMA 与 v1 基线迁移，并启动 DB 线程。
pub fn open(path: &Path) -> DbResult<DbHandle> {
    let conn = Connection::open(path)?;
    configure(&conn)?;
    migrate(&conn)?;
    let (tx, rx): (SyncSender<Job>, Receiver<Job>) = mpsc::sync_channel(256);
    let worker = thread::Builder::new()
        .name("aiya-db".into())
        .spawn(move || run_worker(conn, rx))
        .map_err(|e| DbError::Custom(e.to_string()))?;
    Ok(DbHandle { tx: Some(tx), worker: Arc::new(Mutex::new(Some(worker))) })
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
    let conn = Connection::open(path)?;
    configure(&conn)?;
    migrate(&conn)?;
    Ok(conn)
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
    // 防呆：库比应用新（降级启动/换了空迁移链）时拒绝打开，避免用旧代码
    // 读写新结构造成静默损坏。压平后老开发库（user_version=3）同样会被拦下，
    // 删库重扫即可（当时无存量数据）。
    if version > MIGRATIONS.len() as i64 {
        return Err(DbError::Custom(format!(
            "数据库 user_version={version} 比应用支持的 {max} 新，拒绝打开（旧开发库请直接删除后重扫）",
            max = MIGRATIONS.len()
        )));
    }
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
#[derive(Clone)]
pub struct DbHandle {
    tx: Option<SyncSender<Job>>,
    worker: Arc<Mutex<Option<thread::JoinHandle<()>>>>,
}

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
    /// 必须先释放发送端，worker 的 `recv` 才会返回 Err 而退出。
    pub fn close(&mut self) {
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
            .query_row("SELECT COUNT(*) FROM sqlite_master WHERE type='table'", [], |r| r.get(0))
            .unwrap();
        assert!(n >= 9, "expect >= 9 tables, got {n}");
        let q: i64 = conn.query_row("SELECT COUNT(*) FROM queue_state", [], |r| r.get(0)).unwrap();
        assert_eq!(q, 1, "queue_state singleton row must exist");
        let uv: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
        assert_eq!(uv, MIGRATIONS.len() as i64, "all migrations applied");
        // artworks 表与 tracks.artwork_id 列存在
        let aw: i64 = conn
            .query_row("SELECT COUNT(*) FROM sqlite_master WHERE name='artworks'", [], |r| r.get(0))
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
    }

    #[test]
    fn handle_roundtrip_and_close() {
        // 每进程唯一文件名：避免上次运行的半迁移库残留影响
        let path = std::env::temp_dir().join(format!("aiya-test-handle-{}.db", std::process::id()));
        let mut db = open(&path).unwrap();
        let answer: i64 = db.send(|conn| {
            Ok(conn.query_row("SELECT 40 + 2", [], |r| r.get(0))?)
        }).unwrap();
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
}
