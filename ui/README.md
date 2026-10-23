# ui/

Svelte 5 + Vite + WebView2。v1 **原样复制** demo 的 `apps/desktop/src/renderer/src`，只加 `compat/`：

- `compat/api.ts`：旧 `window.api` → Tauri invoke/events；
- `compat/audio-sink.ts`：保持 `AudioSink` API，但后端出声；
- `compat/spectrum.ts`：compact 频谱；pro 暂不可用；
- `compat/artwork.ts`：`aiya-artwork://`；
- `compat/boot.ts`：首屏主题 + mount。

组件/样式/纯函数先不改；`App.svelte` 暂不拆，等 beets 审阅界面需要新结构时再按 feature 重写。完整清单见 [`../docs/07-ui-reuse.md`](../docs/07-ui-reuse.md)。

不创建 `<audio>` / `AudioContext`，不直接读文件/数据库，不维护第二套播放状态。