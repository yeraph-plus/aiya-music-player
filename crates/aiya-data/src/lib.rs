//! AIYA Music 数据层。
//!
//! 边界（crates/README.md）：不依赖 Tauri、不依赖音频平台细节。
//! SQLite 连接由专用线程独占（`rusqlite::Connection` 非 `Sync`），
//! 调用方通过 `DbHandle` 提交闭包，经有界 channel 进线程，结果经 reply channel 返回。

pub mod artwork;
pub mod beets_sync;
pub mod db;
pub mod history;
pub mod ids;
pub mod lyrics;
pub mod norm;
pub mod playlist;
pub mod playlist_io;
pub mod queue;
pub mod repo;
pub mod scanner;

pub use artwork::{ARTWORK_SCHEME, find_artwork, save_artwork};
pub use db::{DbError, DbHandle, DbResult, open, open_conn};
pub use queue::{QueuePersist, load_queue, save_queue_full, save_queue_state};
pub use repo::{TrackRow, TrackSource, TrackWindowQuery};
