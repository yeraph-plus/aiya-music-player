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

use std::path::Path;
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

/// 在资源管理器中打开 `path` 所在位置并选中它。返回是否成功拉起进程。
///
/// **为什么不能用 `arg(format!("/select,{path}"))`**：标准库给含空格的参数
/// 加引号时是**整体**加（`"/select,C:\a b\x.mp3"`），而 explorer 的命令行
/// 解析器不认这种"引号开头的开关"，于是静默退化成"打开默认文件夹"——用户
/// 点了"在文件夹中显示"却只是弹出文档目录，且没有任何错误可查。
///
/// 正确形态是 `/select,"C:\a b\x.mp3"`：引号只包路径。这只能自己拼命令行，
/// 所以走 `raw_arg`。`raw_arg` 绕过标准库的引号处理，这里必须保证路径本身
/// 不含双引号（Windows 文件名不允许 `"`，所以恒成立）。
pub fn reveal_in_explorer(path: &Path) -> bool {
    let mut cmd = Command::new("explorer");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.raw_arg(format!("/select,\"{}\"", path.display()));
    }
    #[cfg(not(windows))]
    {
        // 其他平台没有 /select 语义：打开父目录即可（本项目只发 Windows 包，
        // 这里只为让 `cargo test` 在别的平台上仍然编得过）
        cmd.arg(path.parent().unwrap_or(path));
    }
    cmd.spawn().is_ok()
}
