//! AIYA Music — Tauri 宿主壳。
//!
//! 边界约定（src-tauri/README.md）：命令注册、事件桥、协议注册；领域业务在 crates/。
//! 本文件把 `aiya-data` 的能力暴露为 IPC 命令，返回与 `@aiya/contracts`
//! 的 `IpcResult<T>` 同构 JSON —— UI 的 `result.ok` 分支零改动。

use std::borrow::Cow;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager, State};

use aiya_data::ids;
use aiya_data::repo::{self, TrackWindowQuery};
use aiya_data::scanner;

mod beets;
mod sidecar;
mod smtc;

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
    /// 播放状态机（ADR-0003：唯一真相）
    player: Mutex<aiya_player::PlayerActor>,
    /// 音频 sidecar（ADR-0002：独立进程 + 命名管道，崩溃隔离）
    engine: Mutex<sidecar::SidecarHandle>,
    /// 扫描互斥标志；Arc 是为了 beets apply 完成后的 reader 线程也能取到
    scanning: Arc<AtomicBool>,
    /// 队列状态行上次落库时间（毫秒，事件泵 5s 节流用）
    last_persist_ms: std::sync::atomic::AtomicU64,
    /// SMTC 通道（smtc::spawn 成功才有）；emit_player_state 处推送
    smtc_tx: Mutex<Option<std::sync::mpsc::Sender<smtc::SmtcUpdate>>>,
    /// 上次推给 SMTC 的曲目 id（换曲才重发 metadata + 查封面）
    smtc_track_id: Mutex<String>,
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

/// 频谱 tap 订阅开关（docs/04 §5.1：订阅制，关闭时引擎内零开销）。
/// UI 频谱面板打开/关闭时调用。
#[tauri::command]
fn spectrum_set(state: State<'_, AppState>, enabled: bool) -> Value {
    engine_send(&state, aiya_audio::EngineCommand::SetSpectrum(enabled));
    ok(json!({ "enabled": enabled }))
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

/// beets 环境状态（设置页诊断区）。低频查询，每次现查（探测 + 一次 status 往返）。
#[tauri::command]
fn beets_status(app: AppHandle) -> Value {
    match serde_json::to_value(beets::probe(&app)) {
        Ok(v) => ok(v),
        Err(e) => fail("E_INTERNAL", &e.to_string()),
    }
}

/// 启动一次只读预览（B1）：事件流走 `beets:import`。
/// 未装 beets → E_BEETS_NOT_FOUND（UI 显示指引）。
#[tauri::command]
fn beets_preview(app: AppHandle, paths: Vec<String>) -> Value {
    match beets::start_preview(app, paths) {
        Ok(()) => ok(json!({ "started": true })),
        Err(e) => fail("E_BEETS_NOT_FOUND", &e),
    }
}

/// B2：应用导入决策（写标签 + 入 beets 库，不移动文件）。
/// decisions 形状见契约 ApplyDecision；完成后自动 beets 对齐读模型
/// （自闭环：进度/汇总走 beets:import 的 librarySynced）。
#[tauri::command]
fn beets_apply(app: AppHandle, state: State<'_, AppState>, decisions: Vec<Value>) -> Value {
    let refresh = beets::RefreshParams {
        db_path: state.db_path.clone(),
        scanning: state.scanning.clone(),
    };
    match beets::start_apply(app, decisions, Some(refresh)) {
        Ok(()) => ok(json!({ "started": true })),
        Err(e) => fail("E_BEETS_NOT_FOUND", &e),
    }
}

/// 取消当前 beets 导入（preview/apply）：sidecar 在下个检查点收尾，
/// 结果走 `cancelled` 事件（UI 据此解锁按钮）。
#[tauri::command]
fn beets_cancel() -> Value {
    ok(json!({ "requested": beets::cancel_session() }))
}

/// 手动触发一次 beets → 自有读模型的全量对齐（异步，结果走 librarySynced 事件）。
#[tauri::command]
fn beets_sync(app: AppHandle, state: State<'_, AppState>) -> Value {
    beets::sync_now(
        app,
        beets::RefreshParams {
            db_path: state.db_path.clone(),
            scanning: state.scanning.clone(),
        },
    );
    ok(json!({ "started": true }))
}

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

/* ---------------- player：PlayerActor + 引擎适配（P2 真实现） ---------------- */

/// 把引擎/Actor 的状态推给 UI。所有状态变化都经此出口。
fn emit_player_state(app: &AppHandle, state: &aiya_player::PlayerState) {
    if let Ok(v) = serde_json::to_value(state) {
        let _ = app.emit("player:state", v);
    }
    smtc_notify(app, state);
}

/// 快照 → SMTC 更新。换曲（track id 变化）才重发 metadata（含封面 DB 查询）；
/// 播放态/进度每次都推（SMTC 端自己合并）。
fn smtc_notify(app: &AppHandle, state: &aiya_player::PlayerState) {
    let Some(app_state) = app.try_state::<AppState>() else {
        return;
    };
    let Some(tx) = app_state.smtc_tx.lock().expect("smtc_tx poisoned").clone() else {
        return;
    };
    let playing = state.status == aiya_player::PlaybackStatus::Playing;
    let _ = tx.send(smtc::SmtcUpdate::Progress {
        playing,
        position_ms: state.position_ms,
    });

    let track_id = state.current.as_ref().map(|c| c.id.clone()).unwrap_or_default();
    let mut last = app_state.smtc_track_id.lock().expect("smtc_track_id poisoned");
    if *last == track_id {
        return;
    }
    *last = track_id;
    drop(last);

    let Some(current) = &state.current else {
        // 队列清空 → SMTC 回 Stopped（推空 Track 由线程侧按 Stopped 处理不了，
        // 直接发 Progress(playing=false) 即可，metadata 保留最后一曲无妨）
        return;
    };
    // 封面：artwork_id → cache_key → originals 文件
    let cover_path = current.artwork_id.as_ref().and_then(|aid| {
        let cache_key: Option<String> = app_state
            .db()
            .send({
                let aid = aid.clone();
                move |conn| {
                    Ok(conn
                        .query_row(
                            "SELECT cache_key FROM artworks WHERE id = ?1",
                            [&aid],
                            |r| r.get(0),
                        )
                        .ok())
                }
            })
            .unwrap_or(None);
        cache_key.map(|k| app_state.artwork_dir.join("originals").join(k))
    });
    let _ = tx.send(smtc::SmtcUpdate::Track {
        title: current.title.clone(),
        artist: current.artist.clone(),
        album: current.album.clone(),
        duration_ms: state.duration_ms,
        cover_path,
    });
}

/* ---------------- 队列持久化 ---------------- */

/// 从 Actor 快照持久化状态行（节流落库 / 退出 flush 用）。
fn queue_persist_from_actor(actor: &aiya_player::PlayerActor) -> aiya_data::QueuePersist {
    let s = actor.state();
    aiya_data::QueuePersist {
        current_index: s.index,
        position_ms: s.position_ms as i64,
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

/// 全量落库（queue_items 重写 + 状态行）：切歌/清空时调用。
fn persist_queue_full(state: &AppState, actor: &aiya_player::PlayerActor) {
    let p = queue_persist_from_actor(actor);
    let ids = actor.queue_ids();
    let result: aiya_data::DbResult<()> = state
        .db()
        .send(move |conn| aiya_data::save_queue_full(conn, &p, &ids));
    if let Err(e) = result {
        tracing::warn!(error = %e, "queue full persist failed");
    }
}

/// 只落状态行（position/音量等，5s 节流）。
fn persist_queue_state_handle(state: &AppState, actor: &aiya_player::PlayerActor) {
    let p = queue_persist_from_actor(actor);
    let result: aiya_data::DbResult<()> =
        state.db().send(move |conn| aiya_data::save_queue_state(conn, &p));
    if let Err(e) = result {
        tracing::warn!(error = %e, "queue state persist failed");
    }
}

/// Actor 归约后统一收尾：取 pending load → 发引擎；同步音量；推状态。
/// 返回新快照（命令返回值用）。
fn drive_and_emit(state: &State<'_, AppState>, app: &AppHandle) -> Value {
    let mut actor = state.player.lock().expect("player poisoned");
    // 1) 引擎装载指令
    if let Some((token, path, start_ms, autoplay)) = actor.take_pending_load() {
        let volume = actor.volume().0 as f32;
        let muted = actor_is_muted(&actor);
        engine_send(&state, aiya_audio::EngineCommand::Load(path));
        engine_send(&state, aiya_audio::EngineCommand::SetVolume(if muted { 0.0 } else { volume }));
        if !autoplay {
            engine_send(&state, aiya_audio::EngineCommand::Pause);
        }
        let _ = (token, start_ms);
        // 切歌立即全量落库（队列结构可能变了；exit 时不必依赖节流尾巴）
        persist_queue_full(state, &actor);
    }
    let snapshot = actor.state();
    drop(actor);
    emit_player_state(app, &snapshot);
    ok(serde_json::to_value(&snapshot).unwrap_or(Value::Null))
}

fn actor_is_muted(actor: &aiya_player::PlayerActor) -> bool {
    actor.volume().1
}

/// 显示并聚焦主窗口（托盘左键 / 菜单共用）。
fn show_main_window(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

/// 引擎命令统一入口（Engine 在 Mutex 里）。&State 自动 deref 成 &AppState。
fn engine_send(state: &AppState, cmd: aiya_audio::EngineCommand) {
    let _ = state.engine.lock().expect("engine poisoned").send(cmd);
}

/// 从 DB 拿 TrackRef 列表（trackIds 保序）。
fn fetch_track_refs(state: &State<'_, AppState>, ids: &[String]) -> Vec<aiya_player::TrackRef> {
    let ids = ids.to_vec();
    state
        .db()
        .send(move |conn| {
            // 音频规格列一并带出：PlayerState.current 是播放条唯一数据源，
            // 徽章/规格/Hi-Res 判定都靠 current.audio（contracts Track.audio）
            let mut stmt = conn.prepare(
                "SELECT t.id, t.title, ar.name, al.title, t.duration_ms, t.artwork_id, t.gain_db, t.path,
                        t.codec, t.container, t.sample_rate, t.bits_per_sample, t.bitrate, t.channels
                 FROM tracks t
                 LEFT JOIN artists ar ON ar.id = t.artist_id
                 LEFT JOIN albums al ON al.id = t.album_id
                 WHERE t.id = ?1",
            )?;
            let mut out = Vec::with_capacity(ids.len());
            for id in &ids {
                let row = stmt.query_row([id], |r| {
                    let codec: Option<String> = r.get(8)?;
                    let container: Option<String> = r.get(9)?;
                    let audio = if codec.is_some() || container.is_some() {
                        Some(aiya_player::TrackAudio {
                            codec,
                            container,
                            sample_rate: r.get(10)?,
                            bits_per_sample: r.get(11)?,
                            bitrate: r.get(12)?,
                            channels: r.get(13)?,
                        })
                    } else {
                        None
                    };
                    Ok(aiya_player::TrackRef {
                        id: r.get(0)?,
                        title: r.get(1)?,
                        artist: r.get(2)?,
                        album: r.get(3)?,
                        duration_ms: r.get(4)?,
                        artwork_id: r.get(5)?,
                        gain_db: r.get(6)?,
                        audio,
                        path: r.get(7)?,
                    })
                });
                if let Ok(t) = row {
                    out.push(t);
                }
            }
            Ok(out)
        })
        .unwrap_or_default()
}

#[tauri::command]
fn player_state(state: State<'_, AppState>) -> Value {
    let actor = state.player.lock().expect("player poisoned");
    ok(serde_json::to_value(actor.state()).unwrap_or(Value::Null))
}

#[tauri::command]
fn player_queue(state: State<'_, AppState>) -> Value {
    let actor = state.player.lock().expect("player poisoned");
    ok(serde_json::to_value(actor.queue_snapshot()).unwrap_or(Value::Null))
}

#[tauri::command]
fn player_play_tracks(
    app: AppHandle,
    state: State<'_, AppState>,
    track_ids: Vec<String>,
    start_index: Option<usize>,
) -> Value {
    let tracks = fetch_track_refs(&state, &track_ids);
    if tracks.is_empty() {
        return fail("E_NOT_FOUND", "没有可播放的曲目");
    }
    let start = start_index.unwrap_or(0);
    let mut actor = state.player.lock().expect("player poisoned");
    actor.reduce(aiya_player::PlayerCommand::Queue(aiya_player::QueueOp::ReplaceAll {
        tracks,
        start,
    }));
    drop(actor);
    drive_and_emit(&state, &app)
}

/// 按 TrackWindowQuery 语义播放查询结果（UI 的"播放全部/播放专辑"）。
#[tauri::command]
fn player_play_query(
    app: AppHandle,
    state: State<'_, AppState>,
    query: Value,
    start_index: Option<usize>,
) -> Value {
    let q: aiya_data::TrackWindowQuery = match serde_json::from_value(query) {
        Ok(q) => q,
        Err(e) => return fail("E_INVALID", &format!("query 无效：{e}")),
    };
    // 拉全量 id（source 过滤 + 排序一致；limit 放大到 10000 —— 播放全部场景）
    let q = aiya_data::TrackWindowQuery { limit: 10_000, offset: None, ..q };
    let ids: Vec<String> = state
        .db()
        .send(move |conn| {
            let page = aiya_data::repo::list_tracks(conn, &q)?;
            Ok(page.items.into_iter().map(|t| t.id).collect())
        })
        .unwrap_or_default();
    if ids.is_empty() {
        return fail("E_NOT_FOUND", "查询结果为空");
    }
    let tracks = fetch_track_refs(&state, &ids);
    let start = start_index.unwrap_or(0);
    let mut actor = state.player.lock().expect("player poisoned");
    actor.reduce(aiya_player::PlayerCommand::Queue(aiya_player::QueueOp::ReplaceAll {
        tracks,
        start,
    }));
    drop(actor);
    drive_and_emit(&state, &app)
}

#[tauri::command]
/// 播放/暂停切换的内部实现（tauri command 与 SMTC 回调共用）。
fn cmd_toggle(state: &AppState, app: &AppHandle) -> Value {
    let status_before = state.player.lock().expect("player poisoned").state().status;
    use aiya_player::PlaybackStatus as PS;
    match status_before {
        PS::Playing => engine_send(state, aiya_audio::EngineCommand::Pause),
        PS::Paused => engine_send(state, aiya_audio::EngineCommand::Resume),
        _ => {
            // Idle/Error：从当前位置重播（Actor 决定 load）
        }
    }
    {
        let mut actor = state.player.lock().expect("player poisoned");
        actor.reduce(aiya_player::PlayerCommand::TogglePause);
    }
    drive_and_emit_inner(state, app)
}

#[tauri::command]
fn player_toggle(app: AppHandle, state: State<'_, AppState>) -> Value {
    cmd_toggle(&state, &app)
}

/// Seek 的内部实现（tauri command 与 SMTC 回调共用）。
fn cmd_seek(state: &AppState, app: &AppHandle, position_ms: u64) -> Value {
    engine_send(state, aiya_audio::EngineCommand::Seek(position_ms));
    {
        let mut actor = state.player.lock().expect("player poisoned");
        actor.reduce(aiya_player::PlayerCommand::Seek(position_ms));
    }
    drive_and_emit_inner(state, app)
}

/// drive_and_emit 的 AppState 版本（SMTC 回调拿不到 State<'_>，语义相同）。
fn drive_and_emit_inner(state: &AppState, app: &AppHandle) -> Value {
    let mut actor = state.player.lock().expect("player poisoned");
    if let Some((token, path, start_ms, autoplay)) = actor.take_pending_load() {
        let volume = actor.volume().0 as f32;
        let muted = actor.volume().1;
        engine_send(state, aiya_audio::EngineCommand::Load(path));
        engine_send(
            state,
            aiya_audio::EngineCommand::SetVolume(if muted { 0.0 } else { volume }),
        );
        if !autoplay {
            engine_send(state, aiya_audio::EngineCommand::Pause);
        }
        let _ = (token, start_ms);
        persist_queue_full(state, &actor);
    }
    let snapshot = actor.state();
    drop(actor);
    emit_player_state(app, &snapshot);
    ok(serde_json::to_value(&snapshot).unwrap_or(Value::Null))
}

#[tauri::command]
fn player_next(app: AppHandle, state: State<'_, AppState>) -> Value {
    {
        let mut actor = state.player.lock().expect("player poisoned");
        actor.reduce(aiya_player::PlayerCommand::Next);
    }
    drive_and_emit(&state, &app)
}

#[tauri::command]
fn player_prev(app: AppHandle, state: State<'_, AppState>) -> Value {
    {
        let mut actor = state.player.lock().expect("player poisoned");
        actor.reduce(aiya_player::PlayerCommand::Prev);
    }
    drive_and_emit(&state, &app)
}

#[tauri::command]
fn player_seek(app: AppHandle, state: State<'_, AppState>, position_ms: u64) -> Value {
    cmd_seek(&state, &app, position_ms)
}

#[tauri::command]
fn player_set_volume(app: AppHandle, state: State<'_, AppState>, volume: f64) -> Value {
    {
        let mut actor = state.player.lock().expect("player poisoned");
        actor.reduce(aiya_player::PlayerCommand::SetVolume { volume, muted: false });
        let v = actor.volume();
        engine_send(
            &state,
            aiya_audio::EngineCommand::SetVolume(if v.1 { 0.0 } else { v.0 as f32 }));
    }
    drive_and_emit(&state, &app)
}

#[tauri::command]
fn player_toggle_mute(app: AppHandle, state: State<'_, AppState>) -> Value {
    {
        let mut actor = state.player.lock().expect("player poisoned");
        let (v, muted) = actor.volume();
        actor.reduce(aiya_player::PlayerCommand::SetVolume { volume: v, muted: !muted });
        engine_send(
            &state,
            aiya_audio::EngineCommand::SetVolume(if !muted { 0.0 } else { v as f32 }));
    }
    drive_and_emit(&state, &app)
}

#[tauri::command]
fn player_set_rate(app: AppHandle, state: State<'_, AppState>, rate: f64) -> Value {
    // 引擎侧线性插值变速（0.5–2.0，v1 变速变调；WSOLA 不变调后补）
    {
        let mut actor = state.player.lock().expect("player poisoned");
        actor.reduce(aiya_player::PlayerCommand::SetRate(rate));
        engine_send(&state, aiya_audio::EngineCommand::SetRate(actor.state().rate as f32));
    }
    drive_and_emit(&state, &app)
}

#[tauri::command]
fn player_set_repeat(app: AppHandle, state: State<'_, AppState>, repeat: String) -> Value {
    let mode = match repeat.as_str() {
        "all" => aiya_player::RepeatMode::All,
        "one" => aiya_player::RepeatMode::One,
        _ => aiya_player::RepeatMode::Off,
    };
    {
        let mut actor = state.player.lock().expect("player poisoned");
        actor.reduce(aiya_player::PlayerCommand::SetRepeat(mode));
    }
    drive_and_emit(&state, &app)
}

#[tauri::command]
fn player_set_shuffle(app: AppHandle, state: State<'_, AppState>, shuffle: bool) -> Value {
    {
        let mut actor = state.player.lock().expect("player poisoned");
        let cur = actor.state().shuffle;
        if cur != shuffle {
            actor.reduce(aiya_player::PlayerCommand::ToggleShuffle);
        }
    }
    drive_and_emit(&state, &app)
}

#[tauri::command]
fn player_enqueue(
    app: AppHandle,
    state: State<'_, AppState>,
    track_ids: Vec<String>,
    options: Option<Value>,
) -> Value {
    let tracks = fetch_track_refs(&state, &track_ids);
    if tracks.is_empty() {
        return fail("E_NOT_FOUND", "没有可播放的曲目");
    }
    // options: { mode: "next" | "end" }（contracts QueueInsertMode）
    let mode = options
        .as_ref()
        .and_then(|o| o.get("mode"))
        .and_then(|m| m.as_str())
        .unwrap_or("end")
        .to_string();
    {
        let mut actor = state.player.lock().expect("player poisoned");
        actor.reduce(aiya_player::PlayerCommand::Queue(if mode == "next" {
            aiya_player::QueueOp::PlayNext { tracks }
        } else {
            aiya_player::QueueOp::Append { tracks }
        }));
    }
    drive_and_emit(&state, &app)
}

#[tauri::command]
fn player_insert_next(app: AppHandle, state: State<'_, AppState>, track_id: String) -> Value {
    let tracks = fetch_track_refs(&state, &[track_id]);
    if tracks.is_empty() {
        return fail("E_NOT_FOUND", "曲目不存在");
    }
    {
        let mut actor = state.player.lock().expect("player poisoned");
        actor.reduce(aiya_player::PlayerCommand::Queue(aiya_player::QueueOp::PlayNext { tracks }));
    }
    drive_and_emit(&state, &app)
}

#[tauri::command]
fn player_remove_at(app: AppHandle, state: State<'_, AppState>, index: usize) -> Value {
    {
        let mut actor = state.player.lock().expect("player poisoned");
        actor.reduce(aiya_player::PlayerCommand::Queue(aiya_player::QueueOp::Remove {
            indices: vec![index],
        }));
    }
    drive_and_emit(&state, &app)
}

#[tauri::command]
fn player_move(_app: AppHandle, state: State<'_, AppState>, from: usize, to: usize) -> Value {
    let actor = state.player.lock().expect("player poisoned");
    let _ = (&from, &to);
    // 队列重排在 P2 后期（UI 拖拽）接入；先原样返回状态
    ok(serde_json::to_value(actor.state()).unwrap_or(Value::Null))
}

#[tauri::command]
fn player_clear(app: AppHandle, state: State<'_, AppState>) -> Value {
    engine_send(&state, aiya_audio::EngineCommand::Stop);
    {
        let mut actor = state.player.lock().expect("player poisoned");
        actor.reduce(aiya_player::PlayerCommand::Queue(aiya_player::QueueOp::ReplaceAll {
            tracks: vec![],
            start: 0,
        }));
    }
    drive_and_emit(&state, &app)
}

/// 旧 UI 的回报通道（<audio> 出口时代）；引擎在 Rust 后只剩诊断价值。
#[tauri::command]
fn player_report(state: State<'_, AppState>, report: Value) -> Value {
    let kind = report.get("kind").and_then(|k| k.as_str()).unwrap_or("").to_string();
    tracing::debug!(kind, "player_report (legacy)");
    let _ = &state;
    ok(json!({ "received": true }))
}

/// 歌词（P4）：同目录同名 .lrc → 解析。没找到回 found=false（UI 显示"暂无"，
/// 不当错误）。trackId 是自有库的 tracks.id。
#[tauri::command]
fn lyrics_get(state: State<'_, AppState>, track_id: String) -> Value {
    let path: Option<String> = state
        .db()
        .send(move |conn| {
            Ok(conn
                .query_row("SELECT path FROM tracks WHERE id = ?1", [&track_id], |r| r.get(0))
                .ok())
        })
        .unwrap_or(None);
    let Some(path) = path else {
        return ok(json!({
            "found": false, "lines": [], "offsetMs": 0, "synced": false, "encoding": null,
        }));
    };
    match aiya_data::lyrics::read_lyrics(std::path::Path::new(&path)) {
        Some(parsed) => ok(serde_json::to_value(&parsed).unwrap_or_else(|_| json!(null))),
        None => ok(json!({
            "found": false, "lines": [], "offsetMs": 0, "synced": false, "encoding": null,
        })),
    }
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
        // 单实例必须最先注册：二次启动唤起已有窗口（docs/05 §7 验收）
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            show_main_window(app);
        }))
        .plugin(tauri_plugin_dialog::init())
        // 关窗 = 隐藏到托盘（ADR：退出走托盘菜单，播放不中断）
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
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
            let engine = sidecar::SidecarHandle::launch()
                .map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
            app.manage(AppState {
                db: Mutex::new(db),
                db_path,
                artwork_dir,
                player: Mutex::new(aiya_player::PlayerActor::new()),
                engine: Mutex::new(engine),
                scanning: Arc::new(AtomicBool::new(false)),
                last_persist_ms: std::sync::atomic::AtomicU64::new(0),
                smtc_tx: Mutex::new(None),
                smtc_track_id: Mutex::new(String::new()),
            });

            // SMTC：主窗口 HWND（COM 对象要求）；失败只记日志，不影响播放
            if let Some(main) = app.get_webview_window("main") {
                if let Ok(hwnd) = main.hwnd() {
                    if let Some(tx) = smtc::spawn(app.handle().clone(), hwnd.0 as isize) {
                        if let Some(state) = app.try_state::<AppState>() {
                            *state.smtc_tx.lock().expect("smtc_tx poisoned") = Some(tx);
                        }
                    }
                }
            }

            // 启动恢复：queue_state + queue_items → Actor hydrate → 引擎装载（暂停）。
            // 恢复不自动播放；shuffle 开启时 Actor 内重新洗牌（排列不落库）。
            if let Some(state) = app.try_state::<AppState>() {
                let loaded: Result<
                    Option<(aiya_data::QueuePersist, Vec<String>)>,
                    aiya_data::DbError,
                > = state.db().send(|conn| aiya_data::load_queue(conn));
                if let Ok(Some((p, ids))) = loaded {
                    if !ids.is_empty() {
                        let tracks = fetch_track_refs(&state, &ids);
                        if !tracks.is_empty() {
                            let repeat = match p.repeat_mode.as_str() {
                                "all" => aiya_player::RepeatMode::All,
                                "one" => aiya_player::RepeatMode::One,
                                _ => aiya_player::RepeatMode::Off,
                            };
                            let current_id = ids.get(p.current_index.max(0) as usize).cloned();
                            let mut actor = state.player.lock().expect("player poisoned");
                            actor.hydrate(
                                tracks,
                                repeat,
                                p.shuffle,
                                p.volume,
                                p.muted,
                                p.rate,
                                p.position_ms.max(0) as u64,
                                current_id.as_deref(),
                            );
                            if let Some((_, path, _, autoplay)) = actor.take_pending_load() {
                                let muted = actor_is_muted(&actor);
                                engine_send(
                                    &state,
                                    aiya_audio::EngineCommand::Load(path),
                                );
                                engine_send(
                                    &state,
                                    aiya_audio::EngineCommand::SetVolume(if muted {
                                        0.0
                                    } else {
                                        p.volume as f32
                                    }),
                                );
                                if !autoplay {
                                    engine_send(&state, aiya_audio::EngineCommand::Pause);
                                }
                            }
                            let snapshot = actor.state();
                            drop(actor);
                            tracing::info!(
                                tracks = snapshot.queue_length,
                                position = snapshot.position_ms,
                                "player state restored"
                            );
                        }
                    }
                }
            }

            // 托盘：左键显示主窗口；菜单 [显示主窗口 / 退出]。
            // 关窗 = 隐藏到托盘（播放不中断）；真正退出走托盘菜单（退出前 flush 队列）。
            {
                use tauri::menu::{Menu, MenuItem};
                let show_item =
                    MenuItem::with_id(app, "show", "显示主窗口", true, None::<&str>)?;
                let quit_item = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
                let menu = Menu::with_items(app, &[&show_item, &quit_item])?;
                let mut tray = tauri::tray::TrayIconBuilder::with_id("aiya-tray")
                    .menu(&menu)
                    .show_menu_on_left_click(false)
                    .tooltip("AIYA Music")
                    .on_menu_event(|app, event| match event.id().as_ref() {
                        "show" => show_main_window(app),
                        "quit" => {
                            // 退出前把队列状态落库（含最终 position）
                            if let Some(state) = app.try_state::<AppState>() {
                                let actor = state.player.lock().expect("player poisoned");
                                if actor.state().queue_length > 0 {
                                    persist_queue_state_handle(&state, &actor);
                                }
                            }
                            app.exit(0);
                        }
                        _ => {}
                    })
                    .on_tray_icon_event(|tray, event| {
                        if let tauri::tray::TrayIconEvent::Click {
                            button: tauri::tray::MouseButton::Left,
                            button_state: tauri::tray::MouseButtonState::Up,
                            ..
                        } = event
                        {
                            show_main_window(tray.app_handle());
                        }
                    });
                if let Some(icon) = app.default_window_icon() {
                    tray = tray.icon(icon.clone());
                }
                tray.build(app)?;
            }

            // 引擎事件泵：EngineEvent → Actor 归约 → player:state 事件。
            // Position 节流到 500ms（渲染循环节拍是 ~10ms，UI 不需要那么密）。
            let pump = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                // 防休眠 per-thread 状态固定在本泵线程维护
                let mut prevent_sleep_last = false;
                loop {
                    tokio::time::sleep(std::time::Duration::from_millis(25)).await;
                    let Some(engine) = pump.try_state::<AppState>() else { continue };
                    // 播放中防系统睡眠（不管屏幕）；状态变化才调系统 API
                    let playing_now = {
                        let actor = engine.player.lock().expect("player poisoned");
                        actor.state().status == aiya_player::PlaybackStatus::Playing
                            && actor.state().current.is_some()
                    };
                    if playing_now != prevent_sleep_last {
                        smtc::set_prevent_sleep(playing_now);
                        prevent_sleep_last = playing_now;
                    }
                    let events: Vec<aiya_audio::EngineEvent> = {
                        let engine_guard = engine.engine.lock().expect("engine poisoned");
                        let mut out = Vec::new();
                        while let Some(ev) = engine_guard.try_recv() {
                            out.push(ev);
                        }
                        out
                    };
                    if events.is_empty() {
                        continue;
                    }
                    let mut should_emit = false;
                    {
                        let mut actor = engine.player.lock().expect("player poisoned");
                        for ev in events {
                            match ev {
                                aiya_audio::EngineEvent::Loaded { duration_ms } => {
                                    let token = actor.state().token;
                                    actor.reduce(aiya_player::PlayerCommand::Loaded {
                                        token,
                                        duration_ms,
                                    });
                                    should_emit = true;
                                }
                                aiya_audio::EngineEvent::Position { position_ms } => {
                                    // 节流：距上次快照 >500ms 才落 Actor 并广播
                                    let last = actor.state().updated_at;
                                    let now_ms = std::time::SystemTime::now()
                                        .duration_since(std::time::UNIX_EPOCH)
                                        .unwrap_or_default()
                                        .as_millis() as u64;
                                    if now_ms >= last + 500 {
                                        actor.reduce(aiya_player::PlayerCommand::Position {
                                            position_ms,
                                        });
                                        should_emit = true;
                                    }
                                }
                                aiya_audio::EngineEvent::Eof => {
                                    let token = actor.state().token;
                                    actor.reduce(aiya_player::PlayerCommand::Ended { token });
                                    should_emit = true;
                                }
                                aiya_audio::EngineEvent::StateChanged { playing } => {
                                    // Pause/Resume 由命令路径处理；这里对齐引擎侧被动变化
                                    let _ = playing;
                                }
                                aiya_audio::EngineEvent::Spectrum { bands } => {
                                    // 频谱帧：不经 Actor，直接转发 UI（~30fps）
                                    let _ = pump.emit("spectrum:frame", bands);
                                    continue;
                                }
                                aiya_audio::EngineEvent::Error { message } => {
                                    actor.reduce(aiya_player::PlayerCommand::Failed { message });
                                    should_emit = true;
                                }
                            }
                        }
                    }
                    if should_emit {
                        let actor = engine.player.lock().expect("player poisoned");
                        let snapshot = actor.state();
                        drop(actor);
                        if let Ok(v) = serde_json::to_value(&snapshot) {
                            let _ = pump.emit("player:state", v);
                        }
                    }
                    // 5s 节流落库状态行（position/音量等；队列结构变化在 drive_and_emit 立即落）
                    let now_ms = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis() as u64;
                    let last = engine
                        .last_persist_ms
                        .load(std::sync::atomic::Ordering::Relaxed);
                    if now_ms >= last + 5_000 {
                        engine
                            .last_persist_ms
                            .store(now_ms, std::sync::atomic::Ordering::Relaxed);
                        let actor = engine.player.lock().expect("player poisoned");
                        if actor.state().queue_length > 0 {
                            persist_queue_state_handle(&engine, &actor);
                        }
                    }
                }
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            app_info,
            spectrum_set,
            app_get_ui_state,
            app_set_ui_state,
            app_get_settings,
            app_set_settings,
            app_diagnostics,
            app_open_data_dir,
            beets_status,
            beets_preview,
            beets_apply,
            beets_cancel,
            beets_sync,
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
            player_play_tracks,
            player_play_query,
            player_toggle,
            player_next,
            player_prev,
            player_seek,
            player_set_volume,
            player_toggle_mute,
            player_set_rate,
            player_set_repeat,
            player_set_shuffle,
            player_enqueue,
            player_insert_next,
            player_remove_at,
            player_move,
            player_clear,
            player_report,
            lyrics_get,
            history_stats,
            history_clear,
        ])
        .run(tauri::generate_context!())
        .expect("AIYA: Tauri 运行失败");
}
