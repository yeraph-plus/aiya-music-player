//! 推流频道的持久化面。
//!
//! 频道身份与段目录在创建时固定，改名不改变收听地址。
//! 节目配置使用独立的版本化 settings 文档，由 server 解析系统歌单。

use rusqlite::params;
use rusqlite::Connection;

use crate::db::{DbError, DbResult};
use crate::ids;

pub const VISIBILITY_PRIVATE: &str = "private";
pub const VISIBILITY_PUBLIC: &str = "public";

#[derive(Debug, Clone)]
pub struct StationRow {
    pub id: String,
    pub name: String,
    pub subtitle: Option<String>,
    pub description: Option<String>,
    pub cover_url: Option<String>,
    pub hls_dir: String,
    pub visibility: String,
    pub active: bool,
    pub created_at: i64,
    pub updated_at: i64,
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

const STATION_COLS: &str = "id, name, subtitle, description, cover_url, hls_dir, visibility, active, created_at, updated_at";

fn station_row(r: &rusqlite::Row) -> rusqlite::Result<StationRow> {
    Ok(StationRow {
        id: r.get(0)?,
        name: r.get(1)?,
        subtitle: r.get(2)?,
        description: r.get(3)?,
        cover_url: r.get(4)?,
        hls_dir: r.get(5)?,
        visibility: r.get(6)?,
        active: r.get::<_, i64>(7)? != 0,
        created_at: r.get(8)?,
        updated_at: r.get(9)?,
    })
}

pub fn list_stations(conn: &Connection) -> DbResult<Vec<StationRow>> {
    let sql = format!("SELECT {STATION_COLS} FROM stations ORDER BY created_at ASC");
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([], station_row)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

pub fn get_station(conn: &Connection, id: &str) -> DbResult<Option<StationRow>> {
    let sql = format!("SELECT {STATION_COLS} FROM stations WHERE id = ?1");
    match conn.query_row(&sql, [id], station_row) {
        Ok(v) => Ok(Some(v)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

pub struct StationCreate {
    pub name: String,
    pub subtitle: Option<String>,
    pub description: Option<String>,
    pub cover_url: Option<String>,
    pub visibility: String,
}

pub fn create_station(conn: &Connection, input: &StationCreate) -> DbResult<StationRow> {
    // 纳秒时间盐区分同秒创建的同名频道，目录前缀碰撞由唯一索引检查并重试。
    for _ in 0..2 {
        let now = now_secs();
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let id = ids::station_id(&input.name, nanos);
        let hls_dir = format!("ch{}", &id[..8.min(id.len())]);
        let dup = conn
            .query_row(
                "SELECT 1 FROM stations WHERE hls_dir = ?1 OR id = ?2",
                params![hls_dir, id],
                |_| Ok(()),
            )
            .is_ok();
        if dup {
            std::thread::sleep(std::time::Duration::from_millis(2));
            continue;
        }
        let vis = normalize_visibility(&input.visibility)?;
        conn.execute(
            "INSERT INTO stations (id, name, subtitle, description, cover_url, hls_dir, visibility, active, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 0, ?8, ?8)",
            params![
                id,
                input.name,
                input.subtitle,
                input.description,
                input.cover_url,
                hls_dir,
                vis,
                now
            ],
        )?;
        return get_station(conn, &id)?
            .ok_or_else(|| DbError::Custom("station row vanished after insert".into()));
    }
    Err(DbError::Custom("station id collision twice".into()))
}

pub struct StationUpdate {
    pub name: String,
    pub subtitle: Option<String>,
    pub description: Option<String>,
    pub cover_url: Option<String>,
    pub visibility: String,
}

pub fn update_station(conn: &Connection, id: &str, input: &StationUpdate) -> DbResult<StationRow> {
    let vis = normalize_visibility(&input.visibility)?;
    let n = conn.execute(
        "UPDATE stations SET name = ?2, subtitle = ?3, description = ?4, cover_url = ?5,
         visibility = ?6, updated_at = ?7 WHERE id = ?1",
        params![
            id,
            input.name,
            input.subtitle,
            input.description,
            input.cover_url,
            vis,
            now_secs()
        ],
    )?;
    if n == 0 {
        return Err(DbError::Custom(format!("station not found: {id}")));
    }
    get_station(conn, id)?
        .ok_or_else(|| DbError::Custom("station row vanished after update".into()))
}

pub fn set_active(conn: &Connection, id: &str, active: bool) -> DbResult<()> {
    let n = conn.execute(
        "UPDATE stations SET active = ?2, updated_at = ?3 WHERE id = ?1",
        params![id, active as i64, now_secs()],
    )?;
    if n == 0 {
        return Err(DbError::Custom(format!("station not found: {id}")));
    }
    Ok(())
}

pub fn delete_station(conn: &Connection, id: &str) -> DbResult<()> {
    // 段目录与节目配置由 server 清理；数据层只删除频道行。
    conn.execute("DELETE FROM stations WHERE id = ?1", [id])?;
    Ok(())
}

fn normalize_visibility(v: &str) -> DbResult<&'static str> {
    match v {
        VISIBILITY_PRIVATE => Ok(VISIBILITY_PRIVATE),
        VISIBILITY_PUBLIC => Ok(VISIBILITY_PUBLIC),
        other => Err(DbError::Custom(format!("invalid visibility: {other}"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_in_memory;

    #[test]
    fn same_name_can_be_created_twice_without_waiting_for_the_next_second() {
        let conn = open_in_memory().unwrap();
        let input = StationCreate {
            name: "Radio".into(),
            subtitle: None,
            description: None,
            cover_url: None,
            visibility: VISIBILITY_PRIVATE.into(),
        };
        let first = create_station(&conn, &input).unwrap();
        let second = create_station(&conn, &input).unwrap();
        assert_ne!(first.id, second.id);
        assert_ne!(first.hls_dir, second.hls_dir);
    }

    #[test]
    fn station_crud_roundtrip() {
        let conn = open_in_memory().unwrap();
        let row = create_station(
            &conn,
            &StationCreate {
                name: "深夜 FM".into(),
                subtitle: Some("sub".into()),
                description: None,
                cover_url: Some("https://x/c.png".into()),
                visibility: VISIBILITY_PUBLIC.into(),
            },
        )
        .unwrap();
        assert_eq!(row.visibility, VISIBILITY_PUBLIC);
        assert!(!row.active, "建站默认停播");
        assert!(row.hls_dir.starts_with("ch"));

        // 改名不换 id、hls_dir 不变（外部地址稳定）
        let renamed = update_station(
            &conn,
            &row.id,
            &StationUpdate {
                name: "改名 FM".into(),
                subtitle: None,
                description: None,
                cover_url: None,
                visibility: VISIBILITY_PRIVATE.into(),
            },
        )
        .unwrap();
        assert_eq!(renamed.id, row.id);
        assert_eq!(renamed.hls_dir, row.hls_dir);
        assert_eq!(renamed.visibility, VISIBILITY_PRIVATE);

        set_active(&conn, &row.id, true).unwrap();
        assert!(get_station(&conn, &row.id).unwrap().unwrap().active);

        delete_station(&conn, &row.id).unwrap();
        assert!(get_station(&conn, &row.id).unwrap().is_none());
    }
}
