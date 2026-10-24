//! 曲目列表查询：双模式分页（cursor / offset）、四种来源、LIKE 搜索。
//!
//! 对齐 `@aiya/contracts` 的 `TrackQuery` / `TrackPage` 形状（serde camelCase），
//! v1 阶段这是 UI 兼容层的一部分；契约迁回 Rust 源时此文件就是雏形。

use std::time::Instant;

use rusqlite::types::{ToSql, Value as SqlValue};
use rusqlite::{Connection, Row};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::db::{DbError, DbResult};
use crate::norm::norm;

type Binder = Box<dyn Fn(&mut Vec<Box<dyn ToSql>>) + Send>;

/* ---------------- DTO ---------------- */

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackRow {
    pub id: String,
    pub title: String,
    pub artist_id: Option<String>,
    pub artist: Option<String>,
    pub album_id: Option<String>,
    pub album: Option<String>,
    pub album_artist: Option<String>,
    pub duration_ms: Option<i64>,
    pub track_no: Option<i64>,
    pub disc_no: Option<i64>,
    pub year: Option<i64>,
    pub added_at: i64,
    pub artwork_id: Option<String>,
    pub play_count: i64,
    pub last_played_at: Option<i64>,
    pub gain_db: Option<f64>,
    /// 文件可用性：online / offline / missing。扫描器与 beets 对账都会改它，
    /// UI 用它给行打灰标（此前 DTO 里没有 → 列表看不出哪首已经找不到文件）。
    pub availability: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackPage {
    pub items: Vec<TrackRow>,
    pub total: i64,
    pub limit: usize,
    pub mode: String,
    pub next_cursor: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub offset: Option<usize>,
    pub took_ms: u32,
}

/* ---------------- 查询参数 ---------------- */

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Sort {
    #[default]
    Title,
    Added,
    Artist,
    Album,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Direction {
    #[default]
    Asc,
    Desc,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum TrackSource {
    Library,
    Playlist { playlist_id: String },
    Recent,
    MostPlayed,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Mode {
    #[default]
    Cursor,
    Offset,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackWindowQuery {
    #[serde(default)]
    pub sort: Sort,
    #[serde(default)]
    pub direction: Direction,
    #[serde(default)]
    pub search: Option<String>,
    #[serde(default)]
    pub artist_id: Option<String>,
    #[serde(default)]
    pub album_id: Option<String>,
    /// 嵌套形状 `{ source: { kind: "library" } }`，与 contracts/query.ts 的
    /// trackSourceSchema（discriminatedUnion('kind')）同构。
    /// 注意不能用 #[serde(flatten)] 把 kind 摊到顶层 —— 那是 Electron demo
    /// 时代的形状，UI 原样搬过后 kind 在 source 对象里。
    #[serde(default = "default_source")]
    pub source: TrackSource,
    #[serde(default = "default_limit")]
    pub limit: usize,
    #[serde(default)]
    pub mode: Mode,
    #[serde(default)]
    pub cursor: Option<String>,
    #[serde(default)]
    pub offset: Option<usize>,
}

fn default_limit() -> usize {
    100
}

fn default_source() -> TrackSource {
    TrackSource::Library
}

/* ---------------- 游标（与 contracts/query.ts 同格式：base64url(JSON [value,id])） ---------------- */

fn encode_cursor(value: &Value, id: &str) -> DbResult<String> {
    let json =
        serde_json::to_string(&[value, &Value::from(id)]).map_err(|e| DbError::Custom(e.to_string()))?;
    use base64::Engine as _;
    Ok(base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(json))
}

fn decode_cursor(cursor: &str) -> DbResult<(Value, String)> {
    use base64::Engine as _;
    let invalid = || DbError::Custom("E_INVALID_CURSOR".into());
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(cursor)
        .map_err(|_| invalid())?;
    let json = String::from_utf8(bytes).map_err(|_| invalid())?;
    let arr: Vec<Value> = serde_json::from_str(&json).map_err(|_| invalid())?;
    if arr.len() != 2 {
        return Err(invalid());
    }
    let id = arr[1].as_str().ok_or_else(invalid)?.to_string();
    Ok((arr[0].clone(), id))
}

/* ---------------- SQL 构造 ---------------- */

const SELECT_COLS: &str = "t.id, t.title, t.artist_id, ar.name, t.album_id, al.title, al_ar.name, \
     t.duration_ms, t.track_no, t.disc_no, t.year, t.added_at, t.artwork_id, \
     t.play_count, t.last_played_at, t.gain_db, t.availability";

/// artist/album 展示列的两个 JOIN（页查询必带；COUNT 只在搜索时带）
const JOIN_ARTIST_ALBUM: &str = "LEFT JOIN artists ar ON ar.id = t.artist_id \
     LEFT JOIN albums al ON al.id = t.album_id \
     LEFT JOIN artists al_ar ON al_ar.id = al.artist_id";

const JOIN: &str = "FROM tracks t \
     LEFT JOIN artists ar ON ar.id = t.artist_id \
     LEFT JOIN albums al ON al.id = t.album_id \
     LEFT JOIN artists al_ar ON al_ar.id = al.artist_id";

const JOIN_PLAYLIST: &str = "FROM playlist_items pi \
     JOIN tracks t ON t.id = pi.track_id \
     LEFT JOIN artists ar ON ar.id = t.artist_id \
     LEFT JOIN albums al ON al.id = t.album_id \
     LEFT JOIN artists al_ar ON al_ar.id = al.artist_id";

const JOIN_AGG: &str = "FROM (SELECT track_id, COUNT(*) AS played_count, MAX(played_at) AS recent_t0 \
     FROM history GROUP BY track_id) hs \
     JOIN tracks t ON t.id = hs.track_id \
     LEFT JOIN artists ar ON ar.id = t.artist_id \
     LEFT JOIN albums al ON al.id = t.album_id \
     LEFT JOIN artists al_ar ON al_ar.id = al.artist_id";

/// JOIN 与固定排序（playlist/recent/mostPlayed 不吃用户 sort）。
fn join_and_order(q: &TrackWindowQuery) -> (&'static str, &'static str) {
    match &q.source {
        TrackSource::Library => (JOIN, ""),
        TrackSource::Playlist { .. } => (JOIN_PLAYLIST, " ORDER BY pi.position ASC"),
        TrackSource::Recent => (JOIN_AGG, " ORDER BY hs.recent_t0 DESC, t.id DESC"),
        TrackSource::MostPlayed => (JOIN_AGG, " ORDER BY hs.played_count DESC, t.id ASC"),
    }
}

/// 排序键表达式（游标与 ORDER BY 共用）。None = 来源有固定顺序，不用游标。
fn key_expr(q: &TrackWindowQuery) -> Option<&'static str> {
    if !matches!(q.source, TrackSource::Library) {
        return None;
    }
    Some(match q.sort {
        Sort::Title => "t.title_norm",
        Sort::Added => "t.added_at",
        Sort::Artist => "COALESCE(ar.name_norm,'')",
        Sort::Album => "COALESCE(al.title_norm,'')",
    })
}

/// 只构造 WHERE 子句与绑定器（页数据与 COUNT 共用，保证两边口径一致）。
/// `cursor_filter` 的占位符排在 WHERE 末尾（对应 binders 尾部追加的游标绑定）。
fn build_where(q: &TrackWindowQuery, cursor_filter: Option<&str>) -> (String, Vec<Binder>) {
    let mut binders: Vec<Binder> = Vec::new();
    let mut where_clauses: Vec<String> = Vec::new();
    if let Some(search) = q.search.as_deref().filter(|s| !s.trim().is_empty()) {
        // LIKE 通配符防误匹配：按字面语义转义 % _ \（\ 同时是 ESCAPE 字符）
        let mut escaped = String::with_capacity(norm(search).len());
        for c in norm(search).chars() {
            match c {
                '%' => escaped.push_str("\\%"),
                '_' => escaped.push_str("\\_"),
                '\\' => escaped.push_str("\\\\"),
                _ => escaped.push(c),
            }
        }
        let pattern = format!("%{escaped}%");
        for _ in 0..3 {
            let p = pattern.clone();
            binders.push(Box::new(move |v: &mut Vec<Box<dyn ToSql>>| {
                v.push(Box::new(p.clone()));
            }));
        }
        where_clauses.push(
            "(t.title_norm LIKE ? ESCAPE '\\' OR COALESCE(ar.name_norm,'') LIKE ? ESCAPE '\\' \
             OR COALESCE(al.title_norm,'') LIKE ? ESCAPE '\\')"
                .to_string(),
        );
    }
    if let Some(artist) = q.artist_id.as_deref() {
        let a = artist.to_string();
        binders.push(Box::new(move |v: &mut Vec<Box<dyn ToSql>>| {
            v.push(Box::new(a.clone()));
        }));
        where_clauses.push("t.artist_id = ?".to_string());
    }
    if let Some(album) = q.album_id.as_deref() {
        let b = album.to_string();
        binders.push(Box::new(move |v: &mut Vec<Box<dyn ToSql>>| {
            v.push(Box::new(b.clone()));
        }));
        where_clauses.push("t.album_id = ?".to_string());
    }
    // 歌单来源：JOIN_PLAYLIST 从 playlist_items 出发，必须按 playlist_id 过滤，
    // 否则任何歌单都返回全部曲目（P0）。占位符顺序 = binder 推入顺序
    // （search → artist → album → playlist → cursor）。
    if let TrackSource::Playlist { playlist_id } = &q.source {
        let pid = playlist_id.clone();
        binders.push(Box::new(move |v: &mut Vec<Box<dyn ToSql>>| {
            v.push(Box::new(pid.clone()));
        }));
        where_clauses.push("pi.playlist_id = ?".to_string());
    }
    if let Some(cf) = cursor_filter {
        where_clauses.push(cf.to_string());
    }

    let where_sql = if where_clauses.is_empty() {
        String::new()
    } else {
        format!(" WHERE {}", where_clauses.join(" AND "))
    };
    (where_sql, binders)
}

/// 构造页查询 SQL（SELECT ... 到 ORDER BY，不含 LIMIT/OFFSET）。
fn build_head(q: &TrackWindowQuery, cursor_filter: Option<&str>) -> (String, Vec<Binder>) {
    let (where_sql, binders) = build_where(q, cursor_filter);
    let (join, fixed_order) = join_and_order(q);
    let order_sql = if fixed_order.is_empty() {
        let dir = if q.direction == Direction::Desc { "DESC" } else { "ASC" };
        format!(" ORDER BY {} {dir}, t.id {dir}", key_expr(q).unwrap_or("t.title_norm"))
    } else {
        fixed_order.to_string()
    };
    (format!("SELECT {SELECT_COLS} {join}{where_sql}{order_sql}"), binders)
}

/// COUNT 用的 FROM：**能省则省**。
///
/// `COUNT(*)` 只要行数，而 artist/album 两个 LEFT JOIN 每行都要探测一次索引
/// —— 5 万行 = 15 万次探测，实测 22.7ms；去掉 JOIN 的 `COUNT(*) FROM tracks`
/// 只要 38µs（500 倍）。所以只有搜索谓词真的引用 `ar.name_norm` /
/// `al.title_norm` 时才带上这两个 JOIN。
fn count_from(q: &TrackWindowQuery) -> String {
    let has_search = q.search.as_deref().map(|s| !s.trim().is_empty()).unwrap_or(false);
    let base = match &q.source {
        TrackSource::Library => "FROM tracks t".to_string(),
        TrackSource::Playlist { .. } => {
            "FROM playlist_items pi JOIN tracks t ON t.id = pi.track_id".to_string()
        }
        // 与 JOIN_AGG 同源：只关心 history 里出现过的曲目
        TrackSource::Recent | TrackSource::MostPlayed => {
            "FROM (SELECT track_id FROM history GROUP BY track_id) hs \
             JOIN tracks t ON t.id = hs.track_id"
                .to_string()
        }
    };
    if has_search {
        format!("{base} {JOIN_ARTIST_ALBUM}")
    } else {
        base
    }
}

/// 构造 COUNT SQL（不含游标谓词 —— total 是"当前条件下总数"，不随翻页缩水）。
fn build_count(q: &TrackWindowQuery) -> (String, Vec<Binder>) {
    let (where_sql, binders) = build_where(q, None);
    (format!("SELECT COUNT(*) {}{where_sql}", count_from(q)), binders)
}

fn bind_all(
    stmt: &mut rusqlite::Statement<'_>,
    binders: &[Binder],
    paging: Option<(i64, i64)>,
) -> DbResult<()> {
    let mut params: Vec<Box<dyn ToSql>> = Vec::new();
    for b in binders {
        b(&mut params);
    }
    if let Some((limit, offset)) = paging {
        params.push(Box::new(limit));
        params.push(Box::new(offset));
    }
    for (i, p) in params.iter().enumerate() {
        stmt.raw_bind_parameter(i + 1, p.as_ref()).map_err(DbError::Sql)?;
    }
    Ok(())
}

fn to_sql_value(v: &Value) -> SqlValue {
    match v {
        Value::Null => SqlValue::Null,
        Value::Bool(b) => SqlValue::Integer(*b as i64),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                SqlValue::Integer(i)
            } else {
                SqlValue::Real(n.as_f64().unwrap_or(0.0))
            }
        }
        Value::String(s) => SqlValue::Text(s.clone()),
        other => SqlValue::Text(other.to_string()),
    }
}

fn row_to_track(r: &Row<'_>) -> rusqlite::Result<TrackRow> {
    Ok(TrackRow {
        id: r.get(0)?,
        title: r.get(1)?,
        artist_id: r.get(2)?,
        artist: r.get(3)?,
        album_id: r.get(4)?,
        album: r.get(5)?,
        album_artist: r.get(6)?,
        duration_ms: r.get(7)?,
        track_no: r.get(8)?,
        disc_no: r.get(9)?,
        year: r.get(10)?,
        added_at: r.get(11)?,
        artwork_id: r.get(12)?,
        play_count: r.get(13)?,
        last_played_at: r.get(14)?,
        gain_db: r.get(15)?,
        availability: r.get(16)?,
    })
}

/// 列表查询入口。返回 `TrackPage`（含 total / nextCursor / tookMs）。
pub fn list_tracks(conn: &Connection, q: &TrackWindowQuery) -> DbResult<TrackPage> {
    let started = Instant::now();
    let limit = q.limit.clamp(1, 500) as i64;

    // ---------- 游标解析 ----------
    let mut cursor_filter: Option<String> = None;
    let mut cursor_binders: Vec<Binder> = Vec::new();
    let mut offset: i64 = 0;

    if q.mode == Mode::Cursor {
        if let Some(cursor) = q.cursor.as_deref() {
            if let Some(expr) = key_expr(q) {
                let (value, id) = decode_cursor(cursor)?;
                let op = if q.direction == Direction::Desc { "<" } else { ">" };
                cursor_filter = Some(format!("(({expr} {op} ?) OR ({expr} = ? AND t.id {op} ?))"));
                let v2 = value;
                cursor_binders.push(Box::new(move |v: &mut Vec<Box<dyn ToSql>>| {
                    v.push(Box::new(to_sql_value(&v2)));
                    v.push(Box::new(to_sql_value(&v2)));
                    v.push(Box::new(id.clone()));
                }));
            }
        }
    } else {
        offset = q.offset.unwrap_or(0) as i64;
    }

    let (head, mut binders) = build_head(q, cursor_filter.as_deref());
    binders.extend(cursor_binders);

    // ---------- COUNT（同 WHERE 但**不含游标谓词**）----------
    // 此前 COUNT 复用了带游标条件的 head，于是 total 是"游标之后的剩余量"，
    // 每翻一页递减 —— UI 拿它画滚动条/显示"N 首"会一路缩水。
    let total: i64 = {
        let (count_sql, count_binders) = build_count(q);
        let mut stmt = conn.prepare(&count_sql)?;
        bind_all(&mut stmt, &count_binders, None)?;
        let mut rows = stmt.raw_query();
        match rows.next()? {
            Some(row) => row.get(0)?,
            None => 0,
        }
    };

    // ---------- 页数据（limit+1 探测 next）----------
    // 读满 limit+1 行才视为 has_more（多出的探测行丢弃）；不能用 total 取模判断
    // —— total 恰为 limit 整数倍时会多发一次空页。
    let (items, has_more): (Vec<TrackRow>, bool) = {
        let sql = format!("{head} LIMIT ? OFFSET ?");
        let mut stmt = conn.prepare(&sql)?;
        bind_all(&mut stmt, &binders, Some((limit + 1, offset)))?;
        let mut rows = stmt.raw_query();
        let mut out: Vec<TrackRow> = Vec::with_capacity(limit as usize + 1);
        let mut more = false;
        while let Some(r) = rows.next()? {
            if out.len() as i64 == limit {
                more = true; // 探测行：只判定还有下一页，不消费进结果
                break;
            }
            out.push(row_to_track(r)?);
        }
        (out, more)
    };

    // ---------- next_cursor ----------
    let next_cursor = if has_more && key_expr(q).is_some() {
        let last = items.last().expect("items nonempty when has_more");
        let value: Value = match q.sort {
            Sort::Title => Value::from(norm(&last.title)),
            Sort::Added => Value::from(last.added_at),
            Sort::Artist => Value::from(norm(last.artist.as_deref().unwrap_or(""))),
            Sort::Album => Value::from(norm(last.album.as_deref().unwrap_or(""))),
        };
        Some(encode_cursor(&value, &last.id)?)
    } else {
        None
    };

    Ok(TrackPage {
        items,
        total,
        limit: limit as usize,
        mode: if q.mode == Mode::Cursor { "cursor" } else { "offset" }.into(),
        next_cursor,
        offset: (q.mode == Mode::Offset).then_some(offset as usize),
        took_ms: started.elapsed().as_millis().min(u32::MAX as u128) as u32,
    })
}

/// 批量取 ID 的结果。`truncated` 表示命中 `max` 上限而被截断
/// （调用方必须能感知：静默截断会让"播放全部"少播一大截而无人知道）。
#[derive(Debug, Clone)]
pub struct IdCollection {
    pub ids: Vec<String>,
    pub truncated: bool,
}

/// 按查询条件收集全量 track id（批量选择 / 播放全部）。
///
/// 为什么不复用 `list_tracks` 的 limit：`list_tracks` 是**窗口化列表**入口，
/// limit 硬钳 500 是对的（虚拟列表不需要更多）；但把它当"取全量"用会静默
/// 截断（此前 player_play_query 以为 limit=10000 就能拿全，实际只拿到 500）。
///
/// 分页推进方式按来源区分——这是修死循环的关键：
/// * library 来源有游标键 → 游标翻页；
/// * playlist/recent/mostPlayed 是固定顺序、没有游标 → **必须用 offset 翻页**。
///   此前命令侧无论什么来源都只写 `q.cursor`，在 offset 模式下 cursor 被
///   完全忽略，于是每轮返回同一页、循环永不退出（点一下批量操作就冻死 App）。
pub fn collect_track_ids(
    conn: &Connection,
    q: &TrackWindowQuery,
    max: usize,
) -> DbResult<IdCollection> {
    const PAGE: usize = 500;
    let max = max.max(1);
    let mut out: Vec<String> = Vec::new();
    let mut truncated = false;

    if key_expr(q).is_some() {
        let mut q = q.clone();
        q.limit = PAGE;
        q.mode = Mode::Cursor;
        q.cursor = None;
        q.offset = None;
        loop {
            let page = list_tracks(conn, &q)?;
            if page.items.is_empty() {
                break;
            }
            out.extend(page.items.iter().map(|t| t.id.clone()));
            if out.len() >= max {
                truncated = true;
                break;
            }
            match page.next_cursor {
                Some(c) => q.cursor = Some(c),
                None => break,
            }
        }
    } else {
        let mut q = q.clone();
        q.limit = PAGE;
        q.mode = Mode::Offset;
        q.cursor = None;
        let mut offset = 0usize;
        loop {
            q.offset = Some(offset);
            let page = list_tracks(conn, &q)?;
            let n = page.items.len();
            out.extend(page.items.iter().map(|t| t.id.clone()));
            if n < PAGE {
                break;
            }
            offset += n;
            if out.len() >= max {
                truncated = true;
                break;
            }
            // 防御：total 已知且已取完
            if offset >= page.total.max(0) as usize {
                break;
            }
        }
    }

    out.truncate(max);
    Ok(IdCollection { ids: out, truncated })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_in_memory;

    /// 重现 UI 报错 "missing field `kind`"：source 是嵌套对象（contracts/query.ts
    /// trackSourceSchema 形状），不能用 flatten 摊平到顶层。
    #[test]
    fn query_deserializes_nested_source() {
        // UI 实际形状：source 嵌套对象
        let v = serde_json::json!({
            "source": { "kind": "library" }, "mode": "cursor", "limit": 200,
            "sort": "title", "direction": "asc"
        });
        let q = serde_json::from_value::<TrackWindowQuery>(v)
            .expect("nested source must deserialize");
        assert_eq!(q.source, TrackSource::Library);

        // 歌单来源：camelCase playlistId → playlist_id
        let v2 = serde_json::json!({
            "source": { "kind": "playlist", "playlistId": "pl1" }, "mode": "offset", "offset": 0
        });
        let q2 = serde_json::from_value::<TrackWindowQuery>(v2).expect("playlist source");
        assert_eq!(q2.source, TrackSource::Playlist { playlist_id: "pl1".into() });

        // mode/limit 缺省也能反序列化（防御老调用方）
        let v3 = serde_json::json!({});
        let q3 = serde_json::from_value::<TrackWindowQuery>(v3).expect("empty defaults");
        assert_eq!(q3.source, TrackSource::Library);
        assert_eq!(q3.limit, 100);
    }

    fn insert_fixtures(conn: &Connection) {
        conn.execute_batch(
            r#"
            INSERT INTO artists (id, name, name_norm) VALUES ('a1','周杰伦','周杰伦');
            INSERT INTO albums (id, title, title_norm, artist_id, year) VALUES ('al1','叶惠美','叶惠美','a1',2003);
            INSERT INTO tracks (id,title,title_norm,artist_id,album_id,duration_ms,added_at,track_no,disc_no,year,path,path_lower,size_bytes,mtime,gain_db,play_count,last_played_at) VALUES
              ('t1','晴天','晴天','a1','al1',269000,100,3,1,2003,'/m/1.flac','/m/1.flac',1,1,-6.5,2,600),
              ('t2','以父之名','以父之名','a1','al1',342000,110,2,1,2003,'/m/2.flac','/m/2.flac',1,1,NULL,1,700),
              ('t3','Beethoven','beethoven',NULL,NULL,1000,120,NULL,NULL,NULL,'/m/3.mp3','/m/3.mp3',1,1,NULL,0,NULL);
            INSERT INTO history (track_id, played_at) VALUES ('t1', 500), ('t1', 600), ('t2', 700);
            "#,
        )
        .unwrap();
    }

    fn q_library(sort: Sort, mode: Mode, cursor: Option<String>, offset: Option<usize>) -> TrackWindowQuery {
        TrackWindowQuery {
            sort,
            direction: Direction::Asc,
            search: None,
            artist_id: None,
            album_id: None,
            source: TrackSource::Library,
            limit: 2,
            mode,
            cursor,
            offset,
        }
    }

    #[test]
    fn title_sort_and_cursor_pagination_no_dup_no_loss() {
        let conn = open_in_memory().unwrap();
        insert_fixtures(&conn);
        let page1 = list_tracks(&conn, &q_library(Sort::Title, Mode::Cursor, None, None)).unwrap();
        assert_eq!(page1.total, 3);
        assert_eq!(page1.items.len(), 2);
        assert!(page1.next_cursor.is_some());
        let page2 =
            list_tracks(&conn, &q_library(Sort::Title, Mode::Cursor, page1.next_cursor, None)).unwrap();
        let mut ids: Vec<_> = page1.items.iter().map(|t| t.id.clone()).collect();
        ids.extend(page2.items.iter().map(|t| t.id.clone()));
        ids.sort();
        assert_eq!(ids, vec!["t1", "t2", "t3"], "cursor pages must tile the whole set");
    }

    #[test]
    fn offset_mode() {
        let conn = open_in_memory().unwrap();
        insert_fixtures(&conn);
        let page = list_tracks(&conn, &q_library(Sort::Added, Mode::Offset, None, Some(1))).unwrap();
        assert_eq!(page.items.len(), 2);
        assert_eq!(page.items[0].id, "t2");
        assert_eq!(page.mode, "offset");
        assert_eq!(page.offset, Some(1));
    }

    #[test]
    fn search_hits_title_artist_album() {
        let conn = open_in_memory().unwrap();
        insert_fixtures(&conn);
        let mut q = q_library(Sort::Title, Mode::Offset, None, Some(0));
        q.search = Some("杰伦".into());
        let page = list_tracks(&conn, &q).unwrap();
        assert_eq!(page.total, 2, "artist-name search matches both tracks");

        q.search = Some("beet".into());
        let page = list_tracks(&conn, &q).unwrap();
        assert_eq!(page.total, 1);
        assert_eq!(page.items[0].id, "t3");
    }

    #[test]
    fn recent_and_most_played() {
        let conn = open_in_memory().unwrap();
        insert_fixtures(&conn);
        let mut q = q_library(Sort::Title, Mode::Offset, None, Some(0));

        q.source = TrackSource::Recent;
        let page = list_tracks(&conn, &q).unwrap();
        assert_eq!(page.items[0].id, "t2", "most recently played first");
        assert_eq!(page.items.len(), 2, "only played tracks appear");

        q.source = TrackSource::MostPlayed;
        let page = list_tracks(&conn, &q).unwrap();
        assert_eq!(page.items[0].id, "t1");
        assert_eq!(page.items[0].play_count, 2);
        assert_eq!(page.items[0].last_played_at, Some(600));
    }

    #[test]
    fn playlist_source_ordered_by_position() {
        let conn = open_in_memory().unwrap();
        insert_fixtures(&conn);
        conn.execute_batch(
            "INSERT INTO playlists (id,name,name_norm,created_at,updated_at) VALUES ('p1','我的歌单','我的歌单',1,1);
             INSERT INTO playlist_items (id,playlist_id,track_id,position,added_at) VALUES
               ('i1','p1','t3',0,1),('i2','p1','t1',1,1);",
        )
        .unwrap();
        let mut q = q_library(Sort::Title, Mode::Offset, None, Some(0));
        q.source = TrackSource::Playlist { playlist_id: "p1".into() };
        let page = list_tracks(&conn, &q).unwrap();
        assert_eq!(page.items[0].id, "t3");
        assert_eq!(page.items[1].id, "t1");
        assert_eq!(page.total, 2);
    }

    #[test]
    fn desc_direction_cursor_works() {
        let conn = open_in_memory().unwrap();
        insert_fixtures(&conn);
        let mut q = q_library(Sort::Added, Mode::Cursor, None, None);
        q.direction = Direction::Desc;
        let page1 = list_tracks(&conn, &q).unwrap();
        assert_eq!(page1.items[0].id, "t3", "newest first");
        let page2 = list_tracks(
            &conn,
            &TrackWindowQuery {
                sort: Sort::Added,
                direction: Direction::Desc,
                search: None,
                artist_id: None,
                album_id: None,
                source: TrackSource::Library,
                limit: 2,
                mode: Mode::Cursor,
                cursor: page1.next_cursor.clone(),
                offset: None,
            },
        )
        .unwrap();
        let mut ids: Vec<_> = page1.items.iter().map(|t| t.id.clone()).collect();
        ids.extend(page2.items.iter().map(|t| t.id.clone()));
        ids.sort();
        assert_eq!(ids, vec!["t1", "t2", "t3"]);
    }

    #[test]
    fn playlist_query_is_filtered_by_playlist_id() {
        let conn = open_in_memory().unwrap();
        insert_fixtures(&conn);
        // 两个歌单各含不同曲目；查 p1 绝不能带出 p2 的曲目
        conn.execute_batch(
            "INSERT INTO playlists (id,name,name_norm,created_at,updated_at) VALUES
               ('p1','歌单一','歌单一',1,1),('p2','歌单二','歌单二',1,1);
             INSERT INTO playlist_items (id,playlist_id,track_id,position,added_at) VALUES
               ('i1','p1','t1',0,1),('i2','p1','t2',1,1),
               ('i3','p2','t3',0,1);",
        )
        .unwrap();
        let mut q = q_library(Sort::Title, Mode::Offset, None, Some(0));
        q.source = TrackSource::Playlist { playlist_id: "p1".into() };
        let page = list_tracks(&conn, &q).unwrap();
        let ids: Vec<_> = page.items.iter().map(|t| t.id.as_str()).collect();
        assert_eq!(ids, vec!["t1", "t2"], "p1 不含 p2 的曲目 t3");
        assert_eq!(page.total, 2);

        q.source = TrackSource::Playlist { playlist_id: "p2".into() };
        let page2 = list_tracks(&conn, &q).unwrap();
        let ids2: Vec<_> = page2.items.iter().map(|t| t.id.as_str()).collect();
        assert_eq!(ids2, vec!["t3"]);
        assert_eq!(page2.total, 1);
    }

    #[test]
    fn search_wildcards_are_literal() {
        let conn = open_in_memory().unwrap();
        insert_fixtures(&conn);
        // 搜 "bee_t" 应按字面匹配（不应把 _ 当单字符通配去匹配 beethoven）
        let mut q = q_library(Sort::Title, Mode::Offset, None, Some(0));
        q.search = Some("bee_t".into());
        let page = list_tracks(&conn, &q).unwrap();
        assert_eq!(page.total, 0, "下划线按字面处理");

        // 同理 % 按字面处理
        q.search = Some("beet%".into());
        let page = list_tracks(&conn, &q).unwrap();
        assert_eq!(page.total, 0);
    }

    #[test]
    fn cursor_no_extra_empty_page_when_total_is_multiple_of_limit() {
        let conn = open_in_memory().unwrap();
        insert_fixtures(&conn);
        // total=3，limit=3：翻完第一页后不应再有下一页（旧行为会多发一次空页）
        let mut q = q_library(Sort::Title, Mode::Cursor, None, None);
        q.limit = 3;
        let page = list_tracks(&conn, &q).unwrap();
        assert_eq!(page.items.len(), 3);
        assert!(page.next_cursor.is_none(), "整除时不得多发空页");
    }

    /// total 必须与游标无关（否则每翻一页数字缩水）
    #[test]
    fn total_is_stable_across_cursor_pages() {
        let conn = open_in_memory().unwrap();
        insert_fixtures(&conn);
        let page1 = list_tracks(&conn, &q_library(Sort::Title, Mode::Cursor, None, None)).unwrap();
        assert_eq!(page1.total, 3);
        let page2 = list_tracks(
            &conn,
            &q_library(Sort::Title, Mode::Cursor, page1.next_cursor.clone(), None),
        )
        .unwrap();
        assert_eq!(page2.total, 3, "第二页的 total 仍是全量 3，不是剩余 1");
    }

    /// 死循环回归：UI 传 offset 模式的 query 进来，批量收集也必须收敛。
    /// 此前命令侧只写 `q.cursor`，offset 模式下被忽略 → 每轮同一页 → 永不退出。
    #[test]
    fn collect_ids_terminates_with_offset_mode_input() {
        let conn = open_in_memory().unwrap();
        insert_fixtures(&conn);
        let mut q = q_library(Sort::Title, Mode::Offset, None, Some(0));
        q.limit = 200; // UI 的列表页大小
        let got = collect_track_ids(&conn, &q, 10_000).unwrap();
        assert_eq!(got.ids.len(), 3, "三首必须全部收集到（且必须终止）");
        assert!(!got.truncated);
    }

    /// playlist 来源没有游标键：必须走 offset 翻页，否则第一页后就停
    #[test]
    fn collect_ids_covers_playlist_source() {
        let conn = open_in_memory().unwrap();
        insert_fixtures(&conn);
        conn.execute_batch(
            "INSERT INTO playlists (id,name,name_norm,created_at,updated_at) VALUES ('p1','单','单',1,1);
             INSERT INTO playlist_items (id,playlist_id,track_id,position,added_at) VALUES
               ('i1','p1','t3',0,1),('i2','p1','t1',1,1),('i3','p1','t2',2,1);",
        )
        .unwrap();
        let mut q = q_library(Sort::Title, Mode::Offset, None, Some(0));
        q.source = TrackSource::Playlist { playlist_id: "p1".into() };
        let got = collect_track_ids(&conn, &q, 10_000).unwrap();
        assert_eq!(got.ids, vec!["t3", "t1", "t2"], "按 position 顺序收全");
        assert!(!got.truncated);
    }

    #[test]
    fn collect_ids_reports_truncation() {
        let conn = open_in_memory().unwrap();
        insert_fixtures(&conn);
        let q = q_library(Sort::Title, Mode::Offset, None, Some(0));
        let got = collect_track_ids(&conn, &q, 2).unwrap();
        assert_eq!(got.ids.len(), 2);
        assert!(got.truncated, "被 max 截断必须显式告知调用方");
    }
}
