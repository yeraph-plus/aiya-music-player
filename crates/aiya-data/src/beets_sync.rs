//! beets 读模型对齐（docs/08 §5）：beets 是写权威，自有 DB 是读模型。
//!
//! 数据源不是文件解析，而是 sidecar `dump_items` 的 beets items（行帧 JSON 流）。
//! 与 `scanner::upsert` 同构（实体 upsert + track upsert + 稳定 ID），差异：
//!   * 对账 key = 文件路径（beets item.path ↔ tracks.path），大小写不敏感
//!     （Windows 文件系统语义；beets 与 lofty 对盘符大小写可能不一致）
//!   * 写 beets_item_id（schema v1 预留列，此前恒 NULL）
//!   * gain_db 来自 beets rg_track_gain（扫描器不读 RG 标签）
//!   * codec 用 beets format（扫描器该列恒 NULL，container 才存 lofty FileType）
//!   * 不动 size/availability（文件事实归扫描器）；mtime 取 beets 值（同源）
//!
//! 边界：自有无、beets 有的路径 → 新导入；自有无、beets 也无 → 扫描器的地盘；
//! beets_item_id 指向的 item 已从 beets 库消失 → 只计数（orphaned），不删行。
//! genre 随 0003 迁移入库（动态歌单的标签数据源，docs/40 §1）：协议 DumpItem
//! 本来就带 genre，这里只补迁移；beets 侧变 None 不清已有值（全文件惯例）。

use std::collections::{HashMap, HashSet};

use rusqlite::Connection;
use serde::Deserialize;

use crate::db::{DbError, DbResult};
use crate::ids;
use crate::norm::norm;

/// sidecar dump_items 单条 item（字段名 = beets 原名，snake_case）。
/// 未列出的字段被 serde 忽略。
#[derive(Debug, Clone, Deserialize)]
pub struct BeetsItem {
    pub id: i64,
    pub path: String,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub albumartist: Option<String>,
    pub album: Option<String>,
    pub genre: Option<String>,
    pub year: Option<i64>,
    pub track: Option<i64>,
    pub disc: Option<i64>,
    pub length: Option<f64>,
    pub bitrate: Option<i64>,
    pub format: Option<String>,
    pub samplerate: Option<i64>,
    pub bitdepth: Option<i64>,
    pub channels: Option<i64>,
    pub mtime: Option<i64>,
    pub rg_track_gain: Option<f64>,
}

#[derive(Debug, Default, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncStats {
    /// 路径命中（= updated + unchanged）
    pub matched: usize,
    /// 命中且实际更新了行（含只补 beets_item_id）
    pub updated: usize,
    /// beets 有、自有无 → 新导入
    pub imported: usize,
    /// 命中且零差异
    pub unchanged: usize,
    /// 自有 beets_item_id 指向的 item 已不在 beets 库（只计数）
    pub orphaned: usize,
}

fn non_empty(s: &Option<String>) -> Option<&str> {
    s.as_deref().map(str::trim).filter(|s| !s.is_empty())
}

/// 命中行现有值（内存对账用，避免逐条回查）。
#[derive(Debug)]
struct Existing {
    id: String,
    path: String,
    title: String,
    genre: Option<String>,
    artist_id: Option<String>,
    album_id: Option<String>,
    duration_ms: Option<i64>,
    track_no: Option<i64>,
    disc_no: Option<i64>,
    year: Option<i64>,
    gain_db: Option<f64>,
    codec: Option<String>,
    sample_rate: Option<i64>,
    bits_per_sample: Option<i64>,
    bitrate: Option<i64>,
    channels: Option<i64>,
    beets_item_id: Option<i64>,
}

impl Existing {
    /// 与 beets 目标值逐字段比较；None = 该字段 beets 没给，保留现值不算差异。
    fn differs_from(&self, it: &BeetsItem) -> bool {
        if self.beets_item_id != Some(it.id) {
            return true;
        }
        let target_title = non_empty(&it.title).map(str::to_string);
        if let Some(t) = target_title {
            if self.title != t {
                return true;
            }
        }
        // genre：beets 给了值才算差异（None 不清已有值）
        if let Some(g) = non_empty(&it.genre) {
            if self.genre.as_deref() != Some(g) {
                return true;
            }
        }
        let artist_name = non_empty(&it.artist).or_else(|| non_empty(&it.albumartist));
        if let Some(n) = artist_name {
            if self.artist_id.as_deref() != Some(ids::artist_id(n).as_str()) {
                return true;
            }
        }
        let album_group = non_empty(&it.albumartist)
            .or_else(|| non_empty(&it.artist))
            .unwrap_or("");
        if let Some(al) = non_empty(&it.album) {
            if self.album_id.as_deref() != Some(ids::album_id(al, album_group).as_str()) {
                return true;
            }
        }
        if it.length.is_some() && self.duration_ms != it.length.map(len_to_ms) {
            return true;
        }
        if it.track.is_some() && self.track_no != it.track {
            return true;
        }
        if it.disc.is_some() && self.disc_no != it.disc {
            return true;
        }
        if it.year.is_some() && self.year != it.year {
            return true;
        }
        if it.rg_track_gain.is_some() && self.gain_db != it.rg_track_gain {
            return true;
        }
        if let Some(f) = non_empty(&it.format) {
            if self.codec.as_deref() != Some(f) {
                return true;
            }
        }
        if it.samplerate.is_some() && self.sample_rate != it.samplerate {
            return true;
        }
        if it.bitdepth.is_some() && self.bits_per_sample != it.bitdepth {
            return true;
        }
        if it.bitrate.is_some() && self.bitrate != it.bitrate {
            return true;
        }
        if it.channels.is_some() && self.channels != it.channels {
            return true;
        }
        false
    }
}

fn len_to_ms(len: f64) -> i64 {
    (len * 1000.0).round() as i64
}

/// 实体 upsert（与 scanner 同构：artist 名 → id，album (title, group) → id）。
fn upsert_entities(
    tx: &Connection,
    it: &BeetsItem,
) -> DbResult<(Option<String>, Option<String>)> {
    let artist_name = non_empty(&it.artist).or_else(|| non_empty(&it.albumartist));
    let artist_id = artist_name.map(ids::artist_id);
    if let (Some(aid), Some(aname)) = (&artist_id, artist_name) {
        tx.execute(
            "INSERT INTO artists (id, name, name_norm) VALUES (?1, ?2, ?3)
             ON CONFLICT(id) DO UPDATE SET name = excluded.name, name_norm = excluded.name_norm",
            rusqlite::params![aid, aname, norm(aname)],
        )?;
    }

    let album_group = non_empty(&it.albumartist)
        .or_else(|| non_empty(&it.artist))
        .unwrap_or("");
    let album_id = non_empty(&it.album).map(|al| ids::album_id(al, album_group));
    if let (Some(alid), Some(alt)) = (&album_id, non_empty(&it.album)) {
        tx.execute(
            "INSERT INTO albums (id, title, title_norm, artist_id, year) VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(id) DO UPDATE SET
               title = excluded.title,
               title_norm = excluded.title_norm,
               artist_id = COALESCE(excluded.artist_id, albums.artist_id),
               year = COALESCE(excluded.year, albums.year)",
            rusqlite::params![alid, alt, norm(alt), artist_id, it.year],
        )?;
    }
    Ok((artist_id, album_id))
}

/// beets item → tracks 行（新插入路径）。
fn insert_track(tx: &Connection, it: &BeetsItem, now: i64) -> DbResult<()> {
    let (artist_id, album_id) = upsert_entities(tx, it)?;
    let title = non_empty(&it.title).unwrap_or("Untitled").to_string();
    tx.execute(
        "INSERT INTO tracks (
            id, title, title_norm, artist_id, album_id, duration_ms, track_no, disc_no, year,
            added_at, artwork_id, gain_db, genre, genre_norm,
            codec, container, sample_rate, bits_per_sample, bitrate, channels,
            play_count, last_played_at,
            path, path_lower, size_bytes, mtime, availability, beets_item_id
         ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,NULL,?11,?12,?13,
                   ?14,NULL,?15,?16,?17,?18,0,NULL,?19,?20,0,?21,'online',?22)",
        rusqlite::params![
            ids::track_id(&it.path),
            title,
            norm(&title),
            artist_id,
            album_id,
            it.length.map(len_to_ms),
            it.track,
            it.disc,
            it.year,
            now,
            it.rg_track_gain,
            non_empty(&it.genre),
            non_empty(&it.genre).map(norm),
            non_empty(&it.format),
            it.samplerate,
            it.bitdepth,
            it.bitrate,
            it.channels,
            it.path,
            it.path.to_lowercase(),
            it.mtime.unwrap_or(0),
            it.id,
        ],
    )?;
    Ok(())
}

/// beets item → 已有行更新（只覆盖 beets 给了值的字段）。
fn update_track(tx: &Connection, ex: &Existing, it: &BeetsItem) -> DbResult<()> {
    let (artist_id, album_id) = upsert_entities(tx, it)?;
    tx.execute(
        "UPDATE tracks SET
           title = COALESCE(?2, title), title_norm = COALESCE(?3, title_norm),
           artist_id = COALESCE(?4, artist_id), album_id = COALESCE(?5, album_id),
           duration_ms = COALESCE(?6, duration_ms), track_no = COALESCE(?7, track_no),
           disc_no = COALESCE(?8, disc_no), year = COALESCE(?9, year),
           gain_db = COALESCE(?10, gain_db),
           genre = COALESCE(?11, genre), genre_norm = COALESCE(?12, genre_norm),
           codec = COALESCE(?13, codec),
           sample_rate = COALESCE(?14, sample_rate), bits_per_sample = COALESCE(?15, bits_per_sample),
           bitrate = COALESCE(?16, bitrate), channels = COALESCE(?17, channels),
           mtime = COALESCE(?18, mtime),
           beets_item_id = ?19
         WHERE id = ?1",
        rusqlite::params![
            ex.id,
            non_empty(&it.title),
            non_empty(&it.title).map(norm),
            artist_id,
            album_id,
            it.length.map(len_to_ms),
            it.track,
            it.disc,
            it.year,
            it.rg_track_gain,
            non_empty(&it.genre),
            non_empty(&it.genre).map(norm),
            non_empty(&it.format),
            it.samplerate,
            it.bitdepth,
            it.bitrate,
            it.channels,
            it.mtime,
            it.id,
        ],
    )?;
    Ok(())
}

/// 文件被 beets 移走后跟随新路径（行 id 与自有数据 —— play_count / 队列引用 —— 不动）。
fn relocate_track(tx: &Connection, ex: &Existing, it: &BeetsItem) -> DbResult<()> {
    tx.execute(
        "UPDATE tracks SET path = ?2, path_lower = ?3 WHERE id = ?1",
        rusqlite::params![ex.id, it.path, it.path.to_lowercase()],
    )?;
    Ok(())
}

/// 单块条目数。语义见 `sync_items` 头注：块与块之间让出写锁。
const SYNC_CHUNK: usize = 500;

/// 自有全表快照（path 小写 → 行）。5 万行 ~16 列在几十 MB 内，
/// 比逐条回查快一个量级；WAL 下这条 SELECT 不持有写锁。
fn load_existing(conn: &Connection) -> DbResult<HashMap<String, Existing>> {
    let mut stmt = conn.prepare(
        "SELECT id, path, title, genre, artist_id, album_id, duration_ms, track_no, disc_no, year,
                gain_db, codec, sample_rate, bits_per_sample, bitrate, channels, beets_item_id
         FROM tracks",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(Existing {
            id: r.get(0)?,
            path: r.get(1)?,
            title: r.get(2)?,
            genre: r.get(3)?,
            artist_id: r.get(4)?,
            album_id: r.get(5)?,
            duration_ms: r.get(6)?,
            track_no: r.get(7)?,
            disc_no: r.get(8)?,
            year: r.get(9)?,
            gain_db: r.get(10)?,
            codec: r.get(11)?,
            sample_rate: r.get(12)?,
            bits_per_sample: r.get(13)?,
            bitrate: r.get(14)?,
            channels: r.get(15)?,
            beets_item_id: r.get(16)?,
        })
    })?;
    let mut out = HashMap::new();
    for row in rows {
        let row = row?;
        out.insert(row.path.to_lowercase(), row);
    }
    Ok(out)
}

/// 对齐入口。`items` 来自 sidecar dump_items 事件流的收集结果。
///
/// **分块提交**（每 `SYNC_CHUNK` 条一个 IMMEDIATE 事务），不是一个大事务：
/// 首对齐 5 万条时单事务的写锁会持续好几秒，期间 `record_history` 只能
/// 在 busy_timeout（5s）里等，等不到就只打一条 warn —— 用户侧表现是
/// "一边听歌一边同步 beets，播放历史悄悄少了几条"。
/// 分块后每次持锁只有几十毫秒，且对齐本身幂等：某块失败时前面已提交的块
/// 保留，下次同步继续（比整批回滚更符合"读模型逐步收敛"的定位）。
pub fn sync_items(conn: &mut Connection, items: &[BeetsItem], now: i64) -> DbResult<SyncStats> {
    let mut existing = load_existing(conn)?;

    // beets_item_id → 自有行 key。apply_move / apply_copy 会改 item.path，
    // 只按 path 对账的话，"同一首被移动过"会被当成新曲目导入一份，
    // 旧行则因为 beets item 仍在（id 相同）而永远不算 orphan —— 同一个文件
    // 在库里裂成两行，播放计数/歌单引用全留在旧的死行上。
    let mut by_beets_id: HashMap<i64, String> = existing
        .iter()
        .filter_map(|(key, ex)| ex.beets_item_id.map(|id| (id, key.clone())))
        .collect();

    let mut stats = SyncStats::default();
    let mut seen_beets_ids: HashSet<i64> = HashSet::with_capacity(items.len());

    for chunk in items.chunks(SYNC_CHUNK) {
        let tx = conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(DbError::Sql)?;
        for it in chunk {
            seen_beets_ids.insert(it.id);
            let key = it.path.to_lowercase();
            // path 命中优先；未命中再看 beets_item_id —— 命中后者说明文件被移走了
            let hit = if existing.contains_key(&key) {
                Some(key.clone())
            } else {
                by_beets_id.get(&it.id).cloned()
            };
            let Some(hit_key) = hit else {
                insert_track(&tx, it, now)?;
                stats.imported += 1;
                continue;
            };

            stats.matched += 1;
            // 先摘出来（改完再按新 key 放回，供 orphan 统计使用）
            let mut ex = existing.remove(&hit_key).expect("hit key 来自 existing");
            let mut path_changed = false;
            if hit_key != key {
                match relocate_track(&tx, &ex, it) {
                    Ok(()) => {
                        tracing::info!(
                            from = %ex.path,
                            to = %it.path,
                            beets_id = it.id,
                            "beets 侧文件已移动，自有行跟随新路径"
                        );
                        ex.path = it.path.clone();
                        path_changed = true;
                    }
                    // 目标路径已被别的行占用（例如扫描器先按新路径建了行）：
                    // 放弃跟随、继续更新字段，不让这一行影响整次同步
                    Err(e) => tracing::warn!(
                        error = %e, from = %ex.path, to = %it.path,
                        "beets 移动跟随失败（目标路径可能已被其它行占用）"
                    ),
                }
            }
            if path_changed || ex.differs_from(it) {
                update_track(&tx, &ex, it)?;
                stats.updated += 1;
            } else {
                stats.unchanged += 1;
            }
            // 放回对账映射：relocate 成功 → 行已在新路径，按新 key；
            // 失败 → 行还在旧路径，必须按旧 key 放回。此前无条件按新路径放，
            // 会把旧行挂在一个它并不在的路径 key 上，污染后续条目的对账。
            let back_key = if path_changed { key } else { hit_key };
            existing.insert(back_key.clone(), ex);
            by_beets_id.insert(it.id, back_key);
        }
        tx.commit().map_err(DbError::Sql)?;
    }

    // 自有行声称关联 beets，但该 item 已从 beets 库消失 → 只记账
    stats.orphaned = existing
        .values()
        .filter(|ex| matches!(ex.beets_item_id, Some(id) if !seen_beets_ids.contains(&id)))
        .count();

    // beets 对齐可能带来封面变化（track 的 artwork_id 随字段更新）→
    // 实体封面同步回填（与扫描收尾同一函数；失败不影响同步结果）
    if let Err(e) = crate::artwork::refresh_entity_artwork(conn) {
        tracing::warn!(error = %e, "beets 同步后实体封面回填失败（跳过）");
    }

    Ok(stats)
}

/// 自有库里全部**在线**曲目路径（按路径升序，保证一次 adopt 的遍历顺序稳定）。
///
/// 只取 `online`：`missing` 的行是扫描器判定"文件不在了"的，把它们登记进
/// beets 库只会立刻制造一批死条目（beets 也不会自己清理）。顺序固定是为了
/// 让日志/失败清单可复现——同一个库两次 adopt 的失败项顺序一致才方便比。
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

    fn item(id: i64, path: &str, title: &str) -> BeetsItem {
        BeetsItem {
            id,
            path: path.into(),
            title: Some(title.into()),
            artist: Some("测试艺人".into()),
            albumartist: Some("测试艺人".into()),
            album: Some("测试专辑".into()),
            genre: Some("测试流派".into()),
            year: Some(2026),
            track: Some(1),
            disc: Some(1),
            length: Some(123.456),
            bitrate: Some(320),
            format: Some("MP3".into()),
            samplerate: Some(44100),
            bitdepth: Some(16),
            channels: Some(2),
            mtime: Some(1700000000),
            rg_track_gain: Some(-8.5),
        }
    }

    #[test]
    fn online_paths_skips_missing_and_orders_by_path() {
        let mut conn = open_in_memory().unwrap();
        // 三条：B（在线）/a（在线）/C（缺失）
        sync_items(
            &mut conn,
            &[
                item(1, r"C:\m\B.mp3", "B"),
                item(2, r"C:\m\a.mp3", "a"),
                item(3, r"C:\m\C.mp3", "C"),
            ],
            100,
        )
        .unwrap();
        conn.execute("UPDATE tracks SET availability = 'missing' WHERE path_lower LIKE '%c.mp3'", [])
            .unwrap();

        // 只取在线行；顺序按 path_lower（小写）——不是按 id、也不是大小写敏感的 path
        assert_eq!(online_paths(&conn).unwrap(), vec![r"C:\m\a.mp3", r"C:\m\B.mp3"]);
    }

    #[test]
    fn import_then_unchanged_then_update() {
        let mut conn = open_in_memory().unwrap();
        let it = item(7, r"C:\m\a.mp3", "标题一");

        // 1) 新导入
        let s = sync_items(&mut conn, std::slice::from_ref(&it), 100).unwrap();
        assert_eq!((s.matched, s.updated, s.imported, s.unchanged), (0, 0, 1, 0));

        let (title, gain, beets_id, codec): (String, f64, i64, String) = conn
            .query_row(
                "SELECT title, gain_db, beets_item_id, codec FROM tracks WHERE beets_item_id = 7",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .unwrap();
        assert_eq!(title, "标题一");
        assert_eq!(gain, -8.5, "rg_track_gain → gain_db");
        assert_eq!(beets_id, 7);
        assert_eq!(codec, "MP3", "beets format → codec");

        // 实体也建好且挂对
        let artist: String = conn
            .query_row(
                "SELECT ar.name FROM tracks t JOIN artists ar ON ar.id = t.artist_id
                 WHERE t.beets_item_id = 7",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(artist, "测试艺人");

        // 2) 同数据再对齐 → unchanged
        let s = sync_items(&mut conn, std::slice::from_ref(&it), 101).unwrap();
        assert_eq!((s.matched, s.updated, s.unchanged), (1, 0, 1));

        // 3) 改标题/曲目号 → updated
        let mut it2 = it.clone();
        it2.title = Some("标题二".into());
        it2.track = Some(2);
        let s = sync_items(&mut conn, &[it2], 102).unwrap();
        assert_eq!((s.matched, s.updated, s.unchanged), (1, 1, 0));
        let (title, track): (String, i64) = conn
            .query_row(
                "SELECT title, track_no FROM tracks WHERE beets_item_id = 7",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!((title.as_str(), track), ("标题二", 2));

        // 4) play_count 等 beets 不管的数据不被冲掉
        conn.execute("UPDATE tracks SET play_count = 5 WHERE beets_item_id = 7", [])
            .unwrap();
        let mut it3 = it.clone();
        it3.year = Some(2027);
        sync_items(&mut conn, &[it3], 103).unwrap();
        let (play, year): (i64, i64) = conn
            .query_row(
                "SELECT play_count, year FROM tracks WHERE beets_item_id = 7",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!((play, year), (5, 2027));
    }

    #[test]
    fn path_key_is_case_insensitive() {
        let mut conn = open_in_memory().unwrap();
        // 自有库按扫描器的写法存（大写盘符）
        let it_a = item(1, r"C:\Music\a.mp3", "A");
        sync_items(&mut conn, &[it_a], 100).unwrap();
        // beets 换小写盘符再来 → 仍应命中而不是重复导入
        let it_b = item(1, r"c:\music\a.mp3", "A");
        let s = sync_items(&mut conn, &[it_b], 101).unwrap();
        assert_eq!((s.matched, s.imported), (1, 0), "大小写变体命中同一行");
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM tracks", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn orphaned_counts_missing_items() {
        let mut conn = open_in_memory().unwrap();
        sync_items(&mut conn, &[item(1, r"C:\m\a.mp3", "A"), item(2, r"C:\m\b.mp3", "B")], 100)
            .unwrap();
        // beets 里 item 2 被删了 → orphaned=1（行保留）
        let s = sync_items(&mut conn, &[item(1, r"C:\m\a.mp3", "A")], 101).unwrap();
        assert_eq!(s.orphaned, 1);
        assert_eq!(s.matched, 1);
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM tracks", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 2, "orphan 行不删，归扫描器管");
    }

    /// apply_move 之后 beets 的 item.path 变了，但 item.id 不变。
    /// 只按 path 对账会"新导入一份 + 旧行留成孤行"，这里锁住按 id 跟随。
    #[test]
    fn moved_item_follows_new_path_without_duplicating() {
        let mut conn = open_in_memory().unwrap();
        sync_items(&mut conn, &[item(9, r"C:\in\a.mp3", "A")], 100).unwrap();
        let old_id: String = conn
            .query_row("SELECT id FROM tracks WHERE beets_item_id = 9", [], |r| r.get(0))
            .unwrap();
        // 自有数据（播放计数）必须跟着行走
        conn.execute("UPDATE tracks SET play_count = 3 WHERE beets_item_id = 9", []).unwrap();

        // beets 把它搬到库目录下，id 不变
        let moved = item(9, r"C:\Music\A\a.mp3", "A");
        let s = sync_items(&mut conn, &[moved], 101).unwrap();
        assert_eq!((s.matched, s.imported, s.updated), (1, 0, 1), "移动算一次更新，不算新导入");
        assert_eq!(s.orphaned, 0, "item 仍在 beets 库里，不该算 orphan");

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM tracks", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1, "同一 beets item 不能裂成两行");

        let (id, path, play): (String, String, i64) = conn
            .query_row(
                "SELECT id, path, play_count FROM tracks WHERE beets_item_id = 9",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(id, old_id, "行 id 不变，歌单/队列引用才不会断");
        assert_eq!(path, r"C:\Music\A\a.mp3");
        assert_eq!(play, 3);

        // 再对齐一次 → unchanged（不会每次都被当成移动）
        let s = sync_items(&mut conn, &[item(9, r"C:\Music\A\a.mp3", "A")], 102).unwrap();
        assert_eq!((s.matched, s.updated, s.unchanged), (1, 0, 1));
    }

    #[test]
    fn beets_none_fields_keep_existing_values() {
        let mut conn = open_in_memory().unwrap();
        sync_items(&mut conn, &[item(1, r"C:\m\a.mp3", "A")], 100).unwrap();
        // beets 侧 year/genre 等变 None：不应把已有值清掉
        let mut partial = item(1, r"C:\m\a.mp3", "A");
        partial.year = None;
        partial.track = None;
        partial.rg_track_gain = None;
        let s = sync_items(&mut conn, &[partial], 101).unwrap();
        assert_eq!(s.unchanged, 1, "None = 保留现值，不算差异");
        let year: i64 = conn
            .query_row("SELECT year FROM tracks WHERE beets_item_id = 1", [], |r| r.get(0))
            .unwrap();
        assert_eq!(year, 2026);
    }

    /// relocate 失败（目标路径在同一次同步里已被新导入的行占用）时：
    /// 行必须留在旧路径上（字段照常更新），且**不能**污染对账映射 ——
    /// 否则旧行会被挂在一个它并不在的路径 key 上。
    #[test]
    fn failed_relocate_keeps_row_at_old_path() {
        let mut conn = open_in_memory().unwrap();
        // 预置：item 1 在旧路径 a.mp3
        sync_items(&mut conn, &[item(1, r"C:\m\a.mp3", "A")], 100).unwrap();

        // 同一次调用里：item 3 先新导入并占用 n.mp3；item 1 搬到同一路径
        // → path 未命中（对账映射还是快照）→ relocate UPDATE 撞 UNIQUE
        let items = [
            item(3, r"C:\m\n.mp3", "N"),
            item(1, r"C:\m\n.mp3", "A2"),
        ];
        let s = sync_items(&mut conn, &items, 101).unwrap();
        assert_eq!(s.imported, 1, "item 3 是新导入");
        assert_eq!(s.updated, 1, "item 1 relocate 失败 → 只更新字段");

        let (path, title): (String, String) = conn
            .query_row(
                "SELECT path, title FROM tracks WHERE beets_item_id = 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(path, r"C:\m\a.mp3", "relocate 失败，行留在旧路径");
        assert_eq!(title, "A2", "字段照常更新，不让失败影响整次同步");

        // 行 1 仍按旧路径对账命中，不重复导入
        let s2 = sync_items(&mut conn, &[item(1, r"C:\m\a.mp3", "A2")], 102).unwrap();
        assert_eq!((s2.matched, s2.imported), (1, 0), "旧行没有裂成两行");
        let count: i64 = conn.query_row("SELECT COUNT(*) FROM tracks", [], |r| r.get(0)).unwrap();
        assert_eq!(count, 2, "a.mp3 与 n.mp3 各一行");
    }
}
