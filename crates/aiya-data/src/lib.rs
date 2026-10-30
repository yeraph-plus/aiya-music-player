//! AIYA Music 数据层。
//!
//! 边界（crates/README.md）：不依赖 Tauri、不依赖音频平台细节。
//! SQLite 连接由专用线程独占（`rusqlite::Connection` 非 `Sync`），
//! 调用方通过 `DbHandle` 提交闭包，经有界 channel 进线程，结果经 reply channel 返回。

pub mod artwork;
pub mod db;
pub mod facets;
pub mod history;
pub mod ids;
pub mod lyrics;
pub mod norm;
pub mod playlist;
pub mod playlist_io;
pub mod queue;
pub mod repo;
pub mod scanner;
pub mod smart;

pub use artwork::{find_artwork, link, save_artwork, stage, StagedArtwork, ARTWORK_SCHEME};
pub use db::{open, open_conn, DbError, DbHandle, DbResult};
pub use queue::{load_queue, save_queue_full, save_queue_state, QueuePersist};
pub use repo::{TrackRow, TrackSource, TrackWindowQuery};
