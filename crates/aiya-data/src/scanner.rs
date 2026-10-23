//! 曲库扫描：walkdir 遍历 + lofty 读标签 → artists/albums/tracks upsert。
//!
//! 设计要点（docs/02 §5.4）：
//!   * 失败隔离：单个文件解析失败只记 error，不中断整批
//!   * 事务分批：每 BATCH 个文件一提交（简化为每文件一事务，批量优化见 pass 2）
//!   * 稳定 ID：track id 从规范化路径派生（ids.rs），重扫不换 ID
//!   * unchanged 判定：mtime+size 都没变就跳过解析（重扫大头收益）

use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use lofty::file::{AudioFile, TaggedFileExt};
use lofty::probe::Probe;
use lofty::tag::{ItemKey, Tag};
use rusqlite::Connection;
use serde::Serialize;

use crate::db::{DbError, DbResult};
use crate::ids;
use crate::norm::norm;

pub const AUDIO_EXTS: &[&str] = &[
    "mp3", "flac", "wav", "ogg", "oga", "opus", "m4a", "aac", "wma", "ape", "wv", "aiff", "aif", "dsf",
];

const BATCH_SIZE: usize = 200;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanProgress {
    pub phase: String, // walking | parsing | artwork | done | cancelled | failed
    pub scanned: usize,
    pub total: Option<usize>,
    pub imported: usize,
    pub skipped: usize,
    pub failed: usize,
    pub current_path: Option<String>,
    pub elapsed_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanError {
    pub path: String,
    pub message: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanResult {
    pub scanned: usize,
    pub imported: usize,
    pub updated: usize,
    pub unchanged: usize,
    pub failed: usize,
    pub cancelled: bool,
    pub errors: Vec<ScanError>,
    pub elapsed_ms: u64,
}

fn now_secs() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs() as i64
}

fn is_audio(p: &Path) -> bool {
    p.extension()
        .and_then(|e| e.to_str())
        .map(|e| AUDIO_EXTS.iter().any(|x| x.eq_ignore_ascii_case(e)))
        .unwrap_or(false)
}

/// 遍历目录收集音频文件。跳过隐藏目录与系统噪音目录。
fn walk(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for entry in walkdir::WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_map(Result::ok)
    {
        if entry.file_type().is_dir() {
            if let Some(name) = entry.file_name().to_str() {
                if name.starts_with('.') || name == "$RECYCLE.BIN" || name == "System Volume Information" {
                    continue;
                }
            }
        }
        let p = entry.path();
        if p.is_file() && is_audio(p) {
            out.push(p.to_path_buf());
        }
    }
    out
}

struct ParsedTrack {
    path: String,
    title: String,
    artist: Option<String>,
    album: Option<String>,
    album_artist: Option<String>,
    track_no: Option<u32>,
    disc_no: Option<u32>,
    year: Option<u32>,
    duration_ms: Option<i64>,
    size_bytes: u64,
    mtime: i64,
    sample_rate: Option<u32>,
    bit_depth: Option<u32>,
    bitrate: Option<u32>,
    channels: Option<u32>,
    container: String,
    /// 第一个嵌入封面（原始字节 + 标签声明的 mime）
    picture: Option<(Vec<u8>, Option<String>)>,
}

fn tag_str(tag: Option<&Tag>, key: ItemKey) -> Option<String> {
    tag.and_then(|t| t.get_string(key))
        .map(str::to_string)
        .filter(|s| !s.trim().is_empty())
}

fn tag_u32(tag: Option<&Tag>, key: ItemKey) -> Option<u32> {
    tag_str(tag, key).and_then(|s| s.split('/').next()?.trim().parse().ok())
}

fn parse_file(path: &Path) -> Result<ParsedTrack, String> {
    let meta = std::fs::metadata(path).map_err(|e| e.to_string())?;
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);

    let tagged = Probe::open(path)
        .map_err(|e| format!("probe: {e}"))?
        .read()
        .map_err(|e| format!("read: {e}"))?;

    let props = tagged.properties();
    let tag = tagged.primary_tag().or_else(|| tagged.first_tag());

    let container = format!("{:?}", tagged.file_type());
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Untitled")
        .to_string();

    let title = tag_str(tag, ItemKey::TrackTitle).unwrap_or(stem);
    let artist = tag_str(tag, ItemKey::TrackArtist);
    let album = tag_str(tag, ItemKey::AlbumTitle);
    let album_artist = tag_str(tag, ItemKey::AlbumArtist);
    let track_no = tag_u32(tag, ItemKey::TrackNumber);
    let disc_no = tag_u32(tag, ItemKey::DiscNumber);
    let year = tag_u32(tag, ItemKey::Year);

    let picture = tag.and_then(|t| t.pictures().first()).and_then(|pic| {
        let data = pic.data();
        if data.is_empty() {
            return None;
        }
        let mime = pic.mime_type().map(|m| m.as_str().to_string());
        Some((data.to_vec(), mime))
    });

    Ok(ParsedTrack {
        path: path.to_string_lossy().into_owned(),
        title,
        artist,
        album,
        album_artist,
        track_no,
        disc_no,
        year,
        duration_ms: Some(props.duration().as_millis().min(i64::MAX as u128) as i64),
        size_bytes: meta.len(),
        mtime,
        sample_rate: props.sample_rate(),
        bit_depth: props.bit_depth().map(|v| v as u32),
        bitrate: props.audio_bitrate(),
        channels: props.channels().map(|v| v as u32),
        container,
        picture,
    })
}

/// 把一条解析结果 upsert 进库。`artwork_cache` 为 None 时不提取封面。
/// 返回 true = 新插入；false = unchanged 跳过。
fn upsert(
    conn: &Connection,
    t: &ParsedTrack,
    now: i64,
    artwork_cache: Option<&Path>,
) -> DbResult<bool> {
    let existing: Option<(String, i64, i64)> = conn
        .query_row(
            "SELECT id, mtime, size_bytes FROM tracks WHERE path = ?1",
            [&t.path],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .ok();
    if let Some((_, old_mtime, old_size)) = &existing {
        if *old_mtime == t.mtime && *old_size == t.size_bytes as i64 {
            return Ok(false);
        }
    }

    let artist_name = t.artist.clone().or_else(|| t.album_artist.clone());
    let artist_id = artist_name.as_deref().map(|n| ids::artist_id(n));
    let album_title = t.album.clone();
    let album_id = album_title.as_deref().map(|al| {
        let group = t.album_artist.as_deref().or(artist_name.as_deref()).unwrap_or("");
        ids::album_id(al, group)
    });

    if let (Some(aid), Some(aname)) = (artist_id.clone(), artist_name.clone()) {
        conn.execute(
            "INSERT INTO artists (id, name, name_norm) VALUES (?1, ?2, ?3)
             ON CONFLICT(id) DO UPDATE SET name = excluded.name, name_norm = excluded.name_norm",
            rusqlite::params![aid, aname, norm(&aname)],
        )?;
    }

    if let (Some(alid), Some(alt)) = (album_id.clone(), album_title.clone()) {
        conn.execute(
            "INSERT INTO albums (id, title, title_norm, artist_id, year) VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(id) DO UPDATE SET
               title = excluded.title,
               title_norm = excluded.title_norm,
               artist_id = COALESCE(excluded.artist_id, albums.artist_id),
               year = COALESCE(excluded.year, albums.year)",
            rusqlite::params![alid, alt, norm(&alt), artist_id, t.year],
        )?;
    }

    let tid = ids::track_id(&t.path);
    let title_norm = norm(&t.title);

    // 封面：提取 → 原子落盘 → artworks upsert → 拿 id
    let artwork_id = match (&t.picture, artwork_cache) {
        (Some((data, mime)), Some(cache_dir)) => crate::artwork::save_artwork(
            conn,
            cache_dir,
            data,
            mime.as_deref(),
            now,
        )
        .map(|r| Some(r.id))
        .unwrap_or_else(|e| {
            tracing::warn!(path = %t.path, error = %e, "artwork extract failed");
            None
        }),
        _ => None,
    };

    conn.execute(
        "INSERT INTO tracks (
            id, title, title_norm, artist_id, album_id, duration_ms, track_no, disc_no, year,
            added_at, artwork_id, gain_db,
            codec, container, sample_rate, bits_per_sample, bitrate, channels,
            play_count, last_played_at,
            path, size_bytes, mtime, availability
         ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,NULL,NULL,?12,?13,?14,?15,?16,0,NULL,?17,?18,?19,'online')
         ON CONFLICT(path) DO UPDATE SET
           title = excluded.title, title_norm = excluded.title_norm,
           artist_id = excluded.artist_id, album_id = excluded.album_id,
           duration_ms = excluded.duration_ms, track_no = excluded.track_no,
           disc_no = excluded.disc_no, year = excluded.year,
           artwork_id = excluded.artwork_id,
           codec = excluded.codec, container = excluded.container,
           sample_rate = excluded.sample_rate, bits_per_sample = excluded.bits_per_sample,
           bitrate = excluded.bitrate, channels = excluded.channels,
           size_bytes = excluded.size_bytes, mtime = excluded.mtime,
           availability = 'online'",
        rusqlite::params![
            tid,
            t.title,
            title_norm,
            artist_id,
            album_id,
            t.duration_ms,
            t.track_no,
            t.disc_no,
            t.year,
            now,
            artwork_id,
            t.container,
            t.sample_rate,
            t.bit_depth,
            t.bitrate,
            t.channels,
            t.path,
            t.size_bytes as i64,
            t.mtime,
        ],
    )?;
    Ok(existing.is_none())
}

/// 扫描入口。`on_progress` 每 BATCH 个文件回调一次（UI 侧再做 120ms 节流）。
/// `artwork_cache`：封面原图缓存目录（None = 不提取封面）。
pub fn scan_directory(
    conn: &mut Connection,
    root: &Path,
    artwork_cache: Option<&Path>,
    mut on_progress: impl FnMut(ScanProgress),
) -> DbResult<ScanResult> {
    let started = Instant::now();
    let mut imported = 0usize;
    let mut unchanged = 0usize;
    let mut failed = 0usize;
    let mut errors: Vec<ScanError> = Vec::new();

    on_progress(ScanProgress {
        phase: "walking".into(),
        scanned: 0,
        total: None,
        imported: 0,
        skipped: 0,
        failed: 0,
        current_path: root.to_string_lossy().into_owned().into(),
        elapsed_ms: 0,
        message: None,
    });

    let files = walk(root);
    let total = files.len();

    on_progress(ScanProgress {
        phase: "parsing".into(),
        scanned: 0,
        total: Some(total),
        imported: 0,
        skipped: 0,
        failed: 0,
        current_path: None,
        elapsed_ms: started.elapsed().as_millis() as u64,
        message: None,
    });

    for (i, path) in files.iter().enumerate() {
        match parse_file(path).map(|parsed| {
            let tx = conn.transaction().map_err(DbError::Sql)?;
            let inserted = upsert(&tx, &parsed, now_secs(), artwork_cache)?;
            tx.commit().map_err(DbError::Sql)?;
            Ok::<bool, DbError>(inserted)
        }) {
            Ok(Ok(true)) => imported += 1,
            Ok(Ok(false)) => unchanged += 1,
            Ok(Err(e)) => {
                failed += 1;
                if errors.len() < 100 {
                    errors.push(ScanError {
                        path: path.to_string_lossy().into_owned(),
                        message: e.to_string(),
                    });
                }
            }
            Err(msg) => {
                failed += 1;
                if errors.len() < 100 {
                    errors.push(ScanError { path: path.to_string_lossy().into_owned(), message: msg });
                }
            }
        }

        if (i + 1) % BATCH_SIZE == 0 || i + 1 == total {
            on_progress(ScanProgress {
                phase: "parsing".into(),
                scanned: i + 1,
                total: Some(total),
                imported,
                skipped: unchanged,
                failed,
                current_path: Some(path.to_string_lossy().into_owned()),
                elapsed_ms: started.elapsed().as_millis() as u64,
                message: None,
            });
        }
    }

    let elapsed_ms = started.elapsed().as_millis() as u64;
    let result = ScanResult {
        scanned: total,
        imported,
        updated: 0,
        unchanged,
        failed,
        cancelled: false,
        errors: std::mem::take(&mut errors),
        elapsed_ms,
    };
    on_progress(ScanProgress {
        phase: "done".into(),
        scanned: result.scanned,
        total: Some(total),
        imported: result.imported,
        skipped: result.unchanged,
        failed: result.failed,
        current_path: None,
        elapsed_ms: result.elapsed_ms,
        message: None,
    });
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_in_memory;
    use std::io::Write;

    /// 最小合法 PNG（1x1 红色像素，IEND 齐全）
    const PNG_1X1: &[u8] = &[
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x02, 0x00, 0x00, 0x00, 0x90,
        0x77, 0x53, 0xDE, 0x00, 0x00, 0x00, 0x0C, 0x49, 0x44, 0x41, 0x54, 0x08, 0xD7, 0x63, 0xF8,
        0xCF, 0xC0, 0x00, 0x00, 0x03, 0x01, 0x01, 0x00, 0x18, 0xDD, 0x8D, 0xB0, 0x00, 0x00, 0x00,
        0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
    ];

    /// 生成最小合法 MP3：ID3v2.4 头（TIT2/TPE1 UTF-8 + 可选 APIC 封面）
    /// + 若干真实 MPEG-1 Layer3 帧（44.1kHz / 128kbps / mono）。
    fn write_test_mp3(dir: &Path, name: &str, title: &str, artist: &str) -> PathBuf {
        let path = dir.join(name);
        let mut f = std::fs::File::create(&path).unwrap();
        let title_b = title.as_bytes();
        let artist_b = artist.as_bytes();
        let mut body: Vec<u8> = Vec::new();
        for (id, content) in [("TIT2", title_b), ("TPE1", artist_b)] {
            body.extend_from_slice(id.as_bytes());
            let sz = (content.len() + 1) as u32; // +1 编码字节
            body.extend_from_slice(&sz.to_be_bytes());
            body.extend_from_slice(&[0, 0]); // flags
            body.push(0x03); // UTF-8
            body.extend_from_slice(content);
        }
        // APIC v2.4：编码(1) + mime(以\0结尾) + pic type(1) + description(\0) + data
        body.extend_from_slice(b"APIC");
        let apic_len = 1 + 10 + 1 + 1 + PNG_1X1.len(); // "image/png\0" = 10
        body.extend_from_slice(&(apic_len as u32).to_be_bytes());
        body.extend_from_slice(&[0, 0]);
        body.push(0x00); // latin-1 编码
        body.extend_from_slice(b"image/png\0");
        body.push(0x03); // front cover
        body.push(0x00); // 空 description
        body.extend_from_slice(PNG_1X1);

        let size = body.len() as u32;
        let syncsafe = [
            ((size >> 21) & 0x7f) as u8,
            ((size >> 14) & 0x7f) as u8,
            ((size >> 7) & 0x7f) as u8,
            (size & 0x7f) as u8,
        ];
        f.write_all(b"ID3\x04\x00\x00").unwrap();
        f.write_all(&syncsafe).unwrap();
        f.write_all(&body).unwrap();

        // MPEG-1 Layer3 帧：FF FB 90 C0，帧长 = 144*128000/44100 = 417 字节
        let frame_len = 417usize;
        let mut frame = vec![0u8; frame_len];
        frame[0] = 0xff;
        frame[1] = 0xfb; // MPEG1, Layer3, no CRC
        frame[2] = 0x90; // 128kbps, 44100Hz, no padding
        frame[3] = 0xc0; // mono
        for _ in 0..20 {
            f.write_all(&frame).unwrap();
        }
        path
    }

    #[test]
    fn scan_inserts_and_rescan_is_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        write_test_mp3(dir.path(), "a.mp3", "测试标题", "测试艺人");
        std::fs::write(dir.path().join("not-audio.txt"), b"nope").unwrap();

        let mut conn = open_in_memory().unwrap();
        let result = scan_directory(&mut conn, dir.path(), None, |_| {}).unwrap();
        assert_eq!(result.imported, 1, "one mp3 imported; errors={:?}", result.errors);
        assert_eq!(result.failed, 0);

        let (title, artist): (String, String) = conn
            .query_row(
                "SELECT t.title, ar.name FROM tracks t JOIN artists ar ON ar.id = t.artist_id",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(title, "测试标题");
        assert_eq!(artist, "测试艺人");

        let result2 = scan_directory(&mut conn, dir.path(), None, |_| {}).unwrap();
        assert_eq!(result2.imported, 0);
        assert_eq!(result2.unchanged, 1);
        assert_eq!(result2.failed, 0);
    }

    #[test]
    fn failure_isolation_counts_and_continues() {
        let dir = tempfile::tempdir().unwrap();
        let good = write_test_mp3(dir.path(), "good.mp3", "Good", "A");
        let bad = dir.path().join("bad.mp3");
        std::fs::write(&bad, b"definitely not mp3 content").unwrap();

        let mut conn = open_in_memory().unwrap();
        let result = scan_directory(&mut conn, dir.path(), None, |_| {}).unwrap();
        assert_eq!(result.failed, 1, "bad file counted as failed; errors={:?}", result.errors);
        assert_eq!(result.imported, 1, "good file still imported");
        assert!(result.errors.iter().any(|e| e.path == bad.to_string_lossy()));
        let _ = good;
    }

    #[test]
    fn scan_extracts_artwork_to_cache_and_links_track() {
        let dir = tempfile::tempdir().unwrap();
        let cache = tempfile::tempdir().unwrap();
        write_test_mp3(dir.path(), "a.mp3", "带封面", "艺人");

        let mut conn = open_in_memory().unwrap();
        let result = scan_directory(&mut conn, dir.path(), Some(cache.path()), |_| {}).unwrap();
        assert_eq!(result.imported, 1, "errors={:?}", result.errors);

        let (art_id, mime, cache_key): (String, String, String) = conn
            .query_row(
                "SELECT t.artwork_id, aw.mime, aw.cache_key FROM tracks t JOIN artworks aw ON aw.id = t.artwork_id",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert!(art_id.starts_with("aw_"));
        assert_eq!(mime, "image/png");
        assert!(cache_key.ends_with(".png"));

        let original = cache.path().join("originals").join(&cache_key);
        assert!(original.exists(), "original saved");
        let saved = std::fs::read(&original).unwrap();
        assert_eq!(&saved[..4], &PNG_1X1[..4], "bytes round-trip");

        // 同专辑第二首共享同一 artwork（hash 去重）
        write_test_mp3(dir.path(), "b.mp3", "同专辑", "艺人");
        let result2 = scan_directory(&mut conn, dir.path(), Some(cache.path()), |_| {}).unwrap();
        assert_eq!(result2.imported, 1);
        let count: i64 = conn.query_row("SELECT COUNT(*) FROM artworks", [], |r| r.get(0)).unwrap();
        assert_eq!(count, 1, "same picture bytes deduped");
    }
}
