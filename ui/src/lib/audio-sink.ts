/**
 * 兼容垫片：旧路径 `lib/audio-sink` → `compat/audio-sink`。
 *
 * Tauri 版里音频在 Rust sidecar 出声（ADR-0006），`<audio>` 只是
 * App.svelte 留下的死挂载点。真实实现见 `../compat/audio-sink.ts`；
 * 这里保持模块路径不变，`stores/player.svelte.ts` 一行不用改。
 */
export { AudioSink } from '../compat/audio-sink.js';
export type { AudioSinkTransport } from '../compat/audio-sink.js';
