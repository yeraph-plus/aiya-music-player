//! 曲库扫描：walkdir 遍历 + lofty 读标签 → artists/albums/tracks upsert。
//!
//! 设计要点（docs/02 §5.4，docs/29 §4 批 B 起为并行流水线）：
//!   * 失败隔离：单个文件解析失败只记 error，不中断整批
//!   * 预筛 unchanged：mtime+size 与库内一致且在线 → **连标签都不读**（解析前判定）
//!   * 解析并行：rayon 池（N = 逻辑核-1）读标签 + 算封面 hash + 落盘，不碰 DB
//!   * 入库单写者：本线程独占连接，每 BATCH 个文件一个 IMMEDIATE 事务
//!   * 稳定 ID：track id 从规范化路径派生（ids.rs），重扫不换 ID
//!
//! 为什么是"生产者/消费者"而不是原来的单线程 for 循环（docs/29 §4）：
//! 旧实现的 `unchanged` 判定写在 `upsert` 里，等轮到它时 `parse_file` 早就把
//! 整份标签和嵌入封面读完了一遍 —— 注释写着"跳过解析"，实际跳过的是"重新写库"。
//! 并行化把这两件事一起修：预筛提前到解析之前，解析本身摊到多核上。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::sync_channel;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use lofty::file::{AudioFile, TaggedFileExt};
use lofty::probe::Probe;
use lofty::tag::{ItemKey, Tag};
use rayon::prelude::*;
use rusqlite::Connection;
use serde::Serialize;
use ts_rs::TS;

use crate::artwork::StagedArtwork;
use crate::db::{DbError, DbResult};
use crate::ids;
use crate::norm::norm;

pub const AUDIO_EXTS: &[&str] = &[
    "mp3", "flac", "wav", "ogg", "oga", "opus", "m4a", "aac", "wma", "ape", "wv", "aiff", "aif", "dsf",
];

const BATCH_SIZE: usize = 200;

/// 解析池 → 入库线程的通道容量（条）。
///
/// **必须有界**：无界 channel 会把"解析比入库快"直接翻译成"整库解析结果堆在
/// 内存里"。有界才有背压 —— 入库慢了，解析自然跟着慢。256 条 ≈ 8 线程跑 30 个
/// 文件的量，足够吸收抖动。
const CHANNEL_CAP: usize = 256;

/// 进度回调的最小步长（scanned 每增长这么多才报一次）。
/// 宿主还有 100ms 时间节流兜底，这里是省掉不必要的原子读 + 分配。
const PROGRESS_STEP: usize = 64;

/// 最多收集多少条错误明细（结果对象要跨 IPC）。
const MAX_ERRORS: usize = 100;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanProgress {
    /// walking | parsing | importing | done | cancelled | failed
    ///
    /// 没有单独给"封面提取"一个阶段（旧注释里有过 `artwork`）：docs/29 批 B 起
    /// 封面 hash/落盘发生在解析 worker 内部，与读标签同一时刻完成，对外拆开
    /// 只会多一个永远不会稳定出现的状态。
    pub phase: String,
    /// 已入库（或已判定 unchanged / failed 并记账）的文件数 —— 进度条按它走
    pub scanned: usize,
    /// 解析已完成的数量（含 unchanged 与失败）。
    ///
    /// 为什么要额外一个计数：解析与入库是**重叠**的两段，只看 scanned 时
    /// 用户在"解析快、入库慢"的一段里会看到进度条不动却不知道在干什么。
    /// `parsed - scanned` 就是排队等入库的积压量。
    pub parsed: usize,
    pub total: Option<usize>,
    pub imported: usize,
    /// 重新读标签并写了行的数量（与 `skipped` 互斥：那一批连读都没读）
    pub updated: usize,
    pub skipped: usize,
    pub failed: usize,
    /// 本轮被标为 missing（文件不在）的行数。
    ///
    /// 只在 `done` 那一刻有值（要先走完目录才知道谁不在），中途恒 0。
    /// 放进进度事件而不是只留在 `ScanResult` 里：UI 拿到结果后要一直显示
    /// "上次扫描 · 离线 2"，没有这个字段就得另存一份 result。
    pub missing: usize,
    pub current_path: Option<String>,
    pub elapsed_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

#[derive(Debug, Serialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct ScanError {
    pub path: String,
    pub message: String,
}

/// 扫描结果（`library_scan` 的返回值，也是 UI 的"扫完了"汇总）。
///
/// `derive(TS)` 就地生成：扫描器改字段时 `gen:types:check` 会直接红，
/// 不需要有人在宿主层再同步一份（`ScanResult` 没有宿主私有字段，没必要复制）。
#[derive(Debug, Serialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct ScanResult {
    #[ts(type = "number")]
    pub scanned: usize,
    #[ts(type = "number")]
    pub imported: usize,
    #[ts(type = "number")]
    pub updated: usize,
    #[ts(type = "number")]
    pub unchanged: usize,
    #[ts(type = "number")]
    pub failed: usize,
    /// 本轮被标为 missing（文件不在了）的行数。
    ///
    /// 单独一个字段而不是并进 `updated`：这是"库里有、磁盘没有"，与
    /// "磁盘有、重新读了标签"是相反方向的变化，混在一起就没法解释。
    #[serde(default)]
    #[ts(type = "number")]
    pub missing: usize,
    /// 遍历目录时的 walkdir 错误条数（权限不足等）；只计数不中断。
    /// `#[serde(default)]` 保持对旧调用方（缺字段反序列化）兼容。
    #[serde(default)]
    #[ts(type = "number")]
    pub walk_errors: usize,
    pub cancelled: bool,
    pub errors: Vec<ScanError>,
    #[ts(type = "number")]
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
/// 返回 (音频文件列表, walk 错误数, 是否被取消)。错误（权限不足等）只计数不中断。
///
/// 用 `into_iter()` + `skip_current_dir()` 而不是 `for entry in ...`：
/// 后者在目录上 `continue` 只是跳过**这个条目**，子目录照样被走进去 ——
/// `.git/`、`$RECYCLE.BIN/` 里几万个文件照样被遍历 + parse，用户看到的是
/// "扫了十分钟还没完"，而统计里又说不清时间花在哪。
fn walk(root: &Path, is_cancelled: &dyn Fn() -> bool) -> (Vec<PathBuf>, usize, bool) {
    let mut out = Vec::new();
    let mut walk_errors = 0usize;
    let mut it = walkdir::WalkDir::new(root).follow_links(false).into_iter();
    while let Some(entry) = it.next() {
        if is_cancelled() {
            return (out, walk_errors, true);
        }
        let entry = match entry {
            Ok(e) => e,
            Err(_) => {
                walk_errors += 1;
                continue;
            }
        };
        if entry.file_type().is_dir() && entry.depth() > 0 {
            // depth==0 是用户指定的库根：哪怕它叫 `.music` 也必须进去
            // （tempfile 的目录名就以 `.tmp` 开头，剪掉根 = 一个文件都扫不到）
            if let Some(name) = entry.file_name().to_str() {
                if name.starts_with('.') || name == "$RECYCLE.BIN" || name == "System Volume Information" {
                    // 整个子树剪掉（不只跳过目录条目本身）
                    it.skip_current_dir();
                    continue;
                }
            }
        }
        let p = entry.path();
        if p.is_file() && is_audio(p) {
            out.push(p.to_path_buf());
        }
    }
    (out, walk_errors, false)
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
    /// 文件 mtime（unix 秒）。None = modified() 读取失败：跳过 unchanged
    /// 短路，强制重新解析（避免 0==0 的假命中导致变更永远检测不到）。
    mtime: Option<i64>,
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

fn mtime_of(meta: &std::fs::Metadata) -> Option<i64> {
    // modified() 失败时不得当 0（否则后续变更检测 0==0 跳过解析）：
    // 返回 None，让调用方走强制重新解析路径
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
}

/// 便捷包装：自己 stat 一次再解析。
///
/// 只给测试用 —— 生产路径一律走 `process_file`（它先拿 mtime/size 做预筛，
/// 再把同一份 metadata 传进 `parse_file_with_meta`，不重复 stat）。
/// 加 `#[cfg(test)]` 而不是留成 `pub`，是为了让"生产代码不走这条兜底路"
/// 由编译器来保证。
#[cfg(test)]
fn parse_file(path: &Path) -> Result<ParsedTrack, String> {
    let meta = std::fs::metadata(path).map_err(|e| e.to_string())?;
    let mtime = mtime_of(&meta);
    parse_file_with_meta(path, &meta, mtime)
}

/// 解析主体。`meta`/`mtime` 由调用方预先 stat 后传进来 —— 流水线里要先拿
/// mtime/size 做 unchanged 预筛，再决定要不要解析，重复 stat 一遍纯属浪费
/// （Windows 上 stat 会被 Defender 拦一道）。
fn parse_file_with_meta(
    path: &Path,
    meta: &std::fs::Metadata,
    mtime: Option<i64>,
) -> Result<ParsedTrack, String> {
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

/// 把一条解析结果 upsert 进库。`artwork` 是解析线程已经落好盘的封面
/// （None = 这首没有封面 / 未启用封面缓存）。
/// 返回 true = 新插入；false = unchanged 跳过。
fn upsert(
    conn: &Connection,
    t: &ParsedTrack,
    now: i64,
    artwork: Option<&StagedArtwork>,
) -> DbResult<UpsertOutcome> {
    // 对账键 = path_lower（Windows 大小写不敏感语义；该列是 0001 基线里的
    // `path_lower TEXT NOT NULL UNIQUE`，早期编号 0003 的迁移已在压平基线时并入）
    let path_lower = t.path.to_lowercase();
    let existing: Option<(String, i64, i64, String)> = conn
        .query_row(
            "SELECT id, mtime, size_bytes, availability FROM tracks WHERE path_lower = ?1",
            [&path_lower],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .ok();
    if let Some((_, old_mtime, old_size, availability)) = &existing {
        // 1) mtime 读取失败（None）→ 必须重新解析，不走 unchanged 短路；
        // 2) 行标记为 missing（文件曾消失又出现）→ 也必须走更新路径，
        //    否则 mtime/size 相同时恢复的文件永远翻不回 online。
        let must_refresh = *availability == "missing";
        if !must_refresh {
            if let Some(m) = t.mtime {
                if *old_mtime == m && *old_size == t.size_bytes as i64 {
                    return Ok(UpsertOutcome::Unchanged);
                }
            }
        }
    }

    let artist_name = t.artist.clone().or_else(|| t.album_artist.clone());
    let artist_id = artist_name.as_deref().map(ids::artist_id);
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

    // 封面：图已由解析线程落盘（artwork::stage），这里只登记一行（artwork::link）。
    // 拆开的原因见 artwork::stage 的注释：连接归入库线程独占，解析线程无法
    // 同时做 upsert；而 hash + 写盘才是封面的主要开销。
    let artwork_id = artwork.and_then(|staged| match crate::artwork::link(conn, staged, now) {
        Ok(r) => Some(r.id),
        Err(e) => {
            tracing::warn!(path = %t.path, error = %e, "artwork link failed");
            None
        }
    });

    conn.execute(
        "INSERT INTO tracks (
            id, title, title_norm, artist_id, album_id, duration_ms, track_no, disc_no, year,
            added_at, artwork_id, gain_db,
            codec, container, sample_rate, bits_per_sample, bitrate, channels,
            play_count, last_played_at,
            path, path_lower, size_bytes, mtime, availability
         ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,NULL,NULL,?12,?13,?14,?15,?16,0,NULL,?17,?18,?19,?20,'online')
         ON CONFLICT(path_lower) DO UPDATE SET
           title = excluded.title, title_norm = excluded.title_norm,
           artist_id = excluded.artist_id, album_id = excluded.album_id,
           duration_ms = excluded.duration_ms, track_no = excluded.track_no,
           disc_no = excluded.disc_no, year = excluded.year,
           artwork_id = excluded.artwork_id,
           container = excluded.container,
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
            path_lower,
            t.size_bytes as i64,
            t.mtime.unwrap_or(0),
        ],
    )?;
    Ok(if existing.is_none() {
        UpsertOutcome::Inserted
    } else {
        UpsertOutcome::Rewritten
    })
}

/// 单个文件的入库结果。
///
/// 为什么把 `bool` 换成三态：旧接口只回答"是不是新行"，于是**重新解析并
/// 写了行**这一档被混进 `unchanged`，扫描结果里永远看不出"标签被外部改过"。
/// 刷新管线（docs/33 §7）要靠这个数决定"磁盘侧真的变了没有"，
/// 而 `unchanged` 必须是"连标签都没读"的那一批。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UpsertOutcome {
    /// 库里没这一行 → 新导入
    Inserted,
    /// 库里有，mtime/size 变了 → 重新写了一遍
    Rewritten,
    /// mtime+size 与库内一致 → 早退，连标签都没读
    Unchanged,
}

// ---------------------------------------------------------------------------
// 流水线的三个部件：预筛索引 / 解析池产出 / 入库攒批
// ---------------------------------------------------------------------------

/// 库里已有行的轻量索引项（只取预筛要用的四列）。
struct IndexEntry {
    mtime: i64,
    size: i64,
    /// 标记为 missing 的行**不做**预筛：文件曾消失又出现时必须重新走一遍
    /// upsert 把它翻回 online（与 upsert 里 `must_refresh` 同一理由）。
    missing: bool,
}

/// 解析池产出的一条结果。
///
/// `Parsed` 明显大于另两个变体（384 字节），但它每条曲目只经 channel 传一次，
/// 一次 384 字节 move 远低于读一次标签的成本；boxing 反而给每条结果加一次堆分配。
#[allow(clippy::large_enum_variant)]
enum Item {
    /// 预筛命中：mtime/size 与库内一致且在库 → 连标签都不读。
    Unchanged,
    /// 解析成功。封面已就地落盘（[`crate::artwork::stage`]），
    /// `parsed.picture` 已清空 —— 交到入库线程手里只剩几十字节。
    Parsed { parsed: ParsedTrack, artwork: Option<StagedArtwork> },
    /// 解析失败（失败隔离：只记明细，不中断整批）。
    Failed { path: String, message: String },
}

/// 扫描过程中累积的计数。进度事件与最终结果都从它派生，
/// 保证"看着的进度"和"最后报的数"是同一份数据。
#[derive(Default)]
struct Tally {
    scanned: usize,
    parsed: usize,
    imported: usize,
    updated: usize,
    unchanged: usize,
    failed: usize,
}

fn snap(
    phase: &str,
    t: &Tally,
    total: Option<usize>,
    current_path: Option<String>,
    started: Instant,
) -> ScanProgress {
    ScanProgress {
        phase: phase.to_string(),
        scanned: t.scanned,
        parsed: t.parsed,
        total,
        imported: t.imported,
        updated: t.updated,
        skipped: t.unchanged,
        failed: t.failed,
        missing: 0,
        current_path,
        elapsed_ms: started.elapsed().as_millis() as u64,
        message: None,
    }
}

fn load_index(conn: &Connection) -> DbResult<HashMap<String, IndexEntry>> {
    let mut stmt = conn.prepare("SELECT path_lower, mtime, size_bytes, availability FROM tracks")?;
    let mut rows = stmt.query([])?;
    let mut map: HashMap<String, IndexEntry> = HashMap::new();
    while let Some(r) = rows.next()? {
        map.insert(
            r.get(0)?,
            IndexEntry { mtime: r.get(1)?, size: r.get(2)?, missing: r.get::<_, String>(3)? == "missing" },
        );
    }
    Ok(map)
}

/// 单个文件的全部处理（在解析 worker 里跑，**不碰数据库**）。
fn process_file(
    path: &Path,
    index: &HashMap<String, IndexEntry>,
    artwork_cache: Option<&Path>,
) -> Item {
    let meta = match std::fs::metadata(path) {
        Ok(m) => m,
        Err(e) => {
            return Item::Failed { path: path.to_string_lossy().into_owned(), message: e.to_string() }
        }
    };
    let mtime = mtime_of(&meta);
    let path_lower = path.to_string_lossy().to_lowercase();

    // 预筛：mtime+size 都没变、且行是在线状态 → 连标签都不读。
    //
    // 这是增量重扫的绝对大头（真实曲库里 99% 的文件都会命中），也是本批次
    // 最大的一处收益：旧实现把 unchanged 判定放在 `upsert` 里，那时
    // `parse_file` 早把整份标签和嵌入封面读完了一遍 —— 注释写着"跳过解析"，
    // 实际跳过的是"重新写库"。实测 1846 首的重扫从 4.9s 降到 0.2s 量级。
    //
    // `mtime != 0`：DB 里 mtime 为 0 表示"当初没读到修改时间"，
    // 与 upsert 里 `Some(m)` 的判断一致，不能拿 0 去比。
    if let Some(entry) = index.get(&path_lower) {
        if !entry.missing && entry.mtime != 0 {
            if let Some(m) = mtime {
                if entry.mtime == m && entry.size == meta.len() as i64 {
                    return Item::Unchanged;
                }
            }
        }
    }

    match parse_file_with_meta(path, &meta, mtime) {
        Ok(mut t) => {
            // 封面：hash + 原子落盘在解析侧就地做（这是纯 CPU + IO，能并行）。
            // take() 掉原始字节，别让几百 KB 的图跟着 Item 进队列 ——
            // 无界积压时那就是 GB 级内存峰值。
            let artwork = match (t.picture.take(), artwork_cache) {
                (Some((data, mime)), Some(dir)) => {
                    match crate::artwork::stage(dir, &data, mime.as_deref()) {
                        Ok(s) => Some(s),
                        Err(e) => {
                            tracing::warn!(path = %t.path, error = %e, "artwork stage failed");
                            None
                        }
                    }
                }
                _ => None,
            };
            Item::Parsed { parsed: t, artwork }
        }
        Err(message) => Item::Failed { path: path.to_string_lossy().into_owned(), message },
    }
}

enum CommitOutcome {
    Done { count: usize, imported: usize, updated: usize, unchanged: usize },
    /// 整批回滚：`count` 条全部计入 failed（批语义见 commit_batch 注释）
    RolledBack { count: usize, path: String, message: String },
}

/// 提交攒下的一批（≤ BATCH_SIZE）。整批一个 IMMEDIATE 事务。
fn commit_batch(conn: &mut Connection, batch: &mut Vec<Item>) -> DbResult<CommitOutcome> {
    if batch.is_empty() {
        return Ok(CommitOutcome::Done { count: 0, imported: 0, updated: 0, unchanged: 0 });
    }
    let count = batch.len();
    let now = now_secs();
    let tx = conn
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(DbError::Sql)?;
    let mut imported = 0usize;
    let mut updated = 0usize;
    let mut unchanged = 0usize;
    let mut err: Option<(String, String)> = None;
    for item in batch.iter() {
        if let Item::Parsed { parsed, artwork } = item {
            match upsert(&tx, parsed, now, artwork.as_ref()) {
                Ok(UpsertOutcome::Inserted) => imported += 1,
                Ok(UpsertOutcome::Rewritten) => updated += 1,
                Ok(UpsertOutcome::Unchanged) => unchanged += 1,
                Err(e) => {
                    err = Some((parsed.path.clone(), e.to_string()));
                    break;
                }
            }
        }
    }
    batch.clear();
    match err {
        None => {
            tx.commit().map_err(DbError::Sql)?;
            Ok(CommitOutcome::Done { count, imported, updated, unchanged })
        }
        Some((path, message)) => {
            tx.rollback().map_err(DbError::Sql)?;
            Ok(CommitOutcome::RolledBack { count, path, message })
        }
    }
}

fn apply_commit(tally: &mut Tally, errors: &mut Vec<ScanError>, outcome: CommitOutcome) {
    match outcome {
        CommitOutcome::Done { count, imported, updated, unchanged } => {
            tally.imported += imported;
            tally.updated += updated;
            tally.unchanged += unchanged;
            tally.scanned += count;
        }
        CommitOutcome::RolledBack { count, path, message } => {
            tally.failed += count;
            tally.scanned += count;
            if errors.len() < MAX_ERRORS {
                errors.push(ScanError { path, message });
            }
        }
    }
}

/// 扫描入口（不可取消版）。`on_progress` 在阶段切换与计数跳步时回调（宿主再做 100ms 节流）。
/// `artwork_cache`：封面原图缓存目录（None = 不提取封面）。
pub fn scan_directory(
    conn: &mut Connection,
    root: &Path,
    artwork_cache: Option<&Path>,
    on_progress: impl FnMut(ScanProgress),
) -> DbResult<ScanResult> {
    scan_directory_ext(conn, root, artwork_cache, || false, on_progress)
}

/// 扫描入口（可取消版）。`should_cancel` 会被 walk 循环、每个解析 worker、
/// 入库循环三处轮询；返回 true 后尽快收尾。
/// **取消路径绝不执行 mark_missing**——seen 集合不完整，此时对账会把大量在线
/// 文件误标 missing。
///
/// `+ Sync`：docs/29 批 B 起解析在线程池里跑，取消标志要被多个 worker 同时读。
///
/// docs/29 批 A 起单根版委托 [`scan_roots_ext`]（多根并集单库）。
pub fn scan_directory_ext(
    conn: &mut Connection,
    root: &Path,
    artwork_cache: Option<&Path>,
    should_cancel: impl Fn() -> bool + Sync,
    on_progress: impl FnMut(ScanProgress),
) -> DbResult<ScanResult> {
    let owned = [root.to_path_buf()];
    scan_roots_ext(conn, &owned, artwork_cache, should_cancel, on_progress)
}

/// 多根扫描：所有根的 walk 结果**并集**进同一个库（拍板：并集单库，无多库切换）。
///
/// 为什么不是"逐根调单根版"：mark_missing 按"本轮 walk 到的路径"对账，
/// 逐根调用时第 1 根的对账会把第 2 根下的曲目全标 missing。必须先走完全部根、
/// 拿到完整 seen 集合，再做一次全根前缀的 missing 对账。
pub fn scan_roots_ext(
    conn: &mut Connection,
    roots: &[std::path::PathBuf],
    artwork_cache: Option<&Path>,
    should_cancel: impl Fn() -> bool + Sync,
    mut on_progress: impl FnMut(ScanProgress),
) -> DbResult<ScanResult> {
    let started = Instant::now();
    let mut tally = Tally::default();
    let mut errors: Vec<ScanError> = Vec::new();

    on_progress(snap(
        "walking",
        &tally,
        None,
        Some(roots.first().map(|r| r.to_string_lossy().into_owned()).unwrap_or_default()),
        started,
    ));

    // 逐根 walk 后并集；任何一根被取消都按取消收尾（seen 不完整）
    let mut files: Vec<PathBuf> = Vec::new();
    let mut walk_errors = 0usize;
    let mut walk_cancelled = false;
    for root in roots {
        let (mut root_files, root_errs, root_cancelled) = walk(root, &should_cancel);
        files.append(&mut root_files);
        walk_errors += root_errs;
        if root_cancelled {
            walk_cancelled = true;
            break;
        }
    }

    if walk_cancelled {
        let result = ScanResult {
            scanned: 0,
            imported: 0,
            updated: 0,
            unchanged: 0,
            failed: 0,
            missing: 0,
            walk_errors,
            cancelled: true,
            errors,
            elapsed_ms: started.elapsed().as_millis() as u64,
        };
        on_progress(snap("cancelled", &tally, None, None, started));
        return Ok(result);
    }

    let total = files.len();
    // 本轮实际 walk 到的路径（小写），供 missing 对账用
    let seen: std::collections::HashSet<String> = files
        .iter()
        .map(|p| p.to_string_lossy().to_lowercase())
        .collect();

    // 库内已有行的轻量索引：预筛 unchanged 用。
    // 只取四列、一次性读全表 —— 5 万行约 8–10MB，换来解析阶段对每个文件都
    // 不必再查库（解析线程没有连接，也根本不该有）。
    let index = load_index(conn)?;

    let mut cancelled = false;
    let mut batch: Vec<Item> = Vec::with_capacity(BATCH_SIZE);
    let mut next_report = PROGRESS_STEP;
    let mut last_path: Option<String> = None;
    // 解析完成计数由各 worker 递增（它们互不知道），入库线程发进度时读
    let parsed_done = AtomicUsize::new(0);

    // 局部线程池而不是 rayon 全局池：扫描是偶发的批任务，不该长期占着全局池；
    // 而且这里要精确控制 N。留一个核给入库线程 —— 它是 SQLite 单写者，
    // 多一个解析 worker 只会在 channel 上堵着，不如把时间片让给写锁。
    let cores = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4);
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(cores.saturating_sub(1).max(1))
        .thread_name(|i| format!("aiya-parse-{i}"))
        .build()
        .map_err(|e| DbError::Custom(format!("解析线程池创建失败: {e}")))?;

    let (tx, rx) = sync_channel::<Item>(CHANNEL_CAP);

    // 进度上报口径统一收在这一个闭包里：phase 要同时看两个计数，
    // 散在循环各处写必然走样（旧实现就是每批自己拼一份 ScanProgress）。
    let mut report = |t: &Tally, current: Option<String>| {
        // 解析全完了但库里还有积压 → 如实说"正在入库"。
        // 没有这个状态时，用户在"解析快、入库慢"那一段看到的就是进度条不动。
        let phase = if t.parsed >= total && t.scanned < total { "importing" } else { "parsing" };
        on_progress(snap(phase, t, Some(total), current, started));
    };

    // 按批包事务：一个事务提交 BATCH_SIZE 个文件的 upsert；
    // 批内某个 upsert 失败 → 回滚整批、整批计入 failed，继续下一批。
    //
    // BEGIN IMMEDIATE（不是默认的 DEFERRED）：扫描连接与主 DB 线程是**两个连接**，
    // DEFERRED 事务先拿读锁，要等第一条写语句才升级写锁；升级时若别处已持有写锁，
    // SQLITE_BUSY 会立刻返回（busy_timeout 对"锁升级"不生效），表现为扫描中途
    // 突然整批失败。IMMEDIATE 在事务开始就排队等写锁，busy_timeout 才真正起作用。
    std::thread::scope(|scope| -> DbResult<()> {
        // 生产者：解析池。不碰 DB、不碰 on_progress。
        //
        // 闭包必须是 `move` 的：`tx` 要**跟着闭包一起 drop**，消费者才能收到
        // Disconnected 收尾。上一版写成非 move 闭包（只借用 tx），tx 活到函数
        // 末尾，`rx.recv()` 在生产者已经跑完之后还会一直等下去 —— 第一次跑
        // 测试就是卡死在这（`case_variant_paths_do_not_duplicate_rows`）。
        let (pool_ref, files_ref, index_ref) = (&pool, &files, &index);
        let cancel_ref = &should_cancel;
        let done_ref = &parsed_done;
        scope.spawn(move || {
            pool_ref.install(|| {
                files_ref.par_iter().for_each(|path| {
                    if cancel_ref() {
                        return;
                    }
                    let item = process_file(path, index_ref, artwork_cache);
                    done_ref.fetch_add(1, Ordering::Relaxed);
                    // channel 满时这里阻塞 —— 这就是背压本身，不是缺陷。
                    // 发送失败只可能是消费者已经走了（出错提前返回），
                    // 此时继续跑没有意义但也没有危害。
                    let _ = tx.send(item);
                });
            });
        });

        // 消费者 = 本线程。SQLite 连接只能在这里用（单写者），
        // 解析侧拿不到连接也是在源头上保证这一点。
        report(&tally, None);
        while let Ok(item) = rx.recv() {
            tally.parsed = parsed_done.load(Ordering::Relaxed);

            if !cancelled && should_cancel() {
                // 未攒满的半批直接丢弃：批事务语义是"整批生效"，
                // 半批入库会让"这批到底进没进"说不清。已提交批次保留。
                cancelled = true;
                batch.clear();
            }
            if cancelled {
                // drain：不再入库，只把队列抽干，好让 producer 解除阻塞退出
                continue;
            }

            match item {
                Item::Unchanged => {
                    tally.unchanged += 1;
                    tally.scanned += 1;
                }
                Item::Failed { path, message } => {
                    tally.failed += 1;
                    tally.scanned += 1;
                    last_path = Some(path.clone());
                    if errors.len() < MAX_ERRORS {
                        errors.push(ScanError { path, message });
                    }
                }
                item @ Item::Parsed { .. } => {
                    if let Item::Parsed { parsed, .. } = &item {
                        last_path = Some(parsed.path.clone());
                    }
                    batch.push(item);
                    if batch.len() >= BATCH_SIZE {
                        let outcome = commit_batch(&mut *conn, &mut batch)?;
                        apply_commit(&mut tally, &mut errors, outcome);
                        // 批边界必报一次：这是用户唯一能感觉到"又前进了一批"的地方
                        // （旧实现也是每批一次）。同时前推 next_report，否则下面的
                        // 步长判断会立刻再报一次一模一样的事件。
                        report(&tally, last_path.clone());
                        next_report = tally.scanned + PROGRESS_STEP;
                    }
                }
            }

            if tally.scanned >= next_report {
                next_report = tally.scanned + PROGRESS_STEP;
                report(&tally, last_path.clone());
            }
        }

        // 尾部不满一批的也要提交 —— 否则最后几个文件永远不入库
        if !batch.is_empty() && !cancelled {
            // 解析已经全完成、只剩这批没写库 —— 这正是 importing 该出现的时刻。
            // 提交前先如实报一次，否则这个状态在正常时序里几乎没有露面机会
            // （只有解析明显快于入库时才会被下面的步长判断撞上）。
            tally.parsed = parsed_done.load(Ordering::Relaxed);
            if tally.parsed >= total {
                report(&tally, last_path.clone());
            }
            let outcome = commit_batch(&mut *conn, &mut batch)?;
            apply_commit(&mut tally, &mut errors, outcome);
        }
        Ok(())
    })?;

    // 取消收尾：绝不 mark_missing（seen 不完整会误标大量在线文件），
    // 已提交的批次保留（增量无害，下次重扫继续）。
    if cancelled {
        let result = ScanResult {
            scanned: tally.scanned,
            imported: tally.imported,
            updated: tally.updated,
            unchanged: tally.unchanged,
            failed: tally.failed,
            missing: 0, // 取消路径不跑 mark_missing（seen 不完整）
            walk_errors,
            cancelled: true,
            errors: std::mem::take(&mut errors),
            elapsed_ms: started.elapsed().as_millis() as u64,
        };
        on_progress(snap("cancelled", &tally, Some(total), None, started));
        return Ok(result);
    }

    let missing = mark_missing(conn, roots, &seen)?;

    let elapsed_ms = started.elapsed().as_millis() as u64;
    let result = ScanResult {
        scanned: total,
        imported: tally.imported,
        updated: tally.updated,
        unchanged: tally.unchanged,
        failed: tally.failed,
        missing,
        walk_errors,
        cancelled: false,
        errors: std::mem::take(&mut errors),
        elapsed_ms,
    };
    // done 事件的计数取自 result 而不是 tally：两者此刻应当一致，
    // 但"最终报出去的数字"只能有一个来源，否则以后迟早对不上。
    on_progress(ScanProgress {
        phase: "done".into(),
        scanned: result.scanned,
        parsed: total,
        total: Some(total),
        imported: result.imported,
        updated: result.updated,
        skipped: result.unchanged,
        failed: result.failed,
        missing: result.missing,
        current_path: None,
        elapsed_ms: result.elapsed_ms,
        message: None,
    });
    Ok(result)
}

/// LIKE 通配符转义（ESCAPE '\'）：字面匹配 % _ \。
fn like_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '%' | '_' | '\\' => {
                out.push('\\');
                out.push(c);
            }
            _ => out.push(c),
        }
    }
    out
}

/// 把「库中 online + 位于**任一**根前缀下 + 本轮未 walk 到」的曲目标为 missing。
/// 重新出现的文件由 upsert 的 `availability = 'online'` 恢复。
///
/// 多根版本（docs/29 批 A）：OR 拼接各根前缀；没有任何根时**不标记**
/// （配置为空是"用户还没填"，把整库标 missing 是灾难）。
fn mark_missing(
    conn: &mut Connection,
    roots: &[std::path::PathBuf],
    seen: &std::collections::HashSet<String>,
) -> DbResult<usize> {
    // 前缀统一小写并以路径分隔符收尾，防止 "C:\m" 误匹配 "C:\music\..."
    let mut prefixes = Vec::with_capacity(roots.len());
    for root in roots {
        let mut prefix = root.to_string_lossy().to_lowercase();
        if !prefix.ends_with('\\') && !prefix.ends_with('/') {
            prefix.push(std::path::MAIN_SEPARATOR);
        }
        prefixes.push(like_escape(&prefix));
    }
    if prefixes.is_empty() {
        tracing::warn!("扫描根列表为空，跳过 missing 对账");
        return Ok(0);
    }
    // OR 子句按根数动态拼，参数占位符 ?1..?N
    let where_clause = prefixes
        .iter()
        .enumerate()
        .map(|(i, _)| format!("path_lower LIKE ?{} ESCAPE '\\'", i + 1))
        .collect::<Vec<_>>()
        .join(" OR ");
    let like_params: Vec<String> = prefixes
        .iter()
        .map(|p| format!("{p}%"))
        .collect();

    // 同批事务的理由：与主 DB 线程并发时 DEFERRED 的锁升级会立刻 BUSY
    let tx = conn
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(DbError::Sql)?;
    tx.execute_batch("CREATE TEMP TABLE IF NOT EXISTS scan_seen (path_lower TEXT PRIMARY KEY)")?;
    tx.execute("DELETE FROM scan_seen", [])?;
    {
        let mut stmt = tx.prepare("INSERT OR IGNORE INTO scan_seen (path_lower) VALUES (?1)")?;
        for p in seen {
            stmt.execute([p])?;
        }
    }
    let missing = tx.execute(
        &format!(
            "UPDATE tracks SET availability = 'missing'
             WHERE availability = 'online'
               AND ({where_clause})
               AND path_lower NOT IN (SELECT path_lower FROM scan_seen)"
        ),
        rusqlite::params_from_iter(like_params.iter()),
    )?;
    tx.execute_batch("DROP TABLE IF EXISTS temp.scan_seen")?;
    tx.commit().map_err(DbError::Sql)?;
    if missing > 0 {
        tracing::info!(missing, "scan marked missing tracks");
    }
    Ok(missing)
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

    /// 隐藏目录 / 系统噪音目录必须**整棵子树**剪掉。
    /// 回归点：旧实现 `for entry in WalkDir` 里对目录 `continue`，
    /// 只跳过目录条目本身，子目录照样走进去（.git 里几万个小文件全被遍历）。
    #[test]
    fn walk_prunes_hidden_and_noise_subtrees() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join(".git/objects/ab")).unwrap();
        std::fs::create_dir_all(root.join("$RECYCLE.BIN/keep")).unwrap();
        std::fs::create_dir_all(root.join("music")).unwrap();
        write_test_mp3(&root.join("music"), "keep.mp3", "留下", "艺");
        // 这些在剪掉的子树里，不该出现在结果中
        for (sub, name) in [
            (".git/objects/ab", "hidden.mp3"),
            ("$RECYCLE.BIN/keep", "junk.mp3"),
        ] {
            write_test_mp3(&root.join(sub), name, "噪音", "艺");
        }

        let (files, errors, cancelled) = walk(root, &|| false);
        assert_eq!(errors, 0);
        assert!(!cancelled);
        assert_eq!(files.len(), 1, "只应有 music/keep.mp3，实际: {files:?}");
        assert!(files[0].ends_with("keep.mp3"));
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

    #[test]
    fn deleted_file_marked_missing_then_recovers() {
        let dir = tempfile::tempdir().unwrap();
        let mp3 = write_test_mp3(dir.path(), "a.mp3", "测试标题", "测试艺人");

        let mut conn = open_in_memory().unwrap();
        let result = scan_directory(&mut conn, dir.path(), None, |_| {}).unwrap();
        assert_eq!(result.imported, 1, "errors={:?}", result.errors);

        let avail = |conn: &Connection| -> String {
            conn.query_row("SELECT availability FROM tracks", [], |r| r.get(0))
                .unwrap()
        };
        assert_eq!(avail(&conn), "online");

        // 删除文件 → 重扫 → missing
        std::fs::remove_file(&mp3).unwrap();
        let result2 = scan_directory(&mut conn, dir.path(), None, |_| {}).unwrap();
        assert_eq!(result2.imported, 0);
        assert_eq!(avail(&conn), "missing", "删除的文件必须标 missing");

        // 文件恢复 → 重扫 → online（重新出现的文件靠 upsert 恢复）
        write_test_mp3(dir.path(), "a.mp3", "测试标题", "测试艺人");
        let result3 = scan_directory(&mut conn, dir.path(), None, |_| {}).unwrap();
        assert_eq!(result3.imported, 0, "同路径文件重新出现走更新而非新插入");
        assert_eq!(avail(&conn), "online", "文件恢复后必须回到 online");
    }

    #[test]
    fn rescan_does_not_wipe_codec() {
        let dir = tempfile::tempdir().unwrap();
        write_test_mp3(dir.path(), "a.mp3", "A", "B");
        let mut conn = open_in_memory().unwrap();
        scan_directory(&mut conn, dir.path(), None, |_| {}).unwrap();
        // 模拟 beets_sync 写入 codec，并把 DB 里的 mtime 改掉，
        // 迫使下次扫描走 upsert 的 DO UPDATE 路径
        conn.execute("UPDATE tracks SET codec = 'MP3', mtime = 1", []).unwrap();
        scan_directory(&mut conn, dir.path(), None, |_| {}).unwrap();
        let codec: Option<String> =
            conn.query_row("SELECT codec FROM tracks", [], |r| r.get(0)).unwrap();
        assert_eq!(codec.as_deref(), Some("MP3"), "重扫不得把 beets 写的 codec 冲回 NULL");
    }

    /// docs/29 批 A：多根并集单库。两根各一首都要入库；missing 对账只作用
    /// 于**根前缀内**的曲目——这正是不能"逐根调单根版"的原因（第 1 根的
    /// 对账会把第 2 根的曲目误标 missing）。
    #[test]
    fn multi_roots_union_scan_and_missing_scope() {
        let dir_a = tempfile::tempdir().unwrap();
        let dir_b = tempfile::tempdir().unwrap();
        write_test_mp3(dir_a.path(), "a.mp3", "甲", "歌手");
        write_test_mp3(dir_b.path(), "b.mp3", "乙", "歌手");

        let mut conn = open_in_memory().unwrap();
        let roots = vec![dir_a.path().to_path_buf(), dir_b.path().to_path_buf()];
        let result = scan_roots_ext(&mut conn, &roots, None, || false, |_| {}).unwrap();
        assert_eq!(result.imported, 2, "errors={:?}", result.errors);

        // 先预置一首"根外"曲目（不在任何根下）——它绝不能被标 missing
        let dir_out = tempfile::tempdir().unwrap();
        write_test_mp3(dir_out.path(), "c.mp3", "丙", "歌手");
        scan_directory(&mut conn, dir_out.path(), None, |_| {}).unwrap();
        let total: i64 = conn.query_row("SELECT COUNT(*) FROM tracks", [], |r| r.get(0)).unwrap();
        assert_eq!(total, 3);

        // 只删 A 根的文件 → 重扫两根：A 根那首 missing，B 根与根外都还是 online
        std::fs::remove_file(dir_a.path().join("a.mp3")).unwrap();
        let result2 = scan_roots_ext(&mut conn, &roots, None, || false, |_| {}).unwrap();
        assert_eq!(result2.imported, 0);
        let avail = |title: &str| -> String {
            conn.query_row(
                "SELECT availability FROM tracks WHERE title = ?1",
                [title],
                |r| r.get(0),
            )
            .unwrap()
        };
        assert_eq!(avail("甲"), "missing", "根内消失的文件要标 missing");
        assert_eq!(avail("乙"), "online", "B 根的文件不得被 A 根的对账误伤");
        assert_eq!(avail("丙"), "online", "根外的文件不得被对账误伤");
    }

    #[test]
    fn case_variant_paths_do_not_duplicate_rows() {
        let dir = tempfile::tempdir().unwrap();
        write_test_mp3(dir.path(), "a.mp3", "A", "B");
        let mut conn = open_in_memory().unwrap();
        let r1 = scan_directory(&mut conn, dir.path(), None, |_| {}).unwrap();
        assert_eq!(r1.imported, 1);
        // 模拟第二次扫描时路径大小写变化（Windows 上盘符/目录大小写可能漂移）
        // —— 直接手工 upsert 一个大写变体路径，验证命中同一行。
        let mut parsed = parse_file(&dir.path().join("a.mp3")).unwrap();
        parsed.path = parsed.path.to_uppercase();
        let tx = conn.transaction().unwrap();
        let inserted = upsert(&tx, &parsed, 999, None).unwrap();
        tx.commit().unwrap();
        assert_eq!(
            inserted,
            UpsertOutcome::Unchanged,
            "大小写变体必须命中已有行（且 mtime/size 未变 → 早退），而不是新插入"
        );
        let count: i64 = conn.query_row("SELECT COUNT(*) FROM tracks", [], |r| r.get(0)).unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn scan_cancelled_skips_mark_missing() {
        let dir = tempfile::tempdir().unwrap();
        let mp3 = write_test_mp3(dir.path(), "a.mp3", "A", "B");
        let mut conn = open_in_memory().unwrap();
        let r1 = scan_directory(&mut conn, dir.path(), None, |_| {}).unwrap();
        assert_eq!(r1.imported, 1);

        // 文件已删除但取消扫描 → 不许把 online 误标 missing（seen 不完整）
        std::fs::remove_file(&mp3).unwrap();
        let r2 = scan_directory_ext(&mut conn, dir.path(), None, || true, |_| {}).unwrap();
        assert!(r2.cancelled);
        let avail: String =
            conn.query_row("SELECT availability FROM tracks", [], |r| r.get(0)).unwrap();
        assert_eq!(avail, "online", "取消路径绝不能执行 mark_missing");

        // 取消后再完整重扫 → 正常标 missing
        let r3 = scan_directory(&mut conn, dir.path(), None, |_| {}).unwrap();
        assert!(!r3.cancelled);
        let avail: String =
            conn.query_row("SELECT availability FROM tracks", [], |r| r.get(0)).unwrap();
        assert_eq!(avail, "missing");
    }

    #[test]
    fn scan_cancelled_mid_batches_keeps_committed_prefix() {
        let dir = tempfile::tempdir().unwrap();
        // BATCH_SIZE=200 → 201 个文件拆成两批；第一批提交后置位取消，
        // 第二批批顶检查点退出：已提交前缀保留、cancelled=true。
        for i in 0..201 {
            write_test_mp3(dir.path(), &format!("t{i:03}.mp3"), &format!("T{i}"), "B");
        }
        let mut conn = open_in_memory().unwrap();
        let cancel = std::sync::atomic::AtomicBool::new(false);
        let r = scan_directory_ext(
            &mut conn,
            dir.path(),
            None,
            || cancel.load(std::sync::atomic::Ordering::Relaxed),
            |p| {
                // 判定条件用 scanned，**不要用 phase**。
                //
                // 并行化之后（docs/29 批 B）解析常常先于入库跑完，第一批提交时
                // `parsed` 已经等于 total，于是这一次事件的 phase 是 `importing`
                // 而不是 `parsing`。写成 `phase == "parsing"` 时取消就永远不会
                // 置位，扫描会正常跑完，断言以"取消没生效"的形式间歇性失败 ——
                // 实际抓到的就是这个（同一条测试跑两次绿、第三次红）。
                if p.scanned > 0 {
                    cancel.store(true, std::sync::atomic::Ordering::Relaxed);
                }
            },
        )
        .unwrap();
        assert!(r.cancelled);
        assert_eq!(r.scanned, BATCH_SIZE, "第二批未开始，scanned 停在批边界");
        assert_eq!(r.imported, BATCH_SIZE, "已提交的第一批保留");
        assert_eq!(r.failed, 0, "取消不该把文件记成失败：{:?}", r.errors);
        let count: i64 = conn.query_row("SELECT COUNT(*) FROM tracks", [], |r| r.get(0)).unwrap();
        assert_eq!(count, BATCH_SIZE as i64, "已提交批次落库");
    }
}
