// 服务端状态与宿主侧 helper：与桌面壳同构的 DB 句柄 + 播放 Actor 直驱，
// 差异只在"引擎意图 drain 后不下发"——出声在浏览器，Windows 驱动不进本
// crate 的编译图（编译期双壳）。

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use aiya_data::DbHandle;
use aiya_player::PlayerActor;

pub const TRACK_REF_IN_CHUNK: usize = 900;
pub const ID_COLLECT_CAP: usize = 50_000;

pub const DEFAULT_SETTINGS: &str = r#"{
  "theme": "dark",
  "ambient": "aurora",
  "normalizeVolume": false,
  "glEffects": true,
  "bgSpectrum": true,
  "npSpectrum": false,
  "spectrumStyle": "radial",
  "ridgeGlow": "soft",
  "output": { "mode": "shared", "bufferMs": 0, "clock": "auto", "releaseOnPause": true },
  "autoplayOnStart": false,
  "rememberPosition": true,
  "preventSleep": true,
  "closeToTray": false,
  "refreshSeconds": 30,
  "historyDays": 90,
  "shortcuts": {}
}"#;

pub struct ServerState {
    /// 登录门：口令/静态令牌 + 内存会话（auth.rs）
    pub auth: crate::auth::AuthGate,
    /// SSE 事件总线（scan:progress + beets×5 的生产者出口）
    pub events: crate::events::EventBus,
    /// 扫描管线控制（scanning/cancel/最近快照）
    pub scan: crate::scan::ScanCtl,
    /// beets 安装登记表（host-core）：子进程收尸槽 + 占位互斥
    pub beets_install: std::sync::Arc<aiya_host_core::InstallRegistry>,
    /// 最近一次位置回报（记账秒数的估算源）：(track_id, ms)
    pub last_played: Mutex<(String, u64)>,
    /// 队列状态行上次落库时间（毫秒；位置回报 5s 节流）
    pub last_persist_ms: std::sync::atomic::AtomicU64,
    pub db: Mutex<DbHandle>,
    pub db_path: PathBuf,
    /// 封面缓存根：<data_root>/cache/artwork（originals/ + thumbs/）
    pub artwork_dir: PathBuf,
    /// beets 索引增强结论库目录：<data_root>/beets-index
    pub beets_index_dir: PathBuf,
    pub player: Mutex<PlayerActor>,
    /// app_settings 内存镜像（与桌面壳同理由：热路径读不跨线程打库）
    pub settings: Mutex<serde_json::Value>,
    pub enrich_store: std::sync::OnceLock<std::sync::Arc<aiya_beets_store::Store>>,
}

impl ServerState {
    /// beets 索引增强结论库（懒打开，进程内单例）。
    pub fn enrich_store(&self) -> Result<std::sync::Arc<aiya_beets_store::Store>, String> {
        if let Some(s) = self.enrich_store.get() {
            return Ok(std::sync::Arc::clone(s));
        }
        let path = self.beets_index_dir.join("enrich.db");
        let store = std::sync::Arc::new(
            aiya_beets_store::Store::open(&path).map_err(|e| format!("{}: {e}", path.display()))?,
        );
        let _ = self.enrich_store.set(std::sync::Arc::clone(&store));
        Ok(self
            .enrich_store
            .get()
            .map(std::sync::Arc::clone)
            .unwrap_or(store))
    }
}

pub fn db_lock(st: &ServerState) -> std::sync::MutexGuard<'_, DbHandle> {
    st.db
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

pub fn player_lock(st: &ServerState) -> std::sync::MutexGuard<'_, PlayerActor> {
    st.player
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

pub fn settings_lock(st: &ServerState) -> std::sync::MutexGuard<'_, serde_json::Value> {
    st.settings
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

pub fn setting_flag(st: &ServerState, key: &str, default: bool) -> bool {
    settings_lock(st)
        .get(key)
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(default)
}

pub fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

/* ---- settings KV（与桌面壳 setting_get/setting_set 同形） ---- */

pub fn setting_get(conn: &rusqlite::Connection, key: &str) -> Option<serde_json::Value> {
    conn.query_row(
        "SELECT value_json FROM settings WHERE key = ?1",
        [key],
        |r| r.get::<_, String>(0),
    )
    .ok()
    .and_then(|s| serde_json::from_str(&s).ok())
}

pub fn setting_set(
    conn: &rusqlite::Connection,
    key: &str,
    value: &serde_json::Value,
) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO settings (key, value_json) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value_json = excluded.value_json",
        rusqlite::params![key, value.to_string()],
    )?;
    Ok(())
}

/* ---- 播放驱动 ---- */

/// 引擎意图 drain + 队列落库 + 快照。桌面壳的 drive_and_emit 去掉引擎下发
/// 与事件广播：装载意图只 drain（客户端按 `state.token` 变化感知换曲），
/// `player:state` 不上 SSE（进度时钟在浏览器，快照由客户端合成）。
pub fn drive(st: &ServerState) -> aiya_player::PlayerState {
    let mut actor = player_lock(st);
    let mut acted = false;
    if actor.take_pending_load().is_some() {
        acted = true;
    }
    if actor.take_pending_stop() {
        acted = true;
    }
    let persist =
        acted.then(|| queue_persist_from_actor(&actor, setting_flag(st, "rememberPosition", true)));
    let snapshot = actor.state();
    drop(actor);
    if let Some(p) = persist {
        persist_queue_full(st, p, &queue_items_of(st));
    }
    snapshot
}

fn queue_items_of(st: &ServerState) -> Vec<aiya_data::QueueItem> {
    player_lock(st)
        .queue_refs()
        .iter()
        .map(queue_item_from_ref)
        .collect()
}

pub fn queue_persist_from_actor(
    actor: &PlayerActor,
    remember_position: bool,
) -> aiya_data::QueuePersist {
    let s = actor.state();
    aiya_data::QueuePersist {
        current_index: s.index.map_or(-1, |i| i as i64),
        position_ms: if remember_position {
            s.position_ms as i64
        } else {
            0
        },
        repeat_mode: match s.repeat {
            aiya_player::RepeatMode::All => "all",
            aiya_player::RepeatMode::One => "one",
            aiya_player::RepeatMode::Off => "off",
        }
        .into(),
        shuffle: s.shuffle,
        volume: s.volume,
        muted: s.muted,
        rate: s.rate,
    }
}

pub fn queue_item_from_ref(r: &aiya_player::TrackRef) -> aiya_data::QueueItem {
    if !r.is_transient() {
        return aiya_data::QueueItem::track(r.id.clone());
    }
    let payload = serde_json::json!({
        "path": r.path, "title": r.title, "artist": r.artist, "durationMs": r.duration_ms,
    });
    let kind = if r.is_stream() {
        aiya_data::QueueKind::Stream
    } else {
        aiya_data::QueueKind::File
    };
    aiya_data::QueueItem::transient(
        kind,
        serde_json::to_string(&payload).unwrap_or_else(|_| "{}".into()),
    )
}

pub fn persist_queue_full(
    st: &ServerState,
    p: aiya_data::QueuePersist,
    items: &[aiya_data::QueueItem],
) {
    let items = items.to_vec();
    let result = db_lock(st).send(move |conn| aiya_data::save_queue_full(conn, &p, &items));
    if let Err(e) = result {
        tracing::warn!(error = %e, "queue full persist failed");
    }
}

/// 库内曲目批量回查 TrackRef（含临时项映射），与桌面壳 track_refs_by_ids_db 同形。
pub fn track_refs_by_ids_db(db: &DbHandle, ids: &[String]) -> Vec<aiya_player::TrackRef> {
    let ids = ids.to_vec();
    db.send(move |conn| {
        let transient: std::collections::HashMap<String, aiya_player::TrackRef> =
            if ids.iter().any(|id| {
                id.starts_with(aiya_player::FILE_ID_PREFIX)
                    || id.starts_with(aiya_player::STREAM_ID_PREFIX)
            }) {
                aiya_data::queue::load_queue(conn)
                    .ok()
                    .flatten()
                    .map(|(_, items)| {
                        items
                            .iter()
                            .filter(|item| item.kind != aiya_data::QueueKind::Track)
                            .filter_map(|item| ref_from_transient(item).map(|r| (r.id.clone(), r)))
                            .collect()
                    })
                    .unwrap_or_default()
            } else {
                Default::default()
            };

        let mut by_id: std::collections::HashMap<String, aiya_player::TrackRef> =
            std::collections::HashMap::new();
        let wanted: Vec<&String> = ids
            .iter()
            .filter(|id| !transient.contains_key(id.as_str()))
            .collect();
        for chunk in wanted.chunks(TRACK_REF_IN_CHUNK) {
            let placeholders = (1..=chunk.len())
                .map(|i| format!("?{i}"))
                .collect::<Vec<_>>()
                .join(", ");
            let sql = format!(
                "SELECT t.id, t.title, ar.name, al.title, t.duration_ms, t.artwork_id, t.gain_db, t.path,
                        t.codec, t.container, t.sample_rate, t.bits_per_sample, t.bitrate, t.channels,
                        t.artist_id, t.album_id
                 FROM tracks t
                 LEFT JOIN artists ar ON ar.id = t.artist_id
                 LEFT JOIN albums al ON al.id = t.album_id
                 WHERE t.id IN ({placeholders})"
            );
            let mut stmt = conn.prepare(&sql)?;
            let rows = stmt.query_map(rusqlite::params_from_iter(chunk.iter()), |r| {
                let codec: Option<String> = r.get(8)?;
                let container: Option<String> = r.get(9)?;
                let audio = if codec.is_some() || container.is_some() {
                    let lossless = aiya_player::TrackAudio::infer_lossless(
                        codec.as_deref(),
                        container.as_deref(),
                    );
                    Some(aiya_player::TrackAudio {
                        codec,
                        container,
                        sample_rate: r.get(10)?,
                        bits_per_sample: r.get(11)?,
                        bitrate: r.get(12)?,
                        channels: r.get(13)?,
                        lossless,
                    })
                } else {
                    None
                };
                Ok(aiya_player::TrackRef {
                    id: r.get(0)?,
                    title: r.get(1)?,
                    artist: r.get(2)?,
                    album: r.get(3)?,
                    artist_id: r.get(14)?,
                    album_id: r.get(15)?,
                    duration_ms: r.get(4)?,
                    artwork_id: r.get(5)?,
                    gain_db: r.get(6)?,
                    audio,
                    path: r.get(7)?,
                })
            })?;
            for row in rows {
                let t = row?;
                by_id.insert(t.id.clone(), t);
            }
        }
        let mut out = Vec::with_capacity(ids.len());
        for id in &ids {
            if let Some(hit) = transient.get(id) {
                out.push(hit.clone());
            } else if let Some(t) = by_id.get(id.as_str()) {
                out.push(t.clone());
            }
        }
        Ok(out)
    })
    .unwrap_or_default()
}

fn ref_from_transient(item: &aiya_data::QueueItem) -> Option<aiya_player::TrackRef> {
    let payload = item.payload.as_deref()?;
    let p: serde_json::Value = serde_json::from_str(payload).ok()?;
    Some(match item.kind {
        aiya_data::QueueKind::Stream => aiya_player::TrackRef::transient_stream(
            p.get("path")?.as_str()?,
            p.get("title").and_then(|v| v.as_str()).unwrap_or_default(),
        ),
        _ => {
            let mut r = aiya_player::TrackRef::transient_file(
                p.get("path")?.as_str()?,
                p.get("title").and_then(|v| v.as_str()).unwrap_or_default(),
            );
            r.artist = p.get("artist").and_then(|v| v.as_str()).map(str::to_string);
            r.duration_ms = p.get("durationMs").and_then(|v| v.as_i64());
            r
        }
    })
}

/// 删除曲目后的队列同步（与桌面壳 prune_queue_for_deleted 同形，去引擎/广播）。
pub fn prune_queue_for_deleted(st: &ServerState, deleted: &[String]) {
    if deleted.is_empty() {
        return;
    }
    let gone: std::collections::HashSet<&str> = deleted.iter().map(String::as_str).collect();
    let mut actor = player_lock(st);
    let indices: Vec<usize> = actor
        .queue_snapshot()
        .ids
        .iter()
        .enumerate()
        .filter(|(_, id)| gone.contains(id.as_str()))
        .map(|(i, _)| i)
        .collect();
    if indices.is_empty() {
        return;
    }
    actor.reduce(aiya_player::PlayerCommand::Queue(
        aiya_player::QueueOp::Remove { indices },
    ));
    let _ = actor.take_pending_load();
    let _ = actor.take_pending_stop();
    let p = queue_persist_from_actor(&actor, setting_flag(st, "rememberPosition", true));
    let items: Vec<aiya_data::QueueItem> =
        actor.queue_refs().iter().map(queue_item_from_ref).collect();
    drop(actor);
    // 队列变了就必须写（桌面壳同规则）：CASCADE 后库内行要与内存一致
    persist_queue_full(st, p, &items);
}

/* ---- library roots helper ---- */

pub fn read_roots(conn: &rusqlite::Connection) -> Vec<String> {
    setting_get(conn, "library.roots")
        .and_then(|v| v.as_array().cloned())
        .map(|a| {
            a.into_iter()
                .filter_map(|s| s.as_str().map(str::to_string))
                .filter(|s| !s.trim().is_empty())
                .collect()
        })
        .or_else(|| {
            setting_get(conn, "library.root")
                .and_then(|v| v.as_str().map(str::to_string))
                .map(|s| vec![s])
        })
        .unwrap_or_default()
}

pub fn read_paused_roots(conn: &rusqlite::Connection) -> Vec<String> {
    setting_get(conn, "library.roots.paused")
        .and_then(|v| v.as_array().cloned())
        .map(|a| {
            a.into_iter()
                .filter_map(|s| s.as_str().map(str::to_string))
                .filter(|s| !s.trim().is_empty())
                .collect()
        })
        .unwrap_or_default()
}

pub fn root_prefix_lower(path: &str) -> String {
    let s = path.trim().to_lowercase();
    if s.ends_with('\\') || s.ends_with('/') || s.is_empty() {
        s
    } else {
        format!("{s}\\")
    }
}

pub fn count_tracks_under_prefix(
    conn: &rusqlite::Connection,
    prefix: &str,
) -> rusqlite::Result<i64> {
    conn.query_row(
        "SELECT COUNT(*) FROM tracks WHERE substr(path_lower, 1, ?2) = ?1",
        rusqlite::params![prefix, prefix.chars().count() as i64],
        |r| r.get(0),
    )
}

/* ---- playlist helper ---- */

pub const PLAYLIST_SELECT: &str = "SELECT p.id, p.name, p.created_at, p.updated_at, p.sort_position, p.kind, p.rule_json,
       (SELECT COUNT(*) FROM playlist_items pi JOIN tracks t ON t.id = pi.track_id
         WHERE pi.playlist_id = p.id),
       (SELECT COALESCE(SUM(t.duration_ms),0) FROM playlist_items pi JOIN tracks t ON t.id = pi.track_id
         WHERE pi.playlist_id = p.id),
       (SELECT t2.artwork_id FROM playlist_items pi2 JOIN tracks t2 ON t2.id = pi2.track_id
         WHERE pi2.playlist_id = p.id ORDER BY pi2.position LIMIT 1)
  FROM playlists p";

pub fn playlist_row(r: &rusqlite::Row) -> rusqlite::Result<aiya_dto::playlist::PlaylistSummary> {
    Ok(aiya_dto::playlist::PlaylistSummary {
        id: r.get(0)?,
        name: r.get(1)?,
        description: None,
        kind: r.get(5)?,
        rule_json: r.get(6)?,
        created_at_ms: r.get::<_, i64>(2)? * 1000,
        updated_at_ms: r.get::<_, i64>(3)? * 1000,
        sort_position: r.get(4)?,
        track_count: r.get(7)?,
        duration_ms: r.get(8)?,
        artwork_id: r.get(9)?,
    })
}

pub fn playlist_list_all(
    conn: &rusqlite::Connection,
) -> aiya_data::DbResult<Vec<aiya_dto::playlist::PlaylistSummary>> {
    let sql = format!("{PLAYLIST_SELECT} ORDER BY p.sort_position ASC, p.created_at ASC");
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([], playlist_row)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

pub fn playlist_summary_by_id(
    conn: &rusqlite::Connection,
    id: &str,
) -> rusqlite::Result<Option<aiya_dto::playlist::PlaylistSummary>> {
    let sql = format!("{PLAYLIST_SELECT} WHERE p.id = ?1");
    match conn.query_row(&sql, [id], playlist_row) {
        Ok(v) => Ok(Some(v)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e),
    }
}

/* ---- history helper ---- */

pub fn history_stats_from(
    conn: &rusqlite::Connection,
) -> aiya_data::DbResult<aiya_dto::history::HistoryStats> {
    let count = |sql: &str| -> i64 { conn.query_row(sql, [], |r| r.get(0)).unwrap_or(0) };
    let total_plays = count("SELECT COUNT(*) FROM history");
    let completed = count("SELECT COUNT(*) FROM history WHERE seconds_played >= 1");
    let mut stmt = conn.prepare(
        "SELECT h.track_id, t.title, ar.name, COUNT(*) AS plays
         FROM history h
         JOIN tracks t ON t.id = h.track_id
         LEFT JOIN artists ar ON ar.id = t.artist_id
         GROUP BY h.track_id ORDER BY plays DESC LIMIT 10",
    )?;
    let top: Vec<aiya_dto::history::HistoryTopTrack> = stmt
        .query_map([], |r| {
            Ok(aiya_dto::history::HistoryTopTrack {
                id: r.get(0)?,
                title: r.get(1)?,
                artist: r.get(2)?,
                play_count: r.get(3)?,
            })
        })?
        .filter_map(|r| r.ok())
        .collect();
    let first = conn
        .query_row("SELECT MIN(played_at) FROM history", [], |r| {
            r.get::<_, Option<i64>>(0)
        })
        .unwrap_or(None);
    let last = conn
        .query_row("SELECT MAX(played_at) FROM history", [], |r| {
            r.get::<_, Option<i64>>(0)
        })
        .unwrap_or(None);
    Ok(aiya_dto::history::HistoryStats {
        total_plays,
        total_ms: count("SELECT COALESCE(SUM(seconds_played),0) FROM history") * 1000,
        completed,
        skipped: total_plays - completed,
        first_play_at_ms: first.map(|s| s * 1000),
        last_play_at_ms: last.map(|s| s * 1000),
        top_tracks: top,
    })
}

/* ---- 启动：扫描入库 + 队列恢复 ---- */

/// 启动扫描：AIYA_MUSIC_DIR 指向的目录并进曲库根并增量扫描（mtime 对账）。
/// 桌面壳的定时刷新管线（进度事件/取消/暂停窗）属 W3，这里只落"数据进库"。
pub fn bootstrap_scan(st: &ServerState, music_dir: &Path) -> Result<(), String> {
    let dir = music_dir.to_string_lossy().into_owned();
    let artwork_dir = st.artwork_dir.clone();
    let result = db_lock(st).send(move |conn| {
        let mut roots = read_roots(conn);
        if !roots
            .iter()
            .any(|r| r.trim_end_matches(['/', '\\']) == dir.trim_end_matches(['/', '\\']))
        {
            roots.push(dir.clone());
            setting_set(conn, "library.roots", &serde_json::json!(roots))?;
        }
        let roots = read_roots(conn);
        let paths: Vec<PathBuf> = roots.iter().map(PathBuf::from).collect();
        aiya_data::scanner::scan_roots_ext(conn, &paths, Some(&artwork_dir), || false, |_| {})
    });
    match result {
        Ok(r) => {
            tracing::info!(
                imported = r.imported,
                updated = r.updated,
                missing = r.missing,
                "bootstrap scan done"
            );
            Ok(())
        }
        Err(e) => Err(e.to_string()),
    }
}

/// 启动恢复：queue_state + queue_items → Actor hydrate（默认暂停，浏览器侧
/// 用户点播放才出声）。与桌面壳同形，去掉引擎装载——首帧快照的 token 变化
/// 会让浏览器装载当前曲。
pub fn restore_queue(st: &ServerState) {
    let remember_position = setting_flag(st, "rememberPosition", true);
    let loaded: Result<
        Option<(aiya_data::QueuePersist, Vec<aiya_data::QueueItem>)>,
        aiya_data::DbError,
    > = db_lock(st).send(|conn| aiya_data::load_queue(conn));
    let Ok(Some((p, items))) = loaded else {
        return;
    };
    if items.is_empty() {
        return;
    }
    let track_ids: Vec<String> = items.iter().filter_map(|i| i.track_id.clone()).collect();
    let mut by_id: std::collections::HashMap<String, aiya_player::TrackRef> =
        track_refs_by_ids_db(&db_lock(st), &track_ids)
            .into_iter()
            .map(|r| (r.id.clone(), r))
            .collect();
    let mut tracks = Vec::with_capacity(items.len());
    let mut current_id = None;
    for (i, item) in items.iter().enumerate() {
        let r = match &item.track_id {
            Some(id) => by_id.remove(id),
            None => ref_from_transient(item),
        };
        let Some(r) = r else { continue };
        if i as i64 == p.current_index {
            current_id = Some(r.id.clone());
        }
        tracks.push(r);
    }
    if tracks.is_empty() {
        return;
    }
    let repeat = match p.repeat_mode.as_str() {
        "all" => aiya_player::RepeatMode::All,
        "one" => aiya_player::RepeatMode::One,
        _ => aiya_player::RepeatMode::Off,
    };
    let is_stream_current = current_id
        .as_deref()
        .is_some_and(|id| id.starts_with(aiya_player::STREAM_ID_PREFIX));
    let start_ms = if remember_position && !is_stream_current {
        p.position_ms.max(0) as u64
    } else {
        0
    };
    let mut actor = player_lock(st);
    actor.hydrate(
        tracks,
        repeat,
        p.shuffle,
        p.volume,
        p.muted,
        p.rate,
        start_ms,
        current_id.as_deref(),
    );
    let _ = actor.take_pending_load();
    let snapshot = actor.state();
    drop(actor);
    tracing::info!(
        tracks = snapshot.queue_length,
        position = snapshot.position_ms,
        "player state restored"
    );
}
