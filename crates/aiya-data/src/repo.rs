//! 曲目列表查询：双模式分页（cursor / offset）、四种来源、LIKE 搜索。
//!
//! 对齐 `@aiya/contracts` 的 `TrackQuery` / `TrackPage` 形状（serde camelCase），
//! v1 阶段这是 UI 兼容层的一部分；契约迁回 Rust 源时此文件就是雏形。

use std::time::Instant;

use rusqlite::types::ToSql;
use rusqlite::{Connection, Row};
use serde::{Deserialize, Serialize};
use ts_rs::TS;
use serde_json::Value;

use crate::db::{DbError, DbResult};
use crate::norm::norm;
use crate::smart::to_sql_value;

type Binder = Box<dyn Fn(&mut Vec<Box<dyn ToSql>>) + Send>;

/* ---------------- DTO ---------------- */

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct TrackRow {
    pub id: String,
    pub title: String,
    pub artist_id: Option<String>,
    pub artist: Option<String>,
    pub album_id: Option<String>,
    pub album: Option<String>,
    pub album_artist: Option<String>,
    #[ts(type = "number | null")]
    pub duration_ms: Option<i64>,
    #[ts(type = "number | null")]
    pub track_no: Option<i64>,
    #[ts(type = "number | null")]
    pub disc_no: Option<i64>,
    #[ts(type = "number | null")]
    pub year: Option<i64>,
    /// 入库时间。**毫秒**（DB 存的是 unix 秒，映射时 ×1000）。
    /// 名字带单位是刻意的：同结构里 `durationMs` 是时长毫秒，
    /// 旧字段 `addedAt` 却是秒，UI 侧 `new Date(addedAt)` 会得到 1970 年。
    #[ts(type = "number")]
    pub added_at_ms: i64,
    pub artwork_id: Option<String>,
    #[ts(type = "number")]
    pub play_count: i64,
    /// 最后播放时间，毫秒（DB 存秒）。从未播过为 null。
    #[ts(type = "number | null")]
    pub last_played_at_ms: Option<i64>,
    pub gain_db: Option<f64>,
    /// 文件可用性：online / offline / missing。扫描器与 beets 对账都会改它，
    /// UI 用它给行打灰标（此前 DTO 里没有 → 列表看不出哪首已经找不到文件）。
    pub availability: String,
    /// 流派（0003 迁移，beets 同步带来）。列表不显示，筛选器 conds 用。
    pub genre: Option<String>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct TrackPage {
    pub items: Vec<TrackRow>,
    #[ts(type = "number")]
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
    /// 智能歌单（0003 迁移）：曲目列表不物化，打开时按 `rule_json` 实时查询
    /// （`smart::load_plan` 编译成 WHERE/ORDER BY）。UI 仍发 `kind:'playlist'`，
    /// 由 `list_tracks` 查 kind 后路由到这里。
    SmartPlaylist { playlist_id: String },
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
    /// 入库时间范围（毫秒，含 from 不含 to）。列表工具条的筛选器模态框用。
    #[serde(default)]
    pub added_from_ms: Option<i64>,
    #[serde(default)]
    pub added_to_ms: Option<i64>,
    /// 上次播放时间范围（毫秒，含 from 不含 to）。
    #[serde(default)]
    pub last_played_from_ms: Option<i64>,
    #[serde(default)]
    pub last_played_to_ms: Option<i64>,
    /// 只看从未播放过的曲目（`last_played_at IS NULL`）；与 last_played 范围互斥，
    /// 同时给时 never 优先。
    #[serde(default)]
    pub last_played_never: Option<bool>,
    /// 列表筛选器的临时条件（`[{field,op,value},...]`）——与智能歌单同一批
    /// 字段键（smart::parse_conds 解析），但**只在本次查询生效、不落库**。
    /// 解析失败明确报错（与智能歌单同原则），UI 侧 validateRule 先拦一道。
    #[serde(default)]
    pub conds: Option<serde_json::Value>,
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
     t.play_count, t.last_played_at, t.gain_db, t.availability, t.genre";

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

/// JOIN 与排序（playlist/recent/mostPlayed 不吃用户 sort 字段）。
/// 例外：recent 吃 `direction`——曲库页「最近播放」tab 二次点击翻转正倒序
/// （2026-10-30），sort 字段仍忽略；mostPlayed 的次键是最近播放时间
/// （docs/28）：次数并列时"最近还在听"排前面——推荐页 top-15 的第 15 名
/// 争夺战里，半年没听的那首先出去。
fn join_and_order(q: &TrackWindowQuery) -> (&'static str, &'static str) {
    match &q.source {
        TrackSource::Library => (JOIN, ""),
        TrackSource::Playlist { .. } => (JOIN_PLAYLIST, " ORDER BY pi.position ASC"),
        // 智能歌单：JOIN 与曲库同源（ar/al 别名给排序用）；ORDER BY 由计划动态给
        TrackSource::SmartPlaylist { .. } => (JOIN, ""),
        TrackSource::Recent => (
            JOIN_AGG,
            if q.direction == Direction::Desc {
                " ORDER BY hs.recent_t0 DESC, t.id DESC"
            } else {
                " ORDER BY hs.recent_t0 ASC, t.id ASC"
            },
        ),
        TrackSource::MostPlayed => (
            JOIN_AGG,
            " ORDER BY hs.played_count DESC, hs.recent_t0 DESC, t.id ASC",
        ),
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
/// `smart` 是智能歌单的已编译计划；`conds` 是筛选器临时条件（同样已编译）。
/// 占位符顺序 = 推入顺序（search → artist → album → time → conds →
/// playlist → smart → cursor）。
fn build_where(
    q: &TrackWindowQuery,
    cursor_filter: Option<&str>,
    smart: Option<&crate::smart::SmartPlan>,
    conds: &[(String, serde_json::Value)],
) -> (String, Vec<Binder>) {
    let mut binders: Vec<Binder> = Vec::new();
    let mut where_clauses: Vec<String> = Vec::new();
    if let Some(search) = q.search.as_deref().filter(|s| !s.trim().is_empty()) {
        // LIKE 通配符防误匹配：按字面语义转义 % _ \（\ 同时是 ESCAPE 字符）
        let pattern = format!("%{}%", crate::norm::escape_like(&norm(search)));
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
    // 时间筛选（筛选器模态框）：DB 存 unix 秒，入参是毫秒——绑定前 /1000。
    // to 是开区间（前端给"下一档起点"），所以用 <；from 用 >=。
    if let Some(ms) = q.added_from_ms {
        binders.push(Box::new(move |v: &mut Vec<Box<dyn ToSql>>| {
            v.push(Box::new(ms / 1000));
        }));
        where_clauses.push("t.added_at >= ?".to_string());
    }
    if let Some(ms) = q.added_to_ms {
        binders.push(Box::new(move |v: &mut Vec<Box<dyn ToSql>>| {
            v.push(Box::new(ms / 1000));
        }));
        where_clauses.push("t.added_at < ?".to_string());
    }
    if q.last_played_never == Some(true) {
        // 「从未播放」与时间范围互斥，never 优先（见字段注释）
        where_clauses.push("t.last_played_at IS NULL".to_string());
    } else {
        if let Some(ms) = q.last_played_from_ms {
            binders.push(Box::new(move |v: &mut Vec<Box<dyn ToSql>>| {
                v.push(Box::new(ms / 1000));
            }));
            where_clauses.push("t.last_played_at >= ?".to_string());
        }
        if let Some(ms) = q.last_played_to_ms {
            binders.push(Box::new(move |v: &mut Vec<Box<dyn ToSql>>| {
                v.push(Box::new(ms / 1000));
            }));
            where_clauses.push("t.last_played_at < ?".to_string());
        }
    }
    // 筛选器临时条件：每条子句恰好一个 `?`，绑定顺序 = clauses 顺序
    for (clause, value) in conds {
        let v = value.clone();
        binders.push(Box::new(move |out: &mut Vec<Box<dyn ToSql>>| {
            out.push(Box::new(to_sql_value(&v)));
        }));
        where_clauses.push(clause.clone());
    }
    // 歌单来源：JOIN_PLAYLIST 从 playlist_items 出发，必须按 playlist_id 过滤，
    // 否则任何歌单都返回全部曲目（P0）。占位符顺序 = binder 推入顺序
    // （search → artist → album → playlist → smart → cursor）。
    // 智能歌单（smart 接管）不走 playlist_items —— 条件在 plan.clauses 里。
    if let (TrackSource::Playlist { playlist_id }, None) = (&q.source, smart) {
        let pid = playlist_id.clone();
        binders.push(Box::new(move |v: &mut Vec<Box<dyn ToSql>>| {
            v.push(Box::new(pid.clone()));
        }));
        where_clauses.push("pi.playlist_id = ?".to_string());
    }
    // 智能歌单规则条件：每条子句恰好一个 `?`，绑定顺序 = clauses 顺序
    if let Some(plan) = smart {
        for (clause, value) in &plan.clauses {
            let v = value.clone();
            binders.push(Box::new(move |out: &mut Vec<Box<dyn ToSql>>| {
                out.push(Box::new(to_sql_value(&v)));
            }));
            where_clauses.push(clause.clone());
        }
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
fn build_head(
    q: &TrackWindowQuery,
    cursor_filter: Option<&str>,
    smart: Option<&crate::smart::SmartPlan>,
    conds: &[(String, serde_json::Value)],
) -> (String, Vec<Binder>) {
    let (where_sql, binders) = build_where(q, cursor_filter, smart, conds);
    let (join, fixed_order) = join_and_order(q);
    // 智能歌单（无论 UI 发 Playlist 还是 SmartPlaylist 来源）都从 tracks 出发
    // —— playlist_items 对它恒空，JOIN_PLAYLIST 会把结果集清成 0 行
    let join = if smart.is_some() { JOIN } else { join };
    // JOIN 换掉后 pi 别名不存在，Playlist 来源自带的"位置序"（pi.position）
    // 也必须一起丢——智能歌单没有手动顺序语义，走下面的列头排序。
    let fixed_order = if smart.is_some() { "" } else { fixed_order };
    let order_sql = if fixed_order.is_empty() {
        // 智能歌单的排序也吃 q.sort（2026-10-28 拍板：排序由列表视图的列头
        // 处理，规则编辑器不再暴露排序键）。UI 的 sort 状态默认 title/asc，
        // 与规则里的兼容默认一致；用户点列头后随点击走。
        // 规则里的 order_sql 仍被 `apply_smart_stats`（侧栏摘要计数）使用。
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
fn count_from(q: &TrackWindowQuery, smart: Option<&crate::smart::SmartPlan>) -> String {
    let has_search = q.search.as_deref().map(|s| !s.trim().is_empty()).unwrap_or(false);
    let base = match &q.source {
        TrackSource::Library => "FROM tracks t".to_string(),
        // 智能歌单与曲库同源（playlist_items 对它恒空）
        TrackSource::Playlist { .. } if smart.is_some() => "FROM tracks t".to_string(),
        TrackSource::Playlist { .. } => {
            "FROM playlist_items pi JOIN tracks t ON t.id = pi.track_id".to_string()
        }
        // 智能歌单条件全部自包含（t.* / IN 子查询），不需要外层 ar/al JOIN
        TrackSource::SmartPlaylist { .. } => "FROM tracks t".to_string(),
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
fn build_count(
    q: &TrackWindowQuery,
    smart: Option<&crate::smart::SmartPlan>,
    conds: &[(String, serde_json::Value)],
) -> (String, Vec<Binder>) {
    let (where_sql, binders) = build_where(q, None, smart, conds);
    (format!("SELECT COUNT(*) {}{where_sql}", count_from(q, smart)), binders)
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

pub fn row_to_track(r: &Row<'_>) -> rusqlite::Result<TrackRow> {
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
        // DB 存 unix 秒，DTO 一律毫秒（docs/17 §3.1）
        added_at_ms: r.get::<_, i64>(11)? * 1000,
        artwork_id: r.get(12)?,
        play_count: r.get(13)?,
        last_played_at_ms: r.get::<_, Option<i64>>(14)?.map(|s| s * 1000),
        gain_db: r.get(15)?,
        availability: r.get(16)?,
        genre: r.get(17)?,
    })
}

/// 列表查询入口。返回 `TrackPage`（含 total / nextCursor / tookMs）。
pub fn list_tracks(conn: &Connection, q: &TrackWindowQuery) -> DbResult<TrackPage> {
    let started = Instant::now();
    let limit = q.limit.clamp(1, 500) as i64;

    // ---------- 智能歌单计划（来源是 SmartPlaylist 才有）----------
    // 在这里查 kind 而不是让 UI 发新 kind：UI 对手动/智能歌单发同一个
    // `kind:'playlist'` 查询，路由是数据层自己的事（collect_track_ids 走
    // list_tracks，自动同源）。
    let smart_plan = match &q.source {
        TrackSource::SmartPlaylist { playlist_id } => {
            Some(crate::smart::load_plan(conn, playlist_id)?)
        }
        TrackSource::Playlist { playlist_id } => {
            // 手动/智能的分流：查一次 kind（歌单行很小，且后续 COUNT/页查询
            // 都是大活，这一次点查成本可忽略）
            let kind: Option<String> = conn
                .query_row(
                    "SELECT kind FROM playlists WHERE id = ?1",
                    [playlist_id],
                    |r| r.get(0),
                )
                .map_err(DbError::Sql)?;
            if kind.as_deref() == Some("smart") {
                Some(crate::smart::load_plan(conn, playlist_id)?)
            } else {
                None
            }
        }
        _ => None,
    };
    let smart = smart_plan.as_ref();

    // ---------- 筛选器临时条件（与智能歌单同键，不落库）----------
    let cond_clauses = crate::smart::conds_from_value(
        q.conds.as_ref().unwrap_or(&serde_json::Value::Null),
    )?;

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

    let (head, mut binders) = build_head(q, cursor_filter.as_deref(), smart, &cond_clauses);
    binders.extend(cursor_binders);

    // ---------- 规则上限（rule_json.limit）----------
    // 页窗口与 total 都钳到上限内：limit=100 时第 3 页只剩不到一页的量，
    // 探测行逻辑（limit+1）保持不变，翻过头就给空页。
    let page_limit = match smart.and_then(|p| p.limit) {
        Some(rl) => (rl - offset).max(0).min(limit),
        None => limit,
    };

    // ---------- COUNT（同 WHERE 但**不含游标谓词**）----------
    // 此前 COUNT 复用了带游标条件的 head，于是 total 是"游标之后的剩余量"，
    // 每翻一页递减 —— UI 拿它画滚动条/显示"N 首"会一路缩水。
    let total: i64 = {
        let (count_sql, count_binders) = build_count(q, smart, &cond_clauses);
        let mut stmt = conn.prepare(&count_sql)?;
        bind_all(&mut stmt, &count_binders, None)?;
        let mut rows = stmt.raw_query();
        let raw: i64 = match rows.next()? {
            Some(row) => row.get(0)?,
            None => 0,
        };
        match smart.and_then(|p| p.limit) {
            Some(rl) => raw.min(rl),
            None => raw,
        }
    };

    // ---------- 页数据（limit+1 探测 next）----------
    // 读满 limit+1 行才视为 has_more（多出的探测行丢弃）；不能用 total 取模判断
    // —— total 恰为 limit 整数倍时会多发一次空页。
    let (items, has_more): (Vec<TrackRow>, bool) = {
        let sql = format!("{head} LIMIT ? OFFSET ?");
        let mut stmt = conn.prepare(&sql)?;
        bind_all(&mut stmt, &binders, Some((page_limit + 1, offset)))?;
        let mut rows = stmt.raw_query();
        let mut out: Vec<TrackRow> = Vec::with_capacity(page_limit as usize + 1);
        let mut more = false;
        while let Some(r) = rows.next()? {
            if out.len() as i64 == page_limit {
                // 探测行：只判定还有下一页，不消费进结果。
                // 规则上限截断（page_limit < limit）时下一页必空，不算 has_more
                more = page_limit == limit;
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
            // 游标要回喂给 SQL（`t.added_at` 是秒），所以这里必须除回秒；
            // 漏了这一步，added 排序的游标分页会静默错位（值差 1000 倍）。
            Sort::Added => Value::from(last.added_at_ms / 1000),
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
            added_from_ms: None,
            added_to_ms: None,
            last_played_from_ms: None,
            last_played_to_ms: None,
            last_played_never: None,
            conds: None,
            source: TrackSource::Library,
            limit: 2,
            mode,
            cursor,
            offset,
        }
    }

    /// 时间筛选（筛选器模态框）：毫秒入参 → 秒比较；never 优先于范围。
    /// 夹具 added_at：t1=100s, t2=110s, t3=120s；last_played：t1=600s, t2=700s, t3=NULL。
    #[test]
    fn time_filters_added_and_last_played() {
        let conn = open_in_memory().unwrap();
        insert_fixtures(&conn);

        // 入库 [105s, 115s) → 只有 t2
        let mut q = q_library(Sort::Title, Mode::Offset, None, Some(0));
        q.added_from_ms = Some(105_000);
        q.added_to_ms = Some(115_000);
        let page = list_tracks(&conn, &q).unwrap();
        assert_eq!(page.items.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(), vec!["t2"]);

        // 上次播放 >= 650s → 只有 t2（t1 是 600s，t3 从未播）
        let mut q = q_library(Sort::Title, Mode::Offset, None, Some(0));
        q.last_played_from_ms = Some(650_000);
        let page = list_tracks(&conn, &q).unwrap();
        assert_eq!(page.items.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(), vec!["t2"]);

        // 从未播放 → 只有 t3；never 与范围同给时 never 优先
        let mut q = q_library(Sort::Title, Mode::Offset, None, Some(0));
        q.last_played_never = Some(true);
        let page = list_tracks(&conn, &q).unwrap();
        assert_eq!(page.items.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(), vec!["t3"]);
        let mut q = q_library(Sort::Title, Mode::Offset, None, Some(0));
        q.last_played_never = Some(true);
        q.last_played_from_ms = Some(0);
        let page = list_tracks(&conn, &q).unwrap();
        assert_eq!(page.items.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(), vec!["t3"]);
    }

    /// 筛选器临时条件（conds）：与智能歌单同键编译成 WHERE，不落库。
    #[test]
    fn filter_conds_narrow_results() {
        let conn = open_in_memory().unwrap();
        insert_fixtures(&conn);
        conn.execute_batch(
            "UPDATE tracks SET genre='Pop', genre_norm='pop' WHERE id='t1';
             UPDATE tracks SET genre='Rock', genre_norm='rock' WHERE id='t2';",
        )
        .unwrap();

        // genre contains "oc" → 只命中 Rock（t2）；total 同口径
        let mut q = q_library(Sort::Title, Mode::Offset, None, Some(0));
        q.conds = Some(serde_json::json!([
            { "field": "genre", "op": "contains", "value": "oc" }
        ]));
        let page = list_tracks(&conn, &q).unwrap();
        assert_eq!(page.items.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(), vec!["t2"]);
        assert_eq!(page.total, 1);

        // 坏键明确报错——UI 的 bug 不该静默变成全量列表
        let mut q = q_library(Sort::Title, Mode::Offset, None, Some(0));
        q.conds = Some(serde_json::json!([{ "field": "bpm", "op": "eq", "value": 1 }]));
        assert!(list_tracks(&conn, &q).is_err());
    }

    /// contains 条件的 LIKE 值按字面处理：`%`/`_` 转义，不当通配符。
    #[test]
    fn cond_contains_wildcards_are_literal() {
        let conn = open_in_memory().unwrap();
        insert_fixtures(&conn);
        conn.execute("UPDATE tracks SET genre='Pop', genre_norm='pop' WHERE id='t1'", []).unwrap();

        // "p%P" 按字面匹配（pop 里没有字面的 %）→ 0 命中；不当转义会命中 t1
        let mut q = q_library(Sort::Title, Mode::Offset, None, Some(0));
        q.conds = Some(serde_json::json!([
            { "field": "genre", "op": "contains", "value": "p%P" }
        ]));
        let page = list_tracks(&conn, &q).unwrap();
        assert_eq!(page.total, 0, "% 必须按字面处理");

        // 同一规则正常子串仍命中
        let mut q = q_library(Sort::Title, Mode::Offset, None, Some(0));
        q.conds = Some(serde_json::json!([
            { "field": "genre", "op": "contains", "value": "po" }
        ]));
        let page = list_tracks(&conn, &q).unwrap();
        assert_eq!(page.items.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(), vec!["t1"]);
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
        // recent 吃 direction（2026-10-30）：本测试的意图是"默认语义顺序"，
        // 显式给 Desc——q_library 缺省 Asc，不显式给会翻成最早优先
        q.direction = Direction::Desc;

        q.source = TrackSource::Recent;
        let page = list_tracks(&conn, &q).unwrap();
        assert_eq!(page.items[0].id, "t2", "most recently played first");
        assert_eq!(page.items.len(), 2, "only played tracks appear");

        q.source = TrackSource::MostPlayed;
        let page = list_tracks(&conn, &q).unwrap();
        assert_eq!(page.items[0].id, "t1");
        assert_eq!(page.items[0].play_count, 2);
        // 600 是 DB 里的秒，DTO 一律毫秒（docs/17 §3.1）——这条断言同时锁住换算
        assert_eq!(page.items[0].last_played_at_ms, Some(600_000));

        // 次数并列时按最近播放优先（docs/28 次键）：t4 与 t2 都是 1 次，
        // t4 的最后播放（800s）晚于 t2（700s），所以 t4 排前面
        conn.execute_batch(
            "INSERT INTO tracks (id,title,title_norm,duration_ms,added_at,path,path_lower,size_bytes,mtime,play_count) VALUES
              ('t4','夜曲','夜曲',1000,130,'/m/4.flac','/m/4.flac',1,1,1);
             INSERT INTO history (track_id, played_at) VALUES ('t4', 800);",
        )
        .unwrap();
        q.limit = 10; // q_library 缺省 2，这里要装下全部 3 首
        let page = list_tracks(&conn, &q).unwrap();
        assert_eq!(
            page.items.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(),
            vec!["t1", "t4", "t2"],
            "tie on play_count resolves by last-played recency"
        );
    }

    /// 曲库页「最近播放」tab 二次点击翻转正倒序（2026-10-30）：
    /// recent 来源的 ORDER BY 跟随 direction，t.id 作稳定次键。
    #[test]
    fn recent_direction_flips_order() {
        let conn = open_in_memory().unwrap();
        insert_fixtures(&conn);
        let mut q = q_library(Sort::Title, Mode::Offset, None, Some(0));
        q.source = TrackSource::Recent;

        q.direction = Direction::Desc;
        let page = list_tracks(&conn, &q).unwrap();
        assert_eq!(
            page.items.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(),
            vec!["t2", "t1"],
            "倒序（语义默认）：最近播的在前"
        );

        q.direction = Direction::Asc;
        let page = list_tracks(&conn, &q).unwrap();
        assert_eq!(
            page.items.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(),
            vec!["t1", "t2"],
            "升序：最早播的在前（sort 字段仍被忽略，只翻方向）"
        );
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
                added_from_ms: None,
                added_to_ms: None,
                last_played_from_ms: None,
                last_played_to_ms: None,
                last_played_never: None,
                conds: None,
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
