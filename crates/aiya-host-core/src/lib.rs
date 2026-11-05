// 宿主编排共享层：桌面壳与 server 壳的 beets/enrichment 领域编排，经
// [`BeetsHost`] 注入各自的环境（事件出口、数据根、应用库单写者、安装登记）。
// 领域逻辑只有一份，壳层差异全部走 trait（编译期双壳的另一面）。

pub mod beets;
pub mod enrichment;
pub mod proc;

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Mutex;

use rusqlite::Connection;

/// 宿主环境抽象。桌面壳 = Tauri emit + paths；server 壳 = SSE 广播 + AIYA_DATA_ROOT。
///
/// `emit` 的形状与 `aiya_beets_client::EventSink` 一致：`Arc<dyn BeetsHost>`
/// 直接当事件出口用（`impl EventSink for dyn BeetsHost`）。
pub trait BeetsHost: Send + Sync + 'static {
    /// 事件出口：通道名是常量子集（beets:import / beets:install / …）。
    fn emit(&self, channel: &'static str, payload: serde_json::Value);
    /// 数据根（C11：桌面 = paths::data_root；server = AIYA_DATA_ROOT）。
    fn data_root(&self) -> PathBuf;
    /// 应用库扫描根（read_roots）。`None` = 库不可用（与桌面 try_state 的松语义一致）。
    fn library_roots(&self) -> Option<Vec<String>>;
    /// 库内在线文件快照（索引增强的 arena 数据源）。
    fn online_files(&self) -> Option<Vec<aiya_data::repo::FileFact>>;
    /// 路径（path_lower）→ 曲目 id 映射（索引增强落库前的对账）。
    fn track_ids_by_paths(
        &self,
        paths: &[String],
    ) -> Option<std::collections::HashMap<String, String>>;
    /// 应用设置 KV 的字符串项（language 等）。
    fn setting_str(&self, key: &str) -> Option<String>;
    /// 安装/卸载登记表：子进程收尸槽 + 占位互斥。
    fn install_registry(&self) -> &InstallRegistry;
    /// 安装互斥占位：false→true 成功返回 true；已有任务在跑返回 false。
    fn try_set_installing(&self) -> bool;
    /// beets 索引增强结论库（懒打开，进程内单例）。
    fn enrich_store(&self) -> Result<std::sync::Arc<aiya_beets_store::Store>, String>;
    /// 应用曲库刷新（apply/adopt/move 完成后的自闭环）：桌面走 refresh 管线
    /// （Trigger::Action），server 起后台扫描线程。
    fn kick_library_refresh(&self);
}

impl aiya_beets_client::EventSink for dyn BeetsHost {
    fn emit(&self, channel: &'static str, payload: serde_json::Value) {
        BeetsHost::emit(self, channel, payload)
    }
}

/// trait 对象之间不做跨 trait 强转：Arc<dyn BeetsHost> → Arc<dyn EventSink
/// 经此适配（仅转发）。
struct BeetsEventSink(std::sync::Arc<dyn BeetsHost>);

impl aiya_beets_client::EventSink for BeetsEventSink {
    fn emit(&self, channel: &'static str, payload: serde_json::Value) {
        self.0.emit(channel, payload)
    }
}

/// Arc<dyn BeetsHost> → Arc<dyn EventSink> 的显式适配入口。
pub fn event_sink(
    host: std::sync::Arc<dyn BeetsHost>,
) -> std::sync::Arc<dyn aiya_beets_client::EventSink> {
    std::sync::Arc::new(BeetsEventSink(host))
}

/// 安装登记表：`child` 槽给退出收尸（桌面 RunEvent::Exit / server 进程退出），
/// `installing` 占位防两条安装线程并发互拆。
pub struct InstallRegistry {
    pub child: Mutex<Option<std::process::Child>>,
    pub installing: AtomicBool,
}

impl InstallRegistry {
    pub fn new() -> Self {
        Self {
            child: Mutex::new(None),
            installing: AtomicBool::new(false),
        }
    }
}

impl Default for InstallRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// 占位守卫：返回/panic 时复位 `installing`。
pub struct InstallGuard<'a>(&'a AtomicBool);

impl<'a> InstallGuard<'a> {
    pub fn new(flag: &'a AtomicBool) -> Self {
        Self(flag)
    }
}

impl Drop for InstallGuard<'_> {
    fn drop(&mut self) {
        self.0.store(false, std::sync::atomic::Ordering::SeqCst);
    }
}

/* ---- 应用设置 KV 读写（两壳同 SQL） ---- */

pub fn setting_get(conn: &Connection, key: &str) -> Option<serde_json::Value> {
    conn.query_row(
        "SELECT value_json FROM settings WHERE key = ?1",
        [key],
        |r| r.get::<_, String>(0),
    )
    .ok()
    .and_then(|s| serde_json::from_str(&s).ok())
}

pub fn setting_set(
    conn: &Connection,
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

pub fn read_roots(conn: &Connection) -> Vec<String> {
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

pub fn read_paused_roots(conn: &Connection) -> Vec<String> {
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

/// Windows 侧路径规范化：斜杠统一、去尾反斜杠、小写（与 `path_lower` 口径一致）。
pub fn canon_path(p: &str) -> String {
    p.trim()
        .replace('/', "\\")
        .trim_end_matches('\\')
        .to_lowercase()
}

/// 路径是否落在根集合内（相等或其子路径）。
pub fn path_within_roots(path: &str, roots: &[String]) -> bool {
    let c = canon_path(path);
    if c.is_empty() {
        return false;
    }
    roots.iter().any(|r| {
        let rc = canon_path(r);
        !rc.is_empty() && (c == rc || c.starts_with(&format!("{rc}\\")))
    })
}

/// 两个目录是否重叠（相等/互为子路径，双向）：inbox 与扫描根互斥的安全闸。
pub fn path_overlaps_roots(path: &str, roots: &[String]) -> bool {
    let c = canon_path(path);
    if c.is_empty() {
        return false;
    }
    roots.iter().any(|r| {
        let rc = canon_path(r);
        if rc.is_empty() {
            return false;
        }
        c == rc || c.starts_with(&format!("{rc}\\")) || rc.starts_with(&format!("{c}\\"))
    })
}
