/**
 * 封面 URL 的唯一构造点。
 *
 * 为什么单独一个文件：
 * - 这是**跨类型边界**的字符串拼装（artworkId → 自定义协议 URL），拼错了
 *   症状是"封面全空"而不是类型报错，必须有单一事实源；
 * - 后端 `aiya-artwork` scheme 的路径与参数名写死在 Rust 侧
 *   （`src-tauri/src/artwork*.rs`），改名要两边同步，注释里标明对端；
 * - 之前它住在 `@aiya/contracts`（旧 Electron 工程的 vendored 副本），
 *   新代码一律从这里取；`contracts/` 在批次 4 整体删除。
 */

/** 后端只生成这三档尺寸；传别的值会落到 96，不是错误。 */
export const ARTWORK_SIZES = [96, 256, 512] as const;
export type ArtworkSize = (typeof ARTWORK_SIZES)[number];
const DEFAULT_SIZE = 96;

/**
 * `artworkId` 为空（无内嵌封面 / 未探测）时返回 `null`，
 * 让调用方用 `?? ''` 或 `{#if}` 自己决定占位——不要在这里造一个 404 URL。
 */
export function artworkUrl(artworkId: string | null, size: number = DEFAULT_SIZE): string | null {
  if (!artworkId) return null;
  // 档外值静默落 96 曾把首页大图打模糊（调用方传 320 → 96 被上采样 1.5 倍，
  // 2026-10-30）：dev 下喊出来，别让这类事再无声发生；生产保持静默纠偏。
  if (import.meta.env.DEV && !(ARTWORK_SIZES as readonly number[]).includes(size)) {
    console.warn(
      `[artwork] size=${size} 不在 ${ARTWORK_SIZES.join('/')} 档内，已回落 ${DEFAULT_SIZE}`,
    );
  }
  const safe = (ARTWORK_SIZES as readonly number[]).includes(size) ? size : DEFAULT_SIZE;
  // Tauri 2 的自定义协议在 Windows 上经 `http://<scheme>.localhost` 暴露。
  // 后端注册 aiya-artwork scheme 并带 ACAO:* —— artwork-blob 的 fetch 与
  // canvas 取色依赖它。v1 只发 Windows；跨平台时按 UA/平台切换前缀。
  return `http://aiya-artwork.localhost/artwork/${encodeURIComponent(artworkId)}?size=${safe}`;
}
