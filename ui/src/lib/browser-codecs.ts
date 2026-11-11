/**
 * 浏览器解码支持探测。
 *
 * Web 宿主里解码归浏览器（`<audio>` + HTTP Range），同一份曲库在不同浏览器
 * 上可播集合不同：Chromium 解不了 AIFF（ALAC 在 Mp4 容器里与 AAC 无从区分），
 * Safari/Firefox 各有出入。探测用 `canPlayType`（不真解码，零开销），结果按
 * 容器名缓存——同一浏览器内的答案不会变。
 *
 * 数据源是行上的 `container`（lofty FileType 的 Debug 串，扫描即有、不依赖
 * beets）。三个口径上的克制：
 * - 未登记的容器一律按可播处理——探测是给"已知解不了"打灰标，不给未知格式
 *   判死刑；已登记但浏览器普遍解不了的（Ape/WavPack/Mpc/Speex）显式列出；
 * - Mp4 容器按可播处理（AAC 是绝对主流），ALAC 藏在 Mp4 里列表层无法区分，
 *   点播时由浏览器解码失败走播放条报错兜底（failed 反馈链已接通）；
 * - 判据与 backend-caps 同源（`__AIYA_WEB__`）：桌面宿主引擎全格式自解恒
 *   可播；mock/浏览器预览保持桌面能力（e2e 形态断言口径），灰标只在 http
 *   实机出现。
 */

/** 扫描器写的容器名（lofty FileType Debug 串，大小写不敏感）→ canPlayType 探测串 */
const CONTAINER_MIME: Record<string, string> = {
  mpeg: 'audio/mpeg',
  mp3: 'audio/mpeg',
  flac: 'audio/flac',
  wav: 'audio/wav',
  aiff: 'audio/aiff',
  aac: 'audio/mp4; codecs="mp4a.40.2"',
  mp4: 'audio/mp4; codecs="mp4a.40.2"',
  m4a: 'audio/mp4; codecs="mp4a.40.2"',
  ogg: 'audio/ogg; codecs="vorbis"',
  vorbis: 'audio/ogg; codecs="vorbis"',
  opus: 'audio/ogg; codecs="opus"',
  // 已登记的"浏览器普遍解不了"：canPlayType 恒空 → 打灰（与未知放行相区分）
  ape: 'audio/ape',
  wavpack: 'audio/wavpack',
  mpc: 'audio/musepack',
  speex: 'audio/ogg; codecs="speex"',
};

let probeEl: HTMLAudioElement | null = null;
const cache = new Map<string, boolean>();

export function browserCanPlay(container: string | null | undefined): boolean {
  if (!container) return true; // 未知不误伤
  const web =
    typeof window !== 'undefined' &&
    !!(window as { __AIYA_WEB__?: unknown }).__AIYA_WEB__;
  if (!web) return true; // 桌面引擎全格式自解；mock/预览按桌面能力
  const hit = cache.get(container);
  if (hit !== undefined) return hit;
  const mime = CONTAINER_MIME[container.toLowerCase()];
  let ok = true;
  if (mime) {
    probeEl ??= document.createElement('audio');
    ok = probeEl.canPlayType(mime) !== '';
  }
  cache.set(container, ok);
  return ok;
}
