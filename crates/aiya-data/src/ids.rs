//! 稳定 ID：从规范化路径派生（uuid v5）。
//!
//! 为什么不用随机 uuid：同一文件重扫必须得到同一 ID，否则
//! 播放列表/队列里的引用会在每次扫描后全部失效。

use uuid::Uuid;

/// 本项目私有命名空间（uuid 1.28 无 uuid! 宏，用 from_u128 常量）。固定不依赖运行环境。
pub const NS_TRACK: Uuid = Uuid::from_u128(0x8f0c_4a1e_6e2f_4b3a_9d5c_1a2b_3c4d_5e6f);
pub const NS_ARTIST: Uuid = Uuid::from_u128(0x9a1d_5b2f_7f30_4c4b_ae6d_2b3c_4d5e_6f70);
pub const NS_ALBUM: Uuid = Uuid::from_u128(0xab2e_6c30_8041_4d5c_bf7e_3c4d_5e6f_7081);
pub const NS_PLAYLIST: Uuid = Uuid::from_u128(0xbc3f_7d41_9152_4e6d_a08f_4d5e_6f70_8192);
pub const NS_PLAYLIST_ITEM: Uuid = Uuid::from_u128(0xcd40_8e52_a263_4f7e_b190_5e6f_7081_92a3);
pub const NS_ARTWORK: Uuid = Uuid::from_u128(0xde51_9f63_b374_408f_a2a1_6f70_8192_a3b4);

pub fn track_id(path: &str) -> String {
    Uuid::new_v5(&NS_TRACK, path.as_bytes()).to_string()
}

pub fn artist_id(name: &str) -> String {
    Uuid::new_v5(&NS_ARTIST, name.as_bytes()).to_string()
}

pub fn album_id(title: &str, artist_name: &str) -> String {
    Uuid::new_v5(&NS_ALBUM, format!("{artist_name}||{title}").as_bytes()).to_string()
}

pub fn playlist_id(name: &str) -> String {
    Uuid::new_v5(&NS_PLAYLIST, name.as_bytes()).to_string()
}

/// 歌单行 ID：**一次性身份，不参与任何派生**（旧仓库用 randomUUID，同此意）。
///
/// 为什么不能用 `playlist_item_id(pid, tid, position)` 这种位置派生：position
/// 会随增删重排，而 id 一旦写入就不再变。于是"删掉第 0 项（B 从 1 挪到 0）、
/// 再把 B 加回来"时，新行算出的 id == 旧行仍占用的 id → 主键冲突 →
/// `INSERT OR IGNORE` 静默吞掉，表现为"加了但列表里没有"（`added` 还会是 1）。
/// 这里用"纳秒时间戳 + 进程内自增"做盐走 v5，无需引入随机数依赖。
pub fn playlist_item_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    // 同一纳秒内最多 65536 次插入仍不撞；跨进程重启靠 nanos 天然错开
    Uuid::new_v5(&NS_PLAYLIST_ITEM, format!("{nanos}:{}", n & 0xffff).as_bytes()).to_string()
}

pub fn artwork_id(content_hash: &str) -> String {
    Uuid::new_v5(&NS_ARTWORK, content_hash.as_bytes()).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stable_across_calls() {
        let a = track_id("D:/music/a.flac");
        let b = track_id("D:/music/a.flac");
        assert_eq!(a, b);
        assert_ne!(track_id("D:/music/b.flac"), a);
    }

    /// 歌单行 id 必须每次调用都不同：它只能当行身份，不能当"位置/曲目"的
    /// 派生键，否则重排后会与残留行撞主键（详见函数注释）。
    #[test]
    fn playlist_item_ids_are_unique() {
        let mut seen = std::collections::HashSet::new();
        for _ in 0..2_000 {
            assert!(seen.insert(playlist_item_id()), "歌单行 id 重复");
        }
    }
}
