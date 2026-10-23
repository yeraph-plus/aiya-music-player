/**
 * 封面 URL 方案（docs/07 §2.3）。
 *
 * 单一事实源在 vendored `contracts/ipc.ts` 的 `artworkUrl()` ——
 * 7 个组件 + media-session 都是静态 import，改那一处即可全量生效。
 * 这里只保留 re-export 与平台注意事项，避免出现两个拼 URL 的地方。
 *
 * 平台注意：
 * - Windows WebView2：自定义协议经 `http://aiya-artwork.localhost` 暴露；
 * - CSP 需允许该 origin（img-src / connect-src），后端响应带 `ACAO: *`，
 *   否则 artwork-blob 的 fetch 与 canvas 取色会静默失败；
 * - Rust 侧尚未实现该协议前，封面显示为占位图（不算错误）。
 */
export { artworkUrl } from '@aiya/contracts';
