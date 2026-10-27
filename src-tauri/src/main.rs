// Prevents an extra console window on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // 便携版：把 WebView2 的用户数据目录也放回程序目录。必须在任何窗口创建之前，
    // 否则 WebView2 环境已经用默认位置初始化完了（见 paths::prepare_portable_env）。
    aiya_lib::prepare_portable_env();
    aiya_lib::run();
}
