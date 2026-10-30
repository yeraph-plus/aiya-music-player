//! 封面：提取（scanner 用）+ 磁盘缓存布局。
//!
//! 磁盘布局（对齐旧仓库 ArtworkCache 的两级设计）：
//!   <cache>/originals/<hash24>.<ext>    扫描时写入的原始图
//!   <cache>/thumbs/<hash24>_<size>.jpg  协议层按需生成
//!
//! 为什么扫描时只存原图：5 万首 × 多尺寸会产生大量用不到的缩略图，
//! 首屏只需要可见的几十张（docs/07 §3 同口径）。
//! 为什么原子写：写临时文件再 rename，目标要么不存在、要么完整，
//! 否则配上一年的 immutable 缓存头会长期破图。

use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use rusqlite::Connection;
use sha2::{Digest, Sha256};

use crate::db::{DbError, DbResult};

pub const ARTWORK_SCHEME: &str = "aiya-artwork";

/// 原子写临时文件名的进程内序号。
///
/// 旧实现用 `.{hash8}.{pid}.tmp`：串行扫描时同 hash 只会有一个线程写，
/// 看着没事；docs/29 批 B 并行解析后，**同专辑多首共用一张封面**是常态，
/// 两个 worker 同时落到同一个 tmp 路径 → 后写的截断先写的、rename 拿到半个文件。
/// 加上序号后进程内唯一，跨进程用 pid 区分，仍然不需要锁。
static STAGE_SEQ: AtomicU64 = AtomicU64::new(0);

/// mime → 扩展名（只认嵌入图最常见的四种，其余兜底 .bin）
fn mime_ext(mime: Option<&str>) -> (&'static str, &'static str) {
    match mime.unwrap_or("").to_ascii_lowercase().as_str() {
        "image/jpeg" | "image/jpg" => (".jpg", "image/jpeg"),
        "image/png" => (".png", "image/png"),
        "image/webp" => (".webp", "image/webp"),
        "image/gif" => (".gif", "image/gif"),
        _ => (".bin", "application/octet-stream"),
    }
}

pub struct ArtworkRecord {
    pub id: String,
    pub cache_key: String,
    pub mime: String,
}

/// `stage` 的产物：图已经完整落在缓存目录里，只差登记一行。
///
/// 持有它是"文件已就绪、DB 未记账"的中间态。入库线程拿它调 [`link`]
/// 即可，不必再持有原始字节 —— 这是并行解析下内存峰值的关键（见 `stage` 注释）。
#[derive(Debug, Clone)]
pub struct StagedArtwork {
    pub id: String,
    pub cache_key: String,
    pub mime: String,
    /// content_hash，`link` 幂等去重要用（artworks.content_hash UNIQUE）
    hash: String,
}

/// 算 hash + 原子落盘。**不碰数据库**，可在解析线程池里并发调用。
///
/// 为什么从 `save_artwork` 里拆出来（docs/29 批 B）：SQLite 连接归入库线程
/// 独占，解析线程没有 conn；而"hash 一张几 MB 的封面 + 写盘"正是典型的
/// 可并行工作。拆开后解析侧就能就地完成落盘，交给入库线程的只是几十字节的
/// `StagedArtwork`，而不是每首几百 KB 的图字节。
pub fn stage(cache_dir: &Path, data: &[u8], mime: Option<&str>) -> DbResult<StagedArtwork> {
    if data.is_empty() {
        return Err(DbError::Custom("empty artwork".into()));
    }
    let hash = format!("{:x}", Sha256::digest(data));
    let (ext, detected_mime) = mime_ext(mime);
    let mime = if ext == ".bin" {
        mime.unwrap_or(detected_mime).to_string()
    } else {
        detected_mime.to_string()
    };
    let id = format!("aw_{}", &hash[..24.min(hash.len())]);
    let cache_key = format!("{}{}", &hash[..24.min(hash.len())], ext);

    let originals = cache_dir.join("originals");
    std::fs::create_dir_all(&originals).map_err(|e| DbError::Custom(e.to_string()))?;
    let target = originals.join(&cache_key);
    if !target.exists() {
        let tmp = originals.join(format!(
            ".{}.{}.{}.tmp",
            &hash[..8.min(hash.len())],
            std::process::id(),
            STAGE_SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::write(&tmp, data).map_err(|e| DbError::Custom(e.to_string()))?;
        // rename 失败分两种：目标已被并发写入 → 幂等成功；
        // 否则是真实 IO 故障 → 清理 tmp 并报错（不能照插 DB 行，否则永久破图）。
        if let Err(e) = std::fs::rename(&tmp, &target) {
            let _ = std::fs::remove_file(&tmp);
            if !target.exists() {
                return Err(DbError::Custom(format!("artwork rename failed: {e}")));
            }
        }
    }

    Ok(StagedArtwork {
        id,
        cache_key,
        mime,
        hash,
    })
}

/// 把 [`stage`] 的结果登记进 artworks 表（同 content_hash 幂等，不覆盖）。
pub fn link(conn: &Connection, staged: &StagedArtwork, now: i64) -> DbResult<ArtworkRecord> {
    conn.execute(
        "INSERT INTO artworks (id, content_hash, mime, width, height, cache_key, created_at)
         VALUES (?1, ?2, ?3, NULL, NULL, ?4, ?5)
         ON CONFLICT(content_hash) DO NOTHING",
        rusqlite::params![staged.id, staged.hash, staged.mime, staged.cache_key, now],
    )?;
    Ok(ArtworkRecord {
        id: staged.id.clone(),
        cache_key: staged.cache_key.clone(),
        mime: staged.mime.clone(),
    })
}

/// 一步到位的兼容入口：`stage` + `link`。串行调用方（测试、beets 导入）用它。
pub fn save_artwork(
    conn: &Connection,
    cache_dir: &Path,
    data: &[u8],
    mime: Option<&str>,
    now: i64,
) -> DbResult<ArtworkRecord> {
    let staged = stage(cache_dir, data, mime)?;
    link(conn, &staged, now)
}

/// 按 artwork id 取记录（协议层用；hash 只取 24 位，防路径穿越交给调用方 basename）。
pub fn find_artwork(conn: &Connection, id: &str) -> DbResult<Option<(String, String)>> {
    // 返回 (cache_key, mime)
    let mut stmt = conn.prepare("SELECT cache_key, mime FROM artworks WHERE id = ?1")?;
    let mut rows = stmt.query([id])?;
    if let Some(r) = rows.next()? {
        return Ok(Some((r.get(0)?, r.get(1)?)));
    }
    Ok(None)
}

/// 扫描收尾 GC：删除不再被任何实体引用的 artwork 行及其 originals/ 原图。
///
/// artworks 与 originals/ 此前只增不减——曲目删除/重扫淘汰封面后，
/// 行和文件永久残留。对账口径：tracks/albums/artists 三处 `artwork_id`
/// 引用任一命中即保留（**含 missing 行的引用**——文件离线不代表封面作废，
/// 文件回来时列表直接有图）。调用时机：扫描全部批次提交完之后（见
/// scanner::scan_roots_ext），增量扫描同样收尾跑一次。
/// 返回清理的行数；单个文件删除失败不致命（DB 行已删，孤儿文件下次 GC 再试）。
pub fn gc_unreferenced(conn: &Connection, cache_dir: &Path) -> DbResult<usize> {
    let orphans: Vec<(String, String)> = {
        let mut stmt = conn.prepare(
            "SELECT id, cache_key FROM artworks a WHERE
               NOT EXISTS (SELECT 1 FROM tracks  t WHERE t.artwork_id  = a.id) AND
               NOT EXISTS (SELECT 1 FROM albums  al WHERE al.artwork_id = a.id) AND
               NOT EXISTS (SELECT 1 FROM artists ar WHERE ar.artwork_id = a.id)",
        )?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
        rows.collect::<Result<Vec<_>, _>>()?
    };
    for (id, cache_key) in &orphans {
        conn.execute("DELETE FROM artworks WHERE id = ?1", [id])?;
        // 行删了文件删不掉（占用/权限）只是留一个孤儿文件，不回滚
        let _ = std::fs::remove_file(cache_dir.join("originals").join(cache_key));
    }
    Ok(orphans.len())
}

/// 实体封面回填：albums/artists 的 `artwork_id` 从曲目聚合——取曲目中
/// **出现次数最多**的封面（并列取 id 最小，稳定），曲目全部无封面时清 NULL。
/// 此前 schema 里这两列从不被写入（恒 NULL），专辑宫格只能显示占位图、
/// 内页走"首曲封面"回落（首曲被删则封面跳变）。调用时机：扫描收尾与
/// beets 同步收尾各一次，对存量库等价于持续回填；迁移 0006 做首次回填。
///
/// **必须在 gc_unreferenced 之前调用**：回填建立的实体引用要先于
/// "删无引用封面"的对账，否则刚该被引用的原图会被 GC 误删。
/// 返回 (albums, artists) 变更行数。
pub fn refresh_entity_artwork(conn: &Connection) -> DbResult<(usize, usize)> {
    let albums = conn
        .execute(
            "UPDATE albums SET artwork_id = (\
               SELECT t.artwork_id FROM tracks t \
               WHERE t.album_id = albums.id AND t.artwork_id IS NOT NULL \
               GROUP BY t.artwork_id ORDER BY COUNT(*) DESC, t.artwork_id ASC LIMIT 1)",
            [],
        )
        .map_err(DbError::Sql)?;
    let artists = conn
        .execute(
            "UPDATE artists SET artwork_id = (\
               SELECT t.artwork_id FROM tracks t \
               WHERE t.artist_id = artists.id AND t.artwork_id IS NOT NULL \
               GROUP BY t.artwork_id ORDER BY COUNT(*) DESC, t.artwork_id ASC LIMIT 1)",
            [],
        )
        .map_err(DbError::Sql)?;
    Ok((albums, artists))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_in_memory;

    #[test]
    fn save_dedupes_by_hash_and_writes_original() {
        let dir = tempfile::tempdir().unwrap();
        let conn = open_in_memory().unwrap();
        let png = vec![0x89u8, b'P', b'N', b'G', 1, 2, 3, 4];

        let r1 = save_artwork(&conn, dir.path(), &png, Some("image/png"), 1).unwrap();
        let r2 = save_artwork(&conn, dir.path(), &png, Some("image/png"), 2).unwrap();
        assert_eq!(r1.id, r2.id, "same bytes → same id");
        assert!(r1.id.starts_with("aw_"));

        let originals = dir.path().join("originals");
        let entries: Vec<_> = std::fs::read_dir(&originals).unwrap().collect();
        assert_eq!(entries.len(), 1, "original written exactly once");

        let (cache_key, mime) = find_artwork(&conn, &r1.id).unwrap().unwrap();
        assert_eq!(cache_key, r1.cache_key);
        assert_eq!(mime, "image/png");

        // 未知 mime 且非图片 → 兜底 octet-stream
        let r3 = save_artwork(&conn, dir.path(), b"???unknown", None, 3).unwrap();
        let (_, mime3) = find_artwork(&conn, &r3.id).unwrap().unwrap();
        assert_eq!(mime3, "application/octet-stream");
    }

    /// docs/29 批 B 回归点：**同一张封面被多个线程同时 stage**。
    ///
    /// 旧实现 tmp 名是 `.{hash8}.{pid}.tmp`，同进程内同 hash 恒定同名 ——
    /// 并行解析时多首同专辑曲目会同时写同一个临时文件，rename 到手的可能是
    /// 被另一线程截断的半张图。并行版加了序号后必须零冲突。
    #[test]
    fn concurrent_stage_same_bytes_does_not_corrupt_target() {
        let dir = tempfile::tempdir().unwrap();
        // 造一张够大的图（小文件写入太快，撞不上窗口）
        let mut png = vec![0u8; 512 * 1024];
        png[..8].copy_from_slice(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]);

        let staged: Vec<StagedArtwork> = std::thread::scope(|s| {
            let handles: Vec<_> = (0..8)
                .map(|_| {
                    let p = dir.path();
                    let bytes = &png;
                    s.spawn(move || stage(p, bytes, Some("image/png")).unwrap())
                })
                .collect();
            handles.into_iter().map(|h| h.join().unwrap()).collect()
        });

        // 8 个线程算出同一个 id（内容相同）
        assert!(staged.iter().all(|s| s.id == staged[0].id));
        let target = dir.path().join("originals").join(&staged[0].cache_key);
        let written = std::fs::read(&target).unwrap();
        assert_eq!(
            written.len(),
            png.len(),
            "目标文件必须是完整的一整份，不能是被截断的半张"
        );
        assert_eq!(written, png);

        // tmp 文件不许残留（每个线程的 tmp 都被自己 rename 掉了）
        let leftovers: Vec<_> = std::fs::read_dir(dir.path().join("originals"))
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
            .collect();
        assert!(leftovers.is_empty(), "残留 tmp: {leftovers:?}");
    }

    /// 实体封面回填：多数票胜出（并列取 id 小者稳定）、曲目全部无封面时清 NULL
    /// （ albums/artists.artwork_id 此前从不写入，2026-10-30 起由此函数维护）。
    #[test]
    fn refresh_entity_artwork_picks_majority_and_clears_stale() {
        let conn = open_in_memory().unwrap();
        // a1: 三首曲目两票 art-a 一票 art-b → art-a 胜；ar1 同曲 → art-a
        conn.execute_batch(
            "INSERT INTO artworks (id, content_hash, mime, cache_key, created_at) VALUES
               ('aw_a','ha','image/png','a.png',0), ('aw_b','hb','image/png','b.png',0);
             INSERT INTO albums (id, title, title_norm) VALUES
               ('al1','专辑','专辑');
             INSERT INTO artists (id, name, name_norm) VALUES
               ('ar1','艺人','艺人');
             INSERT INTO tracks (id, title, title_norm, added_at, path, path_lower,
                                 album_id, artist_id, artwork_id) VALUES
               ('t1','一','一',0,'/1.flac','/1.flac','al1','ar1','aw_a'),
               ('t2','二','二',0,'/2.flac','/2.flac','al1','ar1','aw_a'),
               ('t3','三','三',0,'/3.flac','/3.flac','al1','ar1','aw_b');
             INSERT INTO albums (id, title, title_norm) VALUES
               ('al2','无封面专辑','无封面专辑');",
        )
        .unwrap();

        let (albums, artists) = refresh_entity_artwork(&conn).unwrap();
        assert_eq!(albums, 2, "两张专辑都参与对账");
        assert_eq!(artists, 1);

        let got: String = conn
            .query_row("SELECT artwork_id FROM albums WHERE id = 'al1'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(got, "aw_a", "多数票胜出（2 票 art-a vs 1 票 art-b）");
        let got: String = conn
            .query_row("SELECT artwork_id FROM artists WHERE id = 'ar1'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(got, "aw_a");
        let nulls: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM albums WHERE artwork_id IS NULL",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(nulls, 1, "无封面曲目支撑的专辑保持 NULL");

        // 曲目全部失去封面 → 实体引用清除（GC 口径一致，不留悬空引用）
        conn.execute("UPDATE tracks SET artwork_id = NULL", [])
            .unwrap();
        refresh_entity_artwork(&conn).unwrap();
        let stale: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM albums WHERE artwork_id IS NOT NULL",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(stale, 0, "曲目无封面后实体引用必须清除");
    }
}
