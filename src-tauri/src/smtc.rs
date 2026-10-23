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
    let spawned = std::thread::Builder::new().name("smtc".into()).spawn(move || {
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
                SmtcUpdate::Track { title, artist, album, duration_ms, cover_path } => {
                    let cover_url = cover_path.as_ref().and_then(|p| file_uri(p));
                    let meta = souvlaki::MediaMetadata {
                        title: Some(&title),
                        artist: artist.as_deref(),
                        album: album.as_deref(),
                        duration: duration_ms.map(|ms| Duration::from_millis(ms)),
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
                SmtcUpdate::Progress { playing, position_ms } => {
                    if last_playing != Some(playing) || playing {
                        let _ = controls
                            .set_playback(playback_state(playing, position_ms));
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

fn file_uri(path: &std::path::Path) -> Option<String> {
    // Windows 本地路径 → 规范 file:///C:/...（P2：此前手工拼 file://C:/...，
    // "C:" 被当 authority；且 '%' 未编码，路径含 % 字面量时会被双重解码）。
    // url::Url::from_file_path 产 canonical 形式并做百分号编码。
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
            if state.player.lock().unwrap_or_else(std::sync::PoisonError::into_inner).state().status
                == aiya_player::PlaybackStatus::Paused
            {
                crate::cmd_toggle(&state, app);
            }
        }
    }

    pub fn pause(app: &AppHandle) {
        if let Some(state) = app.try_state::<crate::AppState>() {
            if state.player.lock().unwrap_or_else(std::sync::PoisonError::into_inner).state().status
                == aiya_player::PlaybackStatus::Playing
            {
                crate::cmd_toggle(&state, app);
            }
        }
    }

    pub fn next(app: &AppHandle) {
        if let Some(state) = app.try_state::<crate::AppState>() {
            {
                let mut actor = state.player.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                actor.reduce(aiya_player::PlayerCommand::Next);
            }
            crate::drive_and_emit(&state, app);
        }
    }

    pub fn prev(app: &AppHandle) {
        if let Some(state) = app.try_state::<crate::AppState>() {
            {
                let mut actor = state.player.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
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
    let current = state.player.lock().unwrap_or_else(std::sync::PoisonError::into_inner).state().position_ms;
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
