//! AIYA Music — Tauri 宿主壳。
//!
//! 边界约定（src-tauri/README.md）：命令注册、事件桥、协议注册；领域业务在 crates/。
//!
//! 全部 55 个命令都是 `Result<T, dto::ApiError>`：成功返回**裸载荷**（形状定义在
//! [`dto`]，由 ts-rs 生成到 `ui/src/lib/ipc/generated/`），失败时 Tauri 把
//! `ApiError` 序列化后直接 reject。`{ok,data}` 信封已经不存在了 —— 旧 UI 那层
//! 翻译（`ui/src/compat/`）已于 2026-10-25 批次 4 删除，UI 侧只剩
//! `ui/src/lib/ipc/client.ts` 一层（docs/17 §3.1）。
//!
//! 53 而不是 54：`player_report` 是 v1 `<audio>` 出口的回报通道，引擎搬到
//! Rust 后前端从没调用过、后端 `let _ = &state` 空转，2026-10-25 整条删除。
//! 55 = 53 + `playlist_import`/`playlist_export`（docs/22 G2）。

use std::borrow::Cow;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager, State};

use aiya_data::ids;
use aiya_data::playlist as playlist_repo;
use aiya_data::repo::{self, TrackWindowQuery};
use aiya_data::scanner;

mod beets;
pub mod dto;
mod logging;
mod pump;
mod sidecar;
mod smtc;

use pump::{PumpCore, PumpEffect};

/* ---------------- 命令返回形状约定 ---------------- */

/// 动态 JSON 的别名：用于 settings / ui_state 这类**没有固定形状**的载荷。
///
/// 它们是真正的动态边界（配置项会演化、旧配置要兼容），所以不定义 DTO，
/// 由 UI 侧 zod schema 兜住（docs/17 §6）。写成 `Json` 而不是 `Value` 是为了
/// 在签名上一眼看出"这里是故意的动态"，而不是"还没收口"。
type Json = Value;

fn now_ms_u64() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// 秒级时间戳（DB 里的 created_at / updated_at / played_at 都是 UTC 秒）
fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

#[cfg(test)]
mod lib_tests {
    use crate::pump::position_emit_due;

    /// 历史统计的 SQL 曾经查 `t.artist`——tracks 表没有这一列，prepare 直接
    /// 报 "no such column"，命令恒返回 internal，历史页永远是空态。这条测试
    /// 用真 schema 跑一遍，锁住"能查出来 + 艺人名走 JOIN + 时间换毫秒"。
    #[test]
    fn history_stats_query_works_against_real_schema() {
        let mut conn = aiya_data::db::open_in_memory().expect("内存库");
        conn.execute("INSERT INTO artists (id, name, name_norm) VALUES ('ar1','艺人','艺人')", [])
            .unwrap();
        conn.execute(
            "INSERT INTO tracks (id, title, title_norm, artist_id, added_at, path, path_lower)
             VALUES ('t1','标题','标题','ar1', 0, 'D:/m/a.flac', 'd:/m/a.flac')",
            [],
        )
        .unwrap();
        // 两条：一条听完、一条 0 秒切走
        aiya_data::history::record(&mut conn, "t1", 1_700_000_000, 30).unwrap();
        aiya_data::history::record(&mut conn, "t1", 1_700_000_600, 0).unwrap();

        let s = crate::history_stats_from(&conn).expect("历史统计不能报错");
        assert_eq!(s.total_plays, 2);
        assert_eq!(s.completed, 1, "seconds_played >= 1 才算完成");
        assert_eq!(s.skipped, 1);
        // played_at 存秒、出口毫秒 —— 换算只在这一处
        assert_eq!(s.first_play_at_ms, Some(1_700_000_000_000));
        assert_eq!(s.last_play_at_ms, Some(1_700_000_600_000));
        assert_eq!(s.total_ms, 30_000);
        assert_eq!(s.top_tracks.len(), 1, "同一首曲目只占一行");
        assert_eq!(s.top_tracks[0].artist.as_deref(), Some("艺人"), "艺人名必须来自 JOIN");
        assert_eq!(s.top_tracks[0].play_count, 2);
    }

    /// 空库也必须能查（初次启动、还没听过任何歌）。此前这里靠 `unwrap_or(None)`
    /// 兜住 MIN/MAX，但 prepare 失败会直接上抛——所以这条一起锚住。
    #[test]
    fn history_stats_handles_empty_library() {
        let conn = aiya_data::db::open_in_memory().expect("内存库");
        let s = crate::history_stats_from(&conn).expect("空库也要能查");
        assert_eq!(s.total_plays, 0);
        assert_eq!(s.total_ms, 0);
        assert!(s.first_play_at_ms.is_none());
        assert!(s.top_tracks.is_empty());
    }

    /// 歌单摘要的三个聚合子查询 + 秒→毫秒换算。
    ///
    /// 这段 SQL 以前是手拼 `json!`，字段顺序靠人工对齐；换算写反的话
    /// UI 会把 1970 年显示成"创建时间"，而且是静默的。
    #[test]
    fn playlist_summary_counts_items_and_converts_to_ms() {
        let mut conn = aiya_data::db::open_in_memory().expect("内存库");
        for (id, dur, art) in [("t1", 100_000i64, Some("art1")), ("t2", 20_000, None)] {
            conn.execute(
                "INSERT INTO tracks (id, title, title_norm, duration_ms, added_at, artwork_id,
                                     path, path_lower)
                 VALUES (?1, ?1, ?1, ?2, 0, ?3, ?4, ?4)",
                rusqlite::params![id, dur, art, format!("D:/m/{id}.flac")],
            )
            .unwrap();
        }
        let now = 1_700_000_000i64; // unix 秒
        aiya_data::playlist::create(
            &mut conn,
            "pl1",
            "通勤",
            &["t1".to_string(), "t2".to_string()],
            now,
        )
        .unwrap();

        let all = crate::playlist_list_all(&conn).expect("歌单列表不能报错");
        assert_eq!(all.len(), 1);
        let p = &all[0];
        assert_eq!(p.name, "通勤");
        // 聚合：两首、时长求和、封面取第一首有的那张
        assert_eq!(p.track_count, 2);
        assert_eq!(p.duration_ms, 120_000);
        assert_eq!(p.artwork_id.as_deref(), Some("art1"));
        // 时间：DB 存秒、出口毫秒
        assert_eq!(p.created_at_ms, now * 1000);
        assert!(p.updated_at_ms >= p.created_at_ms);

        let one = crate::playlist_summary_by_id(&conn, "pl1")
            .expect("单查不能报错")
            .expect("歌单存在");
        assert_eq!(one.id, "pl1");
        // 不存在返回 None（命令层映射成 null，不是错误）
        assert!(crate::playlist_summary_by_id(&conn, "nope").unwrap().is_none());
    }

    /// P0-1 回归：节流基准必须是泵私有变量，不能被任何"刚刚被归约"的
    /// 时间戳影响。这里锁住三条性质。
    #[test]
    fn position_throttle_is_based_on_last_emit_only() {
        // 首个事件（last=0）必须立刻放行，否则 UI 要空等 500ms
        assert!(position_emit_due(1_000, 0));
        // 同一毫秒内再来一次（last 已置为当前时刻）：压掉（节流生效）
        assert!(!position_emit_due(1_000, 1_000));
        // 距上次广播不足 500ms 全部压掉
        assert!(!position_emit_due(1_400, 1_000));
        // 满 500ms 放行
        assert!(position_emit_due(1_500, 1_000));
        // 时钟回拨（now < last）不 panic、也不放开节流：saturating 兜住溢出，
        // 结果维持"不广播"，等时间追上再说。
        assert!(!position_emit_due(0, u64::MAX));
    }

    #[test]
    fn snake_case_mapping_matches_tauri_convention() {
        // Tauri 只做 camelCase→snake_case，名字不同就收不到值（P1-17 的成因）。
        // 这里锚定我们依赖的映射规则本身。
        let cases = [("trackIds", "track_ids"), ("state", "state"), ("from", "from")];
        for (camel, snake) in cases {
            let mut out = String::new();
            for c in camel.chars() {
                if c.is_ascii_uppercase() {
                    out.push('_');
                    out.push(c.to_ascii_lowercase());
                } else {
                    out.push(c);
                }
            }
            assert_eq!(out, snake);
        }
    }
}

/* ---------------- 应用状态 ---------------- */

/// 引擎回报的输出状态快照。
///
/// 与 `OutputConfig`（用户请求）分开存：用户要"独占 50ms 轮询"，设备可能给出
/// "共享 200ms 事件"甚至"独占但一帧都不消费"，这三种状态对用户的含义完全不同。
#[derive(Debug, Clone, Default)]
struct AudioStateSnapshot {
    requested: String,
    mode: String,
    clock: String,
    buffer_ms: u32,
    format: String,
    /// 降级/回落/自愈的原因（空 = 完全按请求生效）
    reason: String,
}

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
    /// 扫描取消请求（library_scan_cancel 置位，扫描循环轮询后自行收尾）
    scan_cancel: Arc<AtomicBool>,
    /// 队列状态行上次落库时间（毫秒，事件泵 5s 节流用）
    last_persist_ms: std::sync::atomic::AtomicU64,
    /// SMTC 通道（smtc::spawn 成功才有）；emit_player_state 处推送
    smtc_tx: Mutex<Option<std::sync::mpsc::Sender<smtc::SmtcUpdate>>>,
    /// 上次推给 SMTC 的曲目 id（换曲才重发 metadata + 查封面）
    smtc_track_id: Mutex<String>,
    /// SMTC 可用性（诊断页）
    smtc_active: AtomicBool,
    /// 当前是否在防系统睡眠（诊断页；事件泵线程维护）
    sleep_prevented: AtomicBool,
    /// 托盘是否已创建（诊断页）
    tray_active: AtomicBool,
    /// 实际生效的输出状态（诊断页/设置页展示"设备到底给了什么"）。
    /// 与设置里的"请求值"分开：设备可能拒绝独占而回落共享，也可能因为
    /// 缓冲太深而跑不出数据（引擎会自动降级并把原因写进 reason）。
    audio_state: Mutex<AudioStateSnapshot>,
    /// 列表查询耗时环（诊断页"最近查询"）。
    ///
    /// 为什么是固定 20 条：这张表只用来判断"最近是不是变慢了"，不是审计。
    /// 留多了要滚动、要分页，反而没人看。
    timings: Mutex<VecDeque<dto::ListTiming>>,
    /// app_settings 的内存镜像。
    ///
    /// 为什么必须有：事件泵每 25ms 跑一拍，`preventSleep` / `normalizeVolume`
    /// 这类开关每拍都要读。走 DB 就是每拍一次跨线程往返（DB 单 worker 串行），
    /// 泵会被自己的读库拖慢。启动时灌一次，`app_set_settings` 写入时同步。
    settings: Mutex<Value>,
}

/// 列表耗时环的容量。UI 只展示最近 8 条，留 20 条给"翻两页才看出趋势"。
const TIMING_LIMIT: usize = 20;

impl AppState {
    /// 记一条列表耗时（环形，满了丢最旧）。
    fn push_timing(&self, timing: dto::ListTiming) {
        let mut cell = self
            .timings
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if cell.len() == TIMING_LIMIT {
            cell.pop_front();
        }
        cell.push_back(timing);
    }

    /// 诊断页快照（旧 → 新）。
    fn timings_recent(&self) -> Vec<dto::ListTiming> {
        self.timings
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .iter()
            .cloned()
            .collect()
    }

    fn db(&self) -> std::sync::MutexGuard<'_, aiya_data::DbHandle> {
        self.db
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// 读设置缓存里的布尔项（缓存未初始化时按 default）。
    fn setting_flag(&self, key: &str, default: bool) -> bool {
        self.settings
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(key)
            .and_then(Value::as_bool)
            .unwrap_or(default)
    }

    /// 输出配置（设置缓存 → 引擎类型）。
    ///
    /// 兼容两代字段：老 `exclusiveMode: bool`（UI 尚未升级时仍会写它）与新
    /// `output: { mode, bufferMs, clock, releaseOnPause }`。两代都读不到时用默认
    /// （共享 200ms 事件驱动）——配置坏了不能变成"没声音"。
    fn setting_output(&self) -> aiya_audio::OutputConfig {
        let s = self
            .settings
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(o) = s.get("output") {
            return aiya_audio::OutputConfig::from_parts(
                o.get("mode").and_then(Value::as_str).unwrap_or("shared"),
                o.get("bufferMs").and_then(Value::as_i64).unwrap_or(0),
                o.get("clock").and_then(Value::as_str).unwrap_or("auto"),
                o.get("releaseOnPause").and_then(Value::as_bool).unwrap_or(true),
            );
        }
        if s.get("exclusiveMode").and_then(Value::as_bool).unwrap_or(false) {
            return aiya_audio::OutputConfig::from_parts("exclusive", 0, "auto", true);
        }
        aiya_audio::OutputConfig::default()
    }

    /// 用最新设置整体替换缓存（`app_set_settings` 落库成功后调用）
    fn refresh_settings(&self, settings: &Value) {
        *self
            .settings
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = settings.clone();
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

/// 批量收集 ID 的硬上限（防极端库把队列撑爆；超限会记 warn）。
const ID_COLLECT_CAP: usize = 50_000;

const DEFAULT_SETTINGS: &str = r#"{
  "theme": "dark",
  "density": "comfortable",
  "accentFromArtwork": true,
  "normalizeVolume": false,
  "spectrum": false,
  "spectrumEngine": "pro",
  "mediaKeys": true,
  "output": { "mode": "shared", "bufferMs": 0, "clock": "auto", "releaseOnPause": true },
  "autoplayOnStart": false,
  "rememberPosition": true,
  "preventSleep": true,
  "closeToTray": false,
  "shortcuts": {}
}"#;

/* ---------------- app 命令 ---------------- */

#[tauri::command]
fn app_info(state: State<'_, AppState>) -> Result<dto::AppInfo, dto::ApiError> {
    Ok(dto::AppInfo {
        version: env!("CARGO_PKG_VERSION").into(),
        electron: "tauri".into(),
        platform: std::env::consts::OS.into(),
        data_dir: state
            .db_path
            .parent()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default(),
    })
}

/// 频谱 tap 订阅开关（docs/04 §5.1：订阅制，关闭时引擎内零开销）。
/// UI 频谱面板打开/关闭时调用。
#[tauri::command]
fn spectrum_set(
    state: State<'_, AppState>,
    enabled: bool,
) -> Result<dto::SpectrumToggle, dto::ApiError> {
    engine_send(&state, aiya_audio::EngineCommand::SetSpectrum(enabled));
    Ok(dto::SpectrumToggle { enabled })
}

#[tauri::command]
fn app_get_ui_state(state: State<'_, AppState>) -> Result<Json, dto::ApiError> {
    state
        .db()
        .send(|conn| Ok(setting_get(conn, "ui_state").unwrap_or_else(|| json!({}))))
        .map_err(dto::ApiError::from)
}

/// 注意参数名：Tauri 2 按 camelCase 匹配 JS 的 key，UI 侧的 compat 层发的是
/// `{ state }`。此前 Rust 参数叫 `ui_state`（期望 `uiState`）→ 每次调用都
/// "missing required key" → 视图/滚动/侧栏状态永不落盘，且每 600ms 抛一个
/// unhandled rejection。注入参数改名（app_state）以腾出 `state`。
#[tauri::command]
fn app_set_ui_state(
    app_state: State<'_, AppState>,
    state: Json,
) -> Result<(), dto::ApiError> {
    app_state
        .db()
        .send(move |conn| setting_set(conn, "ui_state", &state).map_err(aiya_data::DbError::Sql))
        .map_err(dto::ApiError::from)
}

#[tauri::command]
fn app_get_settings(state: State<'_, AppState>) -> Result<Json, dto::ApiError> {
    let default: Json = serde_json::from_str(DEFAULT_SETTINGS).expect("valid default");
    state
        .db()
        .send(move |conn| Ok(setting_get(conn, "app_settings").unwrap_or(default)))
        .map_err(dto::ApiError::from)
}

#[tauri::command]
fn app_set_settings(state: State<'_, AppState>, settings: Json) -> Result<Json, dto::ApiError> {
    // "改完立即生效"的项在这里做 diff：只写库不通知引擎的话，用户切了独占要等
    // 下次换曲才生效，体感是"设置没反应"。
    //
    // 输出配置**整体比对**：模式/时钟/缓冲是一组参数，分别下发会多重建一次流
    // （每次重建都要丢掉设备缓冲里几十毫秒的音频），而且中间态没有意义。
    let prev_output = state.setting_output();
    let want_normalize = settings.get("normalizeVolume").and_then(Value::as_bool);
    let prev_normalize = state.setting_flag("normalizeVolume", false);

    // 落库失败直接返回错误：缓存不能先刷（否则"缓存新、库里旧"，
    // 下次启动会莫名回退），副作用也不该执行。
    state
        .db()
        .send({
            let settings = settings.clone();
            move |conn| {
                setting_set(conn, "app_settings", &settings).map_err(aiya_data::DbError::Sql)?;
                Ok(settings)
            }
        })
        .map_err(dto::ApiError::from)?;
    // 落库成功才刷新缓存：写失败时缓存保持旧值，与实际生效行为一致
    state.refresh_settings(&settings);

    // 落库成功后 setting_output() 读的就是新值，直接比对
    let next_output = state.setting_output();
    if next_output != prev_output {
        engine_send(&state, aiya_audio::EngineCommand::SetOutputConfig(next_output));
        // 切回共享/静音是确定的结果；实际状态会由随后到的 AudioMode 确认
        if next_output.mode == aiya_audio::ModePref::Shared {
            let mut st = state
                .audio_state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            st.mode = "shared".into();
            st.requested = "shared".into();
            st.reason.clear();
        }
    }

    // 归一化开关：改完立即重算音量（不等下一曲）。Actor 持有 gain_db，
    // 只需翻开关 + 把新音量送给引擎。
    if let Some(on) = want_normalize {
        if on != prev_normalize {
            let mut actor = state
                .player
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            actor.set_normalize(on);
            let v = actor.engine_volume();
            drop(actor);
            engine_send(&state, aiya_audio::EngineCommand::SetVolume(v));
        }
    }
    Ok(settings)
}

/// 诊断页。v1 第一切：统计字段真实，引擎/系统字段为占位（P2/P4 回填）。
#[tauri::command]
fn app_diagnostics(state: State<'_, AppState>) -> Result<dto::Diagnostics, dto::ApiError> {
    let db_path = state.db_path.clone();
    let smtc_active = state.smtc_active.load(Ordering::Relaxed);
    let sleep_prevented = state.sleep_prevented.load(Ordering::Relaxed);
    let tray_active = state.tray_active.load(Ordering::Relaxed);
    let audio = state
        .audio_state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    // 必须在进入 db 闭包之前读（同一个 DB 线程，闭包里再 send 会死锁）
    let desired = state.setting_output();
    // 同样必须在 db 闭包之前取：闭包跑在 DB 单 worker 上，不能再回头锁别的
    let timings = state.timings_recent();
    let recent = dto::recent_errors();
    state
        .db()
        .send(move |conn| {
            let count = |sql: &str| -> i64 { conn.query_row(sql, [], |r| r.get(0)).unwrap_or(0) };
            let db_bytes = std::fs::metadata(&db_path).map(|m| m.len()).unwrap_or(0) as i64
                + std::fs::metadata(db_path.with_extension("sqlite-wal"))
                    .map(|m| m.len())
                    .unwrap_or(0) as i64;
            Ok(dto::Diagnostics {
                version: env!("CARGO_PKG_VERSION").into(),
                electron: "tauri".into(),
                platform: std::env::consts::OS.into(),
                data_dir: db_path
                    .parent()
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                // 日志位置：出问题时让用户直接把这两个路径发过来
                log_dir: logging::dir().map(|p| p.to_string_lossy().into_owned()),
                log_file: logging::dir().map(|p| {
                    p.join(format!("aiya-{}.log", logging::today()))
                        .to_string_lossy()
                        .into_owned()
                }),
                schema_version: conn
                    .query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                    .unwrap_or(0),
                db_bytes,
                tracks: count("SELECT COUNT(*) FROM tracks"),
                albums: count("SELECT COUNT(*) FROM albums"),
                artists: count("SELECT COUNT(*) FROM artists"),
                playlists: count("SELECT COUNT(*) FROM playlists"),
                history_rows: count("SELECT COUNT(*) FROM history"),
                total_duration_ms: count("SELECT COALESCE(SUM(duration_ms),0) FROM tracks"),
                // 引擎侧目前只暴露 written_frames，没有流计数；保留结构不编数字
                streams: dto::StreamsInfo { opened: 0, closed: 0, active: 0 },
                list_timings: timings,
                recent_errors: recent,
                media_keys: dto::MediaKeysInfo {
                    enabled: smtc_active,
                    active: if smtc_active { vec!["SMTC".into()] } else { Vec::new() },
                    failed: if smtc_active {
                        Vec::new()
                    } else {
                        vec!["SMTC 初始化失败".into()]
                    },
                },
                power: dto::PowerInfo {
                    active: sleep_prevented,
                    kind: sleep_prevented.then(|| "SetThreadExecutionState".into()),
                },
                tray: tray_active,
                audio_mode: dto::AudioModeInfo {
                    requested: audio.requested,
                    mode: audio.mode,
                    clock: audio.clock,
                    buffer_ms: audio.buffer_ms,
                    format: audio.format,
                    note: Some(audio.reason).filter(|s| !s.is_empty()),
                    desired: dto::DesiredOutput {
                        mode: desired.mode.as_str().into(),
                        buffer_ms: desired.buffer_ms,
                        clock: desired.clock.as_str().into(),
                        release_on_pause: desired.release_on_pause,
                    },
                },
            })
        })
        .map_err(dto::ApiError::from)

}

/// 在资源管理器中打开数据目录。打不开也不算失败（用户就是想看看文件在哪，
/// 弹个错误框毫无帮助），所以恒返回 `Ok`。
#[tauri::command]
fn app_open_data_dir(state: State<'_, AppState>) -> Result<(), dto::ApiError> {
    if let Some(dir) = state.db_path.parent() {
        let _ = std::process::Command::new("explorer").arg(dir).spawn();
    }
    Ok(())
}

/* ---------------- library 命令 ---------------- */

/// beets 环境状态（设置页诊断区）。低频查询，每次现查（探测 + 一次 status 往返）。
/// P1：非 async 命令在主线程执行——probe 逐候选解释器尝试最多 ~30s，
/// 曾把主线程整个冻结。spawn_blocking 卸载。
///
/// 返回 `Result` 只是形态统一：`probe` 自己从不失败（失败信息进
/// `error_code` / `error_message` 字段），这里唯一的错误源是工作线程崩了。
#[tauri::command]
async fn beets_status(app: AppHandle) -> Result<dto::beets::BeetsStatus, dto::ApiError> {
    tauri::async_runtime::spawn_blocking(move || beets::probe(&app))
        .await
        .map_err(dto::ApiError::internal)
}

/// 启动一次只读预览（B1）：事件流走 `beets:import`。
/// 未装 beets → `beetsMissing`（UI 显示安装指引）。
/// P1：探测（pick_interpreter）最多 ~30s 阻塞，spawn_blocking 卸载。
#[tauri::command]
async fn beets_preview(
    app: AppHandle,
    paths: Vec<String>,
) -> Result<dto::beets::JobStarted, dto::ApiError> {
    tauri::async_runtime::spawn_blocking(move || beets::start_preview(app, paths))
        .await
        .map_err(dto::ApiError::internal)?
        .map_err(dto::ApiError::beets_missing)?;
    Ok(dto::beets::JobStarted { started: true })
}

/// B2：应用导入决策（写标签 + 入 beets 库，不移动文件）。
/// `decisions` 保持 `Vec<Json>`：它是预览结果**原样回传**给 sidecar 的载荷
/// （含 `candidate` 深层对象），宿主只透传不解读——在这里定形状等于
/// 让宿主编译期依赖 beets 的返回结构，而那份结构是 sidecar 的事（docs/17 §3.2）。
/// P1：探测最多 ~30s 阻塞，spawn_blocking 卸载。
#[tauri::command]
async fn beets_apply(
    app: AppHandle,
    decisions: Vec<Json>,
) -> Result<dto::beets::JobStarted, dto::ApiError> {
    let (db_path, scanning) = {
        let Some(state) = app.try_state::<AppState>() else {
            return Err(dto::ApiError::internal("app state 不可用"));
        };
        (state.db_path.clone(), state.scanning.clone())
    };
    tauri::async_runtime::spawn_blocking(move || {
        let refresh = beets::RefreshParams { db_path, scanning };
        beets::start_apply(app, decisions, Some(refresh))
    })
    .await
    .map_err(dto::ApiError::internal)?
    .map_err(dto::ApiError::beets_missing)?;
    Ok(dto::beets::JobStarted { started: true })
}

/// 取消当前 beets 导入（preview/apply）：sidecar 在下个检查点收尾，
/// 结果走 `cancelled` 事件（UI 据此解锁按钮）。
#[tauri::command]
fn beets_cancel() -> Result<dto::beets::CancelRequested, dto::ApiError> {
    Ok(dto::beets::CancelRequested { requested: beets::cancel_session() })
}

/// 手动触发一次 beets → 自有读模型的全量对齐（异步，结果走 librarySynced 事件）。
#[tauri::command]
fn beets_sync(app: AppHandle, state: State<'_, AppState>) -> Result<dto::beets::JobStarted, dto::ApiError> {
    beets::sync_now(
        app,
        beets::RefreshParams {
            db_path: state.db_path.clone(),
            scanning: state.scanning.clone(),
        },
    );
    Ok(dto::beets::JobStarted { started: true })
}

#[tauri::command]
fn library_list(
    state: State<'_, AppState>,
    query: Json,
) -> Result<aiya_data::repo::TrackPage, dto::ApiError> {
    // 端到端计时起点：含 DB 线程排队（TrackPage::took_ms 只含 SQL）
    let started = std::time::Instant::now();
    // 参数仍然是 Value + 手动反序列化：形状不对时由我们给出 `invalidArg`
    // 分类，而不是把 Tauri 自己的反序列化文案抛给 UI（分类权要留在自己手里）。
    let q: TrackWindowQuery = serde_json::from_value(query)
        .map_err(|e| dto::ApiError::invalid_arg(format!("查询参数不合法：{e}")))?;
    // 埋点要用的元信息必须在 `q` 被 move 进闭包之前取出
    let source = source_label(&q.source);
    let cursor = q.cursor.clone();
    let page = state
        .db()
        .send(move |conn| repo::list_tracks(conn, &q))
        .map_err(dto::ApiError::from)?;
    state.push_timing(dto::ListTiming {
        at_ms: now_ms_u64() as i64,
        source,
        rows: page.items.len() as i64,
        total: page.total,
        took_ms: started.elapsed().as_secs_f64() * 1000.0,
        db_took_ms: page.took_ms as f64,
        cursor,
    });
    Ok(page)
}

/// `TrackSource` → 诊断页"来源"那一列的短标签。
///
/// 不走 serde：序列化出来是 `{"kind":"playlist","playlistId":"..."}` 一整坨，
/// 而这张表只留一列。歌单视图下知道"是 playlist"就够，具体哪个看界面。
fn source_label(source: &aiya_data::repo::TrackSource) -> String {
    use aiya_data::repo::TrackSource;
    match source {
        TrackSource::Library => "library".into(),
        TrackSource::Playlist { .. } => "playlist".into(),
        TrackSource::Recent => "recent".into(),
        TrackSource::MostPlayed => "mostPlayed".into(),
    }
}

/// 单曲/多曲查询（播放条数据源）：带出音频规格。列表查询不带（契约约定）。
#[tauri::command]
fn library_tracks(
    state: State<'_, AppState>,
    ids: Vec<String>,
) -> Result<Vec<dto::TrackDetail>, dto::ApiError> {
    state
        .db()
        .send(move |conn| {
            let mut out = Vec::with_capacity(ids.len());
            for id in &ids {
                // 列顺序必须与 `aiya_data::repo::row_to_track` 一致（0..=16），
                // 音频规格接在后面（17..=22）。此前这里是手拼 17 个字段的 json!，
                // 列一改就静默错位。
                let row = conn.query_row(
                    "SELECT t.id, t.title, t.artist_id, ar.name, t.album_id, al.title, al_ar.name,
                            t.duration_ms, t.track_no, t.disc_no, t.year, t.added_at, t.artwork_id,
                            t.play_count, t.last_played_at, t.gain_db, t.availability,
                            t.codec, t.container, t.sample_rate, t.bits_per_sample, t.bitrate, t.channels
                     FROM tracks t
                     LEFT JOIN artists ar ON ar.id = t.artist_id
                     LEFT JOIN albums al ON al.id = t.album_id
                     LEFT JOIN artists al_ar ON al_ar.id = al.artist_id
                     WHERE t.id = ?1",
                    [id],
                    |r| {
                        let track = aiya_data::repo::row_to_track(r)?;
                        let codec: Option<String> = r.get(17)?;
                        let container: Option<String> = r.get(18)?;
                        let sample_rate: Option<i64> = r.get(19)?;
                        // 全空 → None（标签里没有规格信息），而不是发一堆 null
                        let audio = if codec.is_none() && sample_rate.is_none() {
                            None
                        } else {
                            let lossless =
                                aiya_player::TrackAudio::infer_lossless(codec.as_deref(), container.as_deref());
                            Some(aiya_player::TrackAudio {
                                codec,
                                container,
                                sample_rate,
                                bits_per_sample: r.get::<_, Option<i64>>(20)?,
                                bitrate: r.get::<_, Option<i64>>(21)?,
                                channels: r.get::<_, Option<i64>>(22)?,
                                lossless,
                            })
                        };
                        Ok(dto::TrackDetail::new(track, audio))
                    },
                );
                match row {
                    Ok(v) => out.push(v),
                    // 查不到先跳过：批量查询里缺一首不该让整页失败
                    Err(rusqlite::Error::QueryReturnedNoRows) => {}
                    Err(e) => return Err(aiya_data::DbError::Sql(e)),
                }
            }
            Ok(out)
        })
        .map_err(dto::ApiError::from)
}

#[tauri::command]
fn library_stats(state: State<'_, AppState>) -> Result<dto::LibraryStats, dto::ApiError> {
    let db_path = state.db_path.clone();
    state.db().send(move |conn| {
        let count = |sql: &str| -> i64 { conn.query_row(sql, [], |r| r.get(0)).unwrap_or(0) };
        let db_bytes = std::fs::metadata(&db_path).map(|m| m.len()).unwrap_or(0) as i64;
        Ok(dto::LibraryStats {
            tracks: count("SELECT COUNT(*) FROM tracks"),
            albums: count("SELECT COUNT(*) FROM albums"),
            artists: count("SELECT COUNT(*) FROM artists"),
            // locations / missingLocations 对应旧仓库的 media_locations 两列：
            // 新 schema 把"文件位置"并进 tracks（一曲一位置），所以
            // locations = 曲目数，missingLocations = 非 online 的位置数。
            // 此前两个数都是写死的 0/1 → 侧栏永远不提示"位置缺失"，
            // 用户看着满列表的歌，其实一半文件已经不在盘上了。
            locations: count("SELECT COUNT(*) FROM tracks"),
            missing_locations: count("SELECT COUNT(*) FROM tracks WHERE availability <> 'online'"),
            total_duration_ms: count("SELECT COALESCE(SUM(duration_ms),0) FROM tracks"),
            db_bytes,
            schema_version: conn
                .query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                .unwrap_or(0),
        })
    })
    .map_err(dto::ApiError::from)
}

/// 扫描：阻塞直到完成（进度经 `scan:progress` 事件推送，UI 侧节流渲染）。
/// 用独立连接（WAL 下与主 DB 线程并行），不占用 DbHandle。
/// Tauri 要求 async 命令带引用参数（`State<'_, _>`）必须返回 Result。
#[tauri::command]
async fn library_scan(
    app: AppHandle,
    state: State<'_, AppState>,
    options: Option<Json>,
) -> Result<aiya_data::scanner::ScanResult, dto::ApiError> {
    library_scan_body(app, &state, options).await
}

async fn library_scan_body(
    app: AppHandle,
    state: &State<'_, AppState>,
    options: Option<Json>,
) -> Result<aiya_data::scanner::ScanResult, dto::ApiError> {
    if state.scanning.swap(true, Ordering::SeqCst) {
        return Err(dto::ApiError::busy("已有扫描在进行"));
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
        return Err(dto::ApiError::invalid_arg(format!("目录不存在：{}", root.display())));
    }

    let db_path = state.db_path.clone();
    let artwork_dir = state.artwork_dir.clone();
    let handle = app.clone();
    // 取消标志：新扫描开始前清掉上一次的残留请求
    state.scan_cancel.store(false, Ordering::SeqCst);
    let scan_cancel = state.scan_cancel.clone();

    let result = tauri::async_runtime::spawn_blocking(
        move || -> Result<aiya_data::scanner::ScanResult, dto::ApiError> {
            let mut conn = aiya_data::open_conn(&db_path).map_err(dto::ApiError::internal)?;
            let app2 = handle.clone();
            // 宿主侧 100ms 节流（P2：此前每条目一次 IPC 序列化，10 万文件 =
            // 10 万次跨进程 emit；UI 侧节流只省渲染不省传输）
            let last_emit_ms = std::sync::atomic::AtomicU64::new(0);
            let scan_result = scanner::scan_directory_ext(
                &mut conn,
                &root,
                Some(&artwork_dir),
                || scan_cancel.load(Ordering::Relaxed),
                move |p| {
                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis() as u64;
                    let last = last_emit_ms.load(Ordering::Relaxed);
                    if now >= last + 100 {
                        last_emit_ms.store(now, Ordering::Relaxed);
                        if let Ok(v) = serde_json::to_value(&p) {
                            let _ = app2.emit("scan:progress", v);
                        }
                    }
                },
            );
            let _ = conn.close();
            scan_result.map_err(dto::ApiError::internal)
        },
    )
    .await
    .map_err(dto::ApiError::internal)?;

    // 无论成功失败都要放开互斥，否则一次失败之后整个会话都扫不了库
    state.scanning.store(false, Ordering::SeqCst);
    result
}

#[tauri::command]
fn library_scan_cancel(state: State<'_, AppState>) -> Result<dto::library::ScanCancel, dto::ApiError> {
    // 置位取消请求，扫描循环在 walk 每条目 / 每批事务边界自行收尾
    //（取消路径不执行 mark_missing，防止误标在线文件）。
    if state.scanning.load(Ordering::SeqCst) {
        state.scan_cancel.store(true, Ordering::SeqCst);
        Ok(dto::library::ScanCancel { cancelled: true })
    } else {
        Ok(dto::library::ScanCancel { cancelled: false })
    }
}

/// 选目录并持久化为 `library.root`（UI 的 startScan 不带 path，扫描时从这里取）。
/// P1：blocking_pick_folder 在主线程调用有死锁风险（tauri-plugin-dialog
/// 明确警告），spawn_blocking 卸载到工作线程。
///
/// 用户取消选择返回 `Ok(None)`（→ null），不是错误：取消是正常操作。
#[tauri::command]
async fn library_choose_root(
    app: AppHandle,
) -> Result<Option<dto::library::ChosenRoot>, dto::ApiError> {
    use tauri_plugin_dialog::DialogExt;
    let dialog_app = app.clone();
    let picked = tauri::async_runtime::spawn_blocking(move || {
        dialog_app
            .dialog()
            .file()
            .blocking_pick_folder()
            .and_then(|p| p.as_path().map(|pp| pp.to_path_buf()))
    })
    .await
    .unwrap_or(None);
    let Some(p) = picked else {
        return Ok(None);
    };
    let path_str = p.to_string_lossy().into_owned();
    let to_save = path_str.clone();
    let Some(state) = app.try_state::<AppState>() else {
        return Err(dto::ApiError::internal("app state 不可用"));
    };
    let saved: aiya_data::DbResult<()> = state.db().send(move |conn| {
        setting_set(conn, "library.root", &json!(to_save)).map_err(aiya_data::DbError::Sql)
    });
    saved.map_err(|e| dto::ApiError::internal(format!("无法保存曲库目录：{e}")))?;
    Ok(Some(dto::library::ChosenRoot { path: path_str }))
}

#[tauri::command]
fn library_reveal_track(
    state: State<'_, AppState>,
    track_id: String,
) -> Result<dto::library::RevealResult, dto::ApiError> {
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
            Ok(dto::library::RevealResult {
                revealed,
                reason: if revealed { None } else { Some("explorer 启动失败".into()) },
            })
        }
        Ok(None) => Ok(dto::library::RevealResult {
            revealed: false,
            reason: Some("曲目不在库中".into()),
        }),
        Err(e) => Err(dto::ApiError::from(e)),
    }
}

/// `indices` 的两种形态：字符串 `"all"` 或 0 基下标数组。
///
/// 用 untagged 枚举接住而不是 `Value` 里手工试：解析失败会直接给出
/// `invalidArg`，而不是"传错了就当没选"——后者会让多选删歌静默变成
/// "一条都没选"（用户以为删掉了，其实什么都没发生）。
#[derive(serde::Deserialize)]
#[serde(untagged)]
enum SelectionIndices {
    Tag(String),
    List(Vec<usize>),
}

/// 批量选择请求。`query` 形状是 `TrackWindowQuery`，与 `library_list` 共用。
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct SelectionRequest {
    #[serde(default)]
    query: Json,
    #[serde(default)]
    indices: Option<SelectionIndices>,
}

/// 批量选择 → ID。indices 为字符串 "all" 或下标数组。
#[tauri::command]
fn library_ids_for_selection(
    state: State<'_, AppState>,
    request: Json,
) -> Result<Vec<String>, dto::ApiError> {
    let req: SelectionRequest = serde_json::from_value(request)
        .map_err(|e| dto::ApiError::invalid_arg(format!("选区请求不合法：{e}")))?;
    let query: TrackWindowQuery = serde_json::from_value(req.query)
        .map_err(|e| dto::ApiError::invalid_arg(format!("查询参数不合法：{e}")))?;
    let (select_all, indices) = match req.indices {
        None => (false, Vec::new()),
        Some(SelectionIndices::Tag(tag)) if tag == "all" => (true, Vec::new()),
        Some(SelectionIndices::Tag(other)) => {
            return Err(dto::ApiError::invalid_arg(format!(
                "indices 只接受 \"all\" 或下标数组，收到 \"{other}\""
            )))
        }
        Some(SelectionIndices::List(v)) => (false, v),
    };

    state
        .db()
        .send(move |conn| {
            // 批量取全量 ID 统一走 collect_track_ids：它按来源选择推进方式
            // （library=游标 / 歌单等=offset）。此前这里只写 `q.cursor`，而 UI
            // 传进来的 query 恒为 offset 模式 → cursor 被忽略 → 每轮同一页 →
            // 死循环占死 DB 线程，任何一次多选批量操作都会冻死整个 App。
            let collected = repo::collect_track_ids(conn, &query, ID_COLLECT_CAP)?;
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
        .map_err(dto::ApiError::from)
}

#[tauri::command]
fn library_remove_tracks(
    state: State<'_, AppState>,
    track_ids: Vec<String>,
) -> Result<dto::library::RemoveTracks, dto::ApiError> {
    state
        .db()
        .send(move |conn| {
            let mut removed = 0i64;
            for id in &track_ids {
                removed += conn.execute("DELETE FROM tracks WHERE id = ?1", [id]).unwrap_or(0) as i64;
            }
            Ok(dto::library::RemoveTracks { removed })
        })
        .map_err(dto::ApiError::from)
}

/* ---------------- playlist 命令 ---------------- */

/// 摘要查询：行字段 + 3 个聚合子查询（曲目数 / 总时长 / 首个有封面的曲目）。
/// 顺序由 `sort_position`（0002 迁移新增）承载 —— 侧栏拖动排序要能持久化，
/// 靠 created_at 假装顺序的话重启就回到创建顺序。
const PLAYLIST_SELECT: &str = "SELECT p.id, p.name, p.created_at, p.updated_at, p.sort_position,
       (SELECT COUNT(*) FROM playlist_items pi JOIN tracks t ON t.id = pi.track_id
         WHERE pi.playlist_id = p.id),
       (SELECT COALESCE(SUM(t.duration_ms),0) FROM playlist_items pi JOIN tracks t ON t.id = pi.track_id
         WHERE pi.playlist_id = p.id),
       (SELECT t2.artwork_id FROM playlist_items pi2 JOIN tracks t2 ON t2.id = pi2.track_id
         WHERE pi2.playlist_id = p.id ORDER BY pi2.position LIMIT 1)
  FROM playlists p";

/// 一行 → 摘要。时间列是 **unix 秒**，在出口处换毫秒并改名
/// （`createdAtMs`）——DB 不动，换算只发生在这一处（docs/17 §3.1）。
fn playlist_row(r: &rusqlite::Row) -> rusqlite::Result<dto::playlist::PlaylistSummary> {
    Ok(dto::playlist::PlaylistSummary {
        id: r.get(0)?,
        name: r.get(1)?,
        // schema v1 没有 description 列，恒 null（字段留着是为了形状不变）
        description: None,
        created_at_ms: r.get::<_, i64>(2)? * 1000,
        updated_at_ms: r.get::<_, i64>(3)? * 1000,
        sort_position: r.get(4)?,
        track_count: r.get(5)?,
        duration_ms: r.get(6)?,
        artwork_id: r.get(7)?,
    })
}

fn playlist_summary_by_id(
    conn: &rusqlite::Connection,
    id: &str,
) -> rusqlite::Result<Option<dto::playlist::PlaylistSummary>> {
    let sql = format!("{PLAYLIST_SELECT} WHERE p.id = ?1");
    match conn.query_row(&sql, [id], playlist_row) {
        Ok(v) => Ok(Some(v)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e),
    }
}

fn playlist_list_all(
    conn: &rusqlite::Connection,
) -> aiya_data::DbResult<Vec<dto::playlist::PlaylistSummary>> {
    let sql = format!("{PLAYLIST_SELECT} ORDER BY p.sort_position ASC, p.created_at ASC");
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([], playlist_row)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

/// 写命令统一骨架：闭包 `None` 表示"歌单不存在" → `notFound`。
///
/// 歌单不存在是**可预期**的结果（另一个窗口删掉了、拖拽时列表过期），
/// 所以走 `Err` 而不是 `Ok(null)`：UI 那边据此提示"歌单已被删除"，
/// 而不是把 null 当成空歌单渲染出一堆 `undefined`。
fn playlist_write<T, F>(state: &State<'_, AppState>, f: F) -> Result<T, dto::ApiError>
where
    F: FnOnce(&mut rusqlite::Connection) -> aiya_data::DbResult<Option<T>> + Send + 'static,
    T: Send + 'static,
{
    match state.db().send(f) {
        Ok(Some(v)) => Ok(v),
        Ok(None) => Err(dto::ApiError::not_found("歌单不存在")),
        Err(e) => Err(dto::ApiError::from(e)),
    }
}

#[tauri::command]
fn playlist_list(
    state: State<'_, AppState>,
) -> Result<Vec<dto::playlist::PlaylistSummary>, dto::ApiError> {
    state.db().send(|conn| playlist_list_all(conn)).map_err(dto::ApiError::from)
}

#[tauri::command]
fn playlist_detail(
    state: State<'_, AppState>,
    id: String,
) -> Result<Option<dto::playlist::PlaylistSummary>, dto::ApiError> {
    state
        .db()
        .send(move |conn| {
            playlist_summary_by_id(conn, &id).map_err(aiya_data::DbError::Sql)
        })
        .map_err(dto::ApiError::from)
}

#[tauri::command]
fn playlist_create(
    state: State<'_, AppState>,
    name: String,
    track_ids: Option<Vec<String>>,
) -> Result<dto::playlist::PlaylistSummary, dto::ApiError> {
    let name = playlist_repo::sanitize_name(&name)
        .map_err(|e| dto::ApiError::invalid_arg(e.to_string()))?;
    let track_ids = track_ids.unwrap_or_default();
    // 歌单 id 由名字派生（同名即同一歌单，重复创建不重复建行）
    let pid = ids::playlist_id(&name);
    let now = now_secs();
    let pid_for_db = pid.clone();
    playlist_write(&state, move |conn| {
        playlist_repo::create(conn, &pid_for_db, &name, &track_ids, now)?;
        // 建完读不到 = 事务里出了意料之外的事，不是"歌单不存在"
        Ok(Some(
            playlist_summary_by_id(conn, &pid_for_db)
                .map_err(aiya_data::DbError::Sql)?
                .ok_or_else(|| aiya_data::DbError::Custom("歌单创建后读不到".into()))?,
        ))
    })
}

#[tauri::command]
fn playlist_rename(
    state: State<'_, AppState>,
    id: String,
    name: String,
) -> Result<dto::playlist::PlaylistSummary, dto::ApiError> {
    let name = playlist_repo::sanitize_name(&name)
        .map_err(|e| dto::ApiError::invalid_arg(e.to_string()))?;
    let now = now_secs();
    playlist_write(&state, move |conn| {
        if !playlist_repo::rename(conn, &id, &name, now)? {
            return Ok(None);
        }
        playlist_summary_by_id(conn, &id).map_err(aiya_data::DbError::Sql)
    })
}

#[tauri::command]
fn playlist_delete(
    state: State<'_, AppState>,
    id: String,
) -> Result<dto::playlist::PlaylistDeleted, dto::ApiError> {
    state
        .db()
        .send(move |conn| {
            let deleted = playlist_repo::delete(conn, &id)?;
            Ok(dto::playlist::PlaylistDeleted { deleted })
        })
        .map_err(dto::ApiError::from)
}

/// 侧栏拖动排序。返回排序后的完整列表（UI 直接替换，省一次 roundtrip）。
#[tauri::command]
fn playlist_reorder(
    state: State<'_, AppState>,
    from: i64,
    to: i64,
) -> Result<Vec<dto::playlist::PlaylistSummary>, dto::ApiError> {
    state
        .db()
        .send(move |conn| {
            playlist_repo::reorder_playlists(conn, from, to)?;
            playlist_list_all(conn)
        })
        .map_err(dto::ApiError::from)
}

#[tauri::command]
fn playlist_add_tracks(
    state: State<'_, AppState>,
    id: String,
    track_ids: Vec<String>,
    mode: Option<String>,
) -> Result<dto::playlist::PlaylistAddResult, dto::ApiError> {
    let mode = playlist_repo::AddMode::parse(mode.as_deref())
        .map_err(|e| dto::ApiError::invalid_arg(e.to_string()))?;
    let now = now_secs();
    playlist_write(&state, move |conn| {
        let Some(outcome) = playlist_repo::add_tracks(conn, &id, &track_ids, mode, now)? else {
            return Ok(None);
        };
        let summary = playlist_summary_by_id(conn, &id)
            .map_err(aiya_data::DbError::Sql)?
            .ok_or_else(|| aiya_data::DbError::Custom("歌单在事务后消失".into()))?;
        Ok(Some(dto::playlist::PlaylistAddResult {
            added: outcome.added,
            skipped: outcome.skipped,
            summary,
        }))
    })
}

#[tauri::command]
fn playlist_remove_at(
    state: State<'_, AppState>,
    id: String,
    position: i64,
) -> Result<dto::playlist::PlaylistSummary, dto::ApiError> {
    let now = now_secs();
    playlist_write(&state, move |conn| {
        if playlist_repo::remove_at(conn, &id, position, now)?.is_none() {
            return Ok(None);
        }
        playlist_summary_by_id(conn, &id).map_err(aiya_data::DbError::Sql)
    })
}

#[tauri::command]
fn playlist_move(
    state: State<'_, AppState>,
    id: String,
    from: i64,
    to: i64,
) -> Result<dto::playlist::PlaylistSummary, dto::ApiError> {
    let now = now_secs();
    playlist_write(&state, move |conn| {
        if playlist_repo::move_item(conn, &id, from, to, now)?.is_none() {
            return Ok(None);
        }
        playlist_summary_by_id(conn, &id).map_err(aiya_data::DbError::Sql)
    })
}

#[tauri::command]
fn playlist_clear(
    state: State<'_, AppState>,
    id: String,
) -> Result<dto::playlist::PlaylistSummary, dto::ApiError> {
    let now = now_secs();
    playlist_write(&state, move |conn| {
        if !playlist_repo::clear(conn, &id, now)? {
            return Ok(None);
        }
        playlist_summary_by_id(conn, &id).map_err(aiya_data::DbError::Sql)
    })
}

/* ---------------- m3u / m3u8 导入导出（docs/22 G2） ---------------- */

/// 导入 m3u/m3u8 为歌单。`path = None` 时弹系统文件选择框（UI 默认路径），
/// `Some(path)` 直接导入（测试与恢复用）。歌单名 = 文件名去扩展名，
/// 与 `playlist_create` 同规则派生 id——同名重复导入 = 追加 + 去重，不重复建行。
///
/// 错误语义：
/// - 非 UTF-8 → `unsupported`（GBK 不做自动探测，docs/22 D5）
/// - 含 `#EXT-X-` 标签 → `unsupported`（HLS 流媒体 manifest 不是静态歌单，
///   该走 FM 频道功能）
/// - 一条都匹配不上 → **不是错误**，回 `playlist = null` + 完整 unmatched 清单
///   （清单是用户唯一的排查依据，塞进 ApiError 的 detail 就送不到结构化字段了）。
///   库里没扫过、路径全错、全被 `..` 之类坑过，都走这一条。
#[tauri::command]
async fn playlist_import(
    app: AppHandle,
    path: Option<String>,
) -> Result<dto::playlist::PlaylistImportResult, dto::ApiError> {
    use tauri_plugin_dialog::DialogExt;

    // 文件选择必须离开主线程（tauri-plugin-dialog 的 blocking API 主线程死锁，
    // 同 library_choose_root 的 P1 修正）
    let picked = match path {
        Some(p) => Some(PathBuf::from(p)),
        None => {
            let dialog_app = app.clone();
            tauri::async_runtime::spawn_blocking(move || {
                dialog_app
                    .dialog()
                    .file()
                    .add_filter("播放列表 (m3u/m3u8)", &["m3u", "m3u8"])
                    .blocking_pick_file()
                    .and_then(|p| p.as_path().map(|pp| pp.to_path_buf()))
            })
            .await
            .unwrap_or(None)
        }
    };
    let Some(file) = picked else {
        return Ok(dto::playlist::PlaylistImportResult {
            playlist: None,
            matched: 0,
            duplicates: 0,
            unmatched: Vec::new(),
        });
    };

    let bytes = std::fs::read(&file).map_err(|e| {
        dto::ApiError::new(dto::ApiErrorCode::Io, format!("读不到播放列表文件：{e}"))
    })?;
    let text = String::from_utf8(bytes).map_err(|_| {
        dto::ApiError::new(
            dto::ApiErrorCode::Unsupported,
            "播放列表不是 UTF-8 编码：m3u8 请保存为 UTF-8（GBK 编码暂不支持）",
        )
    })?;

    let base_dir = file.parent().unwrap_or(std::path::Path::new(".")).to_path_buf();
    let entries = aiya_data::playlist_io::parse_playlist(&text, &base_dir).map_err(|e| match e {
        aiya_data::playlist_io::ParseFailure::HlsManifest { tag } => dto::ApiError::new(
            dto::ApiErrorCode::Unsupported,
            format!("这是流媒体 manifest（HLS，#{tag}），不是静态播放列表——请用 FM 频道功能"),
        ),
        aiya_data::playlist_io::ParseFailure::Empty => {
            dto::ApiError::invalid_arg("文件里没有任何曲目条目")
        }
    })?;

    // 匹配：path_lower 精确对账（对账键与扫描器一致，见 0003 迁移）。
    // 斜杠方向都收：m3u 里常见的 "D:/m/a.flac" 与扫描落库的 "D:\m\a.flac"
    // 是同一个文件，失败在斜杠上等于白匹配。
    let state = app
        .try_state::<AppState>()
        .ok_or_else(|| dto::ApiError::internal("app state 不可用"))?;
    let file_for_ids = file.to_string_lossy().into_owned();
    let result: aiya_data::DbResult<dto::playlist::PlaylistImportResult> = state.db().send(move |conn| {
        let mut by_path_lower: std::collections::HashMap<String, String> =
            std::collections::HashMap::new();
        {
            let mut stmt =
                conn.prepare("SELECT path_lower, id FROM tracks")?;
            let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
            for row in rows {
                let (k, v) = row?;
                by_path_lower.insert(k, v);
            }
        }
        let lookup = |target: &std::path::Path| -> Option<String> {
            let s = target.to_string_lossy();
            by_path_lower.get(&s.to_lowercase())
                .cloned()
                .or_else(|| by_path_lower.get(&s.replace('/', "\\").to_lowercase()).cloned())
                .or_else(|| by_path_lower.get(&s.replace('\\', "/").to_lowercase()).cloned())
        };

        let mut matched_ids: Vec<String> = Vec::new();
        let mut unmatched = Vec::new();
        for e in &entries {
            match lookup(&e.target) {
                Some(id) => matched_ids.push(id),
                None => unmatched.push(dto::playlist::PlaylistImportUnmatched {
                    line: e.line as i64,
                    raw: e.raw.clone(),
                    reason: "notFound".into(),
                }),
            }
        }
        if matched_ids.is_empty() {
            // 一条都没匹配上：这是**正常结果**不是库错误（早期版本借
            // DbError::Custom 的文案再 contains 判回来，等于自己造了个
            // "解析文案"的反模式）。不建空歌单，清单原样带回去让 UI 说话。
            return Ok(dto::playlist::PlaylistImportResult {
                playlist: None,
                matched: 0,
                duplicates: 0,
                unmatched,
            });
        }

        // 歌单名 = 文件名去扩展名（create 内部已做去重/幂等）
        let name = file_stem_name(&file_for_ids);
        let pid = ids::playlist_id(&name);
        let now = now_secs();
        let outcome = playlist_repo::create(conn, &pid, &name, &matched_ids, now)?;
        let summary = playlist_summary_by_id(conn, &pid)
            .map_err(aiya_data::DbError::Sql)?
            .ok_or_else(|| aiya_data::DbError::Custom("歌单导入后读不到".into()))?;
        Ok(dto::playlist::PlaylistImportResult {
            playlist: Some(summary),
            matched: outcome.added,
            duplicates: outcome.skipped,
            unmatched,
        })
    });
    result.map_err(dto::ApiError::from)
}

/// 文件名去扩展名，导入歌单名的唯一来源。
fn file_stem_name(file_path: &str) -> String {
    std::path::Path::new(file_path)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "导入的歌单".into())
}

/// 导出歌单为 m3u/m3u8。
/// - `format`: "m3u8"（默认）| "m3u" —— 内容相同，都是 UTF-8（D5 不做 GBK），
///   只有扩展名约定不同，所以作者自己在 `path` 里给扩展名
/// - `style`: "absolute"（默认）| "relative" —— 相对路径锚定导出文件所在目录
/// - `encoding`: 只收 "utf8"（参数位留给将来 GBK，D5）
///
/// 越界值一律 `invalid_arg`（静默回默认更害人：用户以为导出成了 m3u8）。
///
/// 为什么是 `async`：Tauri 的同步命令在**主线程**执行（命令宏默认
/// `ExecutionContext::Blocking`），而这个命令要跨线程等 DB worker 再写盘——
/// 扫描跑着的时候点导出会把窗口冻住。改成 async 就落到达 async runtime 上。
#[tauri::command]
async fn playlist_export(
    state: State<'_, AppState>,
    id: String,
    path: String,
    format: Option<String>,
    style: Option<String>,
    encoding: Option<String>,
) -> Result<dto::playlist::PlaylistExportResult, dto::ApiError> {
    if let Some(enc) = encoding.as_deref() {
        if enc != "utf8" {
            return Err(dto::ApiError::invalid_arg(format!(
                "不支持的导出编码：{enc}（目前只有 utf8）"
            )));
        }
    }
    let relative = match style.as_deref() {
        None | Some("absolute") => false,
        Some("relative") => true,
        Some(other) => return Err(dto::ApiError::invalid_arg(format!("未知的导出风格：{other}"))),
    };
    match format.as_deref() {
        None | Some("m3u8") | Some("m3u") => (),
        Some(other) => return Err(dto::ApiError::invalid_arg(format!("未知的导出格式：{other}"))),
    };

    let out_path = PathBuf::from(&path);
    let base_dir = out_path
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."));
    let items: Vec<aiya_data::playlist_io::ExportItem> = state
        .db()
        .send(move |conn| {
            let mut stmt = conn.prepare(
                "SELECT t.path, t.title, t.duration_ms, ar.name
                 FROM playlist_items pi
                 JOIN tracks t ON t.id = pi.track_id
                 LEFT JOIN artists ar ON ar.id = t.artist_id
                 WHERE pi.playlist_id = ?1
                 ORDER BY pi.position ASC, pi.id ASC",
            )?;
            let rows = stmt.query_map([id], |r| {
                Ok(aiya_data::playlist_io::ExportItem {
                    path: r.get(0)?,
                    title: r.get(1)?,
                    duration_ms: r.get(2)?,
                    artist: r.get(3)?,
                })
            })?;
            Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
        })
        .map_err(dto::ApiError::from)?;

    if items.is_empty() {
        return Err(dto::ApiError::invalid_arg("歌单是空的，没有可导出的曲目"));
    }
    let text = aiya_data::playlist_io::render_playlist(&items, &base_dir, relative);
    let count = items.len() as i64;
    std::fs::write(&out_path, text).map_err(|e| {
        dto::ApiError::new(dto::ApiErrorCode::Io, format!("写不进导出文件：{e}"))
    })?;
    Ok(dto::playlist::PlaylistExportResult { path, count })
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
    let Some(tx) = app_state.smtc_tx.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone() else {
        return;
    };
    let playing = state.status == aiya_player::PlaybackStatus::Playing;
    let _ = tx.send(smtc::SmtcUpdate::Progress {
        playing,
        position_ms: state.position_ms,
    });

    let track_id = state.current.as_ref().map(|c| c.id.clone()).unwrap_or_default();
    let mut last = app_state.smtc_track_id.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
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
///
/// `remember_position` = 设置项；关闭时进度恒写 0，于是下次启动（即使
/// 读库读到旧值）也从 0 开始——比"读的时候再判断"更稳，库里的值与实际
/// 行为永远一致。
fn queue_persist_from_actor(
    actor: &aiya_player::PlayerActor,
    remember_position: bool,
) -> aiya_data::QueuePersist {
    let s = actor.state();
    aiya_data::QueuePersist {
        // PlayerState.index 现在是 Option（空队列=null）；DB 的 -1 哨兵
        // 只在 queue_state 存储层保留，这里做唯一一次换算。
        current_index: s.index.map_or(-1, |i| i as i64),
        position_ms: if remember_position { s.position_ms as i64 } else { 0 },
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
    let p = queue_persist_from_actor(actor, state.setting_flag("rememberPosition", true));
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
    let p = queue_persist_from_actor(actor, state.setting_flag("rememberPosition", true));
    let result: aiya_data::DbResult<()> =
        state.db().send(move |conn| aiya_data::save_queue_state(conn, &p));
    if let Err(e) = result {
        tracing::warn!(error = %e, "queue state persist failed");
    }
}

/// 把 Actor 的"装载/停止"意图下发到引擎。
///
/// 三个调用点（命令路径 / 事件泵 / 启动恢复）此前各抄一遍，且都 `let _ = start_ms`
/// 把起点丢弃 —— "记住播放进度"从未生效。下发逻辑本身在 `pump::apply_engine_intents`
/// （纯逻辑、可注入出口，有单元测试锁定指令顺序），这里只负责接上引擎通道。
fn apply_engine_intents(state: &AppState, actor: &mut aiya_player::PlayerActor) -> bool {
    pump::apply_engine_intents(&mut |cmd| engine_send(state, cmd), actor)
}

/// Actor 归约后统一收尾：取 pending load → 发引擎；同步音量；推状态。
/// 返回新快照（命令返回值用）。
fn drive_and_emit(state: &State<'_, AppState>, app: &AppHandle) -> aiya_player::PlayerState {
    let mut actor = state.player.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let acted = apply_engine_intents(&state, &mut actor);
    // 切歌/停止立即全量落库（队列结构可能变了；exit 时不必依赖节流尾巴）
    if acted {
        persist_queue_full(state, &actor);
    }
    let snapshot = actor.state();
    drop(actor);
    emit_player_state(app, &snapshot);
    snapshot
}

/// 显示并聚焦主窗口（托盘左键 / 菜单共用）。
fn show_main_window(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

/// 统一退出路径（托盘菜单 / SMTC Quit 共用）：先落队列状态（含最终
/// position）再退出。P2：SMTC Quit 曾直接 exit(0) 丢掉最后几秒进度。
pub fn quit_app(app: &AppHandle) {
    if let Some(state) = app.try_state::<AppState>() {
        let actor = state
            .player
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if actor.state().queue_length > 0 {
            persist_queue_state_handle(&state, &actor);
        }
    }
    app.exit(0);
}

/// 引擎命令统一入口（Engine 在 Mutex 里）。&State 自动 deref 成 &AppState。
fn engine_send(state: &AppState, cmd: aiya_audio::EngineCommand) {
    let _ = state.engine.lock().unwrap_or_else(std::sync::PoisonError::into_inner).send(cmd);
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
fn player_state(
    state: State<'_, AppState>,
) -> Result<aiya_player::PlayerState, dto::ApiError> {
    let actor = state.player.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    Ok(actor.state())
}

#[tauri::command]
fn player_queue(
    state: State<'_, AppState>,
) -> Result<aiya_player::QueueSnapshot, dto::ApiError> {
    let actor = state.player.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    Ok(actor.queue_snapshot())
}

#[tauri::command]
fn player_play_tracks(
    app: AppHandle,
    state: State<'_, AppState>,
    track_ids: Vec<String>,
    start_index: Option<usize>,
) -> Result<aiya_player::PlayerState, dto::ApiError> {
    let tracks = fetch_track_refs(&state, &track_ids);
    if tracks.is_empty() {
        return Err(dto::ApiError::not_found("没有可播放的曲目"));
    }
    let start = start_index.unwrap_or(0);
    {
        let mut actor = state.player.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        actor.reduce(aiya_player::PlayerCommand::Queue(aiya_player::QueueOp::ReplaceAll {
            tracks,
            start,
        }));
    }
    Ok(drive_and_emit(&state, &app))
}

/// 按 TrackWindowQuery 语义播放查询结果（UI 的"播放全部/播放专辑"）。
#[tauri::command]
fn player_play_query(
    app: AppHandle,
    state: State<'_, AppState>,
    query: Json,
    start_index: Option<usize>,
) -> Result<aiya_player::PlayerState, dto::ApiError> {
    let q: aiya_data::TrackWindowQuery = serde_json::from_value(query)
        .map_err(|e| dto::ApiError::invalid_arg(format!("query 无效：{e}")))?;
    // 拉全量 id：list_tracks 的 limit 硬钳 500（窗口化列表的正确行为），
    // 此前把 limit 写成 10000 以为能拿全，实际只播了前 500 首。
    let (ids, truncated): (Vec<String>, bool) = state
        .db()
        .send(move |conn| {
            let c = aiya_data::repo::collect_track_ids(conn, &q, ID_COLLECT_CAP)?;
            Ok((c.ids, c.truncated))
        })
        .map_err(dto::ApiError::from)?;
    if truncated {
        tracing::warn!(cap = ID_COLLECT_CAP, "播放全部：曲目数超过上限，已截断");
    }
    if ids.is_empty() {
        return Err(dto::ApiError::not_found("查询结果为空"));
    }
    let tracks = fetch_track_refs(&state, &ids);
    let start = start_index.unwrap_or(0);
    {
        let mut actor = state.player.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        actor.reduce(aiya_player::PlayerCommand::Queue(aiya_player::QueueOp::ReplaceAll {
            tracks,
            start,
        }));
    }
    Ok(drive_and_emit(&state, &app))
}

/// 播放/暂停切换的内部实现（tauri command 与 SMTC 回调共用）。
fn cmd_toggle(state: &AppState, app: &AppHandle) -> aiya_player::PlayerState {
    let status_before = state.player.lock().unwrap_or_else(std::sync::PoisonError::into_inner).state().status;
    use aiya_player::PlaybackStatus as PS;
    match status_before {
        PS::Playing => engine_send(state, aiya_audio::EngineCommand::Pause),
        PS::Paused => engine_send(state, aiya_audio::EngineCommand::Resume),
        _ => {
            // Idle/Error：从当前位置重播（Actor 决定 load）
        }
    }
    {
        let mut actor = state.player.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        actor.reduce(aiya_player::PlayerCommand::TogglePause);
    }
    drive_and_emit_inner(state, app)
}

#[tauri::command]
fn player_toggle(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<aiya_player::PlayerState, dto::ApiError> {
    Ok(cmd_toggle(&state, &app))
}

/// Seek 的内部实现（tauri command 与 SMTC 回调共用）。
fn cmd_seek(state: &AppState, app: &AppHandle, position_ms: u64) -> aiya_player::PlayerState {
    engine_send(state, aiya_audio::EngineCommand::Seek(position_ms));
    {
        let mut actor = state.player.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        actor.reduce(aiya_player::PlayerCommand::Seek(position_ms));
    }
    drive_and_emit_inner(state, app)
}

/// `drive_and_emit` 的 AppState 版本（SMTC 回调拿不到 `State<'_>`，语义相同）。
///
/// 与前者唯一的差别是入参类型，所以逻辑必须**逐行一致**——两处一旦分叉，
/// 走媒体键触发的那次状态更新就会少落一次库。改这里时记得同步改上面。
fn drive_and_emit_inner(state: &AppState, app: &AppHandle) -> aiya_player::PlayerState {
    let mut actor = state.player.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if apply_engine_intents(state, &mut actor) {
        persist_queue_full(state, &actor);
    }
    let snapshot = actor.state();
    drop(actor);
    emit_player_state(app, &snapshot);
    snapshot
}

#[tauri::command]
fn player_next(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<aiya_player::PlayerState, dto::ApiError> {
    {
        let mut actor = state.player.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        actor.reduce(aiya_player::PlayerCommand::Next);
    }
    Ok(drive_and_emit(&state, &app))
}

#[tauri::command]
fn player_prev(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<aiya_player::PlayerState, dto::ApiError> {
    {
        let mut actor = state.player.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        actor.reduce(aiya_player::PlayerCommand::Prev);
    }
    Ok(drive_and_emit(&state, &app))
}

#[tauri::command]
fn player_seek(
    app: AppHandle,
    state: State<'_, AppState>,
    position_ms: u64,
) -> Result<aiya_player::PlayerState, dto::ApiError> {
    Ok(cmd_seek(&state, &app, position_ms))
}

#[tauri::command]
fn player_set_volume(
    app: AppHandle,
    state: State<'_, AppState>,
    volume: f64,
) -> Result<aiya_player::PlayerState, dto::ApiError> {
    {
        let mut actor = state.player.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        actor.reduce(aiya_player::PlayerCommand::SetVolume { volume, muted: false });
        let v = actor.engine_volume();
        engine_send(&state, aiya_audio::EngineCommand::SetVolume(v));
    }
    Ok(drive_and_emit(&state, &app))
}

#[tauri::command]
fn player_toggle_mute(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<aiya_player::PlayerState, dto::ApiError> {
    {
        let mut actor = state.player.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let (v, muted) = actor.volume();
        actor.reduce(aiya_player::PlayerCommand::SetVolume { volume: v, muted: !muted });
        let v = actor.engine_volume();
        engine_send(&state, aiya_audio::EngineCommand::SetVolume(v));
    }
    Ok(drive_and_emit(&state, &app))
}

#[tauri::command]
fn player_set_rate(
    app: AppHandle,
    state: State<'_, AppState>,
    rate: f64,
) -> Result<aiya_player::PlayerState, dto::ApiError> {
    // 引擎侧线性插值变速（0.5–2.0，v1 变速变调；WSOLA 不变调后补）
    {
        let mut actor = state.player.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        actor.reduce(aiya_player::PlayerCommand::SetRate(rate));
        engine_send(&state, aiya_audio::EngineCommand::SetRate(actor.state().rate as f32));
    }
    Ok(drive_and_emit(&state, &app))
}

#[tauri::command]
fn player_set_repeat(
    app: AppHandle,
    state: State<'_, AppState>,
    repeat: String,
) -> Result<aiya_player::PlayerState, dto::ApiError> {
    {
        let mut actor = state.player.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        // `cycle` 是契约（contracts/ipc.ts）里的"轮转"语义：UI 只发 cycle，
        // 由后端按 顺序 → 列表 → 单曲 推进。此前 `_ => Off` 把 cycle 打成
        // Off 且 Off 幂等 —— 循环按钮永远停在"顺序播放"。
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
                return Err(dto::ApiError::invalid_arg(format!("未知 repeat 值：{other}")));
            }
        };
        actor.reduce(aiya_player::PlayerCommand::SetRepeat(mode));
    }
    Ok(drive_and_emit(&state, &app))
}

#[tauri::command]
fn player_set_shuffle(
    app: AppHandle,
    state: State<'_, AppState>,
    shuffle: bool,
) -> Result<aiya_player::PlayerState, dto::ApiError> {
    {
        let mut actor = state.player.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let cur = actor.state().shuffle;
        if cur != shuffle {
            actor.reduce(aiya_player::PlayerCommand::ToggleShuffle);
        }
    }
    Ok(drive_and_emit(&state, &app))
}

#[tauri::command]
fn player_enqueue(
    app: AppHandle,
    state: State<'_, AppState>,
    track_ids: Vec<String>,
    options: Option<Json>,
) -> Result<aiya_player::PlayerState, dto::ApiError> {
    let tracks = fetch_track_refs(&state, &track_ids);
    if tracks.is_empty() {
        return Err(dto::ApiError::not_found("没有可播放的曲目"));
    }
    // options: { mode: "next" | "end" }（contracts QueueInsertMode）
    let mode = options
        .as_ref()
        .and_then(|o| o.get("mode"))
        .and_then(|m| m.as_str())
        .unwrap_or("end");
    {
        let mut actor = state.player.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        actor.reduce(aiya_player::PlayerCommand::Queue(if mode == "next" {
            aiya_player::QueueOp::PlayNext { tracks }
        } else {
            aiya_player::QueueOp::Append { tracks }
        }));
    }
    Ok(drive_and_emit(&state, &app))
}

#[tauri::command]
fn player_insert_next(
    app: AppHandle,
    state: State<'_, AppState>,
    track_id: String,
) -> Result<aiya_player::PlayerState, dto::ApiError> {
    let tracks = fetch_track_refs(&state, &[track_id]);
    if tracks.is_empty() {
        return Err(dto::ApiError::not_found("曲目不存在"));
    }
    {
        let mut actor = state.player.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        actor.reduce(aiya_player::PlayerCommand::Queue(aiya_player::QueueOp::PlayNext { tracks }));
    }
    Ok(drive_and_emit(&state, &app))
}

#[tauri::command]
fn player_remove_at(
    app: AppHandle,
    state: State<'_, AppState>,
    index: usize,
) -> Result<aiya_player::PlayerState, dto::ApiError> {
    {
        let mut actor = state.player.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        actor.reduce(aiya_player::PlayerCommand::Queue(aiya_player::QueueOp::Remove {
            indices: vec![index],
        }));
    }
    Ok(drive_and_emit(&state, &app))
}

#[tauri::command]
fn player_move(
    app: AppHandle,
    state: State<'_, AppState>,
    from: usize,
    to: usize,
) -> Result<aiya_player::PlayerState, dto::ApiError> {
    {
        let mut actor = state.player.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        actor.reduce(aiya_player::PlayerCommand::Queue(aiya_player::QueueOp::Move {
            from,
            to,
        }));
    }
    Ok(drive_and_emit(&state, &app))
}

#[tauri::command]
fn player_clear(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<aiya_player::PlayerState, dto::ApiError> {
    engine_send(&state, aiya_audio::EngineCommand::Stop);
    {
        let mut actor = state.player.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        actor.reduce(aiya_player::PlayerCommand::Queue(aiya_player::QueueOp::ReplaceAll {
            tracks: vec![],
            start: 0,
        }));
    }
    Ok(drive_and_emit(&state, &app))
}

/// 播放历史：一曲播完（EngineEvent::Eof，自然结束或用户切走）记一条。
/// seconds_played = 该曲播放时长（v1 记整曲；中途跳过不细分百分比）。
///
/// 具体写法在 `aiya_data::history::record`（history 行与 tracks 的
/// play_count / last_played_at 同事务；此前 play_count 根本没人维护，
/// mostPlayed 列表永远是空的）。
fn record_history(state: &AppState, track_id: &str, seconds_played: u64) {
    let track_id = track_id.to_string();
    let played_at = now_secs();
    let result: aiya_data::DbResult<()> = state
        .db()
        .send(move |conn| aiya_data::history::record(conn, &track_id, played_at, seconds_played));
    if let Err(e) = result {
        tracing::warn!(error = %e, "history 写入失败");
    }
}

/// 歌词（P4）：同目录同名 .lrc → 解析。没找到回 `found=false`（UI 显示"暂无"，
/// 不当错误）。trackId 是自有库的 tracks.id。
#[tauri::command]
fn lyrics_get(
    state: State<'_, AppState>,
    track_id: String,
) -> Result<dto::lyrics::Lyrics, dto::ApiError> {
    let path: Option<String> = state
        .db()
        .send(move |conn| {
            Ok(conn
                .query_row("SELECT path FROM tracks WHERE id = ?1", [&track_id], |r| r.get(0))
                .ok())
        })
        .map_err(dto::ApiError::from)?;
    let Some(path) = path else {
        return Ok(dto::lyrics::Lyrics::empty());
    };
    Ok(match aiya_data::lyrics::read_lyrics(std::path::Path::new(&path)) {
        Some(parsed) => parsed.into(),
        None => dto::lyrics::Lyrics::empty(),
    })
}

#[tauri::command]
fn history_stats(state: State<'_, AppState>) -> Result<dto::history::HistoryStats, dto::ApiError> {
    // 包一层闭包：`send` 给的是 `&mut Connection`，而 SQL 主体只要 `&Connection`
    state.db().send(|conn| history_stats_from(conn)).map_err(dto::ApiError::from)
}

/// `history_stats` 的 SQL 主体。
///
/// 抽成独立函数是为了能直接喂一个内存库跑测试——这段 SQL 曾经查
/// `t.artist`（tracks 表根本没有这一列），prepare 阶段就报
/// "no such column"，于是命令恒返回 internal、历史页永远是空态；
/// 而浏览器预览走 mock 有数据，界面上完全看不出来。
fn history_stats_from(
    conn: &rusqlite::Connection,
) -> aiya_data::DbResult<dto::history::HistoryStats> {
    let count = |sql: &str| -> i64 { conn.query_row(sql, [], |r| r.get(0)).unwrap_or(0) };
    let total_plays = count("SELECT COUNT(*) FROM history");
    // completed：至少听了 1 秒；skipped：0 秒就切走的
    let completed = count("SELECT COUNT(*) FROM history WHERE seconds_played >= 1");
    // topTracks：按播放次数前 10（已删曲目不计）。
    // 艺人名必须 JOIN artists —— tracks 表**没有** artist 列。
    let mut stmt = conn.prepare(
        "SELECT h.track_id, t.title, ar.name, COUNT(*) AS plays
         FROM history h
         JOIN tracks t ON t.id = h.track_id
         LEFT JOIN artists ar ON ar.id = t.artist_id
         GROUP BY h.track_id ORDER BY plays DESC LIMIT 10",
    )?;
    let top: Vec<dto::history::HistoryTopTrack> = stmt
        .query_map([], |r| {
            Ok(dto::history::HistoryTopTrack {
                id: r.get(0)?,
                title: r.get(1)?,
                artist: r.get(2)?,
                play_count: r.get(3)?,
            })
        })?
        .filter_map(|r| r.ok())
        .collect();
    // played_at 存 unix 秒，出口换毫秒（与 totalMs 同单位）
    let first = conn
        .query_row("SELECT MIN(played_at) FROM history", [], |r| r.get::<_, Option<i64>>(0))
        .unwrap_or(None);
    let last = conn
        .query_row("SELECT MAX(played_at) FROM history", [], |r| r.get::<_, Option<i64>>(0))
        .unwrap_or(None);
    Ok(dto::history::HistoryStats {
        total_plays,
        total_ms: count("SELECT COALESCE(SUM(seconds_played),0) FROM history") * 1000,
        completed,
        skipped: total_plays - completed,
        first_play_at_ms: first.map(|s| s * 1000),
        last_play_at_ms: last.map(|s| s * 1000),
        top_tracks: top,
    })
}

#[tauri::command]
fn history_clear(state: State<'_, AppState>) -> Result<dto::history::HistoryCleared, dto::ApiError> {
    // 清历史必须与曲目统计列归零同事务（见 aiya_data::history::clear）
    state
        .db()
        .send(|conn| aiya_data::history::clear(conn))
        .map_err(dto::ApiError::from)?;
    Ok(dto::history::HistoryCleared { cleared: true })
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
        // 关窗行为按 closeToTray 设置（P2：此前一律 prevent_close+hide，
        // 设置默认 false 时用户想关窗退出也退不了——设置项完全没接线）。
        // 开启设置 → 隐藏到托盘（播放不中断）；默认 → 正常关闭退出。
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                let hide_to_tray = window
                    .app_handle()
                    .try_state::<AppState>()
                    .map(|state| state.setting_flag("closeToTray", false))
                    .unwrap_or(false);
                if hide_to_tray {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .register_uri_scheme_protocol(aiya_data::ARTWORK_SCHEME, |ctx, request| {
            artwork_protocol(ctx.app_handle(), request)
        })
        .setup(|app| {
            // 第一件事：日志。此后的任何失败（建库、起 sidecar）才有据可查，
            // 否则 release 里所有 tracing 输出进虚空。
            logging::init(&app.handle());

            let db_path = default_db_path(&app.handle());
            let db = aiya_data::open(&db_path)
                .map_err(|e| -> Box<dyn std::error::Error> { e.to_string().into() })?;
            let artwork_dir = default_artwork_dir(&app.handle());
            let _ = std::fs::create_dir_all(artwork_dir.join("originals"));
            let _ = std::fs::create_dir_all(artwork_dir.join("thumbs"));
            let engine = sidecar::SidecarHandle::launch()
                .map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
            // 设置缓存灌一次：此后泵/诊断/启动恢复都读内存，不查库。
            // 必须在 manage 之前拿到，否则下面的独占偏好会读到空缓存。
            let settings0: Value = {
                let fallback = || -> Value {
                    serde_json::from_str(DEFAULT_SETTINGS).unwrap_or_else(|_| json!({}))
                };
                let def = fallback();
                db.send(move |conn| Ok(setting_get(conn, "app_settings").unwrap_or(def)))
                    .unwrap_or_else(|_| fallback())
            };
            app.manage(AppState {
                db: Mutex::new(db),
                db_path,
                artwork_dir,
                player: Mutex::new(aiya_player::PlayerActor::new()),
                engine: Mutex::new(engine),
                scanning: Arc::new(AtomicBool::new(false)),
                scan_cancel: Arc::new(AtomicBool::new(false)),
                last_persist_ms: std::sync::atomic::AtomicU64::new(0),
                smtc_tx: Mutex::new(None),
                smtc_track_id: Mutex::new(String::new()),
                smtc_active: AtomicBool::new(false),
                sleep_prevented: AtomicBool::new(false),
                tray_active: AtomicBool::new(false),
                audio_state: Mutex::new(AudioStateSnapshot::default()),
                timings: Mutex::new(VecDeque::with_capacity(TIMING_LIMIT)),
                settings: Mutex::new(settings0),
            });

            // 输出配置先下发：引擎未装载时只记录，首曲 Load 就用上。
            // 设备拒绝独占会回落共享、缓冲太深会自愈降级，两者都通过 AudioMode
            // 事件把原因带回 UI（"设置没生效"必须可解释）。
            {
                let st = app.state::<AppState>();
                engine_send(&st, aiya_audio::EngineCommand::SetOutputConfig(st.setting_output()));
            }
            // 音量归一化偏好同样先灌进 Actor：音量在 pump 里统一算，
            // 不放在 UI（旧 UI 侧的 setNormalization 已是 no-op）
            if let Some(state) = app.try_state::<AppState>() {
                let on = state.setting_flag("normalizeVolume", false);
                state
                    .player
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .set_normalize(on);
            }

            // SMTC：主窗口 HWND（COM 对象要求）；失败只记日志，不影响播放
            if let Some(main) = app.get_webview_window("main") {
                if let Ok(hwnd) = main.hwnd() {
                    if let Some(tx) = smtc::spawn(app.handle().clone(), hwnd.0 as isize) {
                        if let Some(state) = app.try_state::<AppState>() {
                            *state.smtc_tx.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(tx);
                            state.smtc_active.store(true, Ordering::SeqCst);
                        }
                    }
                }
            }

            // 启动恢复：queue_state + queue_items → Actor hydrate → 引擎装载。
            // 默认恢复成"暂停"（用户点播放才出声）；`autoplayOnStart` 打开则立即续播。
            // `rememberPosition` 关闭时起点强制 0（进度不跨会话记忆）。
            if let Some(state) = app.try_state::<AppState>() {
                let remember_position = state.setting_flag("rememberPosition", true);
                let autoplay_on_start = state.setting_flag("autoplayOnStart", false);
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
                            let start_ms = if remember_position {
                                p.position_ms.max(0) as u64
                            } else {
                                0
                            };
                            let mut actor = state.player.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
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
                            // 恢复装载：起点（hydrate 传入的 position_ms）与
                            // 倍速必须一起下发 —— 此前 `let _ = start_ms`
                            // 直接丢弃，UI 显示 30:00、出声从 0 开始。
                            apply_engine_intents(&state, &mut actor);
                            // autoplayOnStart：hydrate 出的是 Paused（引擎已收到
                            // Load + Pause），这里补一条 Resume 并翻转状态。
                            // 只对"有当前曲且处于暂停"生效，避免空队列时误播。
                            let status_now = actor.state().status;
                            if autoplay_on_start
                                && status_now == aiya_player::PlaybackStatus::Paused
                            {
                                engine_send(&state, aiya_audio::EngineCommand::Resume);
                                actor.reduce(aiya_player::PlayerCommand::TogglePause);
                                tracing::info!("autoplayOnStart: 启动即续播");
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
                        "quit" => quit_app(app),
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
                if let Some(state) = app.try_state::<AppState>() {
                    state.tray_active.store(true, Ordering::Relaxed);
                }
            }

            // P1：防休眠是 per-thread API（SetThreadExecutionState），此前在
            // tokio 泵任务里维护——任务可能跨 worker 迁移，标志设在 A 线程、
            // 清除跑到 B 线程，旧线程上的 ES_SYSTEM_REQUIRED 无人清除
            // （退出播放后系统仍不睡）。移到永不迁移的专用 std::thread。
            let prevent_sleep_flag = Arc::new(AtomicBool::new(false));
            {
                let flag = Arc::clone(&prevent_sleep_flag);
                let _ = std::thread::Builder::new()
                    .name("prevent-sleep".into())
                    .spawn(move || {
                        let mut last = false;
                        loop {
                            let now = flag.load(Ordering::Relaxed);
                            if now != last {
                                smtc::set_prevent_sleep(now);
                                last = now;
                            }
                            std::thread::sleep(std::time::Duration::from_millis(250));
                        }
                    });
            }

            // 引擎事件泵：EngineEvent → Actor 归约 → player:state 事件。
            // Position 节流到 500ms（渲染循环节拍是 ~10ms，UI 不需要那么密）。
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                // 防休眠标志由专用线程消费，这里只翻转 AtomicBool
                let mut prevent_sleep_last = false;
                // 泵的归约状态：位置估算 + 广播节流基准（泵私有，见 pump.rs）
                let mut core = PumpCore::new();
                loop {
                    tokio::time::sleep(std::time::Duration::from_millis(25)).await;
                    let Some(engine) = handle.try_state::<AppState>() else { continue };
                    // 防系统睡眠：只在"播放中且设置开启"时置位。
                    // 此前无条件按播放状态防休眠，用户在设置里关掉也没用。
                    let playing_now = engine.setting_flag("preventSleep", true) && {
                        let actor = engine.player.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                        actor.state().status == aiya_player::PlaybackStatus::Playing
                            && actor.state().current.is_some()
                    };
                    if playing_now != prevent_sleep_last {
                        prevent_sleep_flag.store(playing_now, Ordering::Relaxed);
                        prevent_sleep_last = playing_now;
                        engine
                            .sleep_prevented
                            .store(playing_now, Ordering::Relaxed);
                    }
                    let events: Vec<aiya_audio::EngineEvent> = {
                        let engine_guard = engine.engine.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                        let mut out = Vec::new();
                        while let Some(ev) = engine_guard.try_recv() {
                            out.push(ev);
                        }
                        out
                    };
                    if events.is_empty() {
                        continue;
                    }
                    // 归约 + 旁路动作全在 PumpCore 里（可注入时钟与引擎出口，
                    // 事件序列有单元测试锁定）。锁只在归约期间持有，
                    // emit 是同步 IPC 序列化，一律放到锁外。
                    let effects = {
                        let mut actor = engine
                            .player
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner);
                        let mut sent: Vec<aiya_audio::EngineCommand> = Vec::new();
                        let effects = core.ingest(events, &mut actor, now_ms_u64(), &mut |cmd| {
                            sent.push(cmd)
                        });
                        // pending_load / pending_stop 的实际下发仍在锁内做完：
                        // 引擎通道是有界的，正常消费下不会阻塞
                        for cmd in sent {
                            engine_send(&engine, cmd);
                        }
                        if effects.iter().any(|e| *e == PumpEffect::EmitState) {
                            // 意图下发可能改了队列结构 → 全量落库（与命令路径一致）
                            persist_queue_full(&engine, &actor);
                        }
                        effects
                    };
                    // 频谱帧（~30fps）先转发，再广播状态：UI 的动画帧不该排在
                    // 一次状态序列化之后
                    let mut emit_state = false;
                    for effect in effects {
                        match effect {
                            PumpEffect::Spectrum(bands) => {
                                let _ = handle.emit("spectrum:frame", bands);
                            }
                            PumpEffect::History { track_id, seconds } => {
                                record_history(&engine, &track_id, seconds);
                            }
                            PumpEffect::OutputMode {
                                requested,
                                mode,
                                clock,
                                buffer_ms,
                                format,
                                reason,
                            } => {
                                // 回落/自愈/设备更换：更新诊断状态并转发 UI。
                                // 这是"降级信息"不是错误，所以走独立事件，
                                // 不混进 player:state 的 error 字段。
                                let note = reason.unwrap_or_default();
                                {
                                    let mut st = engine
                                        .audio_state
                                        .lock()
                                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                                    st.requested = requested.clone();
                                    st.mode = mode.clone();
                                    st.clock = clock.clone();
                                    st.buffer_ms = buffer_ms;
                                    st.format = format.clone();
                                    st.reason = note.clone();
                                }
                                let _ = handle.emit(
                                    "audio:mode",
                                    dto::AudioModeEvent {
                                        requested,
                                        mode,
                                        clock,
                                        buffer_ms,
                                        format,
                                        reason: Some(note).filter(|s| !s.is_empty()),
                                    },
                                );
                            }
                            PumpEffect::EmitState => emit_state = true,
                        }
                    }
                    if emit_state {
                        // 统一出口 emit_player_state：player:state + SMTC 跟踪
                        // （P2：泵此前直接 emit，自动换曲后系统媒体浮层不更新）
                        let snapshot = {
                            let actor = engine
                                .player
                                .lock()
                                .unwrap_or_else(std::sync::PoisonError::into_inner);
                            actor.state()
                        };
                        emit_player_state(&handle, &snapshot);
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
                        let actor = engine.player.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
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
            playlist_rename,
            playlist_delete,
            playlist_reorder,
            playlist_add_tracks,
            playlist_remove_at,
            playlist_move,
            playlist_clear,
            playlist_import,
            playlist_export,
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
            lyrics_get,
            history_stats,
            history_clear,
        ])
        .build(tauri::generate_context!())
        .expect("AIYA: Tauri 初始化失败")
        .run(|app, event| {
            // 退出前主动收 sidecar。
            //
            // 不在这里收会怎样：进程退出时 AppState 的 Drop 不保证跑到，
            // 看门狗线程在最后几秒里看到 alive 不再刷新 → 判"心跳超时" →
            // **再起一个 sidecar**（generation+1）。用户每次关窗都留一条
            // 假 ERROR 日志 + 一个要等 8s 孤儿窗口才自退的多余进程。
            if let tauri::RunEvent::Exit = event {
                if let Some(state) = app.try_state::<AppState>() {
                    state
                        .engine
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .shutdown();
                }
            }
        });
}
