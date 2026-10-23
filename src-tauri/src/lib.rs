//! AIYA Music — Tauri 2 宿主壳（P0 最小版）。
//!
//! 边界（docs/02、src-tauri/README.md）：这里只做命令注册、事件桥、协议注册，
//! 领域业务在 `crates/`（aiya-player / aiya-data / aiya-audio），本壳不写业务。
//! P0 的验收是"窗口能开、UI 能加载"；真实命令在 P1/P2 按契约逐个接入。

#[tauri::command]
fn app_info() -> serde_json::Value {
    serde_json::json!({
        "ok": true,
        "data": {
            "version": env!("CARGO_PKG_VERSION"),
            "electron": "n/a（Tauri 2）",
            "platform": "windows",
            "dataDir": "(P1 接入 app paths)"
        }
    })
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![app_info])
        .run(tauri::generate_context!())
        .expect("AIYA: Tauri 运行失败");
}
