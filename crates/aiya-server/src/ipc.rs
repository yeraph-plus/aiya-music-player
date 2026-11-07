// 命令路由：`POST /ipc/{cmd}` 按域调 crate（A 桶全量 + player 域，逐条
// 迁移矩阵落批次文档）。与桌面壳的逐条对齐关系由 tests/contract.rs 守卫
// （路由表 vs ui/src/lib/ipc/client.ts 双向覆盖）。
//
// 参数侧沿用桌面壳的口径：命令体手动反序列化，形状不符 → invalidArg，
// 不让 HTTP 层的反序列化错误绕开 ApiError 分类。

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::{json, Value};

use aiya_dto::ApiError;

use crate::state::ServerState;
use crate::state::{
    db_lock, history_stats_from, now_secs, player_lock, playlist_list_all, playlist_summary_by_id,
    read_paused_roots, read_roots, setting_flag, setting_get, setting_set, track_refs_by_ids_db,
    ID_COLLECT_CAP,
};
use std::sync::Arc;

type St = State<Arc<ServerState>>;

fn st_clone(st: &Arc<ServerState>) -> Arc<ServerState> {
    std::sync::Arc::clone(st)
}

/// 本 server 已路由的命令面（契约守卫的锚点）。
pub const ROUTED: &[&str] = &[
    // app
    "app_info",
    "app_get_ui_state",
    "app_set_ui_state",
    "app_get_settings",
    "app_set_settings",
    "app_diagnostics",
    // enrichment
    "enrichment_query",
    "enrichment_gc",
    // library
    "library_list",
    "library_tracks",
    "library_stats",
    "library_albums",
    "library_artists",
    "library_genres",
    "library_roots_get",
    "library_roots_count",
    "library_roots_prune",
    "library_clear",
    "library_remove_tracks",
    "library_ids_for_selection",
    "tracks_resolve_paths",
    // playlist
    "playlist_list",
    "playlist_detail",
    "playlist_create",
    "playlist_set_rule",
    "playlist_refresh",
    "playlist_rename",
    "playlist_delete",
    "playlist_reorder",
    "playlist_add_tracks",
    "playlist_remove_at",
    "playlist_move",
    "playlist_clear",
    // lyrics / history
    "lyrics_get",
    "history_stats",
    "history_clear",
    // player（浏览器出声；装载意图经 token 变化下发）
    "player_state",
    "player_queue",
    "player_play_tracks",
    "player_play_query",
    "player_toggle",
    "player_next",
    "player_prev",
    "player_seek",
    "player_set_volume",
    "player_toggle_mute",
    "player_set_rate",
    "player_set_repeat",
    "player_set_shuffle",
    "player_enqueue",
    "player_insert_next",
    "player_remove_at",
    "player_move",
    "player_clear",
    // 浏览器引擎回报通道（http.ts 引擎内部调用，不在 client.ts 的 ipc.* 面）
    "player_report",
    // ---- W3：编排域收口（beets / 扫描管线 / 事件泵） ----
    "beets_status",
    "beets_preview",
    "beets_apply",
    "beets_cancel",
    // station 频道面：DB 持久化，UI 经 LiveView「我的频道」接线
    "station_list",
    "station_get",
    "station_create",
    "station_update",
    "station_delete",
    "station_set_playlists",
    "station_set_schedules",
    "station_activate",
    "station_skip",
    "station_push",
    "beets_config_get",
    "beets_set_inbox",
    "beets_set_target",
    "beets_inbox_imported_add",
    "refresh_state",
    "beets_install",
    "beets_plugins_set",
    "beets_plugin_table",
    "beets_uninstall",
    "beets_adopt",
    "beets_update",
    "beets_duplicates",
    "beets_move",
    "beets_query",
    "beets_fields",
    "enrichment_worklist",
    "beets_enrich_start",
    "beets_index_import",
    "library_scan",
    "library_scan_cancel",
    "library_roots_set",
];

/* ---- station 组装（docs/68 §6：repo 行 → DTO；运行时字段来自控制层） ---- */

fn station_row_of(
    st: &crate::state::ServerState,
    id: &str,
) -> Result<aiya_data::station::StationRow, ApiError> {
    let owned = id.to_string();
    db_lock(st)
        .send(move |conn| aiya_data::station::get_station(conn, &owned))
        .map_err(ApiError::from)?
        .ok_or_else(|| ApiError::not_found(format!("station not found: {id}")))
}

fn station_summary(
    st: &crate::state::ServerState,
    row: &aiya_data::station::StationRow,
) -> aiya_dto::station::StationSummary {
    let (running, now_playing) = st.station.runtime_of(&row.id);
    aiya_dto::station::StationSummary {
        id: row.id.clone(),
        name: row.name.clone(),
        subtitle: row.subtitle.clone(),
        description: row.description.clone(),
        cover_url: row.cover_url.clone(),
        hls_url: format!("/hls/{}/live.m3u8", row.hls_dir),
        visibility: row.visibility.clone(),
        active: row.active,
        running,
        now_playing,
    }
}

fn station_playlist_dto(
    r: &aiya_data::station::StationPlaylistRow,
) -> aiya_dto::station::StationPlaylist {
    let track_ids: Vec<String> = r
        .track_ids_json
        .as_deref()
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or_default();
    aiya_dto::station::StationPlaylist {
        id: r.id.clone(),
        name: r.name.clone(),
        kind: r.kind.clone(),
        track_ids,
        rule_json: r.rule_json.clone(),
    }
}

fn station_schedule_dto(
    r: &aiya_data::station::StationScheduleRow,
) -> aiya_dto::station::StationSchedule {
    aiya_dto::station::StationSchedule {
        id: r.id.clone(),
        playlist_id: r.playlist_id.clone(),
        start_hm: r.start_hm.clone(),
        end_hm: r.end_hm.clone(),
        days_mask: r.days_mask,
        priority: r.priority,
    }
}

fn station_detail(
    st: &crate::state::ServerState,
    row: &aiya_data::station::StationRow,
) -> aiya_data::DbResult<aiya_dto::station::StationDetail> {
    let id = row.id.clone();
    let (pls, schs) = db_lock(st).send(move |conn| {
        let pls = aiya_data::station::list_playlists(conn, &id)?;
        let schs = aiya_data::station::list_schedules(conn, &id)?;
        Ok((pls, schs))
    })?;
    Ok(aiya_dto::station::StationDetail {
        summary: station_summary(st, row),
        playlists: pls.iter().map(station_playlist_dto).collect(),
        schedules: schs.iter().map(station_schedule_dto).collect(),
    })
}

pub async fn invoke(
    State(st): St,
    accept_language: axum::http::HeaderMap,
    axum::extract::Path(cmd): axum::extract::Path<String>,
    body: axum::body::Bytes,
) -> Response {
    if !ROUTED.contains(&cmd.as_str()) {
        return err_resp(ApiError::not_found(format!("no route for {cmd}")));
    }
    let accept_language = accept_language
        .get(axum::http::header::ACCEPT_LANGUAGE)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    let args: Value = serde_json::from_slice(&body).unwrap_or(json!({}));
    match dispatch(&st, &cmd, &args, accept_language.as_deref()).await {
        Ok(v) => (StatusCode::OK, Json(v)).into_response(),
        Err(e) => err_resp(e),
    }
}

fn err_resp(e: ApiError) -> Response {
    let status = match e.code {
        aiya_dto::ApiErrorCode::InvalidArg | aiya_dto::ApiErrorCode::Unsupported => {
            StatusCode::BAD_REQUEST
        }
        aiya_dto::ApiErrorCode::NotFound => StatusCode::NOT_FOUND,
        aiya_dto::ApiErrorCode::Busy => StatusCode::CONFLICT,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    };
    match serde_json::to_value(e) {
        Ok(v) => (status, Json(v)).into_response(),
        Err(_) => (status, "error serialization failed").into_response(),
    }
}

/* ---- 参数提取（wire 键为 camelCase，与 client.ts 发送侧一致） ---- */

fn s_arg(args: &Value, key: &str) -> Option<String> {
    args.get(key).and_then(Value::as_str).map(str::to_string)
}

fn vec_s_arg(args: &Value, key: &str) -> Option<Vec<String>> {
    args.get(key).and_then(Value::as_array).map(|a| {
        a.iter()
            .filter_map(|v| v.as_str().map(str::to_string))
            .collect()
    })
}

fn num_arg<T: serde::de::DeserializeOwned>(args: &Value, key: &str) -> Option<T> {
    args.get(key)
        .and_then(|v| serde_json::from_value(v.clone()).ok())
}

fn invalid_arg(detail: impl std::fmt::Display) -> ApiError {
    ApiError::invalid_arg(format!("invalid query parameters: {detail}"))
}

async fn blocking<T, F>(f: F) -> Result<T, ApiError>
where
    T: Send + 'static,
    F: FnOnce() -> T + Send + 'static,
{
    tokio::task::spawn_blocking(f)
        .await
        .map_err(ApiError::internal)
}

/* ---- 调度主体 ---- */

async fn dispatch(
    st: &std::sync::Arc<ServerState>,
    cmd: &str,
    a: &Value,
    accept_language: Option<&str>,
) -> Result<Value, ApiError> {
    match cmd {
        /* ---- app ---- */
        "app_info" => ok(json!({
            "dataDir": st.db_path.parent().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default(),
        })),
        "app_get_ui_state" => ok(db_lock(st)
            .send(|conn| Ok(setting_get(conn, "ui_state").unwrap_or_else(|| json!({}))))
            .map_err(ApiError::from)?),
        "app_set_ui_state" => {
            let v = a.clone();
            db_lock(st)
                .send(move |conn| {
                    setting_set(conn, "ui_state", &v).map_err(aiya_data::DbError::Sql)
                })
                .map_err(ApiError::from)?;
            ok(json!(null))
        }
        "app_get_settings" => {
            let default: Value =
                serde_json::from_str(crate::state::DEFAULT_SETTINGS).expect("valid default");
            let blob = db_lock(st)
                .send(move |conn| Ok(setting_get(conn, "app_settings").unwrap_or(default)))
                .map_err(ApiError::from)?;
            // 语言首启：空/缺失 = 未定 → 按请求的 Accept-Language 推导写库，
            // 此后恒按设置值（与桌面壳按系统 UI 语言写库同一职责）
            let unset = blob
                .get("language")
                .and_then(Value::as_str)
                .unwrap_or("")
                .is_empty();
            if unset {
                let lang = match accept_language {
                    Some(al) if al.to_lowercase().starts_with("zh") => "zh",
                    Some(al) if !al.trim().is_empty() => "en",
                    _ => "zh",
                };
                let mut blob = blob.clone();
                blob["language"] = json!(lang);
                let v_for_db = blob.clone();
                db_lock(st)
                    .send(move |conn| {
                        setting_set(conn, "app_settings", &v_for_db)
                            .map_err(aiya_data::DbError::Sql)
                    })
                    .map_err(ApiError::from)?;
                *crate::state::settings_lock(st) = blob.clone();
                return ok(blob);
            }
            ok(blob)
        }
        "app_set_settings" => {
            // W1 口径：只落库 + 更新内存镜像；diff 副作用（扫描重排）随 W3 下沉。
            // 返回值与桌面壳同形（Ok(settings)）：client 成功分支会
            // mergeSettings(响应)——返回 null 会把本地设置打回默认值
            //（语言切 English 后又被顶回 zh 的实测事故）
            let v = a.get("settings").cloned().unwrap_or_else(|| a.clone());
            let v_for_db = v.clone();
            db_lock(st)
                .send(move |conn| {
                    setting_set(conn, "app_settings", &v_for_db).map_err(aiya_data::DbError::Sql)
                })
                .map_err(ApiError::from)?;
            *crate::state::settings_lock(st) = v.clone();
            ok(v)
        }
        "app_diagnostics" => ok(json!({
            "version": env!("CARGO_PKG_VERSION"),
            "dataDir": st.db_path.parent().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default(),
            "recentErrors": serde_json::to_value(aiya_dto::recent_errors()).unwrap_or(json!([])),
            "mediaKeys": null,
        })),

        /* ---- enrichment ---- */
        "enrichment_query" => {
            let args = a.get("query").cloned().unwrap_or(json!({}));
            let filter = aiya_beets_store::Filter {
                source: s_arg(&args, "source"),
                mood: s_arg(&args, "mood"),
                dance_min: num_arg(&args, "danceMin"),
                dance_max: num_arg(&args, "danceMax"),
                bpm_min: num_arg(&args, "bpmMin"),
                bpm_max: num_arg(&args, "bpmMax"),
                limit: num_arg(&args, "limit"),
            };
            let v = enrichment_query_sync(st, filter)?;
            to_value(v)
        }
        "enrichment_gc" => {
            let source = crate::enrich::normalize_source(s_arg(a, "source").as_deref())
                .map_err(ApiError::invalid_arg)?;
            let keep_days = num_arg::<u32>(a, "keepDays").unwrap_or(30).clamp(1, 3650);
            let store = enrich_store(st)?;
            let files = db_lock(st)
                .send(|conn| aiya_data::repo::online_files(conn))
                .map_err(ApiError::internal)?;
            let removed = blocking(move || -> Result<u32, String> {
                let arena = aiya_beets_store::Arena::from_rows(
                    files
                        .into_iter()
                        .map(|f| (f.path, f.path_lower, f.size_bytes, f.mtime)),
                );
                store
                    .gc(&source, now_secs(), keep_days, &arena)
                    .map(|n| n as u32)
                    .map_err(|e| e.to_string())
            })
            .await?
            .map_err(ApiError::internal)?;
            ok(json!({"removed": removed, "keepDays": keep_days}))
        }

        /* ---- library ---- */
        "library_list" => {
            let q: aiya_data::TrackWindowQuery =
                serde_json::from_value(a.get("query").cloned().unwrap_or(json!({})))
                    .map_err(invalid_arg)?;
            let page = blocking({
                let st = st_clone(st);
                move || db_lock(&st).send(move |conn| aiya_data::repo::list_tracks(conn, &q))
            })
            .await?
            .map_err(ApiError::from)?;
            to_value(page)
        }
        "library_tracks" => {
            let ids = vec_s_arg(a, "ids").unwrap_or_default();
            let out = blocking({
                let st = st_clone(st);
                move || {
                    db_lock(&st).send(move |conn| {
                        let mut out = Vec::with_capacity(ids.len());
                        for id in &ids {
                            let row = conn.query_row(
                                "SELECT t.id, t.title, t.artist_id, ar.name, t.album_id, al.title, al_ar.name,
                                        t.duration_ms, t.track_no, t.disc_no, t.year, t.added_at, t.artwork_id,
                                        t.play_count, t.last_played_at, t.gain_db, t.availability, t.genre,
                                        t.codec, t.container, t.sample_rate, t.bits_per_sample, t.bitrate, t.channels
                                 FROM tracks t
                                 LEFT JOIN artists ar ON ar.id = t.artist_id
                                 LEFT JOIN albums al ON al.id = t.album_id
                                 LEFT JOIN artists al_ar ON al_ar.id = al.artist_id
                                 WHERE t.id = ?1",
                                [id],
                                |r| {
                                    // 列序契约：0..=18 与 aiya_data::repo::row_to_track
                                    // 逐位对齐（genre@17、container@18），音频规格接在
                                    // 19..=24——任何一漏列就是 scanner.rs:530 记载过的
                                    // "参数整体错位"同类事故
                                    let track = aiya_data::repo::row_to_track(r)?;
                                    let codec: Option<String> = r.get(18)?;
                                    let container: Option<String> = r.get(19)?;
                                    let sample_rate: Option<i64> = r.get(20)?;
                                    let audio = if codec.is_none() && sample_rate.is_none() {
                                        None
                                    } else {
                                        let lossless = aiya_player::TrackAudio::infer_lossless(
                                            codec.as_deref(),
                                            container.as_deref(),
                                        );
                                        Some(aiya_player::TrackAudio {
                                            codec,
                                            container,
                                            sample_rate,
                                            bits_per_sample: r.get::<_, Option<i64>>(21)?,
                                            bitrate: r.get::<_, Option<i64>>(22)?,
                                            channels: r.get::<_, Option<i64>>(23)?,
                                            lossless,
                                        })
                                    };
                                    Ok(aiya_dto::TrackDetail::new(track, audio))
                                },
                            );
                            match row {
                                Ok(v) => out.push(v),
                                Err(rusqlite::Error::QueryReturnedNoRows) => {}
                                Err(e) => return Err(aiya_data::DbError::Sql(e)),
                            }
                        }
                        Ok(out)
                    })
                }
            })
            .await?
            .map_err(ApiError::from)?;
            to_value(out)
        }
        "library_stats" => {
            let db_path = st.db_path.clone();
            let stats = db_lock(st)
                .send(move |conn| {
                    let count =
                        |sql: &str| -> i64 { conn.query_row(sql, [], |r| r.get(0)).unwrap_or(0) };
                    let db_bytes = std::fs::metadata(&db_path).map(|m| m.len()).unwrap_or(0) as i64;
                    Ok(aiya_dto::LibraryStats {
                        tracks: count("SELECT COUNT(*) FROM tracks"),
                        albums: count("SELECT COUNT(*) FROM albums"),
                        artists: count("SELECT COUNT(*) FROM artists"),
                        locations: count("SELECT COUNT(*) FROM tracks"),
                        missing_locations: count(
                            "SELECT COUNT(*) FROM tracks WHERE availability <> 'online'",
                        ),
                        total_duration_ms: count("SELECT COALESCE(SUM(duration_ms),0) FROM tracks"),
                        total_bytes: count("SELECT COALESCE(SUM(size_bytes),0) FROM tracks"),
                        db_bytes,
                        schema_version: conn
                            .query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                            .unwrap_or(0),
                    })
                })
                .map_err(ApiError::from)?;
            to_value(stats)
        }
        "library_albums" => {
            let search = s_arg(a, "search");
            let page = blocking({
                let st = st_clone(st);
                move || {
                    db_lock(&st)
                        .send(move |conn| aiya_data::facets::list_albums(conn, search.as_deref()))
                }
            })
            .await?
            .map_err(ApiError::from)?;
            to_value(page)
        }
        "library_artists" => {
            let search = s_arg(a, "search");
            let page = blocking({
                let st = st_clone(st);
                move || {
                    db_lock(&st)
                        .send(move |conn| aiya_data::facets::list_artists(conn, search.as_deref()))
                }
            })
            .await?
            .map_err(ApiError::from)?;
            to_value(page)
        }
        "library_genres" => {
            let genres = blocking({
                let st = st_clone(st);
                move || db_lock(&st).send(move |conn| aiya_data::facets::distinct_genres(conn))
            })
            .await?
            .map_err(ApiError::from)?;
            to_value(genres)
        }
        "library_roots_get" => {
            let (roots, paused) = db_lock(st)
                .send(|conn| Ok((read_roots(conn), read_paused_roots(conn))))
                .map_err(|e| ApiError::internal(format!("failed to read scan folders: {e}")))?;
            ok(json!({"roots": roots, "paused": paused}))
        }
        "library_roots_count" => {
            let paths = vec_s_arg(a, "paths").unwrap_or_default();
            let count = db_lock(st)
                .send(move |conn| {
                    let mut total = 0i64;
                    for p in &paths {
                        let prefix = crate::state::root_prefix_lower(p);
                        if prefix == "\\" {
                            continue;
                        }
                        total += crate::state::count_tracks_under_prefix(conn, &prefix)?;
                    }
                    Ok(total)
                })
                .map_err(|e| {
                    ApiError::internal(format!("failed to count tracks in folders: {e}"))
                })?;
            ok(json!({"count": count}))
        }
        "library_roots_prune" => {
            let paths = vec_s_arg(a, "paths").unwrap_or_default();
            let (removed, deleted) = db_lock(st)
                .send(move |conn| {
                    let mut removed = 0i64;
                    let mut deleted = Vec::new();
                    for p in &paths {
                        let prefix = crate::state::root_prefix_lower(p);
                        if prefix == "\\" {
                            continue;
                        }
                        {
                            let mut stmt = conn.prepare(
                                "SELECT id FROM tracks WHERE substr(path_lower, 1, ?2) = ?1",
                            )?;
                            deleted.extend(
                                stmt.query_map(
                                    rusqlite::params![prefix, prefix.chars().count() as i64],
                                    |r| r.get::<_, String>(0),
                                )?
                                .collect::<Result<Vec<_>, _>>()?,
                            );
                        }
                        removed += conn.execute(
                            "DELETE FROM tracks WHERE substr(path_lower, 1, ?2) = ?1",
                            rusqlite::params![prefix, prefix.chars().count() as i64],
                        )? as i64;
                    }
                    Ok((removed, deleted))
                })
                .map_err(|e| ApiError::internal(format!("failed to remove folder tracks: {e}")))?;
            crate::state::prune_queue_for_deleted(st, &deleted);
            ok(json!({"removed": removed}))
        }
        "library_clear" => {
            let (removed, deleted) = db_lock(st)
                .send(|conn| {
                    let ids: Vec<String> = conn
                        .prepare("SELECT id FROM tracks")?
                        .query_map([], |r| r.get(0))?
                        .collect::<Result<Vec<_>, _>>()?;
                    let n = conn.execute("DELETE FROM tracks", [])? as i64;
                    Ok((n, ids))
                })
                .map_err(|e| ApiError::internal(format!("failed to clear the library: {e}")))?;
            crate::state::prune_queue_for_deleted(st, &deleted);
            ok(json!({"removed": removed}))
        }
        "library_remove_tracks" => {
            let track_ids = vec_s_arg(a, "trackIds").unwrap_or_default();
            let (removed, deleted) = db_lock(st)
                .send(move |conn| {
                    let mut removed = 0i64;
                    let mut deleted = Vec::new();
                    for id in &track_ids {
                        match conn.execute("DELETE FROM tracks WHERE id = ?1", [id]) {
                            Ok(n) if n > 0 => {
                                removed += n as i64;
                                deleted.push(id.clone());
                            }
                            _ => {}
                        }
                    }
                    Ok((removed, deleted))
                })
                .map_err(ApiError::from)?;
            crate::state::prune_queue_for_deleted(st, &deleted);
            ok(json!({"removed": removed}))
        }
        "library_ids_for_selection" => {
            let req = a.get("request").cloned().unwrap_or_else(|| a.clone());
            let select_all = req
                .get("indices")
                .map(|i| i.as_str() == Some("all"))
                .unwrap_or(false);
            let indices: Vec<usize> = match req.get("indices") {
                Some(Value::Array(arr)) => arr
                    .iter()
                    .filter_map(|v| v.as_u64().map(|n| n as usize))
                    .collect(),
                Some(Value::String(tag)) if tag == "all" => Vec::new(),
                Some(other) if !other.is_null() => {
                    return Err(ApiError::invalid_arg(format!(
                        "indices accepts only \"all\" or an index array, got \"{other}\""
                    )))
                }
                _ => Vec::new(),
            };
            let query: aiya_data::TrackWindowQuery =
                serde_json::from_value(req.get("query").cloned().unwrap_or(json!({})))
                    .map_err(invalid_arg)?;
            let ids = blocking({
                let st = st_clone(st);
                move || {
                    db_lock(&st).send(move |conn| {
                        let collected =
                            aiya_data::repo::collect_track_ids(conn, &query, ID_COLLECT_CAP)?;
                        if collected.truncated {
                            tracing::warn!(cap = ID_COLLECT_CAP, "选区 ID 收集达到上限，已截断");
                        }
                        Ok(if select_all {
                            collected.ids
                        } else {
                            indices
                                .into_iter()
                                .filter_map(|i| collected.ids.get(i).cloned())
                                .collect()
                        })
                    })
                }
            })
            .await?
            .map_err(ApiError::from)?;
            to_value(ids)
        }
        "tracks_resolve_paths" => {
            let paths = vec_s_arg(a, "paths").unwrap_or_default();
            if paths.is_empty() {
                return ok(json!([]));
            }
            let out = db_lock(st)
                .send(move |conn| aiya_data::repo::resolve_paths_aligned(conn, &paths))
                .map_err(ApiError::from)?;
            to_value(out)
        }

        /* ---- playlist ---- */
        "playlist_list" => {
            let v = db_lock(st)
                .send(|conn| playlist_list_all(conn))
                .map_err(ApiError::from)?;
            to_value(v)
        }
        "playlist_detail" => {
            let id = s_arg(a, "id").unwrap_or_default();
            let v = db_lock(st)
                .send(move |conn| {
                    playlist_summary_by_id(conn, &id).map_err(aiya_data::DbError::Sql)
                })
                .map_err(ApiError::from)?;
            to_value(v)
        }
        "playlist_create" => {
            let name = s_arg(a, "name").unwrap_or_default();
            let name = aiya_data::playlist::sanitize_name(&name)
                .map_err(|e| ApiError::invalid_arg(e.to_string()))?;
            let pid = aiya_data::ids::playlist_id(&name);
            let now = now_secs();
            let track_ids = vec_s_arg(a, "trackIds").unwrap_or_default();
            let rule = a.get("rule").cloned().filter(|v| !v.is_null());
            let summary = db_lock(st)
                .send(move |conn| {
                    match rule {
                        Some(rule) => {
                            aiya_data::playlist::create_smart(conn, &pid, &name, &rule, now)?;
                        }
                        None => {
                            aiya_data::playlist::create(conn, &pid, &name, &track_ids, now)?;
                        }
                    }
                    playlist_summary_by_id(conn, &pid)
                        .map_err(aiya_data::DbError::Sql)?
                        .ok_or_else(|| {
                            aiya_data::DbError::Custom(
                                "playlist not readable after creation".into(),
                            )
                        })
                })
                .map_err(ApiError::from)?;
            to_value(summary)
        }
        "playlist_set_rule" => {
            let id = s_arg(a, "id").unwrap_or_default();
            let rule = a.get("rule").cloned().unwrap_or(json!({}));
            let now = now_secs();
            playlist_write(st, move |conn| {
                match aiya_data::playlist::set_rule(conn, &id, &rule, now) {
                    Ok(true) => {}
                    Ok(false) => return Ok(None),
                    Err(e) => return Err(e),
                }
                playlist_summary_by_id(conn, &id).map_err(aiya_data::DbError::Sql)
            })
        }
        "playlist_refresh" => {
            let id = s_arg(a, "id").unwrap_or_default();
            let now = now_secs();
            let out = db_lock(st)
                .send(move |conn| {
                    let Some(outcome) = aiya_data::playlist::refresh_smart(conn, &id, now)? else {
                        return Ok(None);
                    };
                    Ok(Some(
                        json!({"added": outcome.added, "total": outcome.total}),
                    ))
                })
                .map_err(ApiError::from)?;
            match out {
                Some(v) => ok(v),
                None => Err(ApiError::not_found("smart playlist not found")),
            }
        }
        "playlist_rename" => {
            let id = s_arg(a, "id").unwrap_or_default();
            let name = s_arg(a, "name").unwrap_or_default();
            let name = aiya_data::playlist::sanitize_name(&name)
                .map_err(|e| ApiError::invalid_arg(e.to_string()))?;
            let now = now_secs();
            playlist_write(st, move |conn| {
                if !aiya_data::playlist::rename(conn, &id, &name, now)? {
                    return Ok(None);
                }
                playlist_summary_by_id(conn, &id).map_err(aiya_data::DbError::Sql)
            })
        }
        "playlist_delete" => {
            let id = s_arg(a, "id").unwrap_or_default();
            let deleted = db_lock(st)
                .send(move |conn| aiya_data::playlist::delete(conn, &id))
                .map_err(ApiError::from)?;
            ok(json!({"deleted": deleted}))
        }
        "playlist_reorder" => {
            let from = num_arg::<i64>(a, "from").unwrap_or(0);
            let to = num_arg::<i64>(a, "to").unwrap_or(0);
            let v = db_lock(st)
                .send(move |conn| {
                    aiya_data::playlist::reorder_playlists(conn, from, to)?;
                    playlist_list_all(conn)
                })
                .map_err(ApiError::from)?;
            to_value(v)
        }
        "playlist_add_tracks" => {
            let id = s_arg(a, "id").unwrap_or_default();
            let track_ids = vec_s_arg(a, "trackIds").unwrap_or_default();
            let now = now_secs();
            playlist_write(st, move |conn| {
                let Some(outcome) = aiya_data::playlist::add_tracks(conn, &id, &track_ids, now)?
                else {
                    return Ok(None);
                };
                let summary = playlist_summary_by_id(conn, &id)
                    .map_err(aiya_data::DbError::Sql)?
                    .ok_or_else(|| {
                        aiya_data::DbError::Custom("playlist row missing after transaction".into())
                    })?;
                Ok(Some(json!({
                    "added": outcome.added,
                    "skipped": outcome.skipped,
                    "summary": serde_json::to_value(summary).unwrap_or(json!(null)),
                })))
            })
        }
        "playlist_remove_at" => {
            let id = s_arg(a, "id").unwrap_or_default();
            let position = num_arg::<i64>(a, "position").unwrap_or(0);
            let now = now_secs();
            playlist_write(st, move |conn| {
                if aiya_data::playlist::remove_at(conn, &id, position, now)?.is_none() {
                    return Ok(None);
                }
                playlist_summary_by_id(conn, &id).map_err(aiya_data::DbError::Sql)
            })
        }
        "playlist_move" => {
            let id = s_arg(a, "id").unwrap_or_default();
            let from = num_arg::<i64>(a, "from").unwrap_or(0);
            let to = num_arg::<i64>(a, "to").unwrap_or(0);
            let now = now_secs();
            playlist_write(st, move |conn| {
                if aiya_data::playlist::move_item(conn, &id, from, to, now)?.is_none() {
                    return Ok(None);
                }
                playlist_summary_by_id(conn, &id).map_err(aiya_data::DbError::Sql)
            })
        }
        "playlist_clear" => {
            let id = s_arg(a, "id").unwrap_or_default();
            let now = now_secs();
            playlist_write(st, move |conn| {
                if !aiya_data::playlist::clear(conn, &id, now)? {
                    return Ok(None);
                }
                playlist_summary_by_id(conn, &id).map_err(aiya_data::DbError::Sql)
            })
        }

        /* ---- lyrics / history ---- */
        "lyrics_get" => {
            let track_id = s_arg(a, "trackId").unwrap_or_default();
            let out = blocking({
                let st = st_clone(st);
                move || -> Result<aiya_dto::lyrics::Lyrics, ApiError> {
                    let path: Option<String> = db_lock(&st)
                        .send(move |conn| {
                            Ok(conn
                                .query_row(
                                    "SELECT path FROM tracks WHERE id = ?1",
                                    [&track_id],
                                    |r| r.get(0),
                                )
                                .ok())
                        })
                        .map_err(ApiError::from)?;
                    let Some(path) = path else {
                        return Ok(aiya_dto::lyrics::Lyrics::empty());
                    };
                    Ok(
                        match aiya_data::lyrics::read_lyrics(std::path::Path::new(&path)) {
                            Some(parsed) => parsed.into(),
                            None => aiya_dto::lyrics::Lyrics::empty(),
                        },
                    )
                }
            })
            .await??;
            to_value(out)
        }
        "history_stats" => {
            let v = db_lock(st)
                .send(|conn| history_stats_from(conn))
                .map_err(ApiError::from)?;
            to_value(v)
        }
        "history_clear" => {
            db_lock(st)
                .send(aiya_data::history::clear)
                .map_err(ApiError::from)?;
            ok(json!({"cleared": true}))
        }

        /* ---- player ---- */
        "player_state" => to_value(player_lock(st).state()),
        "player_queue" => to_value(player_lock(st).queue_snapshot()),
        "player_play_tracks" => {
            let track_ids = vec_s_arg(a, "trackIds").unwrap_or_default();
            let start = num_arg::<usize>(a, "startIndex").unwrap_or(0);
            let db = db_lock(st).clone();
            let tracks = blocking(move || track_refs_by_ids_db(&db, &track_ids)).await?;
            if tracks.is_empty() {
                return Err(ApiError::not_found("no playable tracks"));
            }
            {
                let mut actor = player_lock(st);
                actor.reduce(aiya_player::PlayerCommand::Queue(
                    aiya_player::QueueOp::ReplaceAll { tracks, start },
                ));
            }
            to_value(crate::state::drive(st))
        }
        "player_play_query" => {
            let q: aiya_data::TrackWindowQuery =
                serde_json::from_value(a.get("query").cloned().unwrap_or(json!({})))
                    .map_err(invalid_arg)?;
            let start = num_arg::<usize>(a, "startIndex").unwrap_or(0);
            let db = db_lock(st).clone();
            let tracks = blocking(move || -> Result<Vec<aiya_player::TrackRef>, ApiError> {
                let (ids, truncated) = db
                    .send(move |conn| {
                        let c = aiya_data::repo::collect_track_ids(conn, &q, ID_COLLECT_CAP)?;
                        Ok((c.ids, c.truncated))
                    })
                    .map_err(ApiError::from)?;
                if truncated {
                    tracing::warn!(cap = ID_COLLECT_CAP, "播放全部：曲目数超过上限，已截断");
                }
                if ids.is_empty() {
                    return Err(ApiError::not_found("query returned no results"));
                }
                Ok(track_refs_by_ids_db(&db, &ids))
            })
            .await??;
            {
                let mut actor = player_lock(st);
                actor.reduce(aiya_player::PlayerCommand::Queue(
                    aiya_player::QueueOp::ReplaceAll { tracks, start },
                ));
            }
            to_value(crate::state::drive(st))
        }
        "player_toggle" => {
            {
                let mut actor = player_lock(st);
                actor.reduce(aiya_player::PlayerCommand::TogglePause);
            }
            to_value(crate::state::drive(st))
        }
        "player_next" => {
            player_lock(st).reduce(aiya_player::PlayerCommand::Next);
            to_value(crate::state::drive(st))
        }
        "player_prev" => {
            player_lock(st).reduce(aiya_player::PlayerCommand::Prev);
            to_value(crate::state::drive(st))
        }
        "player_seek" => {
            let position_ms = num_arg::<u64>(a, "positionMs").unwrap_or(0);
            player_lock(st).reduce(aiya_player::PlayerCommand::Seek(position_ms));
            to_value(crate::state::drive(st))
        }
        "player_set_volume" => {
            let volume = num_arg::<f64>(a, "volume").unwrap_or(1.0);
            {
                let mut actor = player_lock(st);
                let (_, muted) = actor.volume();
                actor.reduce(aiya_player::PlayerCommand::SetVolume { volume, muted });
            }
            to_value(crate::state::drive(st))
        }
        "player_toggle_mute" => {
            {
                let mut actor = player_lock(st);
                let (v, muted) = actor.volume();
                actor.reduce(aiya_player::PlayerCommand::SetVolume {
                    volume: v,
                    muted: !muted,
                });
            }
            to_value(crate::state::drive(st))
        }
        "player_set_rate" => {
            let rate = num_arg::<f64>(a, "rate").unwrap_or(1.0);
            player_lock(st).reduce(aiya_player::PlayerCommand::SetRate(rate));
            to_value(crate::state::drive(st))
        }
        "player_set_repeat" => {
            let repeat = s_arg(a, "repeat").unwrap_or_default();
            {
                let mut actor = player_lock(st);
                let mode = match repeat.as_str() {
                    "cycle" => match actor.state().repeat {
                        aiya_player::RepeatMode::Off => aiya_player::RepeatMode::All,
                        aiya_player::RepeatMode::All => aiya_player::RepeatMode::One,
                        aiya_player::RepeatMode::One => aiya_player::RepeatMode::Off,
                    },
                    "all" => aiya_player::RepeatMode::All,
                    "one" => aiya_player::RepeatMode::One,
                    "off" => aiya_player::RepeatMode::Off,
                    other => {
                        return Err(ApiError::invalid_arg(format!(
                            "unknown repeat value: {other}"
                        )))
                    }
                };
                actor.reduce(aiya_player::PlayerCommand::SetRepeat(mode));
            }
            to_value(crate::state::drive(st))
        }
        "player_set_shuffle" => {
            let shuffle = num_arg::<bool>(a, "shuffle").unwrap_or(false);
            {
                let mut actor = player_lock(st);
                if actor.state().shuffle != shuffle {
                    actor.reduce(aiya_player::PlayerCommand::ToggleShuffle);
                }
            }
            to_value(crate::state::drive(st))
        }
        "player_enqueue" => {
            let track_ids = vec_s_arg(a, "trackIds").unwrap_or_default();
            let mode = a
                .get("options")
                .and_then(|o| o.get("mode"))
                .and_then(Value::as_str)
                .unwrap_or("end")
                .to_string();
            let db = db_lock(st).clone();
            let tracks = blocking(move || track_refs_by_ids_db(&db, &track_ids)).await?;
            if tracks.is_empty() {
                return Err(ApiError::not_found("no playable tracks"));
            }
            {
                let mut actor = player_lock(st);
                actor.reduce(aiya_player::PlayerCommand::Queue(if mode == "next" {
                    aiya_player::QueueOp::PlayNext { tracks }
                } else {
                    aiya_player::QueueOp::Append { tracks }
                }));
            }
            to_value(crate::state::drive(st))
        }
        "player_insert_next" => {
            let track_id = s_arg(a, "trackId").unwrap_or_default();
            let db = db_lock(st).clone();
            let tracks = blocking(move || track_refs_by_ids_db(&db, &[track_id])).await?;
            if tracks.is_empty() {
                return Err(ApiError::not_found("track not found"));
            }
            {
                let mut actor = player_lock(st);
                actor.reduce(aiya_player::PlayerCommand::Queue(
                    aiya_player::QueueOp::PlayNext { tracks },
                ));
            }
            to_value(crate::state::drive(st))
        }
        "player_remove_at" => {
            let index = num_arg::<usize>(a, "index").unwrap_or(0);
            {
                let mut actor = player_lock(st);
                actor.reduce(aiya_player::PlayerCommand::Queue(
                    aiya_player::QueueOp::Remove {
                        indices: vec![index],
                    },
                ));
            }
            to_value(crate::state::drive(st))
        }
        "player_move" => {
            let from = num_arg::<usize>(a, "from").unwrap_or(0);
            let to = num_arg::<usize>(a, "to").unwrap_or(0);
            {
                let mut actor = player_lock(st);
                actor.reduce(aiya_player::PlayerCommand::Queue(
                    aiya_player::QueueOp::Move { from, to },
                ));
            }
            to_value(crate::state::drive(st))
        }
        "player_clear" => {
            {
                let mut actor = player_lock(st);
                actor.reduce(aiya_player::PlayerCommand::Queue(
                    aiya_player::QueueOp::ReplaceAll {
                        tracks: vec![],
                        start: 0,
                    },
                ));
            }
            to_value(crate::state::drive(st))
        }

        "player_report" => {
            let kind = s_arg(a, "kind").unwrap_or_default();
            let token = num_arg::<u64>(a, "token").unwrap_or(0);
            // 记账数据源：最近一次位置回报（曲终秒数的估算源）。令牌不符的
            // 迟到帧一并跳过——旧标签页的位置不能当新曲的记账源
            if kind == "position" && token == player_lock(st).state().token {
                let pos = num_arg::<u64>(a, "positionMs").unwrap_or(0);
                let track = player_lock(st)
                    .state()
                    .current
                    .map(|c| c.id)
                    .unwrap_or_default();
                *st.last_played
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) = (track, pos);
            }
            {
                let mut actor = player_lock(st);
                match kind.as_str() {
                    "loaded" => actor.reduce(aiya_player::PlayerCommand::Loaded {
                        token,
                        duration_ms: num_arg::<u64>(a, "durationMs").unwrap_or(0),
                    }),
                    "position" => {
                        // 迟到帧丢弃（Loaded/Ended 同口径）：web 多客户端下旧标签页
                        // 的位置不能写进新曲状态，否则记账把 A 曲秒数记到 B 曲头上
                        if token == actor.state().token {
                            actor.reduce(aiya_player::PlayerCommand::Position {
                                position_ms: num_arg::<u64>(a, "positionMs").unwrap_or(0),
                            });
                        }
                    }
                    "ended" => actor.reduce(aiya_player::PlayerCommand::Ended { token }),
                    "failed" => {
                        let code = s_arg(a, "code").unwrap_or_else(|| "io".into());
                        actor.reduce(aiya_player::PlayerCommand::Failed {
                            error: aiya_player::AudioError::new(
                                aiya_player::AudioErrorCode::parse(&code),
                                s_arg(a, "message")
                                    .unwrap_or_else(|| "browser engine error".into()),
                            ),
                        })
                    }
                    other => {
                        return Err(ApiError::invalid_arg(format!(
                            "unknown report kind: {other}"
                        )))
                    }
                }
            }
            // 播放记账：曲终按最近位置落一条历史（桌面泵同口径）
            if kind == "ended" {
                let (track, pos) = st
                    .last_played
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .clone();
                let now = crate::state::now_secs();
                let _ = db_lock(st)
                    .send(move |conn| aiya_data::history::record(conn, &track, now, pos / 1000));
                *st.last_played
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) = (String::new(), 0);
            }
            // 队列状态行 5s 节流落库（桌面泵同口径）
            if kind == "position" {
                let now_ms = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as u64;
                let last = st.last_persist_ms.load(std::sync::atomic::Ordering::SeqCst);
                if now_ms.saturating_sub(last) > 5000 {
                    st.last_persist_ms
                        .store(now_ms, std::sync::atomic::Ordering::SeqCst);
                    let actor = player_lock(st);
                    let p = crate::state::queue_persist_from_actor(
                        &actor,
                        setting_flag(st, "rememberPosition", true),
                    );
                    let items: Vec<aiya_data::QueueItem> = actor
                        .queue_refs()
                        .iter()
                        .map(crate::state::queue_item_from_ref)
                        .collect();
                    drop(actor);
                    crate::state::persist_queue_full(st, p, &items);
                }
            }
            to_value(crate::state::drive(st))
        }
        /* ---- W3：扫描管线 ---- */
        "refresh_state" => {
            let interval_secs = {
                let v = crate::state::settings_lock(st)
                    .get("refreshSeconds")
                    .and_then(Value::as_u64);
                match v {
                    Some(0) => 0,
                    Some(n) => n.clamp(10, 3600),
                    None => crate::scan::DEFAULT_INTERVAL_SECS,
                }
            };
            let last = st
                .scan
                .last
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone();
            ok(json!({
                "intervalSecs": interval_secs,
                "focused": true,
                "last": serde_json::to_value(last).unwrap_or(json!(null)),
            }))
        }
        "library_scan" => {
            // busy 闸在 run_scan_inline 内自持（ScanFlagGuard：早退/panic 全路径复位）
            st.scan
                .cancel
                .store(false, std::sync::atomic::Ordering::SeqCst);
            let st2 = std::sync::Arc::clone(st);
            let result = blocking(move || crate::scan::run_scan_inline(&st2, "action"))
                .await?
                .map_err(ApiError::internal)?;
            to_value(result)
        }
        "library_scan_cancel" => {
            let cancelled = st.scan.scanning.load(std::sync::atomic::Ordering::SeqCst);
            if cancelled {
                st.scan
                    .cancel
                    .store(true, std::sync::atomic::Ordering::SeqCst);
            }
            ok(json!({ "cancelled": cancelled }))
        }
        "library_roots_set" => {
            let normalize = |items: Vec<String>| -> Vec<String> {
                let mut out = Vec::new();
                for item in items {
                    let s = item.trim().trim_matches('"').trim_matches('\'').trim();
                    if !s.is_empty() {
                        out.push(s.to_string());
                    }
                }
                out
            };
            let cleaned = normalize(vec_s_arg(a, "roots").unwrap_or_default());
            let watched: std::collections::HashSet<String> =
                cleaned.iter().map(|r| r.to_lowercase()).collect();
            let paused_cleaned: Vec<String> = normalize(vec_s_arg(a, "paused").unwrap_or_default())
                .into_iter()
                .filter(|p| !watched.contains(&p.to_lowercase()))
                .collect();
            let cleaned2 = cleaned.clone();
            let paused2 = paused_cleaned.clone();
            db_lock(st)
                .send(move |conn| {
                    setting_set(conn, "library.roots", &json!(cleaned2))?;
                    setting_set(conn, "library.roots.paused", &json!(paused2))?;
                    Ok(())
                })
                .map_err(|e| ApiError::internal(format!("failed to save scan folders: {e}")))?;
            crate::scan::spawn_scan(st, "action");
            ok(json!({ "roots": cleaned, "paused": paused_cleaned }))
        }
        /* ---- W3：beets 域 ---- */
        "beets_status" => {
            let host = crate::host::beets_host(st);
            let v = blocking(move || aiya_host_core::beets::probe(&*host)).await?;
            to_value(v)
        }
        "beets_preview" => {
            let paths = vec_s_arg(a, "paths").unwrap_or_default();
            let host = crate::host::beets_host(st);
            blocking(move || aiya_host_core::beets::start_preview(host, paths))
                .await?
                .map_err(ApiError::beets_missing)?;
            ok(json!({ "started": true }))
        }
        "beets_apply" => {
            let decisions = a
                .get("decisions")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            let resume = num_arg::<bool>(a, "resume").unwrap_or(false);
            let host = crate::host::beets_host(st);
            blocking(move || aiya_host_core::beets::start_apply(host, decisions, resume))
                .await?
                .map_err(ApiError::beets_missing)?;
            ok(json!({ "started": true }))
        }
        "beets_cancel" => ok(json!({ "requested": aiya_host_core::beets::cancel_session() })),
        /* ---- station（docs/68 §6/§12：DB 频道面，N1.3） ---- */
        "station_list" => {
            let rows: Vec<aiya_data::station::StationRow> = db_lock(st)
                .send(|conn| aiya_data::station::list_stations(conn))
                .map_err(ApiError::from)?;
            let out: Vec<aiya_dto::station::StationSummary> =
                rows.iter().map(|r| station_summary(st, r)).collect();
            to_value(out)
        }
        "station_get" => {
            let id = s_arg(a, "stationId").ok_or_else(|| invalid_arg("stationId required"))?;
            let row = station_row_of(st, &id)?;
            let detail = station_detail(st, &row).map_err(ApiError::from)?;
            to_value(detail)
        }
        "station_create" => {
            let edit: aiya_dto::station::StationEdit =
                serde_json::from_value(a.clone()).map_err(invalid_arg)?;
            let vis = edit.visibility_or_default().to_owned();
            let row = db_lock(st)
                .send(move |conn| {
                    aiya_data::station::create_station(
                        conn,
                        &aiya_data::station::StationCreate {
                            name: edit.name,
                            subtitle: edit.subtitle,
                            description: edit.description,
                            cover_url: edit.cover_url,
                            visibility: vis,
                        },
                    )
                })
                .map_err(ApiError::from)?;
            to_value(station_summary(st, &row))
        }
        "station_update" => {
            let id = s_arg(a, "stationId").ok_or_else(|| invalid_arg("stationId required"))?;
            let edit: aiya_dto::station::StationEdit =
                serde_json::from_value(a.clone()).map_err(invalid_arg)?;
            let vis = edit.visibility_or_default().to_owned();
            let row = db_lock(st)
                .send(move |conn| {
                    aiya_data::station::update_station(
                        conn,
                        &id,
                        &aiya_data::station::StationUpdate {
                            name: edit.name,
                            subtitle: edit.subtitle,
                            description: edit.description,
                            cover_url: edit.cover_url,
                            visibility: vis,
                        },
                    )
                })
                .map_err(ApiError::from)?;
            // 内存频道表快照同步：public→private 立即回收 /hls 豁免（审查 P0-2）
            st.station.set_visibility(&row.id, &row.visibility);
            to_value(station_summary(st, &row))
        }
        "station_delete" => {
            let id = s_arg(a, "stationId").ok_or_else(|| invalid_arg("stationId required"))?;
            let row = station_row_of(st, &id)?;
            let _ = st.station.stop(&row.id); // 未在跑时静默
            db_lock(st)
                .send(move |conn| aiya_data::station::delete_station(conn, &row.id))
                .map_err(ApiError::from)?;
            // 段目录与顶层 conf/{dir}（审查 P2-1：conf 残留会被 keeper 复活）
            // 都随频道走；失败不拦（boot GC 兜底段目录，conf 由 GC 的孤儿检测
            // 不覆盖——此处是唯一清理点）
            if let Some(root) = st.station.hls_root() {
                let _ = std::fs::remove_dir_all(root.join(&row.hls_dir));
                let dir = row.hls_dir.clone();
                let _ = std::fs::remove_dir_all(root.join("conf").join(dir));
            }
            ok(json!({}))
        }
        "station_set_playlists" => {
            let id = s_arg(a, "stationId").ok_or_else(|| invalid_arg("stationId required"))?;
            let edits: Vec<aiya_dto::station::StationPlaylistEdit> =
                serde_json::from_value(a.get("playlists").cloned().unwrap_or(json!([])))
                    .map_err(invalid_arg)?;
            let inputs: Vec<aiya_data::station::PlaylistInput> = edits
                .into_iter()
                .map(|e| aiya_data::station::PlaylistInput {
                    name: e.name,
                    kind: e.kind,
                    track_ids: e.track_ids.unwrap_or_default(),
                    rule_json: e.rule_json,
                })
                .collect();
            let rows = db_lock(st)
                .send(move |conn| aiya_data::station::replace_playlists(conn, &id, &inputs))
                .map_err(ApiError::from)?;
            let out: Vec<aiya_dto::station::StationPlaylist> =
                rows.iter().map(station_playlist_dto).collect();
            ok(json!({ "playlists": out }))
        }
        "station_set_schedules" => {
            let id = s_arg(a, "stationId").ok_or_else(|| invalid_arg("stationId required"))?;
            let edits: Vec<aiya_dto::station::StationScheduleEdit> =
                serde_json::from_value(a.get("schedules").cloned().unwrap_or(json!([])))
                    .map_err(invalid_arg)?;
            let inputs: Vec<aiya_data::station::ScheduleInput> = edits
                .into_iter()
                .map(|e| aiya_data::station::ScheduleInput {
                    playlist_id: e.playlist_id,
                    start_hm: e.start_hm,
                    end_hm: e.end_hm,
                    days_mask: e.days_mask,
                    priority: e.priority,
                })
                .collect();
            let rows = db_lock(st)
                .send(move |conn| aiya_data::station::replace_schedules(conn, &id, &inputs))
                .map_err(ApiError::from)?;
            let out: Vec<aiya_dto::station::StationSchedule> =
                rows.iter().map(station_schedule_dto).collect();
            ok(json!({ "schedules": out }))
        }
        "station_activate" => {
            let id = s_arg(a, "stationId").ok_or_else(|| invalid_arg("stationId required"))?;
            let active = a.get("active").and_then(Value::as_bool).unwrap_or(false);
            let row = station_row_of(st, &id)?;
            if active {
                // 先落库再起管：重启后 boot 按 active=1 批量拉起
                db_lock(st)
                    .send(move |conn| aiya_data::station::set_active(conn, &row.id, true))
                    .map_err(ApiError::from)?;
                let row = station_row_of(st, &id)?;
                st.station
                    .start(st, &row)
                    .await
                    .map_err(ApiError::internal)?;
            } else {
                let _ = st.station.stop(&id);
                db_lock(st)
                    .send(move |conn| aiya_data::station::set_active(conn, &row.id, false))
                    .map_err(ApiError::from)?;
            }
            let row = station_row_of(st, &id)?;
            to_value(station_summary(st, &row))
        }
        "station_skip" => {
            let id = s_arg(a, "stationId").ok_or_else(|| invalid_arg("stationId required"))?;
            to_value(st.station.skip(&id).await.map_err(ApiError::internal)?)
        }
        "station_push" => {
            let id = s_arg(a, "stationId").ok_or_else(|| invalid_arg("stationId required"))?;
            let track_id = a
                .get("trackId")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .ok_or_else(|| invalid_arg("trackId required"))?
                .to_string();
            let path: Option<String> = db_lock(st)
                .send(move |conn| -> aiya_data::DbResult<Option<String>> {
                    match conn.query_row(
                        "SELECT path FROM tracks WHERE id = ?1 AND availability = 'online'",
                        [&track_id],
                        |r| r.get::<_, String>(0),
                    ) {
                        Ok(p) => Ok(Some(p)),
                        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
                        Err(e) => Err(aiya_data::DbError::Sql(e)),
                    }
                })
                .map_err(ApiError::from)?;
            let path = path.ok_or_else(|| ApiError::internal("track not found or offline"))?;
            to_value(
                st.station
                    .push_file(&id, &path)
                    .await
                    .map_err(ApiError::internal)?,
            )
        }
        "beets_config_get" => {
            let host = crate::host::beets_host(st);
            let roots = host.library_roots().unwrap_or_default();
            let v = blocking(move || aiya_host_core::beets::config_view(&*host, &roots)).await?;
            to_value(v)
        }
        "beets_set_inbox" => {
            let path = s_arg(a, "path");
            let host = crate::host::beets_host(st);
            let trimmed = path.as_deref().map(str::trim).filter(|s| !s.is_empty());
            let roots = host.library_roots().unwrap_or_default();
            if let Some(p) = trimmed {
                if aiya_host_core::path_overlaps_roots(p, &roots) {
                    return Err(ApiError::invalid_arg(
                        "inbox folder must not overlap with library folders",
                    ));
                }
            }
            let inbox = trimmed.map(str::to_string);
            let host2 = std::sync::Arc::clone(&host);
            blocking(move || {
                aiya_host_core::beets::update_settings(&*host, move |s| {
                    s.inbox_path = inbox;
                    s.inbox_imported.clear();
                })
            })
            .await?
            .map_err(ApiError::internal)?;
            let v = blocking(move || aiya_host_core::beets::config_view(&*host2, &roots)).await?;
            to_value(v)
        }
        "beets_set_target" => {
            let path = s_arg(a, "path").unwrap_or_default();
            let host = crate::host::beets_host(st);
            let p = path.trim().to_string();
            let roots = host.library_roots().unwrap_or_default();
            if !aiya_host_core::path_within_roots(&p, &roots) {
                return Err(ApiError::invalid_arg(
                    "organize target must be inside a library folder",
                ));
            }
            let saved = p;
            let host2 = std::sync::Arc::clone(&host);
            blocking(move || {
                aiya_host_core::beets::update_settings(&*host, move |s| s.target_root = Some(saved))
            })
            .await?
            .map_err(ApiError::internal)?;
            let v = blocking(move || aiya_host_core::beets::config_view(&*host2, &roots)).await?;
            to_value(v)
        }
        "beets_inbox_imported_add" => {
            let paths = vec_s_arg(a, "paths").unwrap_or_default();
            let host = crate::host::beets_host(st);
            let roots = host.library_roots().unwrap_or_default();
            let host2 = std::sync::Arc::clone(&host);
            blocking(move || aiya_host_core::beets::remember_imported(&*host, paths))
                .await?
                .map_err(ApiError::internal)?;
            let v = blocking(move || aiya_host_core::beets::config_view(&*host2, &roots)).await?;
            to_value(v)
        }
        "beets_install" => {
            let plugins = vec_s_arg(a, "plugins").unwrap_or_default();
            let host = crate::host::beets_host(st);
            blocking(move || aiya_host_core::beets::start_install(host, plugins))
                .await?
                .map_err(ApiError::beets_missing)?;
            ok(json!({ "started": true }))
        }
        "beets_plugins_set" => {
            let plugins = vec_s_arg(a, "plugins").unwrap_or_default();
            let host = crate::host::beets_host(st);
            blocking(move || aiya_host_core::beets::start_plugins_set(host, plugins))
                .await?
                .map_err(ApiError::beets_missing)?;
            ok(json!({ "started": true }))
        }
        "beets_plugin_table" => {
            let host = crate::host::beets_host(st);
            let v = blocking(move || aiya_host_core::beets::plugin_table(&*host))
                .await?
                .map_err(ApiError::internal)?;
            to_value(v)
        }
        "beets_uninstall" => {
            let host = crate::host::beets_host(st);
            blocking(move || aiya_host_core::beets::start_uninstall(host))
                .await?
                .map_err(ApiError::beets_missing)?;
            ok(json!({ "started": true }))
        }
        "beets_adopt" => {
            let paths = vec_s_arg(a, "paths").unwrap_or_default();
            let host = crate::host::beets_host(st);
            blocking(move || aiya_host_core::beets::start_adopt(host, paths, true))
                .await?
                .map_err(ApiError::beets_missing)?;
            ok(json!({ "started": true }))
        }
        "beets_update" => {
            let pretend = num_arg::<bool>(a, "pretend").unwrap_or(false);
            let host = crate::host::beets_host(st);
            blocking(move || aiya_host_core::beets::start_update(host, pretend))
                .await?
                .map_err(ApiError::beets_missing)?;
            ok(json!({ "started": true }))
        }
        "beets_duplicates" => {
            let keys = vec_s_arg(a, "keys").unwrap_or_default();
            let host = crate::host::beets_host(st);
            blocking(move || aiya_host_core::beets::start_duplicates(host, keys))
                .await?
                .map_err(ApiError::beets_missing)?;
            ok(json!({ "started": true }))
        }
        "beets_move" => {
            let directory = s_arg(a, "directory").unwrap_or_default();
            let pretend = num_arg::<bool>(a, "pretend").unwrap_or(false);
            let host = crate::host::beets_host(st);
            blocking(move || aiya_host_core::beets::start_move(host, directory, pretend, true))
                .await?
                .map_err(ApiError::beets_missing)?;
            ok(json!({ "started": true }))
        }
        "beets_query" => {
            let rules = a.get("rules").cloned().filter(|v| !v.is_null());
            let raw = s_arg(a, "raw");
            let fields = vec_s_arg(a, "fields");
            let limit = num_arg::<u32>(a, "limit");
            let host = crate::host::beets_host(st);
            let v = blocking(move || {
                aiya_host_core::beets::query_page(
                    &*host,
                    rules,
                    raw,
                    fields,
                    limit.map(|v| v as usize),
                )
            })
            .await?
            .map_err(ApiError::beets_missing)?;
            to_value(v)
        }
        "beets_fields" => {
            let host = crate::host::beets_host(st);
            let v = blocking(move || aiya_host_core::beets::field_catalog(&*host))
                .await?
                .map_err(ApiError::beets_missing)?;
            to_value(v)
        }
        "enrichment_worklist" => {
            let source = s_arg(a, "source");
            let include_empty = num_arg::<bool>(a, "includeEmpty").unwrap_or(false);
            let source = crate::enrich::normalize_source(source.as_deref())
                .map_err(ApiError::invalid_arg)?;
            let host = crate::host::beets_host(st);
            let v = blocking(move || {
                aiya_host_core::enrichment::worklist(&*host, &source, include_empty)
            })
            .await?
            .map_err(ApiError::internal)?;
            to_value(v)
        }
        "beets_enrich_start" => {
            let source = s_arg(a, "source");
            let force = num_arg::<bool>(a, "force").unwrap_or(false);
            let source = crate::enrich::normalize_source(source.as_deref())
                .map_err(ApiError::invalid_arg)?;
            let host = crate::host::beets_host(st);
            blocking(move || aiya_host_core::beets::start_enrich(host, source, force))
                .await?
                .map_err(ApiError::beets_missing)?;
            ok(json!({ "started": true }))
        }
        "beets_index_import" => {
            let root = s_arg(a, "root");
            let host = crate::host::beets_host(st);
            blocking(move || aiya_host_core::beets::start_index_import(host, root))
                .await?
                .map_err(ApiError::beets_missing)?;
            ok(json!({ "started": true }))
        }
        other => Err(ApiError::not_found(format!("no route for {other}"))),
    }
}

/* ---- 小工具 ---- */

fn ok(v: Value) -> Result<Value, ApiError> {
    Ok(v)
}

fn to_value<T: serde::Serialize>(v: T) -> Result<Value, ApiError> {
    serde_json::to_value(v).map_err(ApiError::internal)
}

/// playlist 域统一的事务包装：None → notFound（与桌面壳 playlist_write 同形）。
fn playlist_write<T, F>(st: &ServerState, f: F) -> Result<Value, ApiError>
where
    T: serde::Serialize,
    F: FnOnce(&mut rusqlite::Connection) -> aiya_data::DbResult<Option<T>> + Send + 'static,
    T: Send + 'static,
{
    match db_lock(st).send(f) {
        Ok(Some(v)) => to_value(v),
        Ok(None) => Err(ApiError::not_found("playlist not found")),
        Err(e) => Err(ApiError::from(e)),
    }
}

fn enrich_store(st: &ServerState) -> Result<std::sync::Arc<aiya_beets_store::Store>, ApiError> {
    if let Some(s) = st.enrich_store.get() {
        return Ok(std::sync::Arc::clone(s));
    }
    let path = st.beets_index_dir.join("enrich.db");
    let store = std::sync::Arc::new(
        aiya_beets_store::Store::open(&path)
            .map_err(|e| ApiError::internal(format!("{}: {e}", path.display())))?,
    );
    let _ = st.enrich_store.set(std::sync::Arc::clone(&store));
    Ok(st
        .enrich_store
        .get()
        .map(std::sync::Arc::clone)
        .unwrap_or(store))
}

/// enrichment_query 的同步实现：store 行 → 库内曲目 id 回查 → DTO。
fn enrichment_query_sync(
    st: &ServerState,
    filter: aiya_beets_store::Filter,
) -> Result<Vec<aiya_dto::enrichment::EnrichmentMatch>, ApiError> {
    let store = enrich_store(st)?;
    let rows = store.find(&filter).map_err(ApiError::internal)?;
    if rows.is_empty() {
        return Ok(Vec::new());
    }
    let keys: Vec<String> = rows.iter().map(|r| r.path_lower.clone()).collect();
    let ids = db_lock(st)
        .send(move |conn| aiya_data::repo::track_ids_by_paths(conn, &keys))
        .map_err(ApiError::internal)?;
    Ok(rows
        .into_iter()
        .filter_map(|r| {
            let track_id = ids.get(&r.path_lower)?.clone();
            Some(aiya_dto::enrichment::EnrichmentMatch {
                track_id,
                mood_label: r.mood_label,
                mood_score: r.mood_score,
                bpm: r.bpm,
                danceability: r.danceability,
                loudness: r.loudness,
                analyzed_at_ms: r.analyzed_at * 1000,
            })
        })
        .collect())
}
