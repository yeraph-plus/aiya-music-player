//! 播放历史写入与清空。
//!
//! 为什么集中在这里：`history` 行与 `tracks` 上的反范式统计列
//! （`play_count` / `last_played_at`）是一份数据的两种表示 —— 写和清都必须
//! 在**同一个事务**里完成。此前 `record_history` 只插 history、根本没人维护
//! `play_count`（UI 里每次播放都显示 0，mostPlayed 永远空），`history_clear`
//! 也只删 history，清完历史列表还在显示播放次数。

use rusqlite::{params, Connection, TransactionBehavior};

use crate::db::{DbError, DbResult};

/// 记录一次播放：插入历史行 + 同步曲目统计列（同事务）。
pub fn record(
    conn: &mut Connection,
    track_id: &str,
    played_at: i64,
    seconds_played: u64,
) -> DbResult<()> {
    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(DbError::Sql)?;
    tx.execute(
        "INSERT INTO history (track_id, played_at, seconds_played) VALUES (?1, ?2, ?3)",
        params![track_id, played_at, seconds_played as i64],
    )?;
    // 曲目可能刚被删（播放到一半扫描器清理）：UPDATE 命中 0 行不算错，
    // history 行会被 FK 级联带走，这里不额外处理。
    tx.execute(
        "UPDATE tracks SET play_count = play_count + 1, last_played_at = ?2 WHERE id = ?1",
        params![track_id, played_at],
    )?;
    tx.commit().map_err(DbError::Sql)
}

/// 清空历史 + 统计列归零（同事务）。返回是否真的清过（有行被删）。
pub fn clear(conn: &mut Connection) -> DbResult<bool> {
    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(DbError::Sql)?;
    let removed = tx.execute("DELETE FROM history", [])?;
    tx.execute(
        "UPDATE tracks SET play_count = 0, last_played_at = NULL
         WHERE play_count <> 0 OR last_played_at IS NOT NULL",
        [],
    )?;
    tx.commit().map_err(DbError::Sql)?;
    Ok(removed > 0)
}

/// 按保留周期裁剪历史行（防表膨胀）。`cutoff_secs` 之前的行删除。
///
/// 语义约定（2026-10-28 拍板）：history 表 = "保留周期内的播放记录"。
/// **只删 history 行，不动 tracks 的统计列**——`play_count` 是累计
/// 总次数（智能歌单的 playCount 条件用它，语义是"听过多少次"而非
/// "周期内听过几次"），`last_played_at` 是"最后一次"，两者都没有
/// 膨胀问题。周期内的"最近播放" tab 与常听榜靠聚合 history 行自然
/// 遵循周期（裁剪在启动/每次写历史/设置变更时执行，表内容≈周期内）。
///
/// 返回删掉的行数（0 = 无需裁剪，调用方可以跳过日志）。
pub fn prune(conn: &Connection, cutoff_secs: i64) -> DbResult<usize> {
    let n = conn.execute("DELETE FROM history WHERE played_at < ?1", [cutoff_secs])?;
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_in_memory;

    fn fixture() -> Connection {
        let conn = open_in_memory().unwrap();
        conn.execute_batch(
            r#"
            INSERT INTO tracks (id,title,title_norm,added_at,path,path_lower) VALUES
              ('t1','一','一',1,'/m/1.flac','/m/1.flac'),
              ('t2','二','二',2,'/m/2.flac','/m/2.flac');
            "#,
        )
        .unwrap();
        conn
    }

    #[test]
    fn record_updates_play_count_and_last_played_at() {
        let mut conn = fixture();
        record(&mut conn, "t1", 500, 30).unwrap();
        record(&mut conn, "t1", 900, 12).unwrap();

        let (count, last): (i64, i64) = conn
            .query_row("SELECT play_count, last_played_at FROM tracks WHERE id='t1'", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .unwrap();
        assert_eq!(count, 2);
        assert_eq!(last, 900, "取最后一次播放时间");
        let rows: i64 = conn
            .query_row("SELECT COUNT(*) FROM history WHERE track_id='t1'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(rows, 2);
        // 没播过的曲目不受影响
        let (c2, l2): (i64, Option<i64>) = conn
            .query_row("SELECT play_count, last_played_at FROM tracks WHERE id='t2'", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .unwrap();
        assert_eq!((c2, l2), (0, None));
    }

    #[test]
    fn clear_zeroes_stats_in_same_transaction() {
        let mut conn = fixture();
        record(&mut conn, "t1", 500, 30).unwrap();
        record(&mut conn, "t2", 600, 20).unwrap();
        assert!(clear(&mut conn).unwrap());

        let rows: i64 = conn.query_row("SELECT COUNT(*) FROM history", [], |r| r.get(0)).unwrap();
        assert_eq!(rows, 0);
        let nonzero: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM tracks WHERE play_count <> 0 OR last_played_at IS NOT NULL",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(nonzero, 0, "清历史必须把统计列一起归零");
        assert!(!clear(&mut conn).unwrap(), "空历史再清返回 false");
    }

    #[test]
    fn record_for_unknown_track_keeps_history_row() {
        // FK 关闭时（测试库默认开启）插入会失败；这里验证的是
        // "曲目不存在时不 panic、错误如实返回"
        let mut conn = fixture();
        let r = record(&mut conn, "不存在", 500, 10);
        assert!(r.is_err(), "FK 违例应当报错而不是静默成功");
    }

    #[test]
    fn prune_removes_only_rows_older_than_cutoff() {
        let mut conn = fixture();
        record(&mut conn, "t1", 100, 10).unwrap();
        record(&mut conn, "t2", 9_000, 10).unwrap();
        record(&mut conn, "t1", 9_900, 10).unwrap();

        // 周期内下限 5_000：t1@100 被裁，其余保留
        let removed = prune(&conn, 5_000).unwrap();
        assert_eq!(removed, 1);
        let rows: i64 = conn.query_row("SELECT COUNT(*) FROM history", [], |r| r.get(0)).unwrap();
        assert_eq!(rows, 2);

        // 统计列不受裁剪影响（累计语义，见 prune 注释）
        let count: i64 = conn
            .query_row("SELECT play_count FROM tracks WHERE id='t1'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 2);

        // 再裁一次同一周期：幂等（0 行）
        assert_eq!(prune(&conn, 5_000).unwrap(), 0);
    }
}
