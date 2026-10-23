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
//! genre 暂不迁移：tracks 无该列且 UI 无展示位，加了没有消费者（要加时连迁移一起）。

use std::collections::{HashMap, HashSet};

use rusqlite::Connection;
use serde::Deserialize;

use crate::db::DbResult;
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
            added_at, artwork_id, gain_db,
            codec, container, sample_rate, bits_per_sample, bitrate, channels,
            play_count, last_played_at,
            path, size_bytes, mtime, availability, beets_item_id
         ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,NULL,?11,
                   ?12,NULL,?13,?14,?15,?16,0,NULL,?17,0,?18,'online',?19)",
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
            non_empty(&it.format),
            it.samplerate,
            it.bitdepth,
            it.bitrate,
            it.channels,
            it.path,
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
           codec = COALESCE(?11, codec),
           sample_rate = COALESCE(?12, sample_rate), bits_per_sample = COALESCE(?13, bits_per_sample),
           bitrate = COALESCE(?14, bitrate), channels = COALESCE(?15, channels),
           mtime = COALESCE(?16, mtime),
           beets_item_id = ?17,
           availability = 'online'
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

/// 对齐入口。`items` 来自 sidecar dump_items 事件流的收集结果。
pub fn sync_items(conn: &mut Connection, items: &[BeetsItem], now: i64) -> DbResult<SyncStats> {
    let tx = conn.transaction()?;

    // 自有全表入内存（path 小写 → 行）。5 万行 ~15 列在几十 MB 内，比逐条回查快一个量级。
    let mut existing: HashMap<String, Existing> = HashMap::with_capacity(items.len() * 2);
    {
        let mut stmt = tx.prepare(
            "SELECT id, path, title, artist_id, album_id, duration_ms, track_no, disc_no, year,
                    gain_db, codec, sample_rate, bits_per_sample, bitrate, channels, beets_item_id
             FROM tracks",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(Existing {
                id: r.get(0)?,
                path: r.get(1)?,
                title: r.get(2)?,
                artist_id: r.get(3)?,
                album_id: r.get(4)?,
                duration_ms: r.get(5)?,
                track_no: r.get(6)?,
                disc_no: r.get(7)?,
                year: r.get(8)?,
                gain_db: r.get(9)?,
                codec: r.get(10)?,
                sample_rate: r.get(11)?,
                bits_per_sample: r.get(12)?,
                bitrate: r.get(13)?,
                channels: r.get(14)?,
                beets_item_id: r.get(15)?,
            })
        })?;
        for row in rows {
            let row = row?;
            existing.insert(row.path.to_lowercase(), row);
        }
    }

    let mut stats = SyncStats::default();
    let mut seen_beets_ids: HashSet<i64> = HashSet::with_capacity(items.len());

    for it in items {
        seen_beets_ids.insert(it.id);
        let key = it.path.to_lowercase();
        match existing.get(&key) {
            Some(ex) => {
                stats.matched += 1;
                if ex.differs_from(it) {
                    update_track(&tx, ex, it)?;
                    stats.updated += 1;
                } else {
                    stats.unchanged += 1;
                }
            }
            None => {
                insert_track(&tx, it, now)?;
                stats.imported += 1;
            }
        }
    }

    // 自有行声称关联 beets，但该 item 已从 beets 库消失 → 只记账
    stats.orphaned = existing
        .values()
        .filter(|ex| matches!(ex.beets_item_id, Some(id) if !seen_beets_ids.contains(&id)))
        .count();

    tx.commit()?;
    Ok(stats)
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
    fn import_then_unchanged_then_update() {
        let mut conn = open_in_memory().unwrap();
        let it = item(7, r"C:\m\a.mp3", "标题一");

        // 1) 新导入
        let s = sync_items(&mut conn, &[it.clone()], 100).unwrap();
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
        let s = sync_items(&mut conn, &[it.clone()], 101).unwrap();
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
}
