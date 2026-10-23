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

use rusqlite::Connection;
use sha2::{Digest, Sha256};

use crate::db::{DbError, DbResult};

pub const ARTWORK_SCHEME: &str = "aiya-artwork";

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

/// 把一段嵌入图写进缓存并 upsert artworks 表。返回 artwork id（恒 Some）。
pub fn save_artwork(
    conn: &Connection,
    cache_dir: &Path,
    data: &[u8],
    mime: Option<&str>,
    now: i64,
) -> DbResult<ArtworkRecord> {
    if data.is_empty() {
        return Err(DbError::Custom("empty artwork".into()));
    }
    let hash = format!("{:x}", Sha256::digest(data));
    let (ext, detected_mime) = mime_ext(mime);
    let mime = if ext == ".bin" { mime.unwrap_or(detected_mime).to_string() } else { detected_mime.to_string() };
    let id = format!("aw_{}", &hash[..24.min(hash.len())]);
    let cache_key = format!("{}{}", &hash[..24.min(hash.len())], ext);

    let originals = cache_dir.join("originals");
    std::fs::create_dir_all(&originals).map_err(|e| DbError::Custom(e.to_string()))?;
    let target = originals.join(&cache_key);
    if !target.exists() {
        let tmp = originals.join(format!(
            ".{}.{}.tmp",
            &hash[..8.min(hash.len())],
            std::process::id()
        ));
        std::fs::write(&tmp, data).map_err(|e| DbError::Custom(e.to_string()))?;
        // rename 失败分两种：目标已被并发扫描写入 → 幂等成功；
        // 否则是真实 IO 故障 → 清理 tmp 并报错（不能照插 DB 行，否则永久破图）。
        if let Err(e) = std::fs::rename(&tmp, &target) {
            let _ = std::fs::remove_file(&tmp);
            if target.exists() {
                // 并发扫描已写入 → 当作已写入
            } else {
                return Err(DbError::Custom(format!("artwork rename failed: {e}")));
            }
        }
    }

    conn.execute(
        "INSERT INTO artworks (id, content_hash, mime, width, height, cache_key, created_at)
         VALUES (?1, ?2, ?3, NULL, NULL, ?4, ?5)
         ON CONFLICT(content_hash) DO NOTHING",
        rusqlite::params![id, hash, mime, cache_key, now],
    )?;

    Ok(ArtworkRecord { id, cache_key, mime })
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
}
