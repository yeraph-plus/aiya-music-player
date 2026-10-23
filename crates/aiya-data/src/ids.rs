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

pub fn playlist_item_id(playlist_id: &str, track_id: &str, position: i64) -> String {
    Uuid::new_v5(&NS_PLAYLIST_ITEM, format!("{playlist_id}|{track_id}|{position}").as_bytes())
        .to_string()
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
}
