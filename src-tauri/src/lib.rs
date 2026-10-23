//! AIYA Music — Tauri 宿主壳。
//!
//! 边界约定（src-tauri/README.md）：命令注册、事件桥、协议注册；领域业务在 crates/。
//! 本文件把 `aiya-data` 的能力暴露为 IPC 命令，返回与 `@aiya/contracts`
//! 的 `IpcResult<T>` 同构 JSON —— UI 的 `result.ok` 分支零改动。

use std::borrow::Cow;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager, State};

use aiya_data::ids;
use aiya_data::repo::{self, TrackWindowQuery};
use aiya_data::scanner;

/* ---------------- IpcResult 信封（与 contracts/ipc.ts 同构） ---------------- */

fn ok(data: Value) -> Value {
    json!({ "ok": true, "data": data })
}

fn fail(code: &str, message: &str) -> Value {
    json!({ "ok": false, "error": { "code": code, "message": message } })
}

/* ---------------- 应用状态 ---------------- */

pub struct AppState {
    db: Mutex<aiya_data::DbHandle>,
    db_path: PathBuf,
    /// 封面缓存根：<app_data>/cache/artwork（originals/ + thumbs/）
    artwork_dir: PathBuf,
    scanning: AtomicBool,
}

impl AppState {
    fn db(&self) -> std::sync::MutexGuard<'_, aiya_data::DbHandle> {
        self.db.lock().expect("db handle poisoned")
    }
}

fn default_db_path(app: &AppHandle) -> PathBuf {
    let dir = app
        .path()
        .app_data_dir()
        .unwrap_or_else(|_| PathBuf::from("."));
    let _ = std::fs::create_dir_all(&dir);
    dir.join("library.sqlite")
}

fn default_artwork_dir(app: &AppHandle) -> PathBuf {
    app.path()
        .app_data_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("cache")
        .join("artwork")
}

/* ---------------- 工具：settings 表读写 ---------------- */

fn setting_get(conn: &rusqlite::Connection, key: &str) -> Option<Value> {
    conn.query_row("SELECT value_json FROM settings WHERE key = ?1", [key], |r| {
        r.get::<_, String>(0)
    })
    .ok()
    .and_then(|s| serde_json::from_str(&s).ok())
}

fn setting_set(conn: &rusqlite::Connection, key: &str, value: &Value) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO settings (key, value_json) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value_json = excluded.value_json",
        rusqlite::params![key, value.to_string()],
    )?;
    Ok(())
}

const DEFAULT_SETTINGS: &str = r#"{
  "theme": "dark",
  "density": "comfortable",
  "accentFromArtwork": true,
  "normalizeVolume": false,
  "spectrum": false,
  "spectrumEngine": "pro",
  "mediaKeys": true,
  "autoplayOnStart": false,
  "rememberPosition": true,
  "preventSleep": true,
  "closeToTray": false,
  "shortcuts": {}
}"#;

/* ---------------- app 命令 ---------------- */

#[tauri::command]
fn app_info(state: State<'_, AppState>) -> Value {
    ok(json!({
        "version": env!("CARGO_PKG_VERSION"),
        "electron": "tauri",
        "platform": std::env::consts::OS,
        "dataDir": state.db_path.parent().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default(),
    }))
}

#[tauri::command]
fn app_get_ui_state(state: State<'_, AppState>) -> Value {
    let result: aiya_data::DbResult<Value> = state
        .db()
        .send(|conn| Ok(setting_get(conn, "ui_state").unwrap_or_else(|| json!({}))));
    result.map(ok).unwrap_or_else(|e| fail("E_INTERNAL", &e.to_string()))
}

#[tauri::command]
fn app_set_ui_state(state: State<'_, AppState>, ui_state: Value) -> Value {
    let result: aiya_data::DbResult<()> = state.db().send(move |conn| {
        setting_set(conn, "ui_state", &ui_state).map_err(aiya_data::DbError::Sql)
    });
    result.map(|_| ok(json!(null))).unwrap_or_else(|e| fail("E_INTERNAL", &e.to_string()))
}

#[tauri::command]
fn app_get_settings(state: State<'_, AppState>) -> Value {
    let default: Value = serde_json::from_str(DEFAULT_SETTINGS).expect("valid default");
    let result: aiya_data::DbResult<Value> = state.db().send(move |conn| {
        Ok(setting_get(conn, "app_settings").unwrap_or(default))
    });
    result.map(ok).unwrap_or_else(|e| fail("E_INTERNAL", &e.to_string()))
}

#[tauri::command]
fn app_set_settings(state: State<'_, AppState>, settings: Value) -> Value {
    let result: aiya_data::DbResult<Value> = state.db().send(move |conn| {
        setting_set(conn, "app_settings", &settings).map_err(aiya_data::DbError::Sql)?;
        Ok(settings)
    });
    result.map(ok).unwrap_or_else(|e| fail("E_INTERNAL", &e.to_string()))
}

/// 诊断页。v1 第一切：统计字段真实，引擎/系统字段为占位（P2/P4 回填）。
#[tauri::command]
fn app_diagnostics(state: State<'_, AppState>) -> Value {
    let db_path = state.db_path.clone();
    let result: aiya_data::DbResult<Value> = state.db().send(move |conn| {
        let count = |sql: &str| -> i64 { conn.query_row(sql, [], |r| r.get(0)).unwrap_or(0) };
        let db_bytes = std::fs::metadata(&db_path).map(|m| m.len()).unwrap_or(0) as i64
            + std::fs::metadata(db_path.with_extension("sqlite-wal"))
                .map(|m| m.len())
                .unwrap_or(0) as i64;
        Ok(json!({
            "version": env!("CARGO_PKG_VERSION"),
            "electron": "tauri",
            "platform": std::env::consts::OS,
            "dataDir": db_path.parent().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default(),
            "schemaVersion": conn.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0)).unwrap_or(0),
            "dbBytes": db_bytes,
            "tracks": count("SELECT COUNT(*) FROM tracks"),
            "albums": count("SELECT COUNT(*) FROM albums"),
            "artists": count("SELECT COUNT(*) FROM artists"),
            "playlists": count("SELECT COUNT(*) FROM playlists"),
            "historyRows": count("SELECT COUNT(*) FROM history"),
            "totalDurationMs": count("SELECT COALESCE(SUM(duration_ms),0) FROM tracks"),
            "streams": { "opened": 0, "closed": 0, "active": 0 },
            "listTimings": [],
            "recentErrors": [],
            "shortcutConflicts": [],
            "mediaKeys": { "enabled": false, "active": [], "failed": [] },
            "power": { "active": false, "type": null },
            "tray": false,
        }))
    });
    result.map(ok).unwrap_or_else(|e| fail("E_INTERNAL", &e.to_string()))
}

#[tauri::command]
fn app_open_data_dir(state: State<'_, AppState>) -> Value {
    if let Some(dir) = state.db_path.parent() {
        let _ = std::process::Command::new("explorer").arg(dir).spawn();
    }
    ok(json!(null))
}

/* ---------------- library 命令 ---------------- */

#[tauri::command]
fn library_list(state: State<'_, AppState>, query: Value) -> Value {
    let q: TrackWindowQuery = match serde_json::from_value(query) {
        Ok(q) => q,
        Err(e) => return fail("E_BAD_QUERY", &e.to_string()),
    };
    let result: aiya_data::DbResult<Value> = state.db().send(move |conn| {
        serde_json::to_value(repo::list_tracks(conn, &q)?)
            .map_err(|e| aiya_data::DbError::Custom(e.to_string()))
    });
    result.map(ok).unwrap_or_else(|e| fail("E_INTERNAL", &e.to_string()))
}

/// 单曲/多曲查询（播放条数据源）：带出音频规格。列表查询不带（契约约定）。
#[tauri::command]
fn library_tracks(state: State<'_, AppState>, ids: Vec<String>) -> Value {
    let result: aiya_data::DbResult<Value> = state.db().send(move |conn| {
        let mut out = Vec::with_capacity(ids.len());
        for id in &ids {
            let row = conn.query_row(
                "SELECT t.id, t.title, t.artist_id, ar.name, t.album_id, al.title, al_ar.name,
                        t.duration_ms, t.track_no, t.disc_no, t.year, t.added_at, t.artwork_id,
                        t.play_count, t.last_played_at, t.gain_db,
                        t.codec, t.container, t.sample_rate, t.bits_per_sample, t.bitrate, t.channels
                 FROM tracks t
                 LEFT JOIN artists ar ON ar.id = t.artist_id
                 LEFT JOIN albums al ON al.id = t.album_id
                 LEFT JOIN artists al_ar ON al_ar.id = al.artist_id
                 WHERE t.id = ?1",
                [id],
                |r| {
                    let codec: Option<String> = r.get(16)?;
                    let sample_rate: Option<i64> = r.get(18)?;
                    let audio = if codec.is_none() && sample_rate.is_none() {
                        Value::Null
                    } else {
                        json!({
                            "codec": codec,
                            "container": r.get::<_, Option<String>>(17)?,
                            "sampleRate": sample_rate,
                            "bitsPerSample": r.get::<_, Option<i64>>(19)?,
                            "bitrate": r.get::<_, Option<i64>>(20)?,
                            "channels": r.get::<_, Option<i64>>(21)?,
                        })
                    };
                    Ok(json!({
                        "id": r.get::<_, String>(0)?,
                        "title": r.get::<_, String>(1)?,
                        "artistId": r.get::<_, Option<String>>(2)?,
                        "artist": r.get::<_, Option<String>>(3)?,
                        "albumId": r.get::<_, Option<String>>(4)?,
                        "album": r.get::<_, Option<String>>(5)?,
                        "albumArtist": r.get::<_, Option<String>>(6)?,
                        "durationMs": r.get::<_, Option<i64>>(7)?,
                        "trackNo": r.get::<_, Option<i64>>(8)?,
                        "discNo": r.get::<_, Option<i64>>(9)?,
                        "year": r.get::<_, Option<i64>>(10)?,
                        "addedAt": r.get::<_, i64>(11)?,
                        "artworkId": r.get::<_, Option<String>>(12)?,
                        "playCount": r.get::<_, i64>(13)?,
                        "lastPlayedAt": r.get::<_, Option<i64>>(14)?,
                        "gainDb": r.get::<_, Option<f64>>(15)?,
                        "audio": audio,
                    }))
                },
            );
            match row {
                Ok(v) => out.push(v),
                Err(rusqlite::Error::QueryReturnedNoRows) => {}
                Err(e) => return Err(aiya_data::DbError::Sql(e)),
            }
        }
        serde_json::to_value(out).map_err(|e| aiya_data::DbError::Custom(e.to_string()))
    });
    result.map(ok).unwrap_or_else(|e| fail("E_INTERNAL", &e.to_string()))
}

#[tauri::command]
fn library_stats(state: State<'_, AppState>) -> Value {
    let db_path = state.db_path.clone();
    let result: aiya_data::DbResult<Value> = state.db().send(move |conn| {
        let count = |sql: &str| -> i64 { conn.query_row(sql, [], |r| r.get(0)).unwrap_or(0) };
        let db_bytes = std::fs::metadata(&db_path).map(|m| m.len()).unwrap_or(0) as i64;
        Ok(json!({
            "tracks": count("SELECT COUNT(*) FROM tracks"),
            "albums": count("SELECT COUNT(*) FROM albums"),
            "artists": count("SELECT COUNT(*) FROM artists"),
            "locations": 1,
            "missingLocations": 0,
            "totalDurationMs": count("SELECT COALESCE(SUM(duration_ms),0) FROM tracks"),
            "dbBytes": db_bytes,
            "schemaVersion": conn.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0)).unwrap_or(0),
        }))
    });
    result.map(ok).unwrap_or_else(|e| fail("E_INTERNAL", &e.to_string()))
}

/// 扫描：阻塞直到完成（进度经 `scan:progress` 事件推送，UI 侧节流渲染）。
/// 用独立连接（WAL 下与主 DB 线程并行），不占用 DbHandle。
/// Tauri 要求 async 命令带引用参数必须返回 Result —— 外面套 Ok。
#[tauri::command]
async fn library_scan(
    app: AppHandle,
    state: State<'_, AppState>,
    options: Option<Value>,
) -> Result<Value, ()> {
    Ok(library_scan_body(app, &state, options).await)
}

async fn library_scan_body(app: AppHandle, state: &State<'_, AppState>, options: Option<Value>) -> Value {
    if state.scanning.swap(true, Ordering::SeqCst) {
        return fail("E_SCAN_BUSY", "已有扫描在进行");
    }

    // 路径优先级：显式 options.path > settings 里存的 library.root > 系统 Music 目录
    let root: PathBuf = options
        .as_ref()
        .and_then(|o| o.get("path"))
        .and_then(|p| p.as_str())
        .map(PathBuf::from)
        .or_else(|| {
            let from_settings: Option<String> = state
                .db()
                .send(|conn| {
                    Ok(setting_get(conn, "library.root").and_then(|v| v.as_str().map(str::to_string)))
                })
                .ok()
                .flatten();
            from_settings.map(PathBuf::from).or_else(|| {
                std::env::var("USERPROFILE").ok().map(|h| PathBuf::from(h).join("Music"))
            })
        })
        .unwrap_or_default();

    if !root.is_dir() {
        state.scanning.store(false, Ordering::SeqCst);
        return fail("E_BAD_PATH", &format!("目录不存在：{}", root.display()));
    }

    let db_path = state.db_path.clone();
    let artwork_dir = state.artwork_dir.clone();
    let handle = app.clone();

    let result = tauri::async_runtime::spawn_blocking(move || {
        let mut conn = match aiya_data::open_conn(&db_path) {
            Ok(c) => c,
            Err(e) => return fail("E_INTERNAL", &e.to_string()),
        };
        let app2 = handle.clone();
        let scan_result = scanner::scan_directory(&mut conn, &root, Some(&artwork_dir), move |p| {
            if let Ok(v) = serde_json::to_value(&p) {
                let _ = app2.emit("scan:progress", v);
            }
        });
        let _ = conn.close();
        match scan_result {
            Ok(r) => serde_json::to_value(r).map(ok).unwrap_or_else(|e| fail("E_INTERNAL", &e.to_string())),
            Err(e) => fail("E_INTERNAL", &e.to_string()),
        }
    })
    .await
    .unwrap_or_else(|e| fail("E_INTERNAL", &e.to_string()));

    state.scanning.store(false, Ordering::SeqCst);
    result
}

#[tauri::command]
fn library_scan_cancel(state: State<'_, AppState>) -> Value {
    // v1 第一切：scan_directory 还没有取消点；返回 false 让 UI 显示"无法取消"
    // 而不是假装成功。取消能力随 P1-2 的 CancellationToken 补上。
    let _ = &state.scanning;
    ok(json!({ "cancelled": false }))
}

/// 选目录并持久化为 `library.root`（UI 的 startScan 不带 path，扫描时从这里取）。
#[tauri::command]
fn library_choose_root(app: AppHandle, state: State<'_, AppState>) -> Value {
    use tauri_plugin_dialog::DialogExt;
    let picked = app
        .dialog()
        .file()
        .blocking_pick_folder()
        .and_then(|p| p.as_path().map(|pp| pp.to_path_buf()));
    match picked {
        Some(p) => {
            let path_str = p.to_string_lossy().into_owned();
            let to_save = path_str.clone();
            let saved: aiya_data::DbResult<()> = state.db().send(move |conn| {
                setting_set(conn, "library.root", &json!(to_save)).map_err(aiya_data::DbError::Sql)
            });
            if saved.is_err() {
                return fail("E_INTERNAL", "无法保存曲库目录");
            }
            ok(json!({ "path": path_str }))
        }
        None => ok(json!(null)),
    }
}

#[tauri::command]
fn library_reveal_track(state: State<'_, AppState>, track_id: String) -> Value {
    let result: aiya_data::DbResult<Option<String>> = state.db().send(move |conn| {
        conn.query_row("SELECT path FROM tracks WHERE id = ?1", [track_id], |r| r.get(0))
            .map(Some)
            .or_else(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => Ok(None),
                other => Err(aiya_data::DbError::Sql(other)),
            })
    });
    match result {
        Ok(Some(path)) => {
            let revealed = std::process::Command::new("explorer")
                .arg(format!("/select,{path}"))
                .spawn()
                .is_ok();
            ok(json!({
                "revealed": revealed,
                "reason": if revealed { Value::Null } else { json!("explorer 启动失败") },
            }))
        }
        Ok(None) => ok(json!({ "revealed": false, "reason": "曲目不在库中" })),
        Err(e) => fail("E_INTERNAL", &e.to_string()),
    }
}

/// 批量选择 → ID。indices 为字符串 "all" 或下标数组。
#[tauri::command]
fn library_ids_for_selection(state: State<'_, AppState>, request: Value) -> Value {
    let query: TrackWindowQuery =
        match serde_json::from_value(request.get("query").cloned().unwrap_or(json!({}))) {
            Ok(q) => q,
            Err(e) => return fail("E_BAD_QUERY", &e.to_string()),
        };
    let select_all = request.get("indices").map(|i| i == "all").unwrap_or(false);
    let indices: Vec<usize> = request
        .get("indices")
        .and_then(|i| i.as_array())
        .map(|a| a.iter().filter_map(|v| v.as_u64().map(|n| n as usize)).collect())
        .unwrap_or_default();

    let result: aiya_data::DbResult<Value> = state.db().send(move |conn| {
        let mut q = query;
        q.limit = 500;
        let mut ids_out: Vec<String> = Vec::new();
        loop {
            let page = repo::list_tracks(conn, &q)?;
            let fetched = page.items.len();
            ids_out.extend(page.items.iter().map(|t| t.id.clone()));
            match page.next_cursor {
                Some(c) if fetched > 0 => q.cursor = Some(c),
                _ => break,
            }
        }
        let final_ids: Vec<String> = if select_all {
            ids_out
        } else {
            indices.into_iter().filter_map(|i| ids_out.get(i).cloned()).collect()
        };
        serde_json::to_value(final_ids).map_err(|e| aiya_data::DbError::Custom(e.to_string()))
    });
    result.map(ok).unwrap_or_else(|e| fail("E_INTERNAL", &e.to_string()))
}

#[tauri::command]
fn library_remove_tracks(state: State<'_, AppState>, track_ids: Vec<String>) -> Value {
    let result: aiya_data::DbResult<Value> = state.db().send(move |conn| {
        let mut removed = 0i64;
        for id in &track_ids {
            removed += conn.execute("DELETE FROM tracks WHERE id = ?1", [id]).unwrap_or(0) as i64;
        }
        Ok(json!({ "removed": removed }))
    });
    result.map(ok).unwrap_or_else(|e| fail("E_INTERNAL", &e.to_string()))
}

/* ---------------- playlist 命令 ---------------- */

fn playlist_summary(
    conn: &rusqlite::Connection,
    id: &str,
    name: &str,
    created: i64,
    updated: i64,
) -> rusqlite::Result<Value> {
    let (track_count, duration, artwork): (i64, i64, Option<String>) = conn.query_row(
        "SELECT
           (SELECT COUNT(*) FROM playlist_items pi JOIN tracks t ON t.id = pi.track_id
             WHERE pi.playlist_id = ?1),
           (SELECT COALESCE(SUM(t.duration_ms),0) FROM playlist_items pi JOIN tracks t ON t.id = pi.track_id
             WHERE pi.playlist_id = ?1),
           (SELECT t2.artwork_id FROM playlist_items pi2 JOIN tracks t2 ON t2.id = pi2.track_id
             WHERE pi2.playlist_id = ?1 ORDER BY pi2.position LIMIT 1)",
        rusqlite::params![id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    Ok(json!({
        "id": id, "name": name, "description": Value::Null,
        "trackCount": track_count, "durationMs": duration,
        "artworkId": artwork, "createdAt": created, "updatedAt": updated,
        "sortPosition": created,
    }))
}

#[tauri::command]
fn playlist_list(state: State<'_, AppState>) -> Value {
    let result: aiya_data::DbResult<Value> = state.db().send(|conn| {
        let mut stmt = conn.prepare(
            "SELECT id, name, created_at, updated_at FROM playlists ORDER BY created_at ASC",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, i64>(2)?,
                r.get::<_, i64>(3)?,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (id, name, created, updated) = row?;
            out.push(playlist_summary(conn, &id, &name, created, updated)?);
        }
        serde_json::to_value(out).map_err(|e| aiya_data::DbError::Custom(e.to_string()))
    });
    result.map(ok).unwrap_or_else(|e| fail("E_INTERNAL", &e.to_string()))
}

#[tauri::command]
fn playlist_detail(state: State<'_, AppState>, id: String) -> Value {
    let result: aiya_data::DbResult<Value> = state.db().send(move |conn| {
        let row = conn.query_row(
            "SELECT id, name, created_at, updated_at FROM playlists WHERE id = ?1",
            [id],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, i64>(3)?,
                ))
            },
        );
        match row {
            Ok((id, name, created, updated)) => {
                playlist_summary(conn, &id, &name, created, updated).map_err(aiya_data::DbError::Sql)
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(Value::Null),
            Err(e) => Err(aiya_data::DbError::Sql(e)),
        }
    });
    result.map(ok).unwrap_or_else(|e| fail("E_INTERNAL", &e.to_string()))
}

#[tauri::command]
fn playlist_create(state: State<'_, AppState>, name: String, track_ids: Option<Vec<String>>) -> Value {
    let name = name.trim().to_string();
    if name.is_empty() {
        return fail("E_BAD_NAME", "歌单名不能为空");
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    let pid = ids::playlist_id(&name);
    let track_ids = track_ids.unwrap_or_default();
    let count = track_ids.len();
    let name_for_db = name.clone();
    let pid_for_db = pid.clone();

    let result: aiya_data::DbResult<()> = state.db().send(move |conn| {
        let tx = conn.transaction().map_err(aiya_data::DbError::Sql)?;
        tx.execute(
            "INSERT INTO playlists (id, name, name_norm, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?4)
             ON CONFLICT(id) DO NOTHING",
            rusqlite::params![pid_for_db, name_for_db, aiya_data::norm::norm(&name_for_db), now],
        )
        .map_err(aiya_data::DbError::Sql)?;
        for (i, tid) in track_ids.iter().enumerate() {
            tx.execute(
                "INSERT OR IGNORE INTO playlist_items (id, playlist_id, track_id, position, added_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![ids::playlist_item_id(&pid_for_db, tid, i as i64), pid_for_db, tid, i as i64, now],
            )
            .map_err(aiya_data::DbError::Sql)?;
        }
        tx.commit().map_err(aiya_data::DbError::Sql)
    });
    match result {
        Ok(()) => ok(json!({
            "id": pid, "name": name, "description": Value::Null,
            "trackCount": count, "durationMs": 0,
            "artworkId": Value::Null,
            "createdAt": now, "updatedAt": now, "sortPosition": now,
        })),
        Err(e) => fail("E_INTERNAL", &e.to_string()),
    }
}

/* ---------------- player / lyrics / history（P2/P4 接管，先给合法桩） ---------------- */

fn idle_player_state() -> Value {
    json!({
        "queueLength": 0, "queueVersion": 0, "index": -1, "cursor": -1,
        "repeat": "off", "shuffle": false, "status": "idle",
        "positionMs": 0, "durationMs": Value::Null,
        "volume": 0.85, "muted": false, "rate": 1.0,
        "error": Value::Null, "current": Value::Null,
        "token": 0, "updatedAt": 0,
    })
}

#[tauri::command]
fn player_state() -> Value {
    ok(idle_player_state())
}

#[tauri::command]
fn player_queue() -> Value {
    ok(json!({ "ids": [], "index": -1, "version": 0 }))
}

#[tauri::command]
fn lyrics_get(_track_id: String) -> Value {
    fail("E_NOT_FOUND", "歌词功能在 P4 提供")
}

#[tauri::command]
fn history_stats(state: State<'_, AppState>) -> Value {
    let result: aiya_data::DbResult<Value> = state.db().send(|conn| {
        let count = |sql: &str| -> i64 { conn.query_row(sql, [], |r| r.get(0)).unwrap_or(0) };
        Ok(json!({
            "totalPlays": count("SELECT COUNT(*) FROM history"),
            "totalMs": 0,
            "completed": 0,
            "skipped": 0,
            "firstPlayAt": conn.query_row("SELECT MIN(played_at) FROM history", [], |r| r.get::<_, Option<i64>>(0)).unwrap_or(None),
            "lastPlayAt": conn.query_row("SELECT MAX(played_at) FROM history", [], |r| r.get::<_, Option<i64>>(0)).unwrap_or(None),
            "topTracks": [],
        }))
    });
    result.map(ok).unwrap_or_else(|e| fail("E_INTERNAL", &e.to_string()))
}

#[tauri::command]
fn history_clear(state: State<'_, AppState>) -> Value {
    let result: aiya_data::DbResult<()> = state.db().send(|conn| {
        conn.execute("DELETE FROM history", [])
            .map(|_| ())
            .map_err(aiya_data::DbError::Sql)
    });
    result
        .map(|_| ok(json!({ "cleared": true })))
        .unwrap_or_else(|e| fail("E_INTERNAL", &e.to_string()))
}

/* ---------------- aiya-artwork 协议 ---------------- */

/// 处理 `http://aiya-artwork.localhost/artwork/<id>?size=N`。
/// 流程：查 artworks 表 → 读原图 →（需要且可缩放时）生成缩略图缓存 → 响应。
/// 失败一律 404（破图不该让前端 fetch 走重试）。
fn artwork_protocol(app: &AppHandle, request: tauri::http::Request<Vec<u8>>) -> tauri::http::Response<Cow<'static, [u8]>> {
    use tauri::http::header::{ACCESS_CONTROL_ALLOW_ORIGIN, CACHE_CONTROL, CONTENT_TYPE};

    fn not_found() -> tauri::http::Response<Cow<'static, [u8]>> {
        tauri::http::Response::builder().status(404).body(Cow::Borrowed(&b""[..])).unwrap()
    }

    // path: /artwork/<id>；先提取成 String，避免借 request 的生命周期
    let path = request.uri().path().to_string();
    let artwork_id = path.strip_prefix("/artwork/").unwrap_or("").to_string();
    if artwork_id.is_empty() || artwork_id.contains('/') || artwork_id.contains("..") {
        return not_found();
    }
    let size: u32 = request
        .uri()
        .query()
        .and_then(|q| {
            q.split('&').find_map(|kv| kv.strip_prefix("size="))?.parse().ok()
        })
        .unwrap_or(0);

    let state = app.state::<AppState>();

    // 1) 查记录（DB 线程）
    let found = state.db().send(move |conn| {
        Ok(aiya_data::find_artwork(conn, artwork_id.as_str())?)
    });
    let Some((cache_key, mime)) = found.ok().flatten() else {
        return not_found();
    };

    // 2) 读原图（cache_key 只含 hash+ext，但仍然 basename 防御）
    let original = state.artwork_dir.join("originals").join(
        std::path::Path::new(&cache_key).file_name().unwrap_or_default(),
    );
    let Ok(bytes) = std::fs::read(&original) else {
        return not_found();
    };

    // 3) 缩略图：size 指定且 ≤512 才缩（防滥用大尺寸请求），否则原图直出
    let mut thumb_used = false;
    let data: Cow<'static, [u8]> = if (1..=512).contains(&size) && mime.starts_with("image/") {
        let thumb = state
            .artwork_dir
            .join("thumbs")
            .join(format!("{}_{}.jpg", cache_key.split('.').next().unwrap_or("x"), size));
        if let Ok(cached) = std::fs::read(&thumb) {
            thumb_used = true;
            Cow::Owned(cached)
        } else if let Some(resized) = make_thumb(&bytes, size) {
            // 原子写缓存；失败不影响本次响应
            let tmp = thumb.with_extension(format!("tmp.{}", std::process::id()));
            if std::fs::write(&tmp, &resized).is_ok() {
                let _ = std::fs::rename(&tmp, &thumb);
            }
            thumb_used = true;
            Cow::Owned(resized)
        } else {
            Cow::Owned(bytes)
        }
    } else {
        Cow::Owned(bytes)
    };
    let out_mime = if thumb_used { "image/jpeg" } else { &mime };

    tauri::http::Response::builder()
        .status(200)
        .header(CONTENT_TYPE, out_mime)
        .header(ACCESS_CONTROL_ALLOW_ORIGIN, "*")
        .header(CACHE_CONTROL, "public, max-age=31536000, immutable")
        .body(data)
        .unwrap()
}

/// 居中裁方 + 缩放 + JPEG。输入坏图/超小图返回 None（调用方回退原图）。
fn make_thumb(bytes: &[u8], size: u32) -> Option<Vec<u8>> {
    use image::imageops::FilterType;
    let img = image::load_from_memory(bytes).ok()?;
    let thumb = img.resize_to_fill(size, size, FilterType::Lanczos3);
    let mut out = std::io::Cursor::new(Vec::with_capacity(32 * 1024));
    thumb
        .write_to(&mut out, image::ImageFormat::Jpeg)
        .ok()?;
    Some(out.into_inner())
}

/* ---------------- 装配 ---------------- */

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .register_uri_scheme_protocol(aiya_data::ARTWORK_SCHEME, |ctx, request| {
            artwork_protocol(ctx.app_handle(), request)
        })
        .setup(|app| {
            let db_path = default_db_path(&app.handle());
            let db = aiya_data::open(&db_path)
                .map_err(|e| -> Box<dyn std::error::Error> { e.to_string().into() })?;
            let artwork_dir = default_artwork_dir(&app.handle());
            let _ = std::fs::create_dir_all(artwork_dir.join("originals"));
            let _ = std::fs::create_dir_all(artwork_dir.join("thumbs"));
            app.manage(AppState {
                db: Mutex::new(db),
                db_path,
                artwork_dir,
                scanning: AtomicBool::new(false),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            app_info,
            app_get_ui_state,
            app_set_ui_state,
            app_get_settings,
            app_set_settings,
            app_diagnostics,
            app_open_data_dir,
            library_list,
            library_tracks,
            library_stats,
            library_scan,
            library_scan_cancel,
            library_choose_root,
            library_reveal_track,
            library_ids_for_selection,
            library_remove_tracks,
            playlist_list,
            playlist_detail,
            playlist_create,
            player_state,
            player_queue,
            lyrics_get,
            history_stats,
            history_clear,
        ])
        .run(tauri::generate_context!())
        .expect("AIYA: Tauri 运行失败");
}
