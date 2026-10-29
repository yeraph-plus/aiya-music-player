//! beets 侧会用到的曲库查询（docs/43 §8-E 终态）。
//!
//! 历史上这个模块还承载"dump → sync_items 读模型对账"（docs/08 §5 的
//! 自闭环）；那条链已随 docs/43 §8-E **整体移除**——beets 是纯手动增强
//! 工具，曲库的成员资格与行数据 100% 归播放器自己的扫描器，beets 不能
//! 向曲库注入或更新任何行。现在这里只剩 `online_paths`：adopt（手动四件套
//! 之一）需要把曲库的在线文件登记进 beets 库时的路径来源。
//!
//! `tracks.beets_item_id` 列保留（schema 已有），值不再有写入方；
//! 未来的"指定曲库文件跑 beets"显式命令按路径定位，不依赖它。

use rusqlite::Connection;

use crate::db::DbResult;

/// 曲库当前全部**在线**文件的路径（adopt 建基线用）。
///
/// 只取在线行：availability 由扫描器维护，missing 的文件登记进 beets
/// 只会让 update/move 对着不存在的文件空转。顺序按 path_lower（小写）
/// ——稳定排序让冒烟与断言可复现，不是大小写敏感的 path。
pub fn online_paths(conn: &Connection) -> DbResult<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT path FROM tracks WHERE availability = 'online' ORDER BY path_lower",
    )?;
    let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_in_memory;

    #[test]
    fn online_paths_skips_missing_and_orders_by_path() {
        let conn = open_in_memory().unwrap();
        // 三条：B（在线）/a（在线）/C（缺失）——按扫描器的最小行形态种行
        for (path, title) in [
            (r"C:\m\B.mp3", "B"),
            (r"C:\m\a.mp3", "a"),
            (r"C:\m\C.mp3", "C"),
        ] {
            conn.execute(
                "INSERT INTO tracks (id, title, title_norm, added_at, path, path_lower, availability)
                 VALUES (?1, ?2, ?3, 100, ?4, ?5, 'online')",
                rusqlite::params![crate::ids::track_id(path), title, title, path, path.to_lowercase()],
            )
            .unwrap();
        }
        conn.execute("UPDATE tracks SET availability = 'missing' WHERE path_lower LIKE '%c.mp3'", [])
            .unwrap();

        // 只取在线行；顺序按 path_lower（小写）——不是按 id、也不是大小写敏感的 path
        assert_eq!(online_paths(&conn).unwrap(), vec![r"C:\m\a.mp3", r"C:\m\B.mp3"]);
    }
}
