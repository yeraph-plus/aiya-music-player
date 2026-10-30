/**
 * 直播流的展示口径（docs/52）：播放条与直播页共用。
 *
 * 为什么单独抽一层：同一条状态在**两个地方**渲染（底部播放条的徽章、直播页
 * 的大画面），"暂停"在直播语义下不是暂停而是**断开**。两处各写一遍
 * `status === 'playing' ? '直播' : '已断开'` 的话，改文案必漏一处——
 * 这个仓在 `is_http_url` 上已经吃过一次同样的亏。
 */
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

/**
 * 把一条记录放到「最近打开」最前面：**按 URL 去重**，超上限截尾，不改入参。
 *
 * 去重是必须的：同一个台反复开会在列表里排出一屏同名条目，
 * 而"最近打开"这个词的意思是"打开过哪些"，不是"打开过几次"。
 */
export function rememberLive(
  list: readonly LiveRecentEntry[],
  entry: LiveRecentEntry,
  limit = LIVE_RECENT_LIMIT,
): LiveRecentEntry[] {
  const url = entry.url.trim();
  if (!url) return [...list];
  const next = list.filter((item) => item.url !== url);
  next.unshift({ url, ...(entry.title ? { title: entry.title } : {}) });
  return next.slice(0, limit);
}
