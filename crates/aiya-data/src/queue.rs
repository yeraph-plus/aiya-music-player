//! 播放队列持久化（queue_state 单行 + queue_items 位置表）。
//!
//! 决策（对齐旧仓库 ADR-018）：
//! - queue_items 全量重写（事务内 DELETE+INSERT），不做增量 diff —— 队列几百行 <5ms，简单可靠
//! - 随机排列不落库：恢复时按原序 + shuffle 开关重新洗牌（当前曲保持在原位）
//! - 队列里的曲目若已被删除（FK CASCADE 清了 queue_items 行），恢复时静默跳过

use rusqlite::Connection;

use crate::DbError;

/// queue_state 单行的播放会话状态。
#[derive(Debug, Clone, PartialEq)]
pub struct QueuePersist {
    /// 当前曲目在队列原序中的下标；-1 = 无
    pub current_index: i64,
    pub position_ms: i64,
    /// off | all | one
    pub repeat_mode: String,
    pub shuffle: bool,
    pub volume: f64,
    pub muted: bool,
    pub rate: f64,
}

impl Default for QueuePersist {
    fn default() -> Self {
        Self {
            current_index: -1,
            position_ms: 0,
            repeat_mode: "off".into(),
            shuffle: false,
            volume: 0.85,
            muted: false,
            rate: 1.0,
        }
    }
}

/// 读取队列：状态 + 原序 track_ids（按 position 排）。
/// 无持久化队列（items 为空）时返回 None。
///
/// 两条查询包进同一只读事务，避免读到「state 新 / items 旧」的中间态。
/// current_index 归一化：语义上 -1 = 无当前曲目（见 QueuePersist 注释），因此
///   * ids 为空 → 整个队列视为无，返回 None（防御 items 被清光但状态残留）
///   * 超出尾部 → clamp 到 len-1
///   * < -1（脏数据）→ clamp 回 -1 哨兵；-1 本身保留原语义
pub fn load_queue(conn: &Connection) -> Result<Option<(QueuePersist, Vec<String>)>, DbError> {
    let tx = conn.unchecked_transaction().map_err(DbError::Sql)?;
    let mut state = tx
        .query_row(
            "SELECT current_index, position_ms, repeat_mode, shuffle, volume, muted, rate
             FROM queue_state WHERE id = 1",
            [],
            |r| {
                Ok(QueuePersist {
                    current_index: r.get(0)?,
                    position_ms: r.get(1)?,
                    repeat_mode: r.get(2)?,
                    shuffle: r.get::<_, i64>(3)? != 0,
                    volume: r.get(4)?,
                    muted: r.get(5)?,
                    rate: r.get(6)?,
                })
            },
        )
        .map_err(DbError::Sql)?;
    let ids: Vec<String> = tx
        .prepare("SELECT track_id FROM queue_items ORDER BY position")?
        .query_map([], |r| r.get(0))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(DbError::Sql)?;
    tx.commit().map_err(DbError::Sql)?;

    if ids.is_empty() {
        return Ok(None);
    }
    // 归一化越界的 current_index（队列被 CASCADE 删短后常见）
    let max = (ids.len() - 1) as i64;
    if state.current_index > max {
        state.current_index = max;
    } else if state.current_index < -1 {
        state.current_index = -1; // -1 = 无当前曲目，是合法哨兵；更小的值是脏数据
    }
    Ok(Some((state, ids)))
}

/// 全量落库：队列结构 + 状态（切歌/清空/退出时调用）。
pub fn save_queue_full(
    conn: &mut Connection,
    p: &QueuePersist,
    track_ids: &[String],
) -> Result<(), DbError> {
    let tx = conn.transaction().map_err(DbError::Sql)?;
    tx.execute("DELETE FROM queue_items", [])
        .map_err(DbError::Sql)?;
    for (i, tid) in track_ids.iter().enumerate() {
        tx.execute(
            "INSERT INTO queue_items (position, track_id) VALUES (?1, ?2)",
            rusqlite::params![i as i64, tid],
        )
        .map_err(DbError::Sql)?;
    }
    tx.execute(
        "UPDATE queue_state SET current_index=?1, position_ms=?2, repeat_mode=?3,
         shuffle=?4, volume=?5, muted=?6, rate=?7, updated_at=?8 WHERE id=1",
        rusqlite::params![
            p.current_index,
            p.position_ms,
            p.repeat_mode,
            p.shuffle as i64,
            p.volume,
            p.muted as i64,
            p.rate,
            now_ms(),
        ],
    )
    .map_err(DbError::Sql)?;
    tx.commit().map_err(DbError::Sql)
}

/// 只落状态行（position/音量等节流落库用），不动 queue_items。
pub fn save_queue_state(conn: &Connection, p: &QueuePersist) -> Result<(), DbError> {
    conn.execute(
        "UPDATE queue_state SET current_index=?1, position_ms=?2, repeat_mode=?3,
         shuffle=?4, volume=?5, muted=?6, rate=?7, updated_at=?8 WHERE id=1",
        rusqlite::params![
            p.current_index,
            p.position_ms,
            p.repeat_mode,
            p.shuffle as i64,
            p.volume,
            p.muted as i64,
            p.rate,
            now_ms(),
        ],
    )
    .map_err(DbError::Sql)?;
    Ok(())
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::open_conn;
    use std::path::Path;

    /// 插入最小合法曲目行（queue_items 的 FK 目标）。
    fn ins_track(conn: &rusqlite::Connection, id: &str) {
        conn.execute(
            "INSERT INTO tracks (id, title, title_norm, added_at, path, path_lower) VALUES (?1, ?1, ?1, 0, ?2, lower(?2))",
            rusqlite::params![id, format!("D:/m/{id}.flac")],
        )
        .unwrap();
    }

    #[test]
    fn save_and_load_roundtrip() {
        let mut conn = open_conn(Path::new(":memory:")).unwrap();
        for id in ["a", "b", "c"] {
            ins_track(&conn, id);
        }
        let p = QueuePersist {
            current_index: 2,
            position_ms: 45_000,
            repeat_mode: "all".into(),
            shuffle: true,
            volume: 0.6,
            muted: true,
            rate: 1.5,
        };
        let ids = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        save_queue_full(&mut conn, &p, &ids).unwrap();

        let (p2, ids2) = load_queue(&conn).unwrap().unwrap();
        assert_eq!(p2, p);
        assert_eq!(ids2, ids);
    }

    #[test]
    fn load_empty_returns_none() {
        let conn = open_conn(Path::new(":memory:")).unwrap();
        assert!(load_queue(&conn).unwrap().is_none());
    }

    #[test]
    fn state_only_save_keeps_items() {
        let mut conn = open_conn(Path::new(":memory:")).unwrap();
        for id in ["a", "b"] {
            ins_track(&conn, id);
        }
        save_queue_full(
            &mut conn,
            &QueuePersist::default(),
            &["a".to_string(), "b".to_string()],
        )
        .unwrap();

        let p = QueuePersist {
            current_index: 1,
            position_ms: 12_000,
            repeat_mode: "one".into(),
            shuffle: false,
            volume: 0.5,
            muted: false,
            rate: 0.75,
        };
        save_queue_state(&conn, &p).unwrap();

        let (p2, ids2) = load_queue(&conn).unwrap().unwrap();
        assert_eq!(p2, p);
        assert_eq!(ids2, vec!["a", "b"], "state-only 落库不动 items");
    }

    #[test]
    fn full_save_replaces_previous_items() {
        let mut conn = open_conn(Path::new(":memory:")).unwrap();
        for id in ["a", "b", "c"] {
            ins_track(&conn, id);
        }
        save_queue_full(
            &mut conn,
            &QueuePersist::default(),
            &["a".to_string(), "b".to_string()],
        )
        .unwrap();
        save_queue_full(
            &mut conn,
            &QueuePersist {
                current_index: 0,
                ..QueuePersist::default()
            },
            &["c".to_string()],
        )
        .unwrap();
        let (_, ids) = load_queue(&conn).unwrap().unwrap();
        assert_eq!(ids, vec!["c"], "全量重写不留旧行");
    }

    #[test]
    fn load_clamps_out_of_range_current_index() {
        let mut conn = open_conn(Path::new(":memory:")).unwrap();
        for id in ["a", "b"] {
            ins_track(&conn, id);
        }
        // 模拟曲目被删除导致 current_index 越界（items 被 CASCADE 删短）
        save_queue_full(
            &mut conn,
            &QueuePersist { current_index: 5, ..QueuePersist::default() },
            &["a".to_string(), "b".to_string()],
        )
        .unwrap();
        // 直接改 queue_state 制造越界（save 不做校验，防御在 load 侧）
        conn.execute("UPDATE queue_state SET current_index = 5", []).unwrap();
        let (p, ids) = load_queue(&conn).unwrap().unwrap();
        assert_eq!(ids.len(), 2);
        assert_eq!(p.current_index, 1, "超出尾部 clamp 到 len-1");

        conn.execute("UPDATE queue_state SET current_index = -9", []).unwrap();
        let (p2, _) = load_queue(&conn).unwrap().unwrap();
        assert_eq!(p2.current_index, -1, "脏负值 clamp 回 -1 哨兵");
    }
}
