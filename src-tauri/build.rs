fn main() {
    // 插件表是 `include_str!` 进二进制的（`beets.rs::PLUGIN_TABLE_JSON`）。
    // 不说一声的话，改了 JSON 只重编 beets.rs 才会生效 —— 而 JSON 是数据文件，
    // 改了它 cargo 不会认为源文件变了，于是二进制里还是旧表、UI 显示旧插件。
    println!("cargo:rerun-if-changed=../beets-tool/src/aiya_beets/data/plugins.json");

    // 先自己看一眼 sidecar 在不在，再交给 tauri_build。
    //
    // 为什么值得多这几行：`bundle.resources` 里那句
    //   resource path `binaries\aiya-audio-host-x86_64-pc-windows-msvc.exe` doesn't exist
    // 是 tauri-build 打的，它不说"怎么造出来"。干净检出（CI、新克隆、换了机器）
    // 上第一次 `cargo test` 就会撞上，2026-10-27 CI 首跑就是这么红的。
    // 下面把解法直接写在报错里。
    let target = std::env::var("TARGET").unwrap_or_default();
    let exe = if cfg!(windows) { ".exe" } else { "" };
    let sidecar = std::path::Path::new("binaries").join(format!("aiya-audio-host-{target}{exe}"));
    if !sidecar.exists() {
        panic!(
            "\n[build.rs] 缺少 sidecar：{}\n\
             先构建它，再编本 crate：\n\
             \x20   npm run build:sidecar          （从仓库根跑，产物会落进 src-tauri/binaries/）\n\
             \x20   npm run test:all               （门禁脚本已包含上一步）\n",
            sidecar.display()
        );
    }
    tauri_build::build();
}
