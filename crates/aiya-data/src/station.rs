//! 推流频道的持久化面（docs/68 §3，N1.1）。
//!
//! 写路径全走事务（`replace_*` 是全删全插的"整表提交"语义——UI 发完整列表，
//! 单条 CRUD 不开面）；id 一律 `ids::NS_STATION*` v5 派生（服务端权威，改名
//! 不换 id）；`hls_dir` 建站时定死不随改名变（`/hls/{hls_dir}` 是外部收听端
//! 拿到的地址，LIVE_STATION_FIELDS 的"URL 可随部署变，id 稳定"语义）。

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

#[derive(Debug, Clone)]
pub struct StationPlaylistRow {
    pub id: String,
    pub station_id: String,
    pub name: String,
    /// manual | random_all
    pub kind: String,
    /// manual：曲库曲目 id 数组 JSON
    pub track_ids_json: Option<String>,
    pub rule_json: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone)]
pub struct StationScheduleRow {
    pub id: String,
    pub station_id: String,
    pub playlist_id: String,
    /// 'HH:MM'；end_hm <= start_hm 表跨零点
    pub start_hm: String,
    pub end_hm: String,
    /// bit0=周一 … bit6=周日
    pub days_mask: i64,
    pub priority: i64,
    pub created_at: i64,
}

/// replace_playlists 的输入（id 由本层生成，UI 不带）。
#[derive(Debug, Clone)]
pub struct PlaylistInput {
    pub name: String,
    pub kind: String,
    pub track_ids: Vec<String>,
    pub rule_json: Option<String>,
}

/// replace_schedules 的输入。
#[derive(Debug, Clone)]
pub struct ScheduleInput {
    pub playlist_id: String,
    pub start_hm: String,
    pub end_hm: String,
    pub days_mask: i64,
    pub priority: i64,
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
    // id 从名字 + 创建时刻派生（v5 稳定语义：一旦生成永不因改名变）；
    // hls_dir 取 id 前 8 位，唯一索引兜底撞名（同毫秒同名理论碰撞重试一次）。
    // 盐（now）在循环体内取：撞名重试必须派生出不同的 id，冻结的盐让重试恒败。
    for _ in 0..2 {
        let now = now_secs();
        let id = ids::station_id(&input.name, now);
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
    // 子表 CASCADE；段目录与配置由调用方（server 壳）清——aiya-data 不碰文件系统
    conn.execute("DELETE FROM stations WHERE id = ?1", [id])?;
    Ok(())
}

/// 频道歌单全量替换（事务：删旧插新，幂等可重放）。
pub fn replace_playlists(
    conn: &mut Connection,
    station_id: &str,
    inputs: &[PlaylistInput],
) -> DbResult<Vec<StationPlaylistRow>> {
    let now = now_secs();
    let tx = conn.transaction()?;
    // 位置继承 id：整表提交语义下 UI 保持顺序——输入第 i 项继承旧表第 i 行
    // 的 id（未变/改名/中删后前移都自然对齐内容），station_schedules 对旧 id
    // 的引用因此存活，不会被 ON DELETE CASCADE 连带清空。新增项才派生新 id。
    // 改名不断节目表绑定（绑定跟随"第 i 个歌单"这个位置语义）。
    let prev_ids: Vec<String> = {
        let mut stmt = tx.prepare(
            "SELECT id FROM station_playlists WHERE station_id = ?1 ORDER BY created_at ASC",
        )?;
        let rows = stmt.query_map([station_id], |r| r.get::<_, String>(0))?;
        let mut v = Vec::new();
        for r in rows {
            v.push(r?);
        }
        v
    };
    let ids_by_input: Vec<String> = inputs
        .iter()
        .enumerate()
        .map(|(i, p)| {
            prev_ids
                .get(i)
                .cloned()
                .unwrap_or_else(|| ids::station_playlist_id(station_id, &p.name, now))
        })
        .collect();
    tx.execute(
        "DELETE FROM station_playlists WHERE station_id = ?1",
        [station_id],
    )?;
    for (p, id) in inputs.iter().zip(ids_by_input.iter()) {
        // 位置继承的行视为"同一歌单的重建"：created_at 取 now（行级重建时刻）
        let track_ids: Option<String> = if p.track_ids.is_empty() {
            None
        } else {
            Some(
                serde_json::to_string(&p.track_ids)
                    .map_err(|e| DbError::Custom(format!("track_ids_json: {e}")))?,
            )
        };
        tx.execute(
            "INSERT INTO station_playlists (id, station_id, name, kind, track_ids_json, rule_json, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(id) DO UPDATE SET name = ?3, kind = ?4, track_ids_json = ?5,
               rule_json = ?6, updated_at = ?8",
            params![id, station_id, p.name, p.kind, track_ids, p.rule_json, now, now],
        )?;
    }
    tx.commit()?;
    list_playlists(conn, station_id)
}

/// 节目表全量替换（事务）。playlist_id 引用完整性交给 FK。
pub fn replace_schedules(
    conn: &mut Connection,
    station_id: &str,
    inputs: &[ScheduleInput],
) -> DbResult<Vec<StationScheduleRow>> {
    let now = now_secs();
    let tx = conn.transaction()?;
    tx.execute(
        "DELETE FROM station_schedules WHERE station_id = ?1",
        [station_id],
    )?;
    for s in inputs {
        let id = ids::station_schedule_id(station_id, &s.playlist_id, &s.start_hm, now);
        tx.execute(
            "INSERT INTO station_schedules (id, station_id, playlist_id, start_hm, end_hm, days_mask, priority, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![id, station_id, s.playlist_id, s.start_hm, s.end_hm, s.days_mask, s.priority, now],
        )?;
    }
    tx.commit()?;
    list_schedules(conn, station_id)
}

pub fn list_playlists(conn: &Connection, station_id: &str) -> DbResult<Vec<StationPlaylistRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, station_id, name, kind, track_ids_json, rule_json, created_at, updated_at
         FROM station_playlists WHERE station_id = ?1 ORDER BY created_at ASC",
    )?;
    let rows = stmt.query_map([station_id], |r| {
        Ok(StationPlaylistRow {
            id: r.get(0)?,
            station_id: r.get(1)?,
            name: r.get(2)?,
            kind: r.get(3)?,
            track_ids_json: r.get(4)?,
            rule_json: r.get(5)?,
            created_at: r.get(6)?,
            updated_at: r.get(7)?,
        })
    })?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

pub fn list_schedules(conn: &Connection, station_id: &str) -> DbResult<Vec<StationScheduleRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, station_id, playlist_id, start_hm, end_hm, days_mask, priority, created_at
         FROM station_schedules WHERE station_id = ?1 ORDER BY priority DESC, start_hm ASC",
    )?;
    let rows = stmt.query_map([station_id], |r| {
        Ok(StationScheduleRow {
            id: r.get(0)?,
            station_id: r.get(1)?,
            playlist_id: r.get(2)?,
            start_hm: r.get(3)?,
            end_hm: r.get(4)?,
            days_mask: r.get(5)?,
            priority: r.get(6)?,
            created_at: r.get(7)?,
        })
    })?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

/// 频道播放记账（防重复窗口的数据源）。
pub fn record_play(conn: &Connection, station_id: &str, track_id: &str) -> DbResult<()> {
    conn.execute(
        "INSERT INTO station_history (station_id, track_id, played_at) VALUES (?1, ?2, ?3)",
        params![station_id, track_id, now_secs()],
    )?;
    Ok(())
}

/// 防重复窗口：window_secs 内播过的曲目 id。
pub fn recent_track_ids(
    conn: &Connection,
    station_id: &str,
    window_secs: i64,
) -> DbResult<Vec<String>> {
    let since = now_secs() - window_secs;
    let mut stmt = conn.prepare(
        "SELECT DISTINCT track_id FROM station_history WHERE station_id = ?1 AND played_at >= ?2",
    )?;
    let rows = stmt.query_map(params![station_id, since], |r| r.get::<_, String>(0))?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
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
    fn station_crud_roundtrip() {
        let mut conn = open_in_memory().unwrap();
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

        // 歌单与节目表全量替换（id 每次重建——引用方以行内字段为准）
        let pls = replace_playlists(
            &mut conn,
            &row.id,
            &[
                PlaylistInput {
                    name: "夜航".into(),
                    kind: "manual".into(),
                    track_ids: vec!["t1".into(), "t2".into()],
                    rule_json: None,
                },
                PlaylistInput {
                    name: "随机".into(),
                    kind: "random_all".into(),
                    track_ids: vec![],
                    rule_json: None,
                },
            ],
        )
        .unwrap();
        assert_eq!(pls.len(), 2);
        assert!(pls[0].track_ids_json.as_deref().unwrap().contains("t1"));
        assert!(pls[1].track_ids_json.is_none());

        let schs = replace_schedules(
            &mut conn,
            &row.id,
            &[ScheduleInput {
                playlist_id: pls[0].id.clone(),
                start_hm: "22:00".into(),
                end_hm: "01:00".into(), // 跨零点
                days_mask: 0b0001111,
                priority: 5,
            }],
        )
        .unwrap();
        assert_eq!(schs.len(), 1);
        assert_eq!(schs[0].days_mask, 0b0001111);

        // 防重复窗口
        record_play(&conn, &row.id, "t1").unwrap();
        assert_eq!(recent_track_ids(&conn, &row.id, 3600).unwrap(), vec!["t1"]);
        assert!(recent_track_ids(&conn, &row.id, -1).unwrap().is_empty());

        // 删除级联（playlists/schedules 随 station 走）
        delete_station(&conn, &row.id).unwrap();
        assert!(get_station(&conn, &row.id).unwrap().is_none());
        assert!(list_playlists(&conn, &row.id).unwrap().is_empty());
        assert!(list_schedules(&conn, &row.id).unwrap().is_empty());
    }
}
