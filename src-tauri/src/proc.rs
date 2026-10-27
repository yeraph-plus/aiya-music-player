//! 子进程启动的平台细节（Windows 控制台窗口）。
//!
//! 为什么需要这个模块：本项目的 CUI 子进程都从**GUI 子系统**的主程序里
//! spawn 出来 —— 音频 sidecar、Python 版 `aiya-beets serve`、安装器探针。
//! Windows 对 CUI 子进程的默认行为是分配一个**新的控制台窗口**，于是用户
//! 看到播放器启动时冒出一个黑框（关掉播放器才消失）。
//!
//! `CREATE_NO_WINDOW` 是父进程这侧唯一的闸门。子进程自己声明
//! `windows_subsystem = "windows"` 只影响它被 Explorer 直接双击时的行为，
//! 解决不了被别的进程拉起的情况 —— 而且对 audio-host 是有害的：实测改成
//! GUI 子系统后 `--selftest` 在 PowerShell 里重定向得到 0 字节（见
//! `crates/aiya-audio-host/src/main.rs` 顶部注释）。
//!
//! **副作用（每个新增调用点都必须满足）**：该标志连"继承父进程控制台"也
//! 一并抑制，子进程拿不到有效的标准句柄。所以 spawn 前必须显式给
//! stdin/stdout/stderr 赋值（管道 / `Stdio::null()` / 文件），不能依赖
//! 默认继承。
//!
//! 不需要用的场合：`explorer.exe` 这类 GUI 程序本来就不分配控制台。

use std::process::Command;

/// 让子进程不分配控制台窗口（仅 Windows 有效，其他平台空操作）。
pub fn no_window(cmd: &mut Command) -> &mut Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        /// 进程在**没有**控制台窗口的情况下作为控制台程序运行。
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

