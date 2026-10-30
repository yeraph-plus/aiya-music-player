//! 数据目录归宿：安装版走系统应用数据目录，**便携版跟程序目录走**。
//!
//! ## 为什么用标记文件判断，而不是探测"目录可不可写"
//!
//! 探测式判定会让**同一个二进制**在不同机器上落到不同位置：装到 `Program Files`
//! 时目录不可写 → 退回 `%APPDATA%`，解压到 U 盘 → 留在原地。用户备份/迁移时
//! 对不上"我的库到底在哪"。`portable.txt` 是随包分发的事实，谁拿到这个包都是
//! 同一个答案，也能被用户自己删掉/加回去（删了就是安装版语义）。
//!
//! ## 谁是唯一入口
//!
//! 全仓只有 `paths::data_root` 可以回答"数据放哪"。此前有四处各自调
//! `app.path().app_data_dir()`（库、封面缓存、日志、beets 数据/工具），
//! 便携版一做就必须四处同时改；集中到今天这一步，漏一处就是"设置里说在
//! D 盘，日志却在 C 盘"。
//!
//! 目录布局（便携版）：
//! ```text
//! AIYA Music/
//!   AIYA Music.exe
//!   aiya-audio-host.exe
//!   portable.txt        ← 标记，删掉即退回系统目录
//!   data/
//!     library.sqlite
//!     cache/artwork/
//!     logs/
//!     beets/            ← 整理库（beets 的 library.db 与配置）
//!     beets-tool/       ← 安装的 beets 工具本体
//!     webview/          ← WebView2 用户数据（设置见 prepare_portable_env）
//! ```

use std::path::{Path, PathBuf};

use tauri::Manager;

/// 便携标记文件名，与 exe 同目录。
pub const PORTABLE_MARKER: &str = "portable.txt";

/// 纯函数版便携判定（可测）：`<dir>/portable.txt` 存在 → `<dir>/data`。
fn portable_data_root_in(exe_dir: &Path) -> Option<PathBuf> {
    exe_dir
        .join(PORTABLE_MARKER)
        .is_file()
        .then(|| exe_dir.join("data"))
}

/// 当前进程是否便携运行；是则给出数据目录（可能尚未创建）。
pub fn portable_data_root() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    portable_data_root_in(exe.parent()?)
}

/// 数据根目录——**全仓唯一入口**。
///
/// 便携模式下目录建不出来（只读介质、权限不足）就退回系统目录：宁可
/// "不够便携"，不能让启动因为一个目录而失败。这时数据会分裂成两份，所以
/// 要留一行日志，用户下次报"设置丢了"时能查到原因。
pub fn data_root(app: &tauri::AppHandle) -> PathBuf {
    let system = || {
        app.path()
            .app_data_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
    };
    let Some(dir) = portable_data_root() else {
        return system();
    };
    match std::fs::create_dir_all(&dir) {
        Ok(()) => dir,
        Err(e) => {
            // 此刻订阅者还没装好（logging::init 才装），eprintln 是唯一能落地的出口
            eprintln!(
                "[aiya] 便携数据目录不可写（{}：{e}），退回系统应用数据目录",
                dir.display()
            );
            system()
        }
    }
}

/// 便携模式下把 WebView2 的用户数据目录也搬进程序目录。
///
/// **必须在创建任何窗口之前调用**（WebView2 环境的用户数据目录是创建时定的）。
/// 只能靠环境变量：`tauri.conf.json` 里静态声明的窗口没有 `data_directory` 可设，
/// 而 WebView2 只认 `WEBVIEW2_USER_DATA_FOLDER`。
///
/// 不做这一步会怎样：播放器自己的库/封面/日志/beets 都在 `data/` 下，唯独浏览
/// 引擎的缓存还留在 `%LOCALAPPDATA%\<identifier>\EBWebView`——便携包在别人机器上
/// 一跑就往系统盘塞几十 MB，"解压即用、拔盘即走"就不成立了。
pub fn prepare_portable_env() {
    let Some(dir) = portable_data_root() else {
        return;
    };
    let webview = dir.join("webview");
    if std::fs::create_dir_all(&webview).is_err() {
        // 建不出来就别设变量：让 WebView2 用默认位置，至少能启动
        return;
    }
    std::env::set_var("WEBVIEW2_USER_DATA_FOLDER", &webview);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_dir(tag: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!(
            "aiya-paths-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn no_marker_means_installed() {
        let dir = tmp_dir("plain");
        assert_eq!(portable_data_root_in(&dir), None);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn marker_moves_data_next_to_exe() {
        let dir = tmp_dir("portable");
        std::fs::write(dir.join(PORTABLE_MARKER), "portable build\n").unwrap();
        assert_eq!(portable_data_root_in(&dir), Some(dir.join("data")));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn marker_must_be_a_file() {
        let dir = tmp_dir("marker-dir");
        std::fs::create_dir_all(dir.join(PORTABLE_MARKER)).unwrap();
        assert_eq!(portable_data_root_in(&dir), None, "目录同名不能当标记");
        std::fs::remove_dir_all(&dir).ok();
    }
}
