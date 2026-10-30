//! 播放队列持久化（queue_state 单行 + queue_items 位置表）。
//!
//! 决策（对齐旧仓库 ADR-018）：
//! - queue_items 全量重写（事务内 DELETE+INSERT），不做增量 diff —— 队列几百行 <5ms，简单可靠
//! - 随机排列不落库：恢复时按原序 + shuffle 开关重新洗牌（当前曲保持在原位）
//! - 队列里的曲目若已被删除（FK CASCADE 清了 queue_items 行），恢复时静默跳过
//! - **队列可以是多态的**（docs/52 §6，迁移 0010）：`track` 走外键指向曲库，
//!   `file` / `stream` 是"能播但不进曲库"的临时项，只在 `payload` 里存一份
//!   最小显示字段，重启后照原样回到队列

use rusqlite::Connection;

use crate::DbError;

/// 队列元素种类（docs/52 §6.2）。
///
/// 存进 `queue_items.kind` 的就是这里的 `as_str()`；未知值按 `Track` 兜底
/// （旧库升级路径上不可能出现，但读到脏数据时不该整队崩）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum QueueKind {
    /// 曲库里的曲目（`track_id` 非空，外键受 CASCADE 保护）
    #[default]
    Track,
    /// 拖进来的本地文件（不进曲库）
    File,
    /// 网络直播流（HLS）
    Stream,
}

impl QueueKind {
    pub fn as_str(self) -> &'static str {
        match self {
            QueueKind::Track => "track",
            QueueKind::File => "file",
            QueueKind::Stream => "stream",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "track" => Some(QueueKind::Track),
            "file" => Some(QueueKind::File),
            "stream" => Some(QueueKind::Stream),
            _ => None,
        }
    }
}

/// 一个队列项。
///
/// 为什么 `payload` 是**字符串**而不是拆成列：它只给 transient 行用，字段由
/// `TrackRef` 的形状决定（UI 那边还要兼容老 payload）。拆列等于把播放器的
/// 展示字段复制进 schema，加一个字段就要迁移一次。
#[derive(Debug, Clone, PartialEq)]
pub struct QueueItem {
    pub kind: QueueKind,
    /// `kind == Track` 时必有
    pub track_id: Option<String>,
    /// `kind != Track` 时的最小显示字段（JSON）
    pub payload: Option<String>,
}

impl QueueItem {
    /// 库内曲目行
    pub fn track(id: impl Into<String>) -> Self {
        Self {
            kind: QueueKind::Track,
            track_id: Some(id.into()),
            payload: None,
        }
    }

    /// 临时行（拖入文件 / 直播流）
    pub fn transient(kind: QueueKind, payload: impl Into<String>) -> Self {
        Self {
            kind,
            track_id: None,
            payload: Some(payload.into()),
        }
    }
}

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

/// 读取队列：状态 + 原序条目（按 position 排）。
/// 无持久化队列（items 为空）时返回 None。
///
/// 两条查询包进同一只读事务，避免读到「state 新 / items 旧」的中间态。
/// current_index 归一化：语义上 -1 = 无当前曲目（见 QueuePersist 注释），因此
///   * items 为空 → 整个队列视为无，返回 None（防御 items 被清光但状态残留）
///   * 超出尾部 → clamp 到 len-1
///   * < -1（脏数据）→ clamp 回 -1 哨兵；-1 本身保留原语义
pub fn load_queue(conn: &Connection) -> Result<Option<(QueuePersist, Vec<QueueItem>)>, DbError> {
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
    let items: Vec<QueueItem> = tx
        .prepare("SELECT kind, track_id, payload FROM queue_items ORDER BY position")?
        .query_map([], |r| {
            let kind_raw: String = r.get(0)?;
            Ok(QueueItem {
                // 未知 kind 按曲目兜底：宁可显示成"没有封面的一行"，
                // 也不让整队读失败（旧库 / 手改库都能读出来）
                kind: QueueKind::parse(&kind_raw).unwrap_or_default(),
                track_id: r.get(1)?,
                payload: r.get(2)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()
        .map_err(DbError::Sql)?;
    tx.commit().map_err(DbError::Sql)?;

    if items.is_empty() {
        return Ok(None);
    }
    // 归一化越界的 current_index（队列被 CASCADE 删短后常见）
    let max = (items.len() - 1) as i64;
    if state.current_index > max {
        state.current_index = max;
    } else if state.current_index < -1 {
        state.current_index = -1; // -1 = 无当前曲目，是合法哨兵；更小的值是脏数据
    }
    Ok(Some((state, items)))
}

/// 全量落库：队列结构 + 状态（切歌/清空/退出时调用）。
pub fn save_queue_full(
    conn: &mut Connection,
    p: &QueuePersist,
    items: &[QueueItem],
) -> Result<(), DbError> {
    // IMMEDIATE（对齐 history/scanner）：这是写事务，DEFERRED 的锁升级
    // 在另一连接持写锁时立刻 BUSY，busy_timeout 不生效
    let tx = conn
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(DbError::Sql)?;
    tx.execute("DELETE FROM queue_items", [])
        .map_err(DbError::Sql)?;
    // 批量写 prepare 一次循环复用
    {
        let mut stmt = tx
            .prepare(
                "INSERT INTO queue_items (position, kind, track_id, payload)
                 VALUES (?1, ?2, ?3, ?4)",
            )
            .map_err(DbError::Sql)?;
        for (i, item) in items.iter().enumerate() {
            // CHECK 约束会把"kind 与 track_id 不匹配"的写法当场拒掉：
            // 这里不额外兜底，让写错的地方在开发期就炸，而不是读出来才发现
            stmt.execute(rusqlite::params![
                i as i64,
                item.kind.as_str(),
                item.track_id,
                item.payload,
            ])
            .map_err(DbError::Sql)?;
        }
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

    fn tracks(ids: &[&str]) -> Vec<QueueItem> {
        ids.iter().map(|id| QueueItem::track(*id)).collect()
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
        let items = tracks(&["a", "b", "c"]);
        save_queue_full(&mut conn, &p, &items).unwrap();

        let (p2, items2) = load_queue(&conn).unwrap().unwrap();
        assert_eq!(p2, p);
        assert_eq!(items2, items);
    }

    /// docs/52 §6 的核心：临时行（拖入文件 / 直播流）能进队列、能落库、能读回。
    ///
    /// 迁移 0010 之前这条会 FK 失败（`track_id NOT NULL`），而失败是**静默**的
    /// —— 上层只有一句 warn，重启后队列悄悄少一项。这条用例是那个 bug 的锁。
    #[test]
    fn transient_items_roundtrip_without_tracks() {
        let mut conn = open_conn(Path::new(":memory:")).unwrap();
        ins_track(&conn, "a");
        let items = vec![
            QueueItem::track("a"),
            QueueItem::transient(QueueKind::File, r#"{"path":"D:/drop/x.flac","title":"x"}"#),
            QueueItem::transient(
                QueueKind::Stream,
                r#"{"path":"https://hls.test/live.m3u8"}"#,
            ),
        ];
        let p = QueuePersist::default();
        save_queue_full(&mut conn, &p, &items).expect("临时行不该被外键拒掉");

        let (_, items2) = load_queue(&conn).unwrap().unwrap();
        assert_eq!(items2, items);
        assert_eq!(items2[1].kind, QueueKind::File);
        assert_eq!(items2[2].kind, QueueKind::Stream);
        assert!(items2[1].track_id.is_none());
        assert!(items2[1].payload.as_deref().unwrap().contains("x.flac"));
    }

    /// 曲目被删（扫描淘汰）时：只清掉指向它的行，临时行仍在
    /// —— 这正是"保留外键 + 允许 NULL"换来的行为（docs/52 §6.2）。
    #[test]
    fn cascade_removes_track_rows_but_keeps_transient() {
        let mut conn = open_conn(Path::new(":memory:")).unwrap();
        ins_track(&conn, "a");
        ins_track(&conn, "b");
        let items = vec![
            QueueItem::track("a"),
            QueueItem::transient(QueueKind::Stream, r#"{"path":"https://hls.test/l.m3u8"}"#),
            QueueItem::track("b"),
        ];
        save_queue_full(&mut conn, &QueuePersist::default(), &items).unwrap();

        conn.execute("DELETE FROM tracks WHERE id = 'a'", [])
            .unwrap();
        let (_, items2) = load_queue(&conn).unwrap().unwrap();
        assert_eq!(items2.len(), 2, "只剩 cascade 之外的项");
        assert_eq!(items2[0].kind, QueueKind::Stream);
        assert_eq!(items2[1].track_id.as_deref(), Some("b"));
    }

    /// kind 与 track_id 写反了要当场被 CHECK 拒掉（不是读出来才发现）
    #[test]
    fn check_constraint_rejects_inconsistent_rows() {
        let mut conn = open_conn(Path::new(":memory:")).unwrap();
        let bad = QueueItem {
            kind: QueueKind::Track,
            track_id: None,
            payload: None,
        };
        assert!(save_queue_full(&mut conn, &QueuePersist::default(), &[bad]).is_err());
        // 反过来：file 却给了 track_id
        ins_track(&conn, "a");
        let bad2 = QueueItem {
            kind: QueueKind::File,
            track_id: Some("a".into()),
            payload: None,
        };
        assert!(save_queue_full(&mut conn, &QueuePersist::default(), &[bad2]).is_err());
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
        save_queue_full(&mut conn, &QueuePersist::default(), &tracks(&["a", "b"])).unwrap();

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

        let (p2, items2) = load_queue(&conn).unwrap().unwrap();
        assert_eq!(p2, p);
        assert_eq!(items2, tracks(&["a", "b"]), "state-only 落库不动 items");
    }

    #[test]
    fn full_save_replaces_previous_items() {
        let mut conn = open_conn(Path::new(":memory:")).unwrap();
        for id in ["a", "b", "c"] {
            ins_track(&conn, id);
        }
        save_queue_full(&mut conn, &QueuePersist::default(), &tracks(&["a", "b"])).unwrap();
        save_queue_full(
            &mut conn,
            &QueuePersist {
                current_index: 0,
                ..QueuePersist::default()
            },
            &tracks(&["c"]),
        )
        .unwrap();
        let (_, items) = load_queue(&conn).unwrap().unwrap();
        assert_eq!(items, tracks(&["c"]), "全量重写不留旧行");
    }

    #[test]
    fn load_clamps_out_of_range_current_index() {
        let mut conn = open_conn(Path::new(":memory:")).unwrap();
        for id in ["a", "b"] {
            ins_track(&conn, id);
        }
        save_queue_full(&mut conn, &QueuePersist::default(), &tracks(&["a", "b"])).unwrap();
        // 直接改 queue_state 制造越界（save 不做校验，防御在 load 侧）
        conn.execute("UPDATE queue_state SET current_index = 5", [])
            .unwrap();
        let (p, items) = load_queue(&conn).unwrap().unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(p.current_index, 1, "超出尾部 clamp 到 len-1");

        conn.execute("UPDATE queue_state SET current_index = -9", [])
            .unwrap();
        let (p2, _) = load_queue(&conn).unwrap().unwrap();
        assert_eq!(p2.current_index, -1, "脏负值 clamp 回 -1 哨兵");
    }
}
