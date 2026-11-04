/**
 * 直播流的展示口径：播放条与直播页共用。
 *
 * 为什么单独抽一层：同一条状态在**两个地方**渲染（底部播放条的徽章、直播页
 * 的大画面），"暂停"在直播语义下不是暂停而是**断开**。两处各写一遍
 * `status === 'playing' ? '直播' : '已断开'` 的话，改文案必漏一处——
 * 这个仓在 `is_http_url` 上已经吃过一次同样的亏。
 */
import { STREAM_ID_PREFIX } from './format';
import type { LiveRecentEntry } from './settings';
import type { PlaybackStatus } from './ipc/generated/PlaybackStatus';

export interface LivePhase {
  /** 徽章文字 */
  text: string;
  /** on = 连着；wait = 正在连；off = 已断开 */
  tone: 'on' | 'wait' | 'off';
}

/**
 * 直播三态。
 *
 * `paused` 在这条链路上等于**已断开**：用户拍板"暂停即终止连接、播放即重连"
 * （见 `actor.rs` 的 Stream 分支），文案必须跟着说实话。
 */
export function livePhase(status: PlaybackStatus | null | undefined): LivePhase {
  if (status === 'playing') return { text: '直播', tone: 'on' };
  if (status === 'loading') return { text: '连接中', tone: 'wait' };
  return { text: '已断开', tone: 'off' };
}

/** 「最近打开」上限。20 条够翻，再多就该做收藏夹了（那是另一件事） */
export const LIVE_RECENT_LIMIT = 20;

/** 卡组换流的过渡时长，与 LiveView 的 `--live-swap-ms` 同源（后者由组件注入）。 */
export const LIVE_SWAP_MS = 800;

/**
 * 从 `TrackRef.id` 取出直播流的原始 URL。
 *
 * `stream:` 前缀后的部分就是完整地址（见 `format.ts` 与 Rust 侧的
 * `STREAM_ID_PREFIX` 约定），直播页靠它把"正在播的流"对回卡组里的流；
 * 非直播流（本地曲目）返回 null。
 */
export function streamUrlOf(id: string | null | undefined): string | null {
  if (!id || !id.startsWith(STREAM_ID_PREFIX)) return null;
  return id.slice(STREAM_ID_PREFIX.length);
}

/**
 * 从 URL 派生确定性 id（djb2 → base36）。只用于**旧数据补齐**：磁盘上没有
 * id 的历史条目每次加载都派生出同一个 id，不需要写回也能稳定；新建的流
 * 走 `crypto.randomUUID`，两套来源不会撞（前缀不同）。
 */
function legacyLiveId(url: string): string {
  let hash = 5381;
  for (let i = 0; i < url.length; i += 1) {
    hash = ((hash << 5) + hash + url.charCodeAt(i)) | 0;
  }
  return `st-legacy-${(hash >>> 0).toString(36)}`;
}

function newLiveId(): string {
  return typeof crypto !== 'undefined' && 'randomUUID' in crypto
    ? crypto.randomUUID()
    : `st-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 10)}`;
}

/**
 * 旧数据归一：磁盘上的历史条目缺 id / addedAtMs / source（地基字段是后补的，
 * 存储层是 schemaless JSON，读出来缺键不炸），在这里按确定性规则补齐。
 * 每次加载结果一致（legacy id 由 URL 派生），所以**不需要写回**也能稳定。
 * url 都没有的条目是彻底的坏数据，丢弃。
 */
export function normalizeLiveRecent(raw: unknown): LiveRecentEntry[] {
  if (!Array.isArray(raw)) return [];
  const out: LiveRecentEntry[] = [];
  for (const item of raw) {
    if (typeof item !== 'object' || item === null) continue;
    const o = item as Record<string, unknown>;
    if (typeof o.url !== 'string' || !o.url.trim()) continue;
    const url = o.url;
    out.push({
      id: typeof o.id === 'string' && o.id ? o.id : legacyLiveId(url),
      url,
      ...(typeof o.title === 'string' && o.title ? { title: o.title } : {}),
      ...(typeof o.subtitle === 'string' && o.subtitle ? { subtitle: o.subtitle } : {}),
      ...(typeof o.description === 'string' && o.description
        ? { description: o.description }
        : {}),
      source: 'manual',
      addedAtMs: typeof o.addedAtMs === 'number' && o.addedAtMs >= 0 ? o.addedAtMs : 0,
    });
  }
  return out;
}

/**
 * 把一条记录放到「最近打开」最前面：**按 URL 去重**，超上限截尾，不改入参。
 *
 * 去重是必须的：同一条流反复开会在列表里排出一屏同名条目，
 * 而"最近打开"这个词的意思是"打开过哪些"，不是"打开过几次"。
 *
 * 字段合并是"新值优先、缺位保旧"：重开同一条流不带元数据是常态
 * （`openUrl` 只有 url + name），不能把用户填过的标题/副标题抹掉。
 * 地基字段同样保旧：id / addedAtMs / source 归已有流所有，新流现场生成。
 */
export function rememberLive(
  list: readonly LiveRecentEntry[],
  entry: Omit<LiveRecentEntry, 'id' | 'addedAtMs' | 'source'> & Partial<Pick<LiveRecentEntry, 'source'>>,
  limit = LIVE_RECENT_LIMIT,
): LiveRecentEntry[] {
  const url = entry.url.trim();
  if (!url) return [...list];
  const prev = list.find((item) => item.url === url);
  const next = list.filter((item) => item.url !== url);
  const merged: LiveRecentEntry = {
    id: prev?.id ?? newLiveId(),
    url,
    source: entry.source ?? prev?.source ?? 'manual',
    addedAtMs: prev?.addedAtMs ?? Date.now(),
  };
  for (const key of ['title', 'subtitle', 'description', 'cover'] as const) {
    const value = entry[key] || prev?.[key];
    if (value) merged[key] = value;
  }
  next.unshift(merged);
  return next.slice(0, limit);
}
