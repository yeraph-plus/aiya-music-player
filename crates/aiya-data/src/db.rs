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

const INIT_SQL: &str = include_str!("../migrations/0001_init.sql");

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

fn configure(conn: &Connection) -> DbResult<()> {
    // docs/02 §5.1 的启动 PRAGMA；WAL 让读不阻塞写，NORMAL 在掉电时最多丢最后事务
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    conn.pragma_update(None, "busy_timeout", 5_000)?;
    Ok(())
}

fn migrate(conn: &Connection) -> DbResult<()> {
    let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    if version < 1 {
        conn.execute_batch(INIT_SQL)?;
        conn.pragma_update(None, "user_version", 1)?;
    }
    Ok(())
}

fn run_worker(mut conn: Connection, rx: Receiver<Job>) {
    while let Ok(job) = rx.recv() {
        job(&mut conn);
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
            let result = f(conn);
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
        assert_eq!(uv, 1);
    }

    #[test]
    fn handle_roundtrip_and_close() {
        let mut db = open(&std::env::temp_dir().join("aiya-test-handle.db")).unwrap();
        let answer: i64 = db.send(|conn| {
            Ok(conn.query_row("SELECT 40 + 2", [], |r| r.get(0))?)
        }).unwrap();
        assert_eq!(answer, 42);
        db.close();
    }
}
