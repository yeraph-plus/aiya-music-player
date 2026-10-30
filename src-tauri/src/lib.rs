//! AIYA Music — Tauri 宿主壳。
//!
//! 边界约定（src-tauri/README.md）：命令注册、事件桥、协议注册；领域业务在 crates/。
//!
//! 所有命令都是 `Result<T, dto::ApiError>`：成功返回**裸载荷**（形状定义在
//! [`dto`]，由 ts-rs 生成到 `ui/src/lib/ipc/generated/`），失败时 Tauri 把
//! `ApiError` 序列化后直接 reject。`{ok,data}` 信封已经不存在了 —— 旧 UI 那层
//! 翻译（`ui/src/compat/`）已于 2026-10-25 批次 4 删除，UI 侧只剩
//! `ui/src/lib/ipc/client.ts` 一层（docs/17 §3.1）。
//!
//! **命令数别在这里写死**：这段注释先后写过 53/54/55，实际已到 62 —— 唯一的事实源
//! 是文末那处命令注册宏调用（本文件的 `generate_handler` 唯一出现点），
//! `tests/ipc_contract.rs` 会核对它与 `client.ts` 的双向覆盖。
//! （注意：别在注释里写出带方括号的宏调用形态 —— 那个测试是文本扫描，
//! 会锚到第一个匹配，注释里写一次就会让解析结果变成 0 条。）
//!
//! 历史：`player_report` 是 v1 `<audio>` 出口的回报通道，引擎搬到 Rust 后前端
//! 从没调用过、后端 `let _ = &state` 空转，2026-10-25 整条删除；
//! `playlist_import`/`playlist_export` 于 docs/22 G2 加入。

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

mod app_id;
mod beets;
pub mod dto;
mod drop;
mod enrichment;
mod logging;
mod paths;
mod proc;
mod pump;
mod refresh;
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
    use crate::history_cutoff_days;
    use crate::pump::position_emit_due;

    /// 目录前缀归一化：小写 + 尾部分隔符。尾分隔符是命门——不带的话
    /// `D:\Music` 会误伤 `D:\Music Backup`（前缀字符串包含关系）。
    #[test]
    fn root_prefix_normalizes_case_and_separator() {
        assert_eq!(
            crate::root_prefix_lower("D:\\CkMsr\\Music"),
            "d:\\ckmsr\\music\\"
        );
        assert_eq!(crate::root_prefix_lower("d:/music/"), "d:/music/");
        assert_eq!(crate::root_prefix_lower("  D:\\X  "), "d:\\x\\");
        assert_eq!(crate::root_prefix_lower("D:\\"), "d:\\", "盘根不再补分隔符");
    }

    /// 前缀清除的完整语义：只删该目录下、不误伤兄弟目录（Music Backup）、
    /// 外键级联清掉歌单项/队列项/历史（2026-10-28"目录移除不掉"缺陷的回归锚）。
    #[test]
    fn prefix_prune_deletes_only_under_root_with_cascade() {
        let mut conn = aiya_data::db::open_in_memory().expect("内存库");
        for (id, path) in [
            ("t1", "D:\\CkMsr\\Music\\a.flac"),
            ("t2", "D:\\CkMsr\\Music Backup\\b.flac"),
            ("t3", "D:\\FM_Stations\\x.flac"),
        ] {
            conn.execute(
                "INSERT INTO tracks (id, title, title_norm, added_at, path, path_lower)
                 VALUES (?1, ?1, ?1, 0, ?2, ?3)",
                rusqlite::params![id, path, path.to_lowercase()],
            )
            .unwrap();
        }
        // t1 被歌单、队列、历史三方引用；t3 只进歌单（对照组）
        conn.execute("INSERT INTO playlists (id, name, name_norm, created_at, updated_at) VALUES ('p1','歌单','歌单',0,0)", [])
            .unwrap();
        for (pid, pos, tid) in [(1, 0, "t1"), (2, 1, "t3")] {
            conn.execute(
                "INSERT INTO playlist_items (id, playlist_id, track_id, position, added_at)
                 VALUES (?1, 'p1', ?2, ?3, 0)",
                rusqlite::params![format!("pi{pid}"), tid, pos],
            )
            .unwrap();
        }
        conn.execute(
            "INSERT INTO queue_items (position, track_id) VALUES (0, 't1')",
            [],
        )
        .unwrap();
        aiya_data::history::record(&mut conn, "t1", 1_700_000_000, 30).unwrap();

        // 统计：只有 t1 在 D:\CkMsr\Music 下
        let prefix = crate::root_prefix_lower("D:\\CkMsr\\Music");
        assert_eq!(crate::count_tracks_under_prefix(&conn, &prefix).unwrap(), 1);

        // 删除：1 行，t2/t3 幸存
        let removed = conn
            .execute(
                "DELETE FROM tracks WHERE substr(path_lower, 1, ?2) = ?1",
                rusqlite::params![prefix, prefix.chars().count() as i64],
            )
            .unwrap();
        assert_eq!(removed, 1);
        let left: Vec<String> = {
            let mut st = conn.prepare("SELECT id FROM tracks ORDER BY id").unwrap();
            let rows = st.query_map([], |r| r.get::<_, String>(0)).unwrap();
            rows.map(|r| r.unwrap()).collect()
        };
        assert_eq!(left, vec!["t2".to_string(), "t3".to_string()]);

        // 级联：t1 的歌单项/队列项/历史都没了，t3 的歌单项还在
        let n: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM playlist_items WHERE track_id = 't1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n, 0);
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM queue_items", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 0, "队列项随曲目级联清除");
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM history", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 0, "播放历史随曲目级联清除");
        let n: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM playlist_items WHERE track_id = 't3'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n, 1, "不受影响的曲目歌单项必须保留");
    }

    /// 历史统计的 SQL 曾经查 `t.artist`——tracks 表没有这一列，prepare 直接
    /// 报 "no such column"，命令恒返回 internal，历史页永远是空态。这条测试
    /// 用真 schema 跑一遍，锁住"能查出来 + 艺人名走 JOIN + 时间换毫秒"。
    #[test]
    fn history_stats_query_works_against_real_schema() {
        let mut conn = aiya_data::db::open_in_memory().expect("内存库");
        conn.execute(
            "INSERT INTO artists (id, name, name_norm) VALUES ('ar1','艺人','艺人')",
            [],
        )
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
        assert_eq!(
            s.top_tracks[0].artist.as_deref(),
            Some("艺人"),
            "艺人名必须来自 JOIN"
        );
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
        assert!(crate::playlist_summary_by_id(&conn, "nope")
            .unwrap()
            .is_none());
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
        let cases = [
            ("trackIds", "track_ids"),
            ("state", "state"),
            ("from", "from"),
        ];
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

    /* ---------------- 保留周期钳制 ---------------- */

    /// historyDays 极端值不得让裁剪下限回绕（P2）：release 下
    /// `days * 86_400` 溢出回绕成负数时，cutoff 落到未来，一次裁剪
    /// 就把整个播放历史清空。钳到 1..=3650 + saturating_mul 后不可能。
    #[test]
    fn history_cutoff_clamps_extreme_days() {
        const DAY: i64 = 86_400;
        let now = 1_770_000_000;
        // 常规值原样
        assert_eq!(history_cutoff_days(90, now), Some(now - 90 * DAY));
        assert_eq!(history_cutoff_days(1, now), Some(now - DAY));
        // <= 0 = 永久保留（约定语义，不动）
        assert_eq!(history_cutoff_days(0, now), None);
        assert_eq!(history_cutoff_days(-5, now), None);
        // 极端大值：钳到上限，cutoff 永远是"过去的时刻"
        assert_eq!(history_cutoff_days(i64::MAX, now), Some(now - 3650 * DAY));
        assert_eq!(history_cutoff_days(100_000, now), Some(now - 3650 * DAY));
        assert!(
            history_cutoff_days(i64::MAX, now).unwrap() < now,
            "cutoff 不得越过未来"
        );
    }

    /* ---------------- 动态歌单摘要统计 ---------------- */

    /// 零匹配是动态歌单的常态（规则先建、曲目后进）：摘要读取不得因
    /// 封面子查询空结果集而报 QueryReturnedNoRows——此前它把"创建成功
    /// 但摘要失败"炸成整个命令的内部错误，歌单建了 UI 却不刷新
    /// （2026-10-29 动态歌单回归：'米哈'/'崩坏'两条都是这么"消失"的）。
    #[test]
    fn smart_summary_survives_zero_matches() {
        let mut conn = aiya_data::db::open_in_memory().expect("内存库");
        let rule = serde_json::json!({
            "all": [{"field": "album", "op": "contains", "value": "不存在的专辑"}],
            "sort": "title",
            "direction": "asc"
        });
        aiya_data::playlist::create_smart(&mut conn, "sp0", "空结果歌单", &rule, 0).unwrap();
        let summary = crate::playlist_summary_by_id(&conn, "sp0")
            .expect("零匹配不得报错")
            .expect("歌单行必须存在");
        assert_eq!(summary.track_count, 0);
        assert_eq!(summary.duration_ms, 0);
        assert_eq!(summary.artwork_id, None);
    }

    /// 有匹配时摘要口径正确：计数来自规则实时查询，封面取排序后第一个
    /// （本例曲目无封面 → None 而不是报错）。
    #[test]
    fn smart_summary_counts_real_matches() {
        let mut conn = aiya_data::db::open_in_memory().expect("内存库");
        conn.execute(
            "INSERT INTO tracks (id, title, title_norm, added_at, path, path_lower, genre_norm, duration_ms)
             VALUES ('t1', 'A', 'a', 0, 'D:/m/a.flac', 'd:/m/a.flac', 'pop', 120000)",
            [],
        )
        .unwrap();
        let rule = serde_json::json!({
            "all": [{"field": "genre", "op": "eq", "value": "Pop"}],
            "sort": "title",
            "direction": "asc"
        });
        aiya_data::playlist::create_smart(&mut conn, "sp1", "流行", &rule, 0).unwrap();
        let summary = crate::playlist_summary_by_id(&conn, "sp1")
            .expect("有匹配不得报错")
            .expect("歌单行必须存在");
        assert_eq!(summary.track_count, 1);
        assert_eq!(summary.duration_ms, 120000);
        assert_eq!(summary.artwork_id, None);
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
    /// 刷新管线的依赖（docs/33 §7）。`Arc` 是因为它要跨线程（定时器、会话
    /// 收尾回调）共享同一份互斥标志——各拷一份就等于各自为政地写同一个 DB。
    refresh: Arc<refresh::RefreshParams>,
    /// 最近一次刷新结果（诊断区读；也用来判断"上次 beets 段失败了没"）
    last_refresh: Mutex<Option<dto::RefreshSnapshot>>,
    /// 叫醒定时线程的把手（窗口聚焦 / 设置变更）
    refresh_wake: Mutex<Option<std::sync::mpsc::SyncSender<refresh::Wake>>>,
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
    /// 正在运行的 beets 安装子进程句柄（A 轨道安装线程存入，
    /// `RunEvent::Exit` 收尾处 kill+wait——安装器可能卡在 pip 阶段好几分钟，
    /// 不能让它在主进程退出后变孤儿）。
    beets_install_child: Mutex<Option<std::process::Child>>,
    /// beets 模块自有的结论库（`{beets-index}/enrich.db`）。
    ///
    /// 懒打开：绝大多数用户从不点"分析"，为一个可能永不使用的 SQLite 文件
    /// 在启动路径上做一次 IO 不划算。失败不缓存（`OnceLock` 存不进错误），
    /// 下次调用再试——这个文件的失败原因基本只有权限/磁盘满，重试无副作用。
    enrich_store: std::sync::OnceLock<std::sync::Arc<aiya_beets_store::Store>>,
}

/// 列表耗时环的容量。UI 只展示最近 8 条，留 20 条给"翻两页才看出趋势"。
const TIMING_LIMIT: usize = 20;

/// 保留天数 → 裁剪下限（纯计算，可测）。`None` = 永久保留。
///
/// `days <= 0` 视为永久；上限 3650（约 10 年）：极端大值在 release 下
/// `days * 86_400` 回绕成负数，cutoff 越过未来 → 一次裁剪清空全部历史。
/// saturating_mul 兜住剩余边界。
fn history_cutoff_days(days: i64, now_secs: i64) -> Option<i64> {
    const MAX_HISTORY_DAYS: i64 = 3650;
    if days <= 0 {
        return None;
    }
    Some(now_secs - days.clamp(1, MAX_HISTORY_DAYS).saturating_mul(86_400))
}

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

    /// beets 模块的结论库（懒打开、进程内单例）。
    ///
    /// 它不是应用数据：文件在 `{beets-index}` 下，schema 由 `aiya-beets-store`
    /// 自己建，应用库的迁移序列里没有它。放 `AppState` 只是因为它需要一个
    /// 进程内生命周期，而不是因为它属于应用。
    fn enrich_store(
        &self,
        app: &tauri::AppHandle,
    ) -> Result<std::sync::Arc<aiya_beets_store::Store>, String> {
        if let Some(s) = self.enrich_store.get() {
            return Ok(std::sync::Arc::clone(s));
        }
        let path = beets::enrich_db(app);
        let store = std::sync::Arc::new(
            aiya_beets_store::Store::open(&path).map_err(|e| format!("{}: {e}", path.display()))?,
        );
        // 竞态下别人可能先 set 成功；以已存进去的那个为准，本地这份直接丢。
        let _ = self.enrich_store.set(std::sync::Arc::clone(&store));
        Ok(self
            .enrich_store
            .get()
            .map(std::sync::Arc::clone)
            .unwrap_or(store))
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
                o.get("releaseOnPause")
                    .and_then(Value::as_bool)
                    .unwrap_or(true),
            );
        }
        if s.get("exclusiveMode")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
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

    /// 最近播放保留周期（天）→ 裁剪下限（unix 秒）。`None` = 永久保留。
    ///
    /// 缺失/类型坏按默认 90（与前端 DEFAULT_SETTINGS 一致，防配置损坏变成
    /// 无限膨胀）；数值钳制见 [`history_cutoff_days`]。
    fn history_cutoff(&self, now_secs: i64) -> Option<i64> {
        let days = self
            .settings
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get("historyDays")
            .and_then(Value::as_i64)
            .unwrap_or(90);
        history_cutoff_days(days, now_secs)
    }

    /// 按保留周期裁剪播放历史（启动 / 每次写历史 / 设置变更时调用）。
    fn prune_history(&self) {
        let Some(cutoff) = self.history_cutoff(now_secs()) else {
            return;
        };
        let result: aiya_data::DbResult<usize> = self
            .db()
            .send(move |conn| aiya_data::history::prune(conn, cutoff));
        match result {
            Ok(0) => {}
            Ok(n) => tracing::info!(removed = n, "按保留周期裁剪了播放历史"),
            Err(e) => tracing::warn!(error = %e, "播放历史裁剪失败"),
        }
    }
}

fn default_db_path(app: &AppHandle) -> PathBuf {
    let dir = paths::data_root(app);
    let _ = std::fs::create_dir_all(&dir);
    dir.join("library.sqlite")
}

fn default_artwork_dir(app: &AppHandle) -> PathBuf {
    paths::data_root(app).join("cache").join("artwork")
}

/// 启动期致命错误的唯一用户可见出口：release 无控制台、setup 失败时窗口还没建，
/// 不弹框 = 用户双击后"什么都不发生"。只用于确定无法继续启动的场合
/// （数据库打不开等）；可降级的故障一律降级继续跑，不走这里。
#[cfg(windows)]
fn fatal_dialog(message: &str) {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        MessageBoxW, MB_ICONERROR, MB_SETFOREGROUND,
    };
    let wide = |s: &str| -> Vec<u16> {
        std::ffi::OsStr::new(s)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    };
    let text = wide(message);
    let caption = wide("AIYA Music");
    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            text.as_ptr(),
            caption.as_ptr(),
            MB_ICONERROR | MB_SETFOREGROUND,
        );
    }
}

#[cfg(not(windows))]
fn fatal_dialog(message: &str) {
    tracing::error!(%message, "启动失败");
}

/* ---------------- 工具：settings 表读写 ---------------- */

fn setting_get(conn: &rusqlite::Connection, key: &str) -> Option<Value> {
    conn.query_row(
        "SELECT value_json FROM settings WHERE key = ?1",
        [key],
        |r| r.get::<_, String>(0),
    )
    .ok()
    .and_then(|s| serde_json::from_str(&s).ok())
}

/// 键是否存在（不解析值）。`roots_from_settings` 用它区分"空数组（权威）"
/// 与"键没写过（回落旧键）"——只看 `setting_get` 区分不了这两者。
pub(crate) fn setting_has(conn: &rusqlite::Connection, key: &str) -> bool {
    conn.query_row("SELECT 1 FROM settings WHERE key = ?1", [key], |_| Ok(()))
        .is_ok()
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
  "ambient": "aurora",
  "normalizeVolume": false,
  "glEffects": true,
  "bgSpectrum": false,
  "npSpectrum": false,
  "spectrumStyle": "mirror",
  "ridgeGlow": "balanced",
  "mediaKeys": true,
  "output": { "mode": "shared", "bufferMs": 0, "clock": "auto", "releaseOnPause": true },
  "autoplayOnStart": false,
  "rememberPosition": true,
  "preventSleep": true,
  "closeToTray": false,
  "refreshSeconds": 30,
  "historyDays": 90,
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

/// EQ 设置（协议 v6，docs/24 B5b）。持久化独立键 `eq`，落库成功后即时下发。
///
/// 为什么不是 `app_set_settings` 的一部分：EQ 滑杆是高频连续调整（拖动中每
/// 300ms 一次），设置页是低频整体写。混在一起，两边都要做字段级 diff 才能不
/// 互相覆盖；分键之后各写各的，天然无冲突。
#[tauri::command]
fn player_set_eq(
    app_state: State<'_, AppState>,
    eq: Json,
) -> Result<dto::EqSettings, dto::ApiError> {
    let mut v: dto::EqSettings = serde_json::from_value(eq)
        .map_err(|e| dto::ApiError::invalid_arg(format!("EQ 参数不合法：{e}")))?;
    if v.bands.len() != aiya_audio::EQ_BAND_HZ.len() {
        return Err(dto::ApiError::invalid_arg(format!(
            "EQ 需要 {} 段增益，收到 {} 段",
            aiya_audio::EQ_BAND_HZ.len(),
            v.bands.len()
        )));
    }
    // 入口 clamp：持久化的是生效值，UI 回读不会见到"发了 20 实际 12"的漂移
    // （引擎入口还有一次同样的 clamp，两处一致，脏数据无处可藏）
    for b in &mut v.bands {
        *b = b.clamp(-aiya_audio::EQ_GAIN_LIMIT_DB, aiya_audio::EQ_GAIN_LIMIT_DB);
    }
    v.bass_db = v
        .bass_db
        .clamp(-aiya_audio::EQ_GAIN_LIMIT_DB, aiya_audio::EQ_GAIN_LIMIT_DB);
    v.treble_db = v
        .treble_db
        .clamp(-aiya_audio::EQ_GAIN_LIMIT_DB, aiya_audio::EQ_GAIN_LIMIT_DB);
    v.pregain = v.pregain.clamp(
        -aiya_audio::EQ_PREGAIN_LIMIT_DB,
        aiya_audio::EQ_PREGAIN_LIMIT_DB,
    );

    let value = serde_json::to_value(&v).map_err(|e| dto::ApiError::invalid_arg(e.to_string()))?;
    app_state
        .db()
        .send(move |conn| setting_set(conn, "eq", &value).map_err(aiya_data::DbError::Sql))
        .map_err(dto::ApiError::from)?;
    engine_send(
        &app_state,
        aiya_audio::EngineCommand::SetEq(aiya_audio::EqParams {
            bands: std::array::from_fn(|i| v.bands[i]),
            bass_db: v.bass_db,
            treble_db: v.treble_db,
            pregain: v.pregain,
            enabled: v.enabled,
        }),
    );
    Ok(v)
}

/// 当前 EQ 设置（面板打开时回读一次；低频，直接走 DB 不进缓存）。
/// 没保存过 = 默认（关 + 全 0 dB）。
#[tauri::command]
fn player_get_eq(app_state: State<'_, AppState>) -> Result<dto::EqSettings, dto::ApiError> {
    let saved = app_state
        .db()
        .send(|conn| Ok(setting_get(conn, "eq")))
        .map_err(dto::ApiError::from)?;
    Ok(saved
        .and_then(|v| serde_json::from_value::<dto::EqSettings>(v).ok())
        .unwrap_or_else(|| dto::EqSettings {
            enabled: false,
            bands: vec![0.0; aiya_audio::EQ_BAND_HZ.len()],
            bass_db: 0.0,
            treble_db: 0.0,
            pregain: 0.0,
        }))
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
fn app_set_ui_state(app_state: State<'_, AppState>, state: Json) -> Result<(), dto::ApiError> {
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
fn app_set_settings(
    app: AppHandle,
    state: State<'_, AppState>,
    settings: Json,
) -> Result<Json, dto::ApiError> {
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
        engine_send(
            &state,
            aiya_audio::EngineCommand::SetOutputConfig(next_output),
        );
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

    // 刷新间隔是设置项：改完要让定时线程重读（否则"我改了 30 → 120，
    // 结果它还在 30 秒一刷"，得重启才生效）。
    refresh::wake_settings(&app);

    // 保留周期（historyDays）变更：立即按新周期裁剪一次，不等下次写历史。
    // 无差别跑（而不是 diff 旧值）——缓存此刻已是新值，diff 要再开一次库；
    // 空裁剪 0 行删除，代价可忽略。
    if settings
        .get("historyDays")
        .and_then(Value::as_i64)
        .is_some()
    {
        state.prune_history();
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
                streams: dto::StreamsInfo {
                    opened: 0,
                    closed: 0,
                    active: 0,
                },
                list_timings: timings,
                recent_errors: recent,
                media_keys: dto::MediaKeysInfo {
                    enabled: smtc_active,
                    active: if smtc_active {
                        vec!["SMTC".into()]
                    } else {
                        Vec::new()
                    },
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
///
/// `resume`（v2）：中断后续跑。UI 重发**同一批** decisions（会话 store 里
/// 留着），sidecar 按内容指纹跳过断点里已完成的——move/copy 中断后源文件
/// 已移走，不 resume 的重跑会对它们报错。
#[tauri::command]
async fn beets_apply(
    app: AppHandle,
    decisions: Vec<Json>,
    resume: Option<bool>,
) -> Result<dto::beets::JobStarted, dto::ApiError> {
    let Some(state) = app.try_state::<AppState>() else {
        return Err(dto::ApiError::internal("app state 不可用"));
    };
    let refresh = state.refresh.clone();
    let resume = resume.unwrap_or(false);
    tauri::async_runtime::spawn_blocking(move || {
        beets::start_apply(app, decisions, resume, Some(refresh))
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
    Ok(dto::beets::CancelRequested {
        requested: beets::cancel_session(),
    })
}

/* ---------------- beets 配置：待整理目录与整理输出根（docs/40 十三轮） ----------------
 *
 * 用户拍板的入库模型：待整理目录（inbox，用户自选的外部目录）→ beets 整理 →
 * 唯一勾选的整理输出根（曲库目录之一）。三个约束都来自拍板：
 * 1. inbox 必须用户自选，且**不得与任何曲库目录重叠**（相等或在内部都算）——
 *    copy 模式下原件留在 inbox，重叠的话整理副本会被扫描器再扫一遍，重复入库；
 * 2. 整理输出根**单选**（radio），存 `beets.targetRoot`；一个都没勾时回退
 *    第一个曲库目录（与十三轮之前的行为一致，老配置不受影响）；
 * 3. 保留原文件与否（copy/move）是导入时用户逐次选的（ImportPanel 的
 *    moveMode），不是这里的配置。
 *
 * 另有一个配套机制：`beets.inboxImported` 排除表。inbox 是固定目录，copy
 * 之后原件一直躺在那——不排除的话下次"整理入库"会把同一批文件再导一遍
 * （beets 库重复条目 → dump 同步 → 曲库出现离线重复行）。应用成功后 UI
 * 把源路径记进来，preview 时带 `options.excludePaths` 让工具跳过它们。
 */

/// Windows 路径归一（**仅用于比较**，展示/落库保留用户原始输入）：
/// 分隔符统一成 `\`、小写、去尾分隔符。
pub(crate) fn canon_path(p: &str) -> String {
    p.trim()
        .replace('/', "\\")
        .trim_end_matches('\\')
        .to_lowercase()
}

/// 两个目录是否有重叠：相等、a 在 b 里面、b 在 a 里面，三者都算。
///
/// 为什么"父目录"也算：`D:\` 与 `D:\Music` 不相等也不在对方里面（反方向），
/// 但把 `D:\` 当作待整理目录，预览会递归扫到整个曲库——同一批文件被再导一遍。
/// 三个方向覆盖完才叫"不重叠"。
///
/// 分隔符边界不能省：裸 `starts_with` 会把 `D:\Music Backup` 当成 `D:\Music`
/// 的子目录（经典前缀陷阱，单元测试 `sibling_directory_is_allowed` 钉着）。
fn paths_overlap(a: &str, b: &str) -> bool {
    let (a, b) = (canon_path(a), canon_path(b));
    if a.is_empty() || b.is_empty() {
        return false;
    }
    a == b || a.starts_with(&format!("{b}\\")) || b.starts_with(&format!("{a}\\"))
}

/// inbox 与扫描根重叠校验：任一方向重叠都算冲突。
fn path_conflicts_with_roots(path: &str, roots: &[String]) -> bool {
    if canon_path(path).is_empty() {
        return true;
    }
    roots.iter().any(|r| paths_overlap(path, r))
}

/// `path` 是否落在某个根的**内部或与之相等**（不含"是根的父目录"）。
///
/// 整理落点用的就是这条：落点在曲库之外 → 整理完的文件扫描器看不见，用户会
/// 看到"整理成功了，曲库里没有"。
pub(crate) fn path_within_roots(path: &str, roots: &[String]) -> bool {
    let c = canon_path(path);
    if c.is_empty() {
        return false;
    }
    roots.iter().any(|r| {
        let rc = canon_path(r);
        !rc.is_empty() && (c == rc || c.starts_with(&format!("{rc}\\")))
    })
}

/// 从 settings 读一个字符串键（空串视为未设置）。
fn setting_str_value(conn: &rusqlite::Connection, key: &str) -> Option<String> {
    setting_get(conn, key)
        .and_then(|v| v.as_str().map(str::to_string))
        .filter(|s| !s.trim().is_empty())
}

/// 曲库目录（应用事实）。beets 的配置视图要用它判断"勾的输出根还在不在"。
fn roots_of(app: &tauri::AppHandle) -> Result<Vec<String>, dto::ApiError> {
    app.try_state::<AppState>()
        .ok_or_else(|| dto::ApiError::internal("app state 不可用"))?
        .db()
        .send(|conn| Ok(read_roots(conn)))
        .map_err(|e| dto::ApiError::internal(format!("读取曲库目录失败：{e}")))
}

/// beets 配置快照（UI 的待整理目录行与输出根 radio 都从这里读）。
///
/// 数据源是 `{beets}/settings.json`（模块自有），**不是**应用的 settings 表。
#[tauri::command]
fn beets_config_get(app: AppHandle) -> Result<dto::beets::BeetsConfig, dto::ApiError> {
    let roots = roots_of(&app)?;
    Ok(beets::config_view(&app, &roots))
}

/// 设置待整理目录。`None` = 清除（顺带清空已导入记录——旧记录对新目录无意义）。
#[tauri::command]
fn beets_set_inbox(
    app: AppHandle,
    path: Option<String>,
) -> Result<dto::beets::BeetsConfig, dto::ApiError> {
    // 两段式（读 roots → 校验 → 写）：校验放在写之前才能把「与曲库目录重叠」
    // 表达成 invalidArg（UI 可识别人话提示），而不是"保存了但导向重复曲目"。
    let trimmed = path.as_deref().map(str::trim).filter(|s| !s.is_empty());
    let roots = roots_of(&app)?;
    if let Some(p) = trimmed {
        if path_conflicts_with_roots(p, &roots) {
            return Err(dto::ApiError::invalid_arg(
                "待整理目录不能与曲库目录相同或在其内部",
            ));
        }
    }
    let inbox = trimmed.map(str::to_string);
    beets::update_settings(&app, move |s| {
        s.inbox_path = inbox;
        s.inbox_imported.clear();
    })
    .map_err(dto::ApiError::internal)?;
    Ok(beets::config_view(&app, &roots))
}

/// 勾选整理落点（docs/49 §4：落点单独选，可以是曲库目录本身，也可以是它
/// 里面的子目录——"整理到 D:\Music\整理" 这种分法是常态）。
///
/// 必须在曲库目录**之内**：落点在曲库外，整理完的文件扫描器看不见，症状是
/// "整理成功了，曲库里没有"，而 beets 侧一切正常。
///
/// 换落点只影响未来的整理；beets 存的是绝对路径，旧条目不受影响。
#[tauri::command]
fn beets_set_target(
    app: AppHandle,
    path: String,
) -> Result<dto::beets::BeetsConfig, dto::ApiError> {
    let p = path.trim().to_string();
    let roots = roots_of(&app)?;
    if !path_within_roots(&p, &roots) {
        return Err(dto::ApiError::invalid_arg(
            "整理落点必须在曲库目录之内（可以选曲库目录本身或它的子目录）",
        ));
    }
    let saved = p;
    beets::update_settings(&app, move |s| s.target_root = Some(saved))
        .map_err(dto::ApiError::internal)?;
    Ok(beets::config_view(&app, &roots))
}

/// 在资源管理器打开待整理目录（用户往里拖文件的入口）。
#[tauri::command]
fn beets_open_inbox(app: AppHandle) -> Result<(), dto::ApiError> {
    let inbox = beets::load_settings(&app)
        .inbox_path
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let Some(dir) = inbox else {
        return Err(dto::ApiError::invalid_arg("还没设置待整理目录"));
    };
    let _ = std::process::Command::new("explorer").arg(dir).spawn();
    Ok(())
}

/// 记录"已从待整理目录成功导入"的源文件路径（preview 排除表）。
#[tauri::command]
fn beets_inbox_imported_add(
    app: AppHandle,
    paths: Vec<String>,
) -> Result<dto::beets::BeetsConfig, dto::ApiError> {
    beets::remember_imported(&app, paths).map_err(dto::ApiError::internal)?;
    let roots = roots_of(&app)?;
    Ok(beets::config_view(&app, &roots))
}

/// 单元测试：路径重叠校验是这套入库模型的安全边界（inbox 混进扫描根 =
/// 重复曲目），正反例都要钉死。
#[cfg(test)]
mod beets_config_tests {
    use super::{canon_path, path_conflicts_with_roots, path_within_roots};

    fn roots() -> Vec<String> {
        vec!["D:\\Music".into(), "E:\\Audio\\FLAC".into()]
    }

    #[test]
    fn canon_unifies_separators_case_and_trailing_slash() {
        assert_eq!(canon_path("D:/Music/"), canon_path("d:\\music"));
        assert_eq!(canon_path(" D:\\Music "), "d:\\music");
    }

    #[test]
    fn inbox_equal_to_root_conflicts() {
        assert!(path_conflicts_with_roots("D:/Music", &roots()));
    }

    #[test]
    fn inbox_inside_root_conflicts() {
        assert!(path_conflicts_with_roots(
            "D:\\Music\\New\\Albums",
            &roots()
        ));
        assert!(path_conflicts_with_roots("d:/music/new", &roots()));
    }

    #[test]
    fn sibling_directory_is_allowed() {
        // 经典前缀陷阱：D:\Music Backup 不能被 D:\Music 误伤
        assert!(!path_conflicts_with_roots("D:\\Music Backup", &roots()));
        assert!(!path_conflicts_with_roots("E:\\Downloads", &roots()));
    }

    #[test]
    fn parent_of_root_conflicts() {
        // 反方向的重叠：把 D:\ 当整理源，预览会递归扫到整个曲库
        assert!(path_conflicts_with_roots("D:\\", &roots()));
        // 前缀但不在分隔符边界上：D:\Mu 不是 D:\Music 的祖先目录
        assert!(!path_conflicts_with_roots("D:\\Mu", &roots()));
    }

    #[test]
    fn empty_path_conflicts() {
        assert!(path_conflicts_with_roots("  ", &roots()));
    }

    /* ---- 整理落点（docs/49 §4）：在内即可，不必相等 ---- */

    #[test]
    fn target_accepts_root_and_subdirectory() {
        assert!(path_within_roots("D:\\Music", &roots()));
        assert!(path_within_roots("D:\\Music\\整理", &roots()));
        assert!(path_within_roots("d:/music/a/b", &roots()));
    }

    #[test]
    fn target_rejects_outside_and_fake_sibling() {
        // 落点在曲库外 → 整理完扫描器看不见
        assert!(!path_within_roots("E:\\Sorted", &roots()));
        // 假兄弟：裸 starts_with 会把它当成 D:\Music 的子目录
        assert!(!path_within_roots("D:\\Music2", &roots()));
        // 父目录不是"之内"
        assert!(!path_within_roots("D:\\", &roots()));
        assert!(!path_within_roots("  ", &roots()));
    }
}

/// 自动刷新的当前配置与最近一次结果（诊断区读）。
#[tauri::command]
fn refresh_state(state: State<'_, AppState>) -> Result<dto::RefreshState, dto::ApiError> {
    let interval_secs = state
        .settings
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .get("refreshSeconds")
        .and_then(Value::as_u64)
        // 与 refresh::interval_secs 同一口径（下限 10 / 上限 1 h），否则
        // 诊断页显示的间隔和定时器实际跑的对不上
        .map(|n| if n == 0 { 0 } else { n.clamp(10, 3600) })
        .unwrap_or(refresh::DEFAULT_INTERVAL_SECS);
    let last = state
        .last_refresh
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    Ok(dto::RefreshState {
        interval_secs,
        focused: refresh::focused(),
        last,
    })
}

/// 安装 beets 侧组件（docs/33 §6.2 两条轨道 / docs/51 §4）。
///
/// `track` = `installer`（路径 B：本机 Python 建 venv，可勾选插件）/
/// `bundle`（路径 A：免配置套件包，预置插件，不需要本机 Python）。
/// 播放器只做「下载 → 展开/装依赖 → 记下入口 → 跑一次 status」，不碰 Python 工具链。
/// 进度走 `beets:install` 事件，结束时推一条 `finished` 带新 status。
///
/// `plugins` 是勾选的**启用**清单（只有 installer 轨道有意义）：安装器据此装
/// 额外依赖，宿主把它写进 `settings.plugins`，工具侧渲染 config 时照它启用。
#[tauri::command]
async fn beets_install(
    app: AppHandle,
    track: Option<String>,
    plugins: Option<Vec<String>>,
) -> Result<dto::beets::JobStarted, dto::ApiError> {
    beets::start_install(app, track.unwrap_or_default(), plugins.unwrap_or_default())
        .map(|_| dto::beets::JobStarted { started: true })
        .map_err(dto::ApiError::internal)
}

/// 本机 Python 探测（路径 B 的前置条件）。
///
/// 探测顺序：Windows 的 `py -0p` → PATH 上的 `python` / `python3`。
/// 只试后两个会把"装了 Python 但没写 PATH"的机器判成没装（python.org 的
/// 安装包默认就是这样）。找不到时 `reason` 里带一句人话，UI 直接用。
#[tauri::command]
async fn beets_python_probe() -> Result<dto::beets::BeetsPythonProbe, dto::ApiError> {
    tauri::async_runtime::spawn_blocking(beets::python_probe_payload)
        .await
        .map_err(dto::ApiError::internal)
}

/// 内嵌插件表：不依赖已安装组件就能拿到的静态目录（docs/51 §1）。
///
/// 与 `beets_plugin_catalog` 的分工：这条回答"有哪些插件、各自要装什么"
/// （编译期内嵌 JSON，永远可用）；那条回答"装了之后哪些真的 ready"
/// （要起 sidecar，未安装时不可用）。
#[tauri::command]
async fn beets_plugin_table() -> Result<Vec<dto::beets::BeetsPluginItem>, dto::ApiError> {
    tauri::async_runtime::spawn_blocking(beets::plugin_table)
        .await
        .map_err(dto::ApiError::internal)?
        .map_err(dto::ApiError::internal)
}

/// 卸载组件：只删安装目录（venv / bundle / 缓存），**不动**整理库与分析库。
///
/// 数据由应用自己的清理入口管（2026-10-30 拍板：卸载 = 安装的反向操作）。
/// 进度复用 `beets:install` 事件流，结束时补 `finished` 带新 status。
#[tauri::command]
async fn beets_uninstall(app: AppHandle) -> Result<dto::beets::JobStarted, dto::ApiError> {
    beets::start_uninstall(app)
        .map(|_| dto::beets::JobStarted { started: true })
        .map_err(dto::ApiError::internal)
}

/// 在资源管理器中定位一个**绝对路径**（批 D 查重结果用）。
///
/// 为什么不复用 `library_reveal_track`：那条按 track id 查自有库拿路径，而
/// 查重结果来自 beets 库——里面可能有自有库没有的条目（例如用户在 beets
/// 侧导入过、还没对账的文件）。传路径是最直接的，也只做 `/select` 定位，
/// 不会打开文件本身。
#[tauri::command]
fn beets_reveal(path: String) -> Result<dto::library::RevealResult, dto::ApiError> {
    if path.trim().is_empty() || !std::path::Path::new(&path).is_file() {
        return Ok(dto::library::RevealResult {
            revealed: false,
            reason: Some("文件不存在".into()),
        });
    }
    let revealed = proc::reveal_in_explorer(std::path::Path::new(&path));
    Ok(dto::library::RevealResult {
        revealed,
        reason: if revealed {
            None
        } else {
            Some("explorer 启动失败".into())
        },
    })
}

/* ---------------- 批 D：beets 曲库维护 ---------------- */

/// 维护任务启动失败的错误分类。
///
/// 只有"没有可用的 beets 解释器/组件"该映射成 `beetsMissing`（UI 弹安装指引，
/// 与 preview/apply 一致）；"已有任务在执行"、"脚本落盘失败"这些是普通失败，
/// 塞进 `beetsMissing` 会让用户以为自己没装 beets 而反复重装。
/// 靠消息串判断是因为这条边界本身就是 String（sidecar 是外部进程）。
///
/// 两个关键词都要认：`tool_or_err` 的文案是"未安装 beets 组件…"（不是
/// "interpreter"），只匹配后者会让"没装"落到 `invalidArg`，UI 就不给安装指引了。
fn map_beets_start(e: String) -> dto::ApiError {
    if e.contains("interpreter") || e.contains("未安装") {
        dto::ApiError::beets_missing(e)
    } else {
        dto::ApiError::invalid_arg(e)
    }
}

/// 建 beets 库基线：把自有库里的**在线**曲目按现状登记进 beets 库
/// （as-is 入库：不联网、不改标签、不移动文件）。
///
/// `paths` 为空 = 取自有库全部在线路径。UI 永远传空数组进来——让前端把
/// 上千条路径经 IPC 传一遍纯属浪费，而且它拿不到"哪些行是可播的"这个语义
/// （那是 DB 的事实，不是 UI 的）。
/// 完成后自动跑一次读模型对账（`beets_item_id` 要落库，后续 move 的路径
/// 跟随才有双键依据）。
#[tauri::command]
async fn beets_adopt(
    app: AppHandle,
    paths: Vec<String>,
) -> Result<dto::beets::JobStarted, dto::ApiError> {
    let Some(state) = app.try_state::<AppState>() else {
        return Err(dto::ApiError::internal("app state 不可用"));
    };
    let refresh = state.refresh.clone();
    let paths = if paths.is_empty() {
        state
            .db()
            .send(|conn| aiya_data::repo::online_paths(conn))
            .map_err(dto::ApiError::from)?
    } else {
        paths
    };
    tauri::async_runtime::spawn_blocking(move || beets::start_adopt(app, paths, Some(refresh)))
        .await
        .map_err(dto::ApiError::internal)?
        .map_err(map_beets_start)?;
    Ok(dto::beets::JobStarted { started: true })
}

/// 用文件里的标签刷新 beets 库。`pretend` = 只报 diff、不写库（预览）。
#[tauri::command]
async fn beets_update(
    app: AppHandle,
    pretend: bool,
) -> Result<dto::beets::JobStarted, dto::ApiError> {
    tauri::async_runtime::spawn_blocking(move || beets::start_update(app, pretend))
        .await
        .map_err(dto::ApiError::internal)?
        .map_err(map_beets_start)?;
    Ok(dto::beets::JobStarted { started: true })
}

/// 按字段组查重（只读报告，不删任何文件）。
#[tauri::command]
async fn beets_duplicates(
    app: AppHandle,
    keys: Vec<String>,
) -> Result<dto::beets::JobStarted, dto::ApiError> {
    tauri::async_runtime::spawn_blocking(move || beets::start_duplicates(app, keys))
        .await
        .map_err(dto::ApiError::internal)?
        .map_err(map_beets_start)?;
    Ok(dto::beets::JobStarted { started: true })
}

/// 按 path_formats 整理文件（`pretend` = 只出清单，不搬文件）。
///
/// `directory` 是**校验值**：sidecar 拿它跟 config.yaml 的 `directory` 比对，
/// 不一致就拒绝执行——库根只能有一处定义，两个基准搬文件会让读模型对错号。
/// 完成后自动对齐读模型（路径漂移必须立刻跟随，不等下次扫描）。
#[tauri::command]
async fn beets_move(
    app: AppHandle,
    directory: String,
    pretend: bool,
) -> Result<dto::beets::JobStarted, dto::ApiError> {
    let Some(state) = app.try_state::<AppState>() else {
        return Err(dto::ApiError::internal("app state 不可用"));
    };
    let refresh = state.refresh.clone();
    tauri::async_runtime::spawn_blocking(move || {
        beets::start_move(app, directory, pretend, Some(refresh))
    })
    .await
    .map_err(dto::ApiError::internal)?
    .map_err(map_beets_start)?;
    Ok(dto::beets::JobStarted { started: true })
}

/// beets 查询页：读 beets 库（工作台家），返回一页命中（docs/47）。
///
/// 参数三选一（`rules` 结构化 / `raw` 逃生口 / 都不给=全库），`rules` 与
/// `raw` 同时给会被工具侧拒——不在这里做"谁优先"的判断，那只会让调用方
/// 永远不知道自己实际查了什么。
#[tauri::command]
async fn beets_query(
    app: AppHandle,
    rules: Option<Json>,
    raw: Option<String>,
    fields: Option<Vec<String>>,
    limit: Option<u32>,
) -> Result<dto::beets::BeetsQueryPage, dto::ApiError> {
    let raw = raw.filter(|s| !s.trim().is_empty());
    tauri::async_runtime::spawn_blocking(move || {
        beets::query_page(&app, rules, raw, fields, limit.map(|v| v as usize))
    })
    .await
    .map_err(dto::ApiError::internal)?
    .map_err(map_beets_start)
}

/// 可查字段清单（含插件写入的 flexible 键）+ 识别源三态。
///
/// 与 `enrichment_query` 的分工：这条查的是 **beets 库**（用户自己的库，
/// 字段取决于他装了哪些插件）；那条查的是**索引增强的结论库**（应用独占，
/// 字段固定是 xtractor 的那 20 个）。查询页用这条，智能歌单用那条。
#[tauri::command]
async fn beets_fields(app: AppHandle) -> Result<dto::beets::BeetsFieldCatalog, dto::ApiError> {
    tauri::async_runtime::spawn_blocking(move || beets::field_catalog(&app))
        .await
        .map_err(dto::ApiError::internal)?
        .map_err(map_beets_start)
}

/// 插件市场目录（docs/40 §3 白名单）：三态 + 来源档，一次性短进程查询。
#[tauri::command]
async fn beets_plugin_catalog(
    app: AppHandle,
) -> Result<Vec<dto::beets::BeetsMarketItem>, dto::ApiError> {
    tauri::async_runtime::spawn_blocking(move || beets::plugin_catalog(&app))
        .await
        .map_err(dto::ApiError::internal)?
        .map_err(map_beets_start)
}

/// 安装一个白名单插件（仅 pypi 档有意义）。事件流走 `beets:maintenance`
/// （pluginProgress → pluginDone），与维护四件套共用同一会话槽。
#[tauri::command]
async fn beets_plugin_install(
    app: AppHandle,
    name: String,
) -> Result<dto::beets::JobStarted, dto::ApiError> {
    tauri::async_runtime::spawn_blocking(move || beets::start_plugin_install(app, name))
        .await
        .map_err(dto::ApiError::internal)?
        .map_err(map_beets_start)?;
    Ok(dto::beets::JobStarted { started: true })
}

/* ---------------- 索引增强（docs/45 §5.4） ---------------- */

/// 曲库索引卡的数据源：一次聚合计数 + 清单前若干条。
///
/// `include_empty` = "连上次跑空的也重算"。默认 false：`empty` 的语义是
/// "这个源对这类文件没结论"，每次分析都重试等于让那批文件永久占着清单。
#[tauri::command]
async fn enrichment_worklist(
    app: AppHandle,
    source: Option<String>,
    include_empty: Option<bool>,
) -> Result<dto::enrichment::EnrichmentWorklist, dto::ApiError> {
    let source = enrichment::normalize_source(source).map_err(dto::ApiError::invalid_arg)?;
    let include_empty = include_empty.unwrap_or(false);
    tauri::async_runtime::spawn_blocking(move || enrichment::worklist(&app, &source, include_empty))
        .await
        .map_err(dto::ApiError::internal)?
        .map_err(dto::ApiError::internal)
}

/// 起一次分析（清单为空就不起进程）。
///
/// `force` 同时表达两件事，缺一不可：
/// - 宿主侧：把 `status='empty'` 的也捞进清单（否则"强制重算"对上次跑空的
///   文件毫无作用，用户会以为按钮坏了）；
/// - sidecar 侧：先清旧再补登（`item.remove()` + 重导），否则 beets 会把
///   "已在库"的文件直接跳过，插件根本不跑（docs/45 §3.4 实测）。
///
/// 清单在 `beets::start_enrich` 里现算而不是在这里：算清单要用到"曲库快照 +
/// 结论库"两份状态，把其中一半摊在命令层只会让"谁的清单"这个问题有两个答案。
#[tauri::command]
async fn beets_enrich_start(
    app: AppHandle,
    source: Option<String>,
    force: Option<bool>,
) -> Result<dto::beets::JobStarted, dto::ApiError> {
    let source = enrichment::normalize_source(source).map_err(dto::ApiError::invalid_arg)?;
    let force = force.unwrap_or(false);
    tauri::async_runtime::spawn_blocking(move || beets::start_enrich(app, source, force))
        .await
        .map_err(dto::ApiError::internal)?
        .map_err(map_beets_start)?;
    Ok(dto::beets::JobStarted { started: true })
}

/// 只读入库（索引库建索引）：把**曲库根**整批登记进索引家，不做分析。
///
/// 为什么单独一条命令：查询页读的是索引库，它空着的时候任何筛选都是 0 命中。
/// 而 `enrich` 在分析器（xtractor）未接线时整批被拒——入库不该被分析器的
/// 进度绑架（docs/49 §3.8）。进度走 `beets:index`，终态 `indexDone`。
///
/// `root` 缺省 = 曲库根（UI 上的「建立索引 / 重建索引」）。整理完的自动补索引
/// 由宿主自己调 `beets::start_index_import` 并传落点，不经过这里——那条路径
/// 不需要 UI 参与（关掉整理页也应当补上）。
#[tauri::command]
async fn beets_index_import(
    app: AppHandle,
    root: Option<String>,
) -> Result<dto::beets::JobStarted, dto::ApiError> {
    tauri::async_runtime::spawn_blocking(move || beets::start_index_import(app, root))
        .await
        .map_err(dto::ApiError::internal)?
        .map_err(map_beets_start)?;
    Ok(dto::beets::JobStarted { started: true })
}

/// [`enrichment_query`] 的参数。放这里而不是 `dto`：它是**入参**，
/// 参数形状出错要映射成 `invalidArg`，不能靠 Tauri 自己的反序列化文案。
#[derive(Debug, Default, serde::Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct EnrichmentQueryArgs {
    source: Option<String>,
    mood: Option<String>,
    dance_min: Option<f64>,
    dance_max: Option<f64>,
    bpm_min: Option<f64>,
    bpm_max: Option<f64>,
    limit: Option<usize>,
}

/// 按维度查（曲库页/智能歌单）。未装 beets 时返回空数组而不是报错：
/// 关联表里的历史结论与被分析的对象是否可用无关。
#[tauri::command]
async fn enrichment_query(
    app: AppHandle,
    query: Option<Json>,
) -> Result<Vec<dto::enrichment::EnrichmentMatch>, dto::ApiError> {
    let args: EnrichmentQueryArgs = match query {
        None => EnrichmentQueryArgs::default(),
        Some(v) => serde_json::from_value(v)
            .map_err(|e| dto::ApiError::invalid_arg(format!("查询参数不合法：{e}")))?,
    };
    let filter = aiya_beets_store::Filter {
        source: args.source,
        mood: args.mood,
        dance_min: args.dance_min,
        dance_max: args.dance_max,
        bpm_min: args.bpm_min,
        bpm_max: args.bpm_max,
        limit: args.limit,
    };
    tauri::async_runtime::spawn_blocking(move || enrichment::find(&app, filter))
        .await
        .map_err(dto::ApiError::internal)?
        .map_err(dto::ApiError::internal)
}

/// 清理残留行：曲库已经没有对应曲目、且 `keepDays` 天内没更新过。
///
/// 为什么要有宽限期：换曲库根的那一分钟里所有旧路径都"找不到曲目"，
/// 立刻删等于把用户几小时的分析成果一键清零（docs/45 §7②）。
#[tauri::command]
async fn enrichment_gc(
    app: AppHandle,
    source: Option<String>,
    keep_days: Option<u32>,
) -> Result<dto::enrichment::EnrichmentGc, dto::ApiError> {
    let source = enrichment::normalize_source(source).map_err(dto::ApiError::invalid_arg)?;
    let keep_days = keep_days.unwrap_or(30).clamp(1, 3650);
    tauri::async_runtime::spawn_blocking(move || enrichment::gc(&app, &source, keep_days))
        .await
        .map_err(dto::ApiError::internal)?
        .map_err(dto::ApiError::internal)
}

#[tauri::command]
async fn library_list(
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
    // P2（docs/38）：此命令曾是同步——Tauri 2 的同步命令跑在主线程，每页
    // 都要在主线程干等 DB 线程回包（5 万行深页 10–35ms），快速滚动时多页
    // 请求在主线程排成连续冻结段。改 async + spawn_blocking：等待发生在
    // tokio 池，主线程全程空闲。DbHandle 是克隆型句柄（db.rs §161）。
    let db = state.db().clone();
    let page = tauri::async_runtime::spawn_blocking(move || {
        db.send(move |conn| repo::list_tracks(conn, &q))
    })
    .await
    .map_err(dto::ApiError::internal)?
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
    state
        .db()
        .send(move |conn| {
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
                missing_locations: count(
                    "SELECT COUNT(*) FROM tracks WHERE availability <> 'online'",
                ),
                total_duration_ms: count("SELECT COALESCE(SUM(duration_ms),0) FROM tracks"),
                // 文件容量：曲目文件字节之和（DB 存 unix 秒、size 是扫描时的
                // meta.len()）。离线曲目仍按入库时的大小计入——它回答的是
                // "这个库装了多少音乐"，不是"现在盘上还剩多少"。
                total_bytes: count("SELECT COALESCE(SUM(size_bytes),0) FROM tracks"),
                db_bytes,
                schema_version: conn
                    .query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                    .unwrap_or(0),
            })
        })
        .map_err(dto::ApiError::from)
}

/// 专辑浏览聚合（docs/24 §3.2 决策 2）。只含实际有曲目的专辑，
/// 一次性返回前 N（`facets::FACET_LIMIT`），无游标——聚合本身是全表
/// GROUP BY，5 万行量级个位数毫秒，分页反而多余。
#[tauri::command]
async fn library_albums(
    state: State<'_, AppState>,
    search: Option<String>,
) -> Result<aiya_data::facets::AlbumPage, dto::ApiError> {
    // P2（docs/38）：与 library_list 同理——宫格滚动/搜索每键一次，
    // 不能占主线程等 DB。GROUP BY 本身个位数毫秒，卸载是零成本保险。
    let db = state.db().clone();
    let page = tauri::async_runtime::spawn_blocking(move || {
        db.send(move |conn| aiya_data::facets::list_albums(conn, search.as_deref()))
    })
    .await
    .map_err(dto::ApiError::internal)?
    .map_err(dto::ApiError::from)?;
    Ok(page)
}

/// 艺人浏览聚合。与 `library_albums` 同形状（拆两条命令的理由见 facets.rs 头注释）。
#[tauri::command]
async fn library_artists(
    state: State<'_, AppState>,
    search: Option<String>,
) -> Result<aiya_data::facets::ArtistPage, dto::ApiError> {
    // P2（docs/38）：与 library_albums 同理。
    let db = state.db().clone();
    let page = tauri::async_runtime::spawn_blocking(move || {
        db.send(move |conn| aiya_data::facets::list_artists(conn, search.as_deref()))
    })
    .await
    .map_err(dto::ApiError::internal)?
    .map_err(dto::ApiError::from)?;
    Ok(page)
}

/// 曲库现有流派列表（动态歌单"流派"条件的下拉数据源）。
#[tauri::command]
async fn library_genres(state: State<'_, AppState>) -> Result<Vec<String>, dto::ApiError> {
    let db = state.db().clone();
    let genres = tauri::async_runtime::spawn_blocking(move || {
        db.send(move |conn| aiya_data::facets::distinct_genres(conn))
    })
    .await
    .map_err(dto::ApiError::internal)?
    .map_err(dto::ApiError::from)?;
    Ok(genres)
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

    // 根列表优先级（docs/29 批 A，多目录并集单库）：
    // 显式 options.roots > 设置里的根。
    //
    // 设置那份解析交给 `refresh::roots_from_settings` 统一做：手动扫描与
    // 自动刷新必须扫同一批目录，两处各写一遍的话迟早会不一样
    // （症状：手动扫得到新歌，30 秒定时扫不到）。
    let mut roots: Vec<PathBuf> = if let Some(list) = options
        .as_ref()
        .and_then(|o| o.get("roots"))
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|s| s.as_str())
                .filter(|s| !s.trim().is_empty())
                .map(PathBuf::from)
                .collect::<Vec<_>>()
        })
        .filter(|v: &Vec<PathBuf>| !v.is_empty())
    {
        list
    } else {
        // DB 故障要如实报 internal，不能吞成"未配置扫描目录"（那会把用户
        // 引去改配置，而配置根本没坏）。scanning 已置位，提前返回前先复位。
        match state
            .db()
            .send(|conn| Ok(refresh::roots_from_settings(conn)))
        {
            Ok(roots) => roots,
            Err(e) => {
                state.scanning.store(false, Ordering::SeqCst);
                return Err(dto::ApiError::internal(e));
            }
        }
    };

    // 逐个验证：无效根跳过（不中断扫描），全部无效才报错——报错信息带上每个根，
    // 让用户能对照 textarea 修
    let invalid: Vec<String> = roots
        .iter()
        .filter(|r| !r.is_dir())
        .map(|r| r.to_string_lossy().into_owned())
        .collect();
    roots.retain(|r| r.is_dir());
    if roots.is_empty() {
        state.scanning.store(false, Ordering::SeqCst);
        let detail = if invalid.is_empty() {
            "未配置任何扫描目录".to_string()
        } else {
            format!("目录不存在：{}", invalid.join("、"))
        };
        return Err(dto::ApiError::invalid_arg(detail));
    }
    if !invalid.is_empty() {
        tracing::warn!(skipped = %invalid.join("、"), "扫描跳过不存在的根目录");
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
            let scan_result = scanner::scan_roots_ext(
                &mut conn,
                &roots,
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
fn library_scan_cancel(
    state: State<'_, AppState>,
) -> Result<dto::library::ScanCancel, dto::ApiError> {
    // 置位取消请求，扫描循环在 walk 每条目 / 每批事务边界自行收尾
    //（取消路径不执行 mark_missing，防止误标在线文件）。
    if state.scanning.load(Ordering::SeqCst) {
        state.scan_cancel.store(true, Ordering::SeqCst);
        Ok(dto::library::ScanCancel { cancelled: true })
    } else {
        Ok(dto::library::ScanCancel { cancelled: false })
    }
}

/// 选一个文件夹，**只回路径，不写任何设置**。
///
/// 为什么不能拿 `library_choose_root` 顶替：那条会把选中的目录**追加进
/// `library.roots`**（它是"加一个曲库目录"的动作）。用它来选「待整理目录」
/// 会把待整理目录变成曲库根——copy 模式下原件留在 inbox，扫描器再扫一遍就是
/// 重复曲目；更要命的是紧接着 `beets_set_inbox` 的重叠校验会拒绝这个选择，
/// 用户看到的是"待整理目录不能与曲库目录相同或在其内部"，完全猜不到是自己
/// 刚才那一下把它加进去的。整理落点同理（落点校验比的是"在不在曲库里"）。
///
/// `beets_preview` 的源、`beets_set_inbox` 的值、`beets_set_target` 的值都用这条。
#[tauri::command]
async fn pick_folder(app: AppHandle) -> Result<Option<String>, dto::ApiError> {
    use tauri_plugin_dialog::DialogExt;
    // blocking_pick_folder 在主线程调用有死锁风险（tauri-plugin-dialog 警告），
    // 与 library_choose_root 同样卸到工作线程
    let picked = tauri::async_runtime::spawn_blocking(move || {
        app.dialog()
            .file()
            .blocking_pick_folder()
            .and_then(|p| p.as_path().map(|pp| pp.to_path_buf()))
    })
    .await
    .unwrap_or(None);
    Ok(picked.map(|p| p.to_string_lossy().into_owned()))
}

/// 选目录并**追加**进 `library.roots`（docs/29 批 A：多目录并集单库）。
/// UI 的 startScan 不带 roots，扫描时从这里取。
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
    let Some(state) = app.try_state::<AppState>() else {
        return Err(dto::ApiError::internal("app state 不可用"));
    };
    let chosen = path_str.clone();
    let saved: aiya_data::DbResult<Vec<String>> = state.db().send(move |conn| {
        let mut roots = read_roots(conn);
        // 大小写不敏感去重（Windows 路径）：重复选择同一目录不动列表
        let chosen_lower = chosen.to_lowercase();
        if !roots.iter().any(|r| r.to_lowercase() == chosen_lower) {
            roots.push(chosen);
        }
        setting_set(conn, "library.roots", &json!(roots))?;
        Ok(roots)
    });
    let roots = saved.map_err(|e| dto::ApiError::internal(format!("无法保存曲库目录：{e}")))?;
    Ok(Some(dto::library::ChosenRoot {
        path: path_str,
        roots,
    }))
}

/// 读 settings 里的根列表：`library.roots` 数组优先，旧 `library.root` 单串兜底。
/// 仅在持有连接的调用里用（内部 helper）。
pub(crate) fn read_roots(conn: &rusqlite::Connection) -> Vec<String> {
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

/// 读扫描根列表（UI 的 textarea 数据源）。
#[tauri::command]
fn library_roots_get(
    state: State<'_, AppState>,
) -> Result<dto::library::LibraryRoots, dto::ApiError> {
    let roots = state
        .db()
        .send(|conn| Ok(read_roots(conn)))
        .map_err(|e| dto::ApiError::internal(format!("读取扫描目录失败：{e}")))?;
    Ok(dto::library::LibraryRoots { roots })
}

/// 覆盖写扫描根列表（目录条目列表的保存动作）。
///
/// 参数收 `Vec<String>` 强类型而不是 `Json`：客户端传的就是数组本身
/// （Tauri 按参数名 `roots` 取值），此前用 `Json` 再 `.get("roots")` 是
/// 把"参数对象"和"参数值"搞混了——实测每次都报「缺少 roots 数组」。
/// 归一化在服务端再做一遍：trim、去空行、去引号、大小写不敏感去重。
#[tauri::command]
fn library_roots_set(
    state: State<'_, AppState>,
    roots: Vec<String>,
) -> Result<dto::library::LibraryRoots, dto::ApiError> {
    let mut cleaned: Vec<String> = Vec::new();
    for item in roots {
        let s = item.trim();
        if s.is_empty() {
            continue;
        }
        // 去掉从别处复制路径时可能带上的引号
        let s = s.trim_matches('"').trim_matches('\'').trim().to_string();
        let lower = s.to_lowercase();
        if !cleaned.iter().any(|r| r.to_lowercase() == lower) {
            cleaned.push(s);
        }
    }
    let to_save = cleaned.clone();
    state
        .db()
        .send(move |conn| {
            setting_set(conn, "library.roots", &json!(to_save))?;
            // 目录列表变了：整理输出根若被移除就清掉（回退首根）
            Ok(())
        })
        .map_err(|e| dto::ApiError::internal(format!("保存扫描目录失败：{e}")))?;
    Ok(dto::library::LibraryRoots { roots: cleaned })
}

/// 把用户输入的目录归一化成 `path_lower` 前缀（带尾部分隔符）。
///
/// 扫描器落库时 `path_lower = path.to_lowercase()`（scanner.rs），这里必须
/// 用同一归一化。尾部分隔符必须有：不带的话 `D:\Music` 会误伤 `D:\Music Backup`。
/// 盘根（`D:\`）本身就以分隔符结尾，不动。
fn root_prefix_lower(path: &str) -> String {
    let s = path.trim().to_lowercase();
    if s.ends_with('\\') || s.ends_with('/') || s.is_empty() {
        s
    } else {
        format!("{s}\\")
    }
}

/// 前缀命中数。用 `substr` 前缀比较而不是 LIKE：路径里的 `%`/`_` 得转义，
/// 得不偿失。全表扫不走索引，但这是低频用户操作，5 万行也是毫秒级。
fn count_tracks_under_prefix(conn: &rusqlite::Connection, prefix: &str) -> rusqlite::Result<i64> {
    conn.query_row(
        "SELECT COUNT(*) FROM tracks WHERE substr(path_lower, 1, ?2) = ?1",
        rusqlite::params![prefix, prefix.chars().count() as i64],
        |r| r.get(0),
    )
}

/// 移除这些目录时会波及多少曲目（UI 确认条用，只读不删）。
#[tauri::command]
fn library_roots_count(
    state: State<'_, AppState>,
    paths: Vec<String>,
) -> Result<dto::library::RootsTrackCount, dto::ApiError> {
    let count = state
        .db()
        .send(move |conn| {
            let mut total = 0i64;
            for p in &paths {
                let prefix = root_prefix_lower(p);
                if prefix == "\\" {
                    continue;
                }
                total += count_tracks_under_prefix(conn, &prefix)?;
            }
            Ok(total)
        })
        .map_err(|e| dto::ApiError::internal(format!("统计目录曲目失败：{e}")))?;
    Ok(dto::library::RootsTrackCount { count })
}

/// 从曲库删除这些目录下的曲目（**只删索引，不动磁盘文件**，与
/// `library_remove_tracks` 同一语义；playlist_items / queue_items / history
/// 靠外键级联清理）。逐目录自洽提交：中途失败已删的部分保留，重试幂等
/// （前缀已删空再删是 0 行）。
#[tauri::command]
fn library_roots_prune(
    state: State<'_, AppState>,
    paths: Vec<String>,
) -> Result<dto::library::RemoveTracks, dto::ApiError> {
    let removed = state
        .db()
        .send(move |conn| {
            let mut removed = 0i64;
            for p in &paths {
                let prefix = root_prefix_lower(p);
                if prefix == "\\" {
                    continue;
                }
                removed += conn.execute(
                    "DELETE FROM tracks WHERE substr(path_lower, 1, ?2) = ?1",
                    rusqlite::params![prefix, prefix.chars().count() as i64],
                )? as i64;
            }
            Ok(removed)
        })
        .map_err(|e| dto::ApiError::internal(format!("清除目录曲目失败：{e}")))?;
    Ok(dto::library::RemoveTracks { removed })
}

/// 清空整个曲库记录（**只删索引，不动磁盘文件**）。歌单条目 / 队列 / 播放
/// 历史靠外键级联清空，歌单壳（名称/规则）保留。用户重建曲库的入口：
/// 目录换盘、索引损坏重扫，都比逐目录删除省事。
#[tauri::command]
fn library_clear(state: State<'_, AppState>) -> Result<dto::library::RemoveTracks, dto::ApiError> {
    let removed = state
        .db()
        .send(|conn| {
            let n = conn.execute("DELETE FROM tracks", [])? as i64;
            Ok(n)
        })
        .map_err(|e| dto::ApiError::internal(format!("清空曲库失败：{e}")))?;
    Ok(dto::library::RemoveTracks { removed })
}

#[tauri::command]
fn library_reveal_track(
    state: State<'_, AppState>,
    track_id: String,
) -> Result<dto::library::RevealResult, dto::ApiError> {
    let result: aiya_data::DbResult<Option<String>> = state.db().send(move |conn| {
        conn.query_row("SELECT path FROM tracks WHERE id = ?1", [track_id], |r| {
            r.get(0)
        })
        .map(Some)
        .or_else(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => Ok(None),
            other => Err(aiya_data::DbError::Sql(other)),
        })
    });
    match result {
        Ok(Some(path)) => {
            let revealed = proc::reveal_in_explorer(std::path::Path::new(&path));
            Ok(dto::library::RevealResult {
                revealed,
                reason: if revealed {
                    None
                } else {
                    Some("explorer 启动失败".into())
                },
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
                removed += conn
                    .execute("DELETE FROM tracks WHERE id = ?1", [id])
                    .unwrap_or(0) as i64;
            }
            Ok(dto::library::RemoveTracks { removed })
        })
        .map_err(dto::ApiError::from)
}

/* ---------------- 路径解析：另一个库的路径 → 本库曲目 id ---------------- */

/// 一批文件路径逐条解析成曲目 id（`null` = 曲库里没有这个文件）。
///
/// 谁在用：beets 查询页。它读的是**另一个库**（beets 的 `library.db`），那边
/// 只有文件路径，也没有本应用的 `track_id`——而且刻意不该有，两个库的身份
/// 不该互相泄漏。所以"这个命中对应曲库里哪首歌"必须由宿主回答。
///
/// 返回同序等长：调用方要按行标注（"这行不在曲库，存歌单时会跳过"），
/// 错位就等于标错了歌。空数组进 → 空数组出，不报错。
///
/// 未命中的路径**不造行**：另一个库可能收录了还没扫描进曲库的文件。
#[tauri::command]
fn tracks_resolve_paths(
    state: State<'_, AppState>,
    paths: Vec<String>,
) -> Result<Vec<Option<String>>, dto::ApiError> {
    if paths.is_empty() {
        return Ok(Vec::new());
    }
    state
        .db()
        .send(move |conn| aiya_data::repo::resolve_paths_aligned(conn, &paths))
        .map_err(dto::ApiError::from)
}

/* ---------------- playlist 命令 ---------------- */

/// 摘要查询：行字段 + 3 个聚合子查询（曲目数 / 总时长 / 首个有封面的曲目）。
/// 顺序由 `sort_position`（0002 迁移新增）承载 —— 侧栏拖动排序要能持久化，
/// 靠 created_at 假装顺序的话重启就回到创建顺序。
///
/// 动态歌单（0003）：末两列带出 kind / rule_json，聚合子查询的结果由
/// 歌单摘要查询：末两列带出 kind / rule_json，聚合子查询按 playlist_items
/// 统计（曲目数 / 总时长 / 首个有封面曲目）。动态歌单物化后（2026-10-30）
/// 与手动歌单走完全相同的统计口径。
const PLAYLIST_SELECT: &str = "SELECT p.id, p.name, p.created_at, p.updated_at, p.sort_position, p.kind, p.rule_json,
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
        let summary = r?;
        out.push(summary);
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
    state
        .db()
        .send(|conn| playlist_list_all(conn))
        .map_err(dto::ApiError::from)
}

#[tauri::command]
fn playlist_detail(
    state: State<'_, AppState>,
    id: String,
) -> Result<Option<dto::playlist::PlaylistSummary>, dto::ApiError> {
    state
        .db()
        .send(move |conn| playlist_summary_by_id(conn, &id).map_err(aiya_data::DbError::Sql))
        .map_err(dto::ApiError::from)
}

#[tauri::command]
fn playlist_create(
    state: State<'_, AppState>,
    name: String,
    track_ids: Option<Vec<String>>,
    rule: Option<Json>,
) -> Result<dto::playlist::PlaylistSummary, dto::ApiError> {
    let name = playlist_repo::sanitize_name(&name)
        .map_err(|e| dto::ApiError::invalid_arg(e.to_string()))?;
    // 歌单 id 由名字派生（同名即同一歌单，重复创建不重复建行）
    let pid = ids::playlist_id(&name);
    let now = now_secs();
    // 带 rule = 动态歌单（docs/40 §1）：规则入库前编译校验；track_ids 忽略
    // （动态歌单不物化曲目，UI 也不会同时给）
    if let Some(rule) = rule {
        playlist_write(&state, move |conn| {
            playlist_repo::create_smart(conn, &pid, &name, &rule, now)?;
            Ok(Some(
                playlist_summary_by_id(conn, &pid)
                    .map_err(aiya_data::DbError::Sql)?
                    .ok_or_else(|| aiya_data::DbError::Custom("歌单创建后读不到".into()))?,
            ))
        })
    } else {
        let track_ids = track_ids.unwrap_or_default();
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
}

/// 更新动态歌单的规则（统一模态框"编辑"的保存动作之一）。
#[tauri::command]
fn playlist_set_rule(
    state: State<'_, AppState>,
    id: String,
    rule: Json,
) -> Result<dto::playlist::PlaylistSummary, dto::ApiError> {
    let now = now_secs();
    playlist_write(&state, move |conn| {
        match playlist_repo::set_rule(conn, &id, &rule, now) {
            Ok(true) => {}
            Ok(false) => return Ok(None),
            // 手动歌单设规则是 UI 的 bug（编辑入口只对动态歌单开），明确报错
            Err(e) => return Err(e),
        }
        playlist_summary_by_id(conn, &id).map_err(aiya_data::DbError::Sql)
    })
}

/// 刷新动态歌单：重跑规则，只追加新命中的曲目（已在表内的原位不动，
/// 已不匹配的也不删）。返回本次新增数与刷新后总数。
#[tauri::command]
fn playlist_refresh(
    state: State<'_, AppState>,
    id: String,
) -> Result<dto::playlist::PlaylistRefresh, dto::ApiError> {
    let now = now_secs();
    state
        .db()
        .send(move |conn| {
            let Some(outcome) = playlist_repo::refresh_smart(conn, &id, now)? else {
                return Ok(None);
            };
            Ok(Some(dto::playlist::PlaylistRefresh {
                added: outcome.added,
                total: outcome.total,
            }))
        })
        .map_err(dto::ApiError::from)
        .and_then(|inner| match inner {
            Some(v) => Ok(v),
            None => Err(dto::ApiError::not_found("动态歌单不存在")),
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
) -> Result<dto::playlist::PlaylistAddResult, dto::ApiError> {
    // 2026-10-30 语义：加歌一律首行插入（最后入库的在最前），不再有 append/next 分档
    let now = now_secs();
    playlist_write(&state, move |conn| {
        let Some(outcome) = playlist_repo::add_tracks(conn, &id, &track_ids, now)? else {
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

    let base_dir = file
        .parent()
        .unwrap_or(std::path::Path::new("."))
        .to_path_buf();
    let entries =
        aiya_data::playlist_io::parse_playlist(&text, &base_dir).map_err(|e| match e {
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
    let result: aiya_data::DbResult<dto::playlist::PlaylistImportResult> =
        state.db().send(move |conn| {
            let mut by_path_lower: std::collections::HashMap<String, String> =
                std::collections::HashMap::new();
            {
                let mut stmt = conn.prepare("SELECT path_lower, id FROM tracks")?;
                let rows =
                    stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
                for row in rows {
                    let (k, v) = row?;
                    by_path_lower.insert(k, v);
                }
            }
            let lookup = |target: &std::path::Path| -> Option<String> {
                let s = target.to_string_lossy();
                by_path_lower
                    .get(&s.to_lowercase())
                    .cloned()
                    .or_else(|| {
                        by_path_lower
                            .get(&s.replace('/', "\\").to_lowercase())
                            .cloned()
                    })
                    .or_else(|| {
                        by_path_lower
                            .get(&s.replace('\\', "/").to_lowercase())
                            .cloned()
                    })
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
        Some(other) => {
            return Err(dto::ApiError::invalid_arg(format!(
                "未知的导出风格：{other}"
            )))
        }
    };
    match format.as_deref() {
        None | Some("m3u8") | Some("m3u") => (),
        Some(other) => {
            return Err(dto::ApiError::invalid_arg(format!(
                "未知的导出格式：{other}"
            )))
        }
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
    std::fs::write(&out_path, text)
        .map_err(|e| dto::ApiError::new(dto::ApiErrorCode::Io, format!("写不进导出文件：{e}")))?;
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
    let Some(tx) = app_state
        .smtc_tx
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
    else {
        return;
    };
    let playing = state.status == aiya_player::PlaybackStatus::Playing;
    let _ = tx.send(smtc::SmtcUpdate::Progress {
        playing,
        position_ms: state.position_ms,
    });

    let track_id = state
        .current
        .as_ref()
        .map(|c| c.id.clone())
        .unwrap_or_default();
    let mut last = app_state
        .smtc_track_id
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
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

/// 全量落库所需的数据快照。player 锁内取，锁外写——拆成两半是因为
/// `save_queue_full` 走 DbHandle 单 worker 串行，锁内等它会把命令路径、
/// 事件泵、SMTC 全部拖住。
struct QueueFullPersist {
    p: aiya_data::QueuePersist,
    items: Vec<aiya_data::QueueItem>,
}

/* ---------------- 多态队列：TrackRef ⇄ QueueItem（docs/52 §6） ---------------- */

/// 临时项（`file:` / `stream:`）落库时要留在 `payload` 里的字段。
///
/// 为什么要自己定一个结构而不是直接序列化 `TrackRef`：`TrackRef.path` 是
/// `skip_serializing`（不发给 UI 的安全约束），而队列恢复**恰恰需要**它；
/// 直接序列化会静默丢路径，重启后临时项就变成空路径。
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct TransientPayload {
    path: String,
    title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    artist: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    duration_ms: Option<i64>,
}

/// `TrackRef` → 队列表行。库内曲目只存 id；临时项存 payload。
fn queue_item_from_ref(r: &aiya_player::TrackRef) -> aiya_data::QueueItem {
    if !r.is_transient() {
        return aiya_data::QueueItem::track(r.id.clone());
    }
    let payload = TransientPayload {
        path: r.path.clone(),
        title: r.title.clone(),
        artist: r.artist.clone(),
        duration_ms: r.duration_ms,
    };
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

/// 队列表行 → `TrackRef`（临时行）；库内行返回 `None`（由 DB 查询补全）。
fn ref_from_transient(item: &aiya_data::QueueItem) -> Option<aiya_player::TrackRef> {
    let payload = item.payload.as_deref()?;
    let p: TransientPayload = serde_json::from_str(payload).ok()?;
    Some(match item.kind {
        aiya_data::QueueKind::Stream => aiya_player::TrackRef::transient_stream(&p.path, p.title),
        // 未知 kind（脏数据）也按本地文件恢复：至少路径是真的
        _ => {
            let mut r = aiya_player::TrackRef::transient_file(&p.path, p.title);
            r.artist = p.artist;
            r.duration_ms = p.duration_ms;
            r
        }
    })
}

/// 锁内：取全量落库数据（不做 IO）。
fn queue_full_persist_snapshot(
    state: &AppState,
    actor: &aiya_player::PlayerActor,
) -> QueueFullPersist {
    QueueFullPersist {
        p: queue_persist_from_actor(actor, state.setting_flag("rememberPosition", true)),
        // 从 refs 而不是 ids 构造：临时项的标题/路径只在 `TrackRef` 上，
        // 拿 ids 回查库对它们永远是空（docs/52 §6.3）
        items: actor
            .queue_refs()
            .iter()
            .map(queue_item_from_ref)
            .collect(),
    }
}

/// 锁外：全量落库（queue_items 重写 + 状态行）。切歌/清空后调用。
fn persist_queue_full_snapshot(state: &AppState, snap: QueueFullPersist) {
    let QueueFullPersist { p, items } = snap;
    let result: aiya_data::DbResult<()> = state
        .db()
        .send(move |conn| aiya_data::save_queue_full(conn, &p, &items));
    if let Err(e) = result {
        tracing::warn!(error = %e, "queue full persist failed");
    }
}

/// 只落状态行（position/音量等，5s 节流）。
fn persist_queue_state_handle(state: &AppState, actor: &aiya_player::PlayerActor) {
    let p = queue_persist_from_actor(actor, state.setting_flag("rememberPosition", true));
    let result: aiya_data::DbResult<()> = state
        .db()
        .send(move |conn| aiya_data::save_queue_state(conn, &p));
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
    let mut actor = state
        .player
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let acted = apply_engine_intents(state, &mut actor);
    // 门控 acted：只有真的下发了引擎意图（切歌/停止）才全量落库。
    // 锁内只取数据，写库在锁外（save_queue_full 全量重写走单 worker，
    // player 锁内等它会把命令/泵/SMTC 全拖住）。
    let persist = acted.then(|| queue_full_persist_snapshot(state, &actor));
    let snapshot = actor.state();
    drop(actor);
    if let Some(snap) = persist {
        persist_queue_full_snapshot(state, snap);
    }
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
    state
        .engine
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .send(cmd);
}

/// 从 DB 拿 TrackRef 列表（trackIds 保序）。
///
/// **两类 id 都能解**：
/// 1. 库内曲目 → 查 `tracks`（连音频规格一起带出，播放条的徽章靠它）；
/// 2. 临时项（`file:` / `stream:` 前缀）→ 查 `queue_items.payload`：标题/艺人
///    是拖入那一刻写进去的，`tracks` 表里根本没有它们。
///
/// 第 2 条是**队列面板点播**必须的：面板回传的是队列里的原 id，里面可能混着
/// 临时项。只查 `tracks` 会把它们静默丢掉 —— 于是"点第 3 行播的是第 1 行"，
/// 而且队列里只要有一首直播流，整个点播都会因为空列表直接报"没有可播放的曲目"。
fn track_refs_by_ids(state: &State<'_, AppState>, ids: &[String]) -> Vec<aiya_player::TrackRef> {
    let ids = ids.to_vec();
    state
        .db()
        .send(move |conn| {
            // 临时项先一次读完建映射：条数是个位数，比按 id 逐条回查省事。
            // 读失败按"没有临时项"处理——库内曲目那条路不该被它带塌。
            let transient: std::collections::HashMap<String, aiya_player::TrackRef> =
                aiya_data::queue::load_queue(conn)
                    .ok()
                    .flatten()
                    .map(|(_, items)| {
                        items
                            .iter()
                            .filter(|item| item.kind != aiya_data::QueueKind::Track)
                            .filter_map(|item| {
                                ref_from_transient(item).map(|r| (r.id.clone(), r))
                            })
                            .collect()
                    })
                    .unwrap_or_default();

            // 音频规格列一并带出：PlayerState.current 是播放条唯一数据源，
            // 徽章/规格/Hi-Res 判定都靠 current.audio（contracts Track.audio）
            let mut stmt = conn.prepare(
                "SELECT t.id, t.title, ar.name, al.title, t.duration_ms, t.artwork_id, t.gain_db, t.path,
                        t.codec, t.container, t.sample_rate, t.bits_per_sample, t.bitrate, t.channels,
                        t.artist_id, t.album_id
                 FROM tracks t
                 LEFT JOIN artists ar ON ar.id = t.artist_id
                 LEFT JOIN albums al ON al.id = t.album_id
                 WHERE t.id = ?1",
            )?;
            let mut out = Vec::with_capacity(ids.len());
            for id in &ids {
                if let Some(hit) = transient.get(id) {
                    out.push(hit.clone());
                    continue;
                }
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
                        artist_id: r.get(14)?,
                        album_id: r.get(15)?,
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

/// 队列恢复：多态条目 → `TrackRef` 列表（docs/52 §6.3）。
///
/// 返回 `(tracks, current_id)`：
/// * `tracks` 按条目顺序，**跳过已在库里被淘汰的**（CASCADE 之后正常不会出现，
///   防御手改库）；
/// * `current_id` 是按 `current_index` 取到的那一项的 ref id（临时项的 id 是
///   前缀派生的，不能直接从条目上读）。
///
/// 为什么不能让宿主"拿 id 列表回查"：临时项不在 `tracks` 表里，回查永远空。
fn build_queue_refs(
    state: &State<'_, AppState>,
    items: &[aiya_data::QueueItem],
    current_index: i64,
) -> (Vec<aiya_player::TrackRef>, Option<String>) {
    let track_ids: Vec<String> = items.iter().filter_map(|i| i.track_id.clone()).collect();
    let mut by_id: std::collections::HashMap<String, aiya_player::TrackRef> =
        track_refs_by_ids(state, &track_ids)
            .into_iter()
            .map(|r| (r.id.clone(), r))
            .collect();
    let mut out: Vec<aiya_player::TrackRef> = Vec::with_capacity(items.len());
    let mut current_id = None;
    for (i, item) in items.iter().enumerate() {
        let r = match &item.track_id {
            Some(id) => by_id.remove(id),
            None => ref_from_transient(item),
        };
        let Some(r) = r else {
            continue; // 曲目已被删：与 CASCADE 的语义一致，静默跳过
        };
        if i as i64 == current_index {
            current_id = Some(r.id.clone());
        }
        out.push(r);
    }
    (out, current_id)
}

#[tauri::command]
fn player_state(state: State<'_, AppState>) -> Result<aiya_player::PlayerState, dto::ApiError> {
    let actor = state
        .player
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    Ok(actor.state())
}

#[tauri::command]
fn player_queue(state: State<'_, AppState>) -> Result<aiya_player::QueueSnapshot, dto::ApiError> {
    let actor = state
        .player
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    Ok(actor.queue_snapshot())
}

#[tauri::command]
fn player_play_tracks(
    app: AppHandle,
    state: State<'_, AppState>,
    track_ids: Vec<String>,
    start_index: Option<usize>,
) -> Result<aiya_player::PlayerState, dto::ApiError> {
    let tracks = track_refs_by_ids(&state, &track_ids);
    if tracks.is_empty() {
        return Err(dto::ApiError::not_found("没有可播放的曲目"));
    }
    let start = start_index.unwrap_or(0);
    {
        let mut actor = state
            .player
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        actor.reduce(aiya_player::PlayerCommand::Queue(
            aiya_player::QueueOp::ReplaceAll { tracks, start },
        ));
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
    let tracks = track_refs_by_ids(&state, &ids);
    let start = start_index.unwrap_or(0);
    {
        let mut actor = state
            .player
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        actor.reduce(aiya_player::PlayerCommand::Queue(
            aiya_player::QueueOp::ReplaceAll { tracks, start },
        ));
    }
    Ok(drive_and_emit(&state, &app))
}

/// 播放/暂停切换的内部实现（tauri command 与 SMTC 回调共用）。
fn cmd_toggle(state: &AppState, app: &AppHandle) -> aiya_player::PlayerState {
    let (status_before, source_before) = {
        let actor = state
            .player
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let s = actor.state();
        (s.status, s.source)
    };
    use aiya_player::PlaybackStatus as PS;
    // 直播流（docs/52 §10）：暂停 = 断开、播放 = 重连，**都不走 Pause/Resume**。
    // 这两个意图由 Actor 转成 pending_stop / pending_load，和其余引擎意图同一
    // 出口下发——在这里直接发 Pause 会把流留在连着的状态（暂停 30 秒回来听到的
    // 是 30 秒前），直接发 Resume 则根本没有流可放。
    let is_stream = source_before == aiya_player::AudioSource::Stream;
    if !is_stream {
        match status_before {
            PS::Playing => engine_send(state, aiya_audio::EngineCommand::Pause),
            PS::Paused => engine_send(state, aiya_audio::EngineCommand::Resume),
            _ => {
                // Idle/Error：从当前位置重播（Actor 决定 load）
            }
        }
    }
    {
        let mut actor = state
            .player
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
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
        let mut actor = state
            .player
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        actor.reduce(aiya_player::PlayerCommand::Seek(position_ms));
    }
    drive_and_emit_inner(state, app)
}

/// `drive_and_emit` 的 AppState 版本（SMTC 回调拿不到 `State<'_>`，语义相同）。
///
/// 与前者唯一的差别是入参类型，所以逻辑必须**逐行一致**——两处一旦分叉，
/// 走媒体键触发的那次状态更新就会少落一次库。改这里时记得同步改上面。
fn drive_and_emit_inner(state: &AppState, app: &AppHandle) -> aiya_player::PlayerState {
    let mut actor = state
        .player
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let acted = apply_engine_intents(state, &mut actor);
    // 门控 acted + 锁外落库：与 drive_and_emit 逐行一致（见那里的说明）
    let persist = acted.then(|| queue_full_persist_snapshot(state, &actor));
    let snapshot = actor.state();
    drop(actor);
    if let Some(snap) = persist {
        persist_queue_full_snapshot(state, snap);
    }
    emit_player_state(app, &snapshot);
    snapshot
}

#[tauri::command]
fn player_next(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<aiya_player::PlayerState, dto::ApiError> {
    {
        let mut actor = state
            .player
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
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
        let mut actor = state
            .player
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
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
        let mut actor = state
            .player
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        actor.reduce(aiya_player::PlayerCommand::SetVolume {
            volume,
            muted: false,
        });
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
        let mut actor = state
            .player
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let (v, muted) = actor.volume();
        actor.reduce(aiya_player::PlayerCommand::SetVolume {
            volume: v,
            muted: !muted,
        });
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
        let mut actor = state
            .player
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        actor.reduce(aiya_player::PlayerCommand::SetRate(rate));
        engine_send(
            &state,
            aiya_audio::EngineCommand::SetRate(actor.state().rate as f32),
        );
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
        let mut actor = state
            .player
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
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
                return Err(dto::ApiError::invalid_arg(format!(
                    "未知 repeat 值：{other}"
                )));
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
        let mut actor = state
            .player
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
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
    let tracks = track_refs_by_ids(&state, &track_ids);
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
        let mut actor = state
            .player
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
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
    let tracks = track_refs_by_ids(&state, &[track_id]);
    if tracks.is_empty() {
        return Err(dto::ApiError::not_found("曲目不存在"));
    }
    {
        let mut actor = state
            .player
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        actor.reduce(aiya_player::PlayerCommand::Queue(
            aiya_player::QueueOp::PlayNext { tracks },
        ));
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
        let mut actor = state
            .player
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        actor.reduce(aiya_player::PlayerCommand::Queue(
            aiya_player::QueueOp::Remove {
                indices: vec![index],
            },
        ));
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
        let mut actor = state
            .player
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        actor.reduce(aiya_player::PlayerCommand::Queue(
            aiya_player::QueueOp::Move { from, to },
        ));
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
        let mut actor = state
            .player
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        actor.reduce(aiya_player::PlayerCommand::Queue(
            aiya_player::QueueOp::ReplaceAll {
                tracks: vec![],
                start: 0,
            },
        ));
    }
    Ok(drive_and_emit(&state, &app))
}

/* ---------------- 拖放 / 直播（docs/52 §5、§7.3） ---------------- */

/// 拖入条目的可播形态（分类之后的中间态）。
enum DropCandidate {
    /// 本地音频文件（可能已在曲库里）
    Audio(String),
    /// HLS 清单（本地路径）
    Hls(String),
}

/// 把一批路径展开成"可播条目"：音频直接过、HLS 直接过、老式 m3u 展开后再过。
/// 目录与不支持的格式进 `skipped`（人话说明，UI 直接展示在吐司里）。
fn expand_dropped(paths: &[String], skipped: &mut Vec<String>) -> Vec<DropCandidate> {
    let mut out = Vec::new();
    for path in paths {
        let item = drop::classify(std::path::Path::new(path));
        match item.kind {
            dto::DroppedKind::Audio => out.push(DropCandidate::Audio(item.path)),
            dto::DroppedKind::HlsPlaylist => out.push(DropCandidate::Hls(item.path)),
            dto::DroppedKind::Playlist => {
                // 歌单展开一层（不再递归：歌单套歌单是病，且容易成环）
                match drop::expand_playlist(std::path::Path::new(&item.path)) {
                    Ok(entries) => {
                        for entry in entries {
                            let sub = drop::classify(std::path::Path::new(&entry));
                            match sub.kind {
                                dto::DroppedKind::Audio => {
                                    out.push(DropCandidate::Audio(sub.path))
                                }
                                dto::DroppedKind::HlsPlaylist => {
                                    out.push(DropCandidate::Hls(sub.path))
                                }
                                _ => skipped.push(format!("{}（{}）", sub.label, sub.note)),
                            }
                        }
                    }
                    Err(e) => skipped.push(format!("{}（{e}）", item.label)),
                }
            }
            // 目录**不识别**（用户 2026-10-31 拍板）：一次误拖不该触发递归扫描
            dto::DroppedKind::Directory => {
                skipped.push(format!("{}（文件夹不展开，走设置 → 曲库导入）", item.label))
            }
            dto::DroppedKind::Unsupported => skipped.push(format!("{}（{}）", item.label, item.note)),
        }
    }
    out
}

/// 路径分类（拖放与"打开文件…"共用；分类器唯一实现在 `drop` 模块）。
///
/// UI 用它决定 overlay 上显示什么、跳过项怎么提示——**判定不重复写一份**，
/// 这是这个仓在 `is_http_url` 上吃过亏之后的既定做法。
#[tauri::command]
fn classify_paths(paths: Vec<String>) -> Vec<dto::DroppedItem> {
    paths
        .iter()
        .map(|p| drop::classify(std::path::Path::new(p)))
        .collect()
}

/// 拖入 / 选中一批文件 → **替换队列并播放第一项**（docs/52 §5.3）。
///
/// "识别 → 进入播放列表"就是这句话：不追加、不进曲库、不触发扫描。
/// 已在曲库里的音频文件走库内行（封面/艺人/时长齐全），不在库里的按临时项
/// （`file:` 前缀）进队列——两种混在同一个队列里，next/prev/队列面板零分支。
#[tauri::command]
fn player_open_paths(
    app: AppHandle,
    state: State<'_, AppState>,
    paths: Vec<String>,
) -> Result<dto::OpenPathsReceipt, dto::ApiError> {
    let mut skipped: Vec<String> = Vec::new();
    let candidates = expand_dropped(&paths, &mut skipped);
    if candidates.is_empty() {
        // 全部跳过时说清为什么（比"没有可播放的文件"有用）
        return Err(dto::ApiError::invalid_arg(if skipped.is_empty() {
            "没有可播放的文件".to_string()
        } else {
            format!("没有可播放的文件：{}", skipped.join("；"))
        }));
    }

    // 库内解析一次问完（逐条查库是 N 次往返，曲子多的歌单会明显卡）
    let audio_paths: Vec<String> = candidates
        .iter()
        .filter_map(|c| match c {
            DropCandidate::Audio(p) => Some(p.clone()),
            DropCandidate::Hls(_) => None,
        })
        .collect();
    let resolved: Vec<Option<String>> = if audio_paths.is_empty() {
        Vec::new()
    } else {
        state
            .db()
            .send(move |conn| aiya_data::repo::resolve_paths_aligned(conn, &audio_paths))
            .map_err(dto::ApiError::from)?
    };
    let library_ids: Vec<String> = resolved.iter().flatten().cloned().collect();
    let library = if library_ids.is_empty() {
        Vec::new()
    } else {
        track_refs_by_ids(&state, &library_ids)
    };

    let mut tracks: Vec<aiya_player::TrackRef> = Vec::with_capacity(candidates.len());
    let mut audio_i = 0usize;
    for candidate in candidates {
        match candidate {
            DropCandidate::Audio(path) => {
                let hit = resolved.get(audio_i).and_then(|o| o.as_ref()).cloned();
                audio_i += 1;
                match hit {
                    Some(id) => {
                        if let Some(r) = library.iter().find(|r| r.id == id) {
                            tracks.push(r.clone());
                        }
                    }
                    None => {
                        let title = std::path::Path::new(&path)
                            .file_stem()
                            .map(|s| s.to_string_lossy().into_owned())
                            .unwrap_or_else(|| path.clone());
                        tracks.push(aiya_player::TrackRef::transient_file(&path, title));
                    }
                }
            }
            DropCandidate::Hls(path) => {
                let title = std::path::Path::new(&path)
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_else(|| path.clone());
                tracks.push(aiya_player::TrackRef::transient_stream(&path, title));
            }
        }
    }
    if tracks.is_empty() {
        return Err(dto::ApiError::not_found("队列里没有可播放的条目"));
    }

    let opened = tracks.len();
    {
        let mut actor = state
            .player
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        actor.reduce(aiya_player::PlayerCommand::Queue(
            aiya_player::QueueOp::ReplaceAll { tracks, start: 0 },
        ));
    }
    drive_and_emit(&state, &app);
    Ok(dto::OpenPathsReceipt { opened, skipped })
}

/// 打开一个 HLS 地址（直播页地址框）。标题缺省取 URL 末段，再不行取主机名。
#[tauri::command]
fn live_open(
    app: AppHandle,
    state: State<'_, AppState>,
    url: String,
    title: Option<String>,
) -> Result<aiya_player::PlayerState, dto::ApiError> {
    let url = url.trim().to_string();
    if !aiya_player::is_stream_path(&url) {
        return Err(dto::ApiError::invalid_arg(
            "只支持 http(s) 的 HLS 地址（本地文件请用「打开文件」）",
        ));
    }
    let title = title
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
        .unwrap_or_else(|| default_stream_title(&url));
    let track = aiya_player::TrackRef::transient_stream(&url, title);
    {
        let mut actor = state
            .player
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        actor.reduce(aiya_player::PlayerCommand::Queue(
            aiya_player::QueueOp::ReplaceAll {
                tracks: vec![track],
                start: 0,
            },
        ));
    }
    Ok(drive_and_emit(&state, &app))
}

/// 地址框留空时的标题：URL 末段文件名 → 主机名 → 原样。
fn default_stream_title(url: &str) -> String {
    let stripped = url.split(['?', '#']).next().unwrap_or(url);
    let tail = stripped.trim_end_matches('/').rsplit('/').next().unwrap_or("");
    if !tail.is_empty() && tail.contains('.') && !tail.starts_with("http") {
        // 末段像文件名（`index.m3u8`）：去掉扩展名更像"台名"
        let stem = tail.split('.').next().unwrap_or(tail);
        if !stem.is_empty() {
            return stem.to_string();
        }
    }
    stripped
        .split("://")
        .nth(1)
        .and_then(|rest| rest.split('/').next())
        .filter(|h| !h.is_empty())
        .unwrap_or(url)
        .to_string()
}

/// 选一个本地播放列表文件（直播页「打开文件…」）。
///
/// 与 `pick_folder` 同样的线程约束：`blocking_pick_file` 在主线程有死锁风险，
/// 卸到工作线程。取消返回 `None`（取消是正常操作，不是错误）。
#[tauri::command]
async fn live_pick_playlist(app: AppHandle) -> Result<Option<String>, dto::ApiError> {
    use tauri_plugin_dialog::DialogExt;
    let picked = tauri::async_runtime::spawn_blocking(move || {
        app.dialog()
            .file()
            .add_filter("播放列表", &["m3u8", "m3u"])
            .blocking_pick_file()
            .and_then(|p| p.as_path().map(|pp| pp.to_path_buf()))
    })
    .await
    .unwrap_or(None);
    Ok(picked.map(|p| p.to_string_lossy().into_owned()))
}

/// 播放历史：一曲播完（EngineEvent::Eof，自然结束或用户切走）记一条。
/// seconds_played = 该曲播放时长（v1 记整曲；中途跳过不细分百分比）。
///
/// 具体写法在 `aiya_data::history::record`（history 行与 tracks 的
/// play_count / last_played_at 同事务；此前 play_count 根本没人维护，
/// mostPlayed 列表永远是空的）。
fn record_history(state: &AppState, track_id: &str, seconds_played: u64) {
    // 临时项（拖入的本地文件 / 直播流）不在 `tracks` 表里：history.track_id 是
    // `NOT NULL REFERENCES tracks(id)`，写进去整笔事务会外键失败（docs/52 §6.1）。
    // 历史是"曲目"语义，这类条目不记账——不需要改表，一个闸门就够。
    if track_id.starts_with(aiya_player::FILE_ID_PREFIX)
        || track_id.starts_with(aiya_player::STREAM_ID_PREFIX)
    {
        return;
    }
    let track_id = track_id.to_string();
    let played_at = now_secs();
    let result: aiya_data::DbResult<()> = state
        .db()
        .send(move |conn| aiya_data::history::record(conn, &track_id, played_at, seconds_played));
    if let Err(e) = result {
        tracing::warn!(error = %e, "history 写入失败");
    }
    // 每次写历史顺手裁剪：写是 history 唯一的增长点，增长点带清理就不会膨胀
    state.prune_history();
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
                .query_row("SELECT path FROM tracks WHERE id = ?1", [&track_id], |r| {
                    r.get(0)
                })
                .ok())
        })
        .map_err(dto::ApiError::from)?;
    let Some(path) = path else {
        return Ok(dto::lyrics::Lyrics::empty());
    };
    Ok(
        match aiya_data::lyrics::read_lyrics(std::path::Path::new(&path)) {
            Some(parsed) => parsed.into(),
            None => dto::lyrics::Lyrics::empty(),
        },
    )
}

#[tauri::command]
fn history_stats(state: State<'_, AppState>) -> Result<dto::history::HistoryStats, dto::ApiError> {
    // 包一层闭包：`send` 给的是 `&mut Connection`，而 SQL 主体只要 `&Connection`
    state
        .db()
        .send(|conn| history_stats_from(conn))
        .map_err(dto::ApiError::from)
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
        .query_row("SELECT MIN(played_at) FROM history", [], |r| {
            r.get::<_, Option<i64>>(0)
        })
        .unwrap_or(None);
    let last = conn
        .query_row("SELECT MAX(played_at) FROM history", [], |r| {
            r.get::<_, Option<i64>>(0)
        })
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
fn history_clear(
    state: State<'_, AppState>,
) -> Result<dto::history::HistoryCleared, dto::ApiError> {
    // 清历史必须与曲目统计列归零同事务（见 aiya_data::history::clear）
    state
        .db()
        .send(aiya_data::history::clear)
        .map_err(dto::ApiError::from)?;
    Ok(dto::history::HistoryCleared { cleared: true })
}

/* ---------------- aiya-artwork 协议 ---------------- */

/// 处理 `http://aiya-artwork.localhost/artwork/<id>?size=N`。
/// 流程：查 artworks 表 → 读原图 →（需要且可缩放时）生成缩略图缓存 → 响应。
/// 失败一律 404（破图不该让前端 fetch 走重试）。
/// **在阻塞池线程执行**（注册点用 asynchronous 协议 + spawn_blocking 转交，
/// docs/38 P1）——本函数内部不做任何主线程假设；无 AppHandle 之外的
/// 线程局部依赖。
fn artwork_protocol(
    app: &AppHandle,
    request: tauri::http::Request<Vec<u8>>,
) -> tauri::http::Response<Cow<'static, [u8]>> {
    use tauri::http::header::{ACCESS_CONTROL_ALLOW_ORIGIN, CACHE_CONTROL, CONTENT_TYPE};

    fn not_found() -> tauri::http::Response<Cow<'static, [u8]>> {
        tauri::http::Response::builder()
            .status(404)
            .body(Cow::Borrowed(&b""[..]))
            .unwrap()
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
            q.split('&')
                .find_map(|kv| kv.strip_prefix("size="))?
                .parse()
                .ok()
        })
        .unwrap_or(0);

    let state = app.state::<AppState>();

    // 1) 查记录（DB 线程）
    let found = state
        .db()
        .send(move |conn| aiya_data::find_artwork(conn, artwork_id.as_str()));
    let Some((cache_key, mime)) = found.ok().flatten() else {
        return not_found();
    };

    // 2) 读原图（cache_key 只含 hash+ext，但仍然 basename 防御）
    let original = state.artwork_dir.join("originals").join(
        std::path::Path::new(&cache_key)
            .file_name()
            .unwrap_or_default(),
    );
    let Ok(bytes) = std::fs::read(&original) else {
        return not_found();
    };

    // 3) 缩略图：size 指定且 ≤512 才缩（防滥用大尺寸请求），否则原图直出
    let mut thumb_used = false;
    let data: Cow<'static, [u8]> = if (1..=512).contains(&size) && mime.starts_with("image/") {
        let thumb = state.artwork_dir.join("thumbs").join(format!(
            "{}_{}.jpg",
            cache_key.split('.').next().unwrap_or("x"),
            size
        ));
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
    thumb.write_to(&mut out, image::ImageFormat::Jpeg).ok()?;
    Some(out.into_inner())
}

/* ---------------- 装配 ---------------- */

/// 便携版环境准备（`main` 在建任何窗口之前调用，见 `paths::prepare_portable_env`）。
pub use paths::prepare_portable_env;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // 应用身份必须在**任何 UI 出现之前**设置（微软对 AppUserModelID 的硬要求）：
    // 窗口一旦建出来，壳层就已经按"没有身份"缓存过这条会话了，之后再设不生效。
    // 放在这里而不是 setup 里，是因为 config 里的窗口在 Builder 构建时就创建了。
    app_id::install();
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
            // 切回窗口 = "用户刚在资源管理器/mp3tag 改完回来"（docs/33 §7.2）。
            // 失焦时定时器暂停，聚焦时立刻补一次 —— 两者共用同一个标志。
            if let tauri::WindowEvent::Focused(focused) = event {
                refresh::set_focused(window.app_handle(), *focused);
                return;
            }
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
        // P1（docs/38）：必须是 asynchronous 版本——同步 handler 在主线程执行，
        // 未命中缩略图缓存时 DB 往返 + 读原图 + Lanczos3 缩放（30–80ms/张）
        // 全阻塞主线程，首次滚动长列表每张新封面卡一下。改为：handler 只在
        // 主线程做"收包转交"，整个 artwork_protocol 在阻塞池跑完再经
        // responder 回包——主线程零阻塞。请求对象本来就只是 URI 解析，开销可忽略。
        .register_asynchronous_uri_scheme_protocol(
            aiya_data::ARTWORK_SCHEME,
            |ctx, request, responder| {
                let app = ctx.app_handle().clone();
                tauri::async_runtime::spawn_blocking(move || {
                    responder.respond(artwork_protocol(&app, request));
                });
            },
        )
        .setup(|app| {
            // 第一件事：日志。此后的任何失败（建库、起 sidecar）才有据可查，
            // 否则 release 里所有 tracing 输出进虚空。
            logging::init(app.handle());
            // 身份是 run() 最前面设的（要早于任何 UI），结果等日志能落盘了再打。
            app_id::log_outcome();

            let db_path = default_db_path(app.handle());
            // 库文件损坏已在 aiya_data::open 内部 quarantine + 重建；走到这个
            // Err 的只剩真 IO 故障（磁盘满/权限）。必须弹原生错误框：release
            // 无控制台、此刻也没有窗口，不弹 = 用户双击后"什么都不发生"。
            let db = aiya_data::open(&db_path).map_err(|e| -> Box<dyn std::error::Error> {
                tracing::error!(error = %e, db = %db_path.display(), "数据库打开失败");
                fatal_dialog(&format!(
                    "AIYA Music 无法打开音乐库数据库：\n{e}\n\n位置：{}\n\n请检查磁盘空间与权限后重试；若反复出现，可把该文件移走后重新启动（将重建空库）。",
                    db_path.display()
                ));
                e.into()
            })?;
            let artwork_dir = default_artwork_dir(app.handle());
            let _ = std::fs::create_dir_all(artwork_dir.join("originals"));
            let _ = std::fs::create_dir_all(artwork_dir.join("thumbs"));
            // launch 内部已做首连降级（失败不再 Err），保留 map_err 作为兜底。
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
            // EQ 偏好（协议 v6）同样在 manage 前取：db 马上要被 move 进 AppState
            let eq0: aiya_audio::EqParams = db
                .send(|conn| Ok(setting_get(conn, "eq")))
                .ok()
                .flatten()
                .and_then(|v| serde_json::from_value::<dto::EqSettings>(v).ok())
                .map(|s| aiya_audio::EqParams {
                    bands: std::array::from_fn(|i| s.bands.get(i).copied().unwrap_or(0.0)),
                    bass_db: s.bass_db,
                    treble_db: s.treble_db,
                    pregain: s.pregain,
                    enabled: s.enabled,
                })
                .unwrap_or_default();
            let scanning = Arc::new(AtomicBool::new(false));
            let scan_cancel = Arc::new(AtomicBool::new(false));
            let refresh_params = Arc::new(refresh::RefreshParams {
                db_path: db_path.clone(),
                artwork_dir: artwork_dir.clone(),
                scanning: Arc::clone(&scanning),
                scan_cancel: Arc::clone(&scan_cancel),
            });
            app.manage(AppState {
                db: Mutex::new(db),
                db_path,
                artwork_dir,
                player: Mutex::new(aiya_player::PlayerActor::new()),
                engine: Mutex::new(engine),
                scanning,
                scan_cancel,
                refresh: Arc::clone(&refresh_params),
                last_refresh: Mutex::new(None),
                refresh_wake: Mutex::new(None),
                last_persist_ms: std::sync::atomic::AtomicU64::new(0),
                smtc_tx: Mutex::new(None),
                smtc_track_id: Mutex::new(String::new()),
                smtc_active: AtomicBool::new(false),
                sleep_prevented: AtomicBool::new(false),
                tray_active: AtomicBool::new(false),
                audio_state: Mutex::new(AudioStateSnapshot::default()),
                timings: Mutex::new(VecDeque::with_capacity(TIMING_LIMIT)),
                settings: Mutex::new(settings0),
                beets_install_child: Mutex::new(None),
                enrich_store: std::sync::OnceLock::new(),
            });

            // 一次性迁移：把 settings 表里的 `beets.*` 旧键搬到模块自己的
            // `settings.json`（docs/47）。搬完就删键——留着的话它还躺在应用的
            // "设置"里，等于解耦只做了一半。
            //
            // 只在 `settings.json` 不存在时执行（`adopt_legacy_settings` 判的）：
            // 用户已经在新文件里改过配置之后再拿旧值覆盖他，就是"每次启动
            // 回滚一次设置"。
            {
                let st = app.state::<AppState>();
                let legacy = st.db().send(|conn| {
                    Ok(beets::Settings {
                        tool_path: setting_str_value(conn, "beets.toolPath"),
                        tool_args: setting_str_value(conn, "beets.toolArgs"),
                        target_root: setting_str_value(conn, "beets.targetRoot"),
                        inbox_path: setting_str_value(conn, "beets.inboxPath"),
                        inbox_imported: setting_get(conn, "beets.inboxImported")
                            .and_then(|v| serde_json::from_value(v).ok())
                            .unwrap_or_default(),
                        // 旧表里没有这个键：迁移过来就是"没选过"，
                        // 工具侧会回落到 plugins.json 的 profiles.workbench
                        plugins: None,
                    })
                });
                if let Ok(legacy) = legacy {
                    if beets::adopt_legacy_settings(app.handle(), legacy) {
                        let _ = st.db().send(|conn| {
                            for key in [
                                "beets.toolPath",
                                "beets.toolArgs",
                                "beets.targetRoot",
                                "beets.inboxPath",
                                "beets.inboxImported",
                            ] {
                                setting_set(conn, key, &Value::Null)
                                    .map_err(aiya_data::DbError::Sql)?;
                            }
                            Ok(())
                        });
                    }
                }
            }

            // 曲库刷新管线（docs/33 §7）：定时 30 s + 窗口聚焦。
            // 起在这里而不是首次用到时：定时器要的是"一直在跑"，用户不该为了
            // "外部改完标签能自动发现"先去点一次什么按钮。
            refresh::start_scheduler(app.handle().clone(), refresh_params);

            // 输出配置先下发：引擎未装载时只记录，首曲 Load 就用上。
            // 设备拒绝独占会回落共享、缓冲太深会自愈降级，两者都通过 AudioMode
            // 事件把原因带回 UI（"设置没生效"必须可解释）。
            {
                let st = app.state::<AppState>();
                engine_send(&st, aiya_audio::EngineCommand::SetOutputConfig(st.setting_output()));
                // EQ 偏好同理（协议 v6）：设置项，与是否装载无关
                engine_send(&st, aiya_audio::EngineCommand::SetEq(eq0));
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
                // 启动即按保留周期裁剪播放历史：覆盖"上次把周期调小后
                // 一直没播放"的场景（写历史 / 设置变更各自也会触发）。
                state.prune_history();
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
                    Option<(aiya_data::QueuePersist, Vec<aiya_data::QueueItem>)>,
                    aiya_data::DbError,
                > = state.db().send(|conn| aiya_data::load_queue(conn));
                if let Ok(Some((p, items))) = loaded {
                    if !items.is_empty() {
                        // 多态恢复（docs/52 §6.3）：库内曲目查库、临时项读 payload
                        let (tracks, current_id) =
                            build_queue_refs(&state, &items, p.current_index);
                        if !tracks.is_empty() {
                            let repeat = match p.repeat_mode.as_str() {
                                "all" => aiya_player::RepeatMode::All,
                                "one" => aiya_player::RepeatMode::One,
                                _ => aiya_player::RepeatMode::Off,
                            };
                            // 当前项是直播流：起点强制 0（直播没有"上次听到哪"）
                            let is_stream_current = current_id
                                .as_deref()
                                .is_some_and(|id| id.starts_with(aiya_player::STREAM_ID_PREFIX));
                            let start_ms = if remember_position && !is_stream_current {
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
                    let (effects, persist) = {
                        let mut actor = engine
                            .player
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner);
                        let mut sent: Vec<aiya_audio::EngineCommand> = Vec::new();
                        // 门控用 acted（本批是否真下发了引擎意图）：Position 节流
                        // 也置 EmitState，但队列结构没变 —— 此前用 EmitState 门控，
                        // 播放中每 500ms 全量 DELETE+重写一次 queue_items。
                        let (effects, acted) =
                            core.ingest(events, &mut actor, now_ms_u64(), &mut |cmd| {
                                sent.push(cmd)
                            });
                        // pending_load / pending_stop 的实际下发仍在锁内做完：
                        // 引擎通道是有界的，正常消费下不会阻塞
                        for cmd in sent {
                            engine_send(&engine, cmd);
                        }
                        // 落库数据锁内取、锁外写（save_queue_full 走单 worker，
                        // 不能在 player 锁里等它）
                        let persist =
                            acted.then(|| queue_full_persist_snapshot(&engine, &actor));
                        (effects, persist)
                    };
                    if let Some(snap) = persist {
                        persist_queue_full_snapshot(&engine, snap);
                    }
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
            player_set_eq,
            player_get_eq,
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
            beets_config_get,
            beets_set_inbox,
            beets_set_target,
            beets_open_inbox,
            beets_inbox_imported_add,
            refresh_state,
            beets_install,
            beets_adopt,
            beets_update,
            beets_duplicates,
            beets_move,
            beets_query,
            beets_fields,
            beets_plugin_catalog,
            beets_plugin_table,
            beets_python_probe,
            beets_uninstall,
            beets_plugin_install,
            enrichment_worklist,
            beets_enrich_start,
            beets_index_import,
            enrichment_query,
            enrichment_gc,
            library_genres,
            beets_reveal,
            library_list,
            library_albums,
            library_artists,
            library_genres,
            library_tracks,
            library_stats,
            library_scan,
            library_scan_cancel,
            library_choose_root,
            pick_folder,
            library_roots_get,
            library_roots_set,
            library_roots_count,
            library_roots_prune,
            library_clear,
            library_reveal_track,
            library_ids_for_selection,
            library_remove_tracks,
            tracks_resolve_paths,
            playlist_list,
            playlist_detail,
            playlist_create,
            playlist_rename,
            playlist_set_rule,
            playlist_refresh,
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
            classify_paths,
            player_open_paths,
            live_open,
            live_pick_playlist,
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
                    // beets 安装子进程（A 轨道）：安装线程可能正卡在 pip 阶段，
                    // 退出时收尸 —— 槽被 take 走后安装线程自行报"被退出中断"。
                    if let Some(mut c) = state
                        .beets_install_child
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .take()
                    {
                        let _ = c.kill();
                        let _ = c.wait();
                    }
                    state
                        .engine
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .shutdown();
                }
            }
        });
}
