# ui/（规划）

Svelte 5 + Vite + WebView2。只依赖：

- `src/bindings.ts`（由 Rust 生成）；
- 本地 stores；
- `lib/` 纯函数。

不创建 `<audio>` / `AudioContext`，不直接读文件/数据库，不维护第二套播放状态。

迁移时优先复用旧仓库：

- `renderer/src/lib/{virtual,selection,track-window,drag,list-keys,lyrics-clock,format,spectrum}.ts`
- `renderer/src/components/*` 的视觉与交互
- `renderer/src/stores/*` 的结构

`App.svelte` 从第一天按 feature 拆分，目标 < 300 行。