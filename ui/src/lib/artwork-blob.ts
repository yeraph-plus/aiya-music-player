/**
 * 封面字节的共享加载层。
 *
 * 为什么需要一层：`MediaMetadata.artwork` 只接受 http/https/data/blob 四种 scheme，
 * 而我们的封面走自定义 `media://`。直接塞 `media://` 会被 Blink 拒绝
 * （"MediaImage src can only be of http/https/data/blob scheme"），
 * 结果就是系统媒体中心没有封面。
 *
 * 所以统一在渲染层把 `media://artwork/...` fetch 成 Blob，再转成 blob: URL：
 * - Media Session 用 blob: URL 才能被接受
 * - 主色提取本来就要 fetch 一次，复用同一份 Blob，不重复请求
 *
 * 缓存策略：Blob 按 URL 记 Promise（并发去重），object URL 用固定容量 LRU，
 * 淘汰时 revoke，避免长时间播放后 object URL 无限堆积。
 */

const MAX_BLOBS = 48;
const MAX_OBJECT_URLS = 24;

const blobTasks = new Map<string, Promise<Blob | null>>();
const objectUrls = new Map<string, string>();

/** 按插入顺序淘汰（Map 保证顺序即写入顺序） */
function evictOldest<V>(
  cache: Map<string, V>,
  limit: number,
  onEvict?: (key: string, value: V) => void,
): void {
  while (cache.size > limit) {
    const oldest = cache.entries().next();
    if (oldest.done) return;
    const [key, value] = oldest.value;
    cache.delete(key);
    onEvict?.(key, value);
  }
}

export function loadArtworkBlob(url: string): Promise<Blob | null> {
  const cached = blobTasks.get(url);
  if (cached) return cached;

  const task = (async (): Promise<Blob | null> => {
    try {
      const response = await fetch(url);
      if (!response.ok) return null;
      return await response.blob();
    } catch {
      return null;
    }
  })();

  blobTasks.set(url, task);
  evictOldest(blobTasks, MAX_BLOBS);
  return task;
}

/**
 * 拿到可用于 MediaMetadata 的 blob: URL。
 * 失败返回 null —— 封面缺失不该让系统媒体中心整体失效。
 */
export async function artworkObjectUrl(url: string): Promise<string | null> {
  const existing = objectUrls.get(url);
  if (existing) return existing;

  const blob = await loadArtworkBlob(url);
  if (!blob) return null;

  // 并发下可能已由另一条路径创建，直接复用，避免泄漏一个 object URL
  const raced = objectUrls.get(url);
  if (raced) return raced;

  const objectUrl = URL.createObjectURL(blob);
  objectUrls.set(url, objectUrl);
  evictOldest(objectUrls, MAX_OBJECT_URLS, (_key, evicted) => {
    URL.revokeObjectURL(evicted);
  });
  return objectUrl;
}
