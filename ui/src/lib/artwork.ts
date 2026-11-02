/**
 * 封面 URL 的构造：`artworkId` + 尺寸档位 → `aiya-artwork` 协议 URL。
 *
 * 全前端唯一的构造点。拼接跨越类型边界：`artworkId` 在类型上只是字符串，URL
 * 形状由 Rust 侧的协议定义，拼错的结果是封面请求全部失败，而类型检查不报错。
 *
 * 对端：协议处理器见 `src-tauri/src/lib.rs::artwork_protocol`，scheme 常量见
 * `crates/aiya-data/src/artwork.rs::ARTWORK_SCHEME`。路径 `/artwork/<id>` 与查询
 * 参数名 `size` 两侧共用，改名须同步。
 */

/** 后端生成的尺寸档位。档外值由 `artworkUrl` 收敛到 96，不作为错误处理。 */
export const ARTWORK_SIZES = [96, 256, 512] as const;
export type ArtworkSize = (typeof ARTWORK_SIZES)[number];
const DEFAULT_SIZE = 96;

/**
 * 返回封面 URL；`artworkId` 为空（无内嵌封面或未探测）时返回 `null`。
 *
 * 空值不转换为占位地址：占位由调用方决定（`?? ''`、`{#if}` 分支），本函数只
 * 构造有效地址。
 */
export function artworkUrl(artworkId: string | null, size: number = DEFAULT_SIZE): string | null {
  if (!artworkId) return null;
  // 档外值收敛到 96 会被上采样（放大后模糊），因此 dev 环境告警；生产环境静默收敛。
  if (import.meta.env.DEV && !(ARTWORK_SIZES as readonly number[]).includes(size)) {
    console.warn(
      `[artwork] size=${size} 不在 ${ARTWORK_SIZES.join('/')} 档内，已回落 ${DEFAULT_SIZE}`,
    );
  }
  const safe = (ARTWORK_SIZES as readonly number[]).includes(size) ? size : DEFAULT_SIZE;
  // Tauri 2 的自定义协议在 Windows 上经 `http://<scheme>.localhost` 暴露。处理器返回 `Access-Control-Allow-Origin:
  // *`，`artwork-blob` 的 fetch 与 canvas 取色依赖该响应头。当前仅发布 Windows；跨平台时按平台切换 URL 前缀。
  return `http://aiya-artwork.localhost/artwork/${encodeURIComponent(artworkId)}?size=${safe}`;
}
