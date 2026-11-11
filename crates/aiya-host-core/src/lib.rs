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

/// 路径归一（**仅用于比较**，展示/落库保留用户原始输入）。
///
/// Windows：分隔符统一成 `\`、去尾 `\`、小写（文件系统大小写不敏感、两种
/// 分隔符等价；与曲库 `path_lower` / `aiya_data::repo::path_key` 口径一致）。
/// unix：分隔符不动、去尾 `/`、小写**同样做**——曲库侧 `path_key` 在 unix
/// 也是纯小写（写入与读取同口径），这里必须跟它一致，否则 beets 回报路径与
/// 库内记录对不上。代价：大小写敏感文件系统上仅大小写不同的两个真实路径
/// 会被视为同一位置——与 path_key 同一取舍，比"大小写漂移导致全部对不上"
/// 便宜得多。此前 Windows 分支被无条件下发到 Linux（2026-11-09 审查）：
/// 等值/子路径判断碰巧自洽（双方同变），但 `/music/` 会归一成 `\music\`
/// 形态的前缀判断，纯属 Windows 形态漏到了另一侧。
pub fn canon_path(p: &str) -> String {
    if cfg!(windows) {
        p.trim()
            .replace('/', "\\")
            .trim_end_matches('\\')
            .to_lowercase()
    } else {
        let t = p.trim();
        let stripped = t.trim_end_matches('/');
        // 唯一根 "/" 去尾会变空串（空串在调用方一律判"无效"），保留原样
        let kept = if stripped.is_empty() && !t.is_empty() {
            t
        } else {
            stripped
        };
        kept.to_lowercase()
    }
}

/// canon 形态下的分隔符。子路径前缀判断必须带上它：裸 `starts_with` 会把
/// `/music2` 当成 `/music` 的子目录（经典前缀陷阱，单测 `sibling_directory_*`
/// 钉着）。
fn canon_sep() -> &'static str {
    if cfg!(windows) {
        "\\"
    } else {
        "/"
    }
}

/// 子路径前缀判断的边界：canon 形态加尾分隔符；形态本身以分隔符结尾（裸根
/// `/`、盘符根 `D:\`）则原样——裸根就是一切绝对路径的祖先边界，补分隔符
/// 反而判丢（`/` + `/` = `//`，没有任何路径以它开头）。
fn canon_boundary(p: &str) -> String {
    if p.ends_with(canon_sep()) {
        p.to_string()
    } else {
        format!("{p}{}", canon_sep())
    }
}

/// 路径是否落在根集合内（相等或其子路径）。
///
/// 整理落点用的就是这条：落点在曲库之外 → 整理完的文件扫描器看不见，用户会
/// 看到"整理成功了，曲库里没有"。
pub fn path_within_roots(path: &str, roots: &[String]) -> bool {
    let c = canon_path(path);
    if c.is_empty() {
        return false;
    }
    roots.iter().any(|r| {
        let rc = canon_path(r);
        !rc.is_empty() && (c == rc || c.starts_with(&canon_boundary(&rc)))
    })
}

/// 两个目录是否重叠（相等/互为子路径，双向）：inbox 与扫描根互斥的安全闸。
///
/// 为什么"父目录"也算：`D:\` 与 `D:\Music` 不相等也不在对方里面（反方向），
/// 但把 `D:\` 当作待整理目录，预览会递归扫到整个曲库——同一批文件被再导一遍。
/// 三个方向覆盖完才叫"不重叠"。
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
        c == rc || c.starts_with(&canon_boundary(&rc)) || rc.starts_with(&canon_boundary(&c))
    })
}

#[cfg(test)]
mod canon_path_tests {
    use super::{canon_path, path_overlaps_roots, path_within_roots};

    /// inbox 与扫描根重叠校验是入库模型的安全边界（inbox 混进扫描根 =
    /// 重复曲目），正反例都要钉死。测试按平台各钉一份：归一语义本就分平台。
    #[cfg(windows)]
    fn roots() -> Vec<String> {
        vec!["D:\\Music".into(), "E:\\Audio\\FLAC".into()]
    }
    #[cfg(unix)]
    fn roots() -> Vec<String> {
        vec!["/music".into(), "/srv/audio/flac".into()]
    }

    #[test]
    #[cfg(windows)]
    fn canon_unifies_separators_case_and_trailing_slash() {
        assert_eq!(canon_path("D:/Music/"), canon_path("d:\\music"));
        assert_eq!(canon_path(" D:\\Music "), "d:\\music");
    }

    #[test]
    #[cfg(unix)]
    fn canon_keeps_separators_folds_case_and_trailing_slash() {
        assert_eq!(canon_path("/Music/"), "/music");
        assert_eq!(canon_path(" /Music "), "/music");
        // 唯一根 "/" 不被去尾成空串（空串 = 无效）
        assert_eq!(canon_path("/"), "/");
    }

    #[test]
    #[cfg(windows)]
    fn inbox_equal_to_root_conflicts() {
        assert!(path_overlaps_roots("D:/Music", &roots()));
    }

    #[test]
    #[cfg(unix)]
    fn inbox_equal_to_root_conflicts() {
        assert!(path_overlaps_roots("/music", &roots()));
    }

    #[test]
    #[cfg(windows)]
    fn inbox_inside_root_conflicts() {
        assert!(path_overlaps_roots("D:\\Music\\New\\Albums", &roots()));
        assert!(path_overlaps_roots("d:/music/new", &roots()));
    }

    #[test]
    #[cfg(unix)]
    fn inbox_inside_root_conflicts() {
        assert!(path_overlaps_roots("/music/new/albums", &roots()));
    }

    #[test]
    #[cfg(windows)]
    fn sibling_directory_is_allowed() {
        // 经典前缀陷阱：D:\Music Backup 不能被 D:\Music 误伤
        assert!(!path_overlaps_roots("D:\\Music Backup", &roots()));
        assert!(!path_overlaps_roots("E:\\Downloads", &roots()));
    }

    #[test]
    #[cfg(unix)]
    fn sibling_directory_is_allowed() {
        // /music2 不能被 /music 误伤（裸 starts_with 的经典前缀陷阱）
        assert!(!path_overlaps_roots("/music2", &roots()));
        assert!(!path_overlaps_roots("/downloads", &roots()));
    }

    #[test]
    #[cfg(windows)]
    fn parent_of_root_conflicts() {
        // 反方向的重叠：把 D:\ 当整理源，预览会递归扫到整个曲库
        assert!(path_overlaps_roots("D:\\", &roots()));
        // 前缀但不在分隔符边界上：D:\Mu 不是 D:\Music 的祖先目录
        assert!(!path_overlaps_roots("D:\\Mu", &roots()));
    }

    #[test]
    #[cfg(unix)]
    fn parent_of_root_conflicts() {
        // 反方向的重叠：把 "/" 当整理源，预览会递归扫到整个曲库
        assert!(path_overlaps_roots("/", &roots()));
        // 前缀但不在分隔符边界上：/mus 不是 /music 的祖先目录
        assert!(!path_overlaps_roots("/mus", &roots()));
    }

    #[test]
    #[cfg(windows)]
    fn empty_path_is_not_overlap() {
        // 空串 = 未设置：两壳都在上游把空串滤成 None（清除语义），
        // 这里只保证"空串不产生伪重叠"
        assert!(!path_overlaps_roots("  ", &roots()));
    }

    #[test]
    #[cfg(unix)]
    fn empty_path_is_not_overlap() {
        assert!(!path_overlaps_roots("  ", &roots()));
    }

    /* 整理落点：在内即可，不必相等 */

    #[test]
    #[cfg(windows)]
    fn target_accepts_root_and_subdirectory() {
        assert!(path_within_roots("D:\\Music", &roots()));
        assert!(path_within_roots("D:\\Music\\整理", &roots()));
        assert!(path_within_roots("d:/music/a/b", &roots()));
    }

    #[test]
    #[cfg(unix)]
    fn target_accepts_root_and_subdirectory() {
        assert!(path_within_roots("/music", &roots()));
        assert!(path_within_roots("/music/整理", &roots()));
    }

    #[test]
    #[cfg(windows)]
    fn target_rejects_outside_and_fake_sibling() {
        // 落点在曲库外 → 整理完扫描器看不见
        assert!(!path_within_roots("E:\\Sorted", &roots()));
        // 假兄弟：裸 starts_with 会把它当成 D:\Music 的子目录
        assert!(!path_within_roots("D:\\Music2", &roots()));
        // 父目录不是"之内"
        assert!(!path_within_roots("D:\\", &roots()));
        assert!(!path_within_roots("  ", &roots()));
    }

    #[test]
    #[cfg(unix)]
    fn target_rejects_outside_and_fake_sibling() {
        assert!(!path_within_roots("/srv/sorted", &roots()));
        assert!(!path_within_roots("/music2", &roots()));
        assert!(!path_within_roots("/", &roots()));
        assert!(!path_within_roots("  ", &roots()));
    }
}
