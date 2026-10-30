//! 系统集成（P4）：SMTC + 媒体键 + 防休眠。
//!
//! - SMTC 走 souvlaki（Windows 官方 SystemMediaTransportControls 封装）：
//!   系统媒体浮层 / 锁屏 / 蓝牙耳机可见可控，键盘媒体键也经它路由。
//!   COM 对象非 Send，MediaControls 全生命周期驻留专用线程；
//!   事件泵推 `SmtcUpdate` 进来，按钮事件经回调驱动播放器。
//! - UI 侧 `media-session.ts` 依赖 WebView 内 media 元素，引擎进 Rust 后
//!   不会产生系统条目，与本模块无冲突（WebView 侧留作浏览器预览用）。
//! - 防休眠：`SetThreadExecutionState(ES_CONTINUOUS|ES_SYSTEM_REQUIRED)`，
//!   播放中防系统睡眠（不锁显示器策略，v1 不挡屏幕休眠）。
//!   该状态是 per-thread 的，由 lib.rs 里专用的 prevent-sleep std::thread
//!   维护（P1：tokio 泵任务会跨 worker 迁移，per-thread 状态不可靠）。

use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::time::Duration;

use tauri::{AppHandle, Manager};

/// 推给 SMTC 线程的更新。Track 只在换曲时发；Progress 跟事件泵节流（500ms）。
pub enum SmtcUpdate {
    Track {
        title: String,
        artist: Option<String>,
        album: Option<String>,
        duration_ms: Option<u64>,
        /// 本地文件绝对路径（souvlaki Windows 用 file:// 加载缩略图）
        cover_path: Option<PathBuf>,
    },
    Progress {
        playing: bool,
        position_ms: u64,
    },
}

/// 启动 SMTC 专用线程。失败（COM 初始化等）返回 None，播放器不受影响。
pub fn spawn(app: AppHandle, hwnd: isize) -> Option<Sender<SmtcUpdate>> {
    let (tx, rx) = std::sync::mpsc::channel::<SmtcUpdate>();

    // P2：MediaControls 构造 + attach 全部移进专用线程——此前构造在主线程、
    // 使用在专用线程，跨 COM apartment 依赖 WinRT agility，不保证可靠。
    // 初始化结果经一次性 channel 回传，保持"失败返回 None"的对外语义。
    let (init_tx, init_rx) = std::sync::mpsc::channel::<bool>();
    let thread_app = app.clone();
    let spawned = std::thread::Builder::new()
        .name("smtc".into())
        .spawn(move || {
            let config = souvlaki::PlatformConfig {
                dbus_name: "aiya",
                display_name: "AIYA Music",
                hwnd: Some(hwnd as *mut std::ffi::c_void),
            };
            let mut controls = match souvlaki::MediaControls::new(config) {
                Ok(c) => c,
                Err(e) => {
                    tracing::warn!(error = %e, "SMTC 初始化失败，系统媒体控制不可用");
                    let _ = init_tx.send(false);
                    return;
                }
            };

            // 按钮事件 → 播放器动作。回调在 WinRT 线程池跑，AppHandle 是 Send+Sync。
            let cb_app = thread_app.clone();
            if let Err(e) = controls.attach(move |event| handle_event(&cb_app, event)) {
                tracing::warn!(error = %e, "SMTC 事件订阅失败");
                let _ = init_tx.send(false);
                return;
            }
            // 初始为 Stopped，系统里出现条目但不可按
            let _ = controls.set_playback(souvlaki::MediaPlayback::Stopped);
            let _ = init_tx.send(true);

            let mut last_playing: Option<bool> = None;
            while let Ok(update) = rx.recv() {
                match update {
                    SmtcUpdate::Track {
                        title,
                        artist,
                        album,
                        duration_ms,
                        cover_path,
                    } => {
                        let cover_url = cover_path.as_deref().and_then(cover_url);
                        // debug 级（默认 info 看不到）：排查媒体浮层"没封面/不能拖"时
                        // 用它确认三件事——封面路径拿到没有、缩略图通道是否启用、时长有没有值。
                        // 0x800700A1 那类问题全部发生在这里之后，没有这行就无法区分
                        // "没进缩略图分支"和"进了但失败"。
                        tracing::debug!(
                            thumb = cover_url.is_some(),
                            cover = cover_path
                                .as_ref()
                                .map(|p| p.display().to_string())
                                .unwrap_or_default(),
                            duration_ms = duration_ms.unwrap_or(0),
                            "SMTC 推送曲目"
                        );
                        let meta = souvlaki::MediaMetadata {
                            title: Some(&title),
                            artist: artist.as_deref(),
                            album: album.as_deref(),
                            duration: duration_ms.map(Duration::from_millis),
                            cover_url: cover_url.as_deref(),
                        };
                        if let Err(e) = controls.set_metadata(meta) {
                            tracing::warn!(error = %e, "SMTC set_metadata 失败");
                        }
                        // 换曲后重申当前播放态（SMTC 的 UI 以最后一次设置为准）
                        if let Some(playing) = last_playing {
                            let _ = controls.set_playback(playback_state(playing, 0));
                        }
                    }
                    SmtcUpdate::Progress {
                        playing,
                        position_ms,
                    } => {
                        if last_playing != Some(playing) || playing {
                            let _ = controls.set_playback(playback_state(playing, position_ms));
                        }
                        last_playing = Some(playing);
                    }
                }
                let _ = &thread_app; // 保持句柄存活到线程结束
            }
        });
    match spawned {
        Err(e) => {
            tracing::warn!(error = %e, "SMTC 线程创建失败");
            None
        }
        Ok(_) => match init_rx.recv_timeout(Duration::from_secs(5)) {
            Ok(true) => Some(tx),
            _ => None,
        },
    }
}

fn playback_state(playing: bool, position_ms: u64) -> souvlaki::MediaPlayback {
    let progress = Some(souvlaki::MediaPosition(Duration::from_millis(position_ms)));
    if playing {
        souvlaki::MediaPlayback::Playing { progress }
    } else {
        souvlaki::MediaPlayback::Paused { progress }
    }
}

/// 交给 souvlaki 的封面 URL。
///
/// ## 为什么不是规范 file URL（这就是 0x800700A1 的根因）
///
/// souvlaki 0.8.3 的 Windows 后端（`platform/windows/mod.rs` 的 `set_metadata`）
/// 拿到 cover_url 后只做一件事：
///
/// ```ignore
/// let path = url.trim_start_matches("file://");
/// StorageFile::GetFileFromPathAsync(&HSTRING::from(path))?;
/// ```
///
/// 它**不解百分号编码、也不认三斜杠形式**。所以此前用
/// `Url::from_file_path` 生成的规范形式 `file:///C:/a%20b/x.jpg` 喂进去，
/// trim 之后是 `/C:/a%20b/x.jpg` —— 一个在 Windows 上不存在的路径，
/// `GetFileFromPathAsync` 报 0x800700A1（ERROR_BAD_PATHNAME）。
/// 更糟的是 `set_metadata` 里这一句带 `?`：缩略图失败会**直接 return**，
/// 后面的 StartTime/EndTime/MinSeekTime/MaxSeekTime 全都没设上，
/// 媒体浮层因此没有进度区间（不能拖），而日志里只有一行看起来无关的 WARN。
///
/// 所以这里按它的口味给：`file://` + 原生绝对路径（反斜杠、空格原样，
/// 不做百分号编码——它不解码，编码了反而找不到文件）。UNC 路径同样成立：
/// `file://\\server\share\x.jpg` trim 完还是 `\\server\share\x.jpg`。
///
/// 顺带：文件不存在时返回 None 而不是让它报错。封面缓存被清理后，
/// 宁可少一张图，也不能因为一张图把时长/进度区间一起丢进去。
#[cfg(windows)]
fn cover_url(path: &std::path::Path) -> Option<String> {
    if !path.is_file() {
        return None;
    }
    // to_str 而不是 display()：display() 对非 UTF-8 是 lossy 替换，
    // 替换后的路径指不到真文件，等于把"找不到"伪装成"找到"。
    let raw = path.to_str()?;
    path.is_absolute().then(|| format!("file://{raw}"))
}

/// 非 Windows 后端（MPRIS）要的是真的 file URI，规范形式即可。
#[cfg(not(windows))]
fn cover_url(path: &std::path::Path) -> Option<String> {
    tauri::Url::from_file_path(path).ok().map(|u| u.to_string())
}

/// 播放器命令路径（供 SMTC 回调与 tauri command 共用）。
pub mod commands {
    use tauri::{AppHandle, Manager};

    pub fn toggle(app: &AppHandle) {
        if let Some(state) = app.try_state::<crate::AppState>() {
            crate::cmd_toggle(&state, app);
        }
    }

    pub fn play(app: &AppHandle) {
        if let Some(state) = app.try_state::<crate::AppState>() {
            if state
                .player
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .state()
                .status
                == aiya_player::PlaybackStatus::Paused
            {
                crate::cmd_toggle(&state, app);
            }
        }
    }

    pub fn pause(app: &AppHandle) {
        if let Some(state) = app.try_state::<crate::AppState>() {
            if state
                .player
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .state()
                .status
                == aiya_player::PlaybackStatus::Playing
            {
                crate::cmd_toggle(&state, app);
            }
        }
    }

    pub fn next(app: &AppHandle) {
        if let Some(state) = app.try_state::<crate::AppState>() {
            {
                let mut actor = state
                    .player
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                actor.reduce(aiya_player::PlayerCommand::Next);
            }
            crate::drive_and_emit(&state, app);
        }
    }

    pub fn prev(app: &AppHandle) {
        if let Some(state) = app.try_state::<crate::AppState>() {
            {
                let mut actor = state
                    .player
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                actor.reduce(aiya_player::PlayerCommand::Prev);
            }
            crate::drive_and_emit(&state, app);
        }
    }

    pub fn seek(app: &AppHandle, position_ms: u64) {
        if let Some(state) = app.try_state::<crate::AppState>() {
            crate::cmd_seek(&state, app, position_ms);
        }
    }
}

fn handle_event(app: &AppHandle, event: souvlaki::MediaControlEvent) {
    use souvlaki::{MediaControlEvent as E, SeekDirection};
    let result: Result<(), String> = (|| {
        match event {
            E::Play => commands::play(app),
            E::Pause => commands::pause(app),
            E::Toggle => commands::toggle(app),
            E::Next => commands::next(app),
            E::Previous => commands::prev(app),
            E::Stop => commands::pause(app),
            E::Seek(dir) => {
                let delta: i64 = match dir {
                    SeekDirection::Forward => 10_000,
                    SeekDirection::Backward => -10_000,
                };
                seek_relative(app, delta)?;
            }
            E::SeekBy(dir, d) => {
                let ms = i64::try_from(d.as_millis()).unwrap_or(0);
                let delta = match dir {
                    SeekDirection::Forward => ms,
                    SeekDirection::Backward => -ms,
                };
                seek_relative(app, delta)?;
            }
            E::SetPosition(pos) => {
                let ms = u64::try_from(pos.0.as_millis()).unwrap_or(0);
                commands::seek(app, ms);
            }
            E::Raise => crate::show_main_window(app),
            E::Quit => crate::quit_app(app), // P2：退出前 flush 队列（与托盘退出一致）
            // MPRIS 专属（Linux），Windows 后端不会发
            E::OpenUri(_) | E::SetVolume(_) => {}
        }
        Ok(())
    })();
    if let Err(e) = result {
        tracing::warn!(error = %e, "SMTC 事件处理失败");
    }
}

fn seek_relative(app: &AppHandle, delta_ms: i64) -> Result<(), String> {
    let state = app
        .try_state::<crate::AppState>()
        .ok_or_else(|| "app state 不可用".to_string())?;
    let current = state
        .player
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .state()
        .position_ms;
    let target = (current as i64 + delta_ms).max(0) as u64;
    commands::seek(app, target);
    Ok(())
}

/* ---------------- 防休眠 ---------------- */

/// 播放中防系统睡眠。ES_CONTINUOUS 是 per-thread 状态：
/// 在事件泵线程调用并保持，线程常驻所以状态持续有效。
#[cfg(windows)]
pub fn set_prevent_sleep(prevent: bool) {
    use windows_sys::Win32::System::Power::{
        SetThreadExecutionState, ES_CONTINUOUS, ES_SYSTEM_REQUIRED,
    };
    let flags = if prevent {
        ES_CONTINUOUS | ES_SYSTEM_REQUIRED
    } else {
        ES_CONTINUOUS
    };
    unsafe {
        SetThreadExecutionState(flags);
    }
}

#[cfg(not(windows))]
pub fn set_prevent_sleep(_prevent: bool) {}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    /// 复刻 souvlaki 的取路径逻辑，断言它拿到的确实是能用的本地路径。
    ///
    /// 这个断言盯着的是**跨 crate 的隐式契约**：souvlaki 不文档化它要什么形态的
    /// URL，只在自己实现里 trim 一次。升级 souvlaki 时这条测试会先叫。
    #[test]
    fn cover_url_survives_souvlaki_trim() {
        let file = std::env::temp_dir().join("aiya smtc cover.txt");
        std::fs::write(&file, b"x").unwrap();

        let url = cover_url(&file).expect("存在的文件应当给出 URL");
        assert!(
            url.starts_with("file://"),
            "souvlaki 只认 file:// 前缀：{url}"
        );

        // souvlaki 的原话：url.trim_start_matches("file://")
        let stripped = url.trim_start_matches("file://");
        assert_eq!(
            stripped,
            file.to_str().unwrap(),
            "trim 之后必须正好是原生路径"
        );
        assert_eq!(
            stripped.chars().nth(1),
            Some(':'),
            "盘符必须在第一位：前导斜杠会被 GetFileFromPathAsync 判成 0x800700A1，实际 {stripped}"
        );
        assert!(!stripped.contains("%20"), "不做百分号编码——souvlaki 不解码");
        std::fs::remove_file(&file).ok();
    }

    /// 反例留档：规范 file URL 正是 0x800700A1 的来源，别改回去。
    #[test]
    fn canonical_file_url_would_lose_the_drive_root() {
        let canonical = tauri::Url::from_file_path("C://a b//cover.jpg")
            .unwrap()
            .to_string();
        assert_eq!(canonical, "file:///C:/a%20b/cover.jpg");
        let stripped = canonical.trim_start_matches("file://");
        assert!(
            stripped.starts_with('/'),
            "规范形式 trim 后留着前导斜杠 → ERROR_BAD_PATHNAME：{stripped}"
        );
        assert_ne!(stripped, "C://a b//cover.jpg");
    }

    /// 封面文件被清理掉时不许硬报错：少一张图可以，连带把时长/进度区间
    /// 一起丢掉不行（souvlaki 的 `?` 会让 set_metadata 提前返回）。
    #[test]
    fn missing_cover_is_skipped() {
        let gone = std::env::temp_dir().join("aiya-no-such-cover-xyz.jpg");
        std::fs::remove_file(&gone).ok();
        assert_eq!(cover_url(&gone), None);
    }
}
