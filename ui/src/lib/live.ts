/**
 * 直播流的展示口径：播放条与直播页共用。
 *
 * 为什么单独抽一层：同一条状态在**两个地方**渲染（底部播放条的徽章、直播页
 * 的大画面），"暂停"在直播语义下不是暂停而是**断开**。两处各写一遍
 * `status === 'playing' ? '直播' : '已断开'` 的话，改文案必漏一处——
 * 这个仓在 `is_http_url` 上已经吃过一次同样的亏。
 */
import { t } from './i18n.svelte';
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
  // 三态文案与播放条同一口径：shell 域的 shell.live.*（zh 值与迁移前逐字一致）
  if (status === 'playing') return { text: t('shell.live.on'), tone: 'on' };
  if (status === 'loading') return { text: t('shell.live.wait'), tone: 'wait' };
  return { text: t('shell.live.off'), tone: 'off' };
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

/**
 * 站单下发（docs/68 §12.1/§12.3）：把服务端频道列表合并进「最近打开」。
 *
 * 合并方向与 rememberLive **相反**——rememberLive 是"现在带的信息优先"
 * （开播现场带 url+name），站单是"**用户已填字段赢、服务端只补空白**"
 * （LIVE_STATION_FIELDS 契约：服务端权威 id、url 随部署更新、用户填过的
 * title/subtitle/描述不被覆盖）。id 用服务端权威 id（station.id），所以
 * 同一频道换端口/换部署不会长出第二张卡。被删除的频道不再下发——已下发
 * 的旧卡片保留（用户可以手动 forget），不做反向清除。
 */
export function mergeServiceStations(
  list: readonly LiveRecentEntry[],
  summaries: {
    id: string;
    name: string;
    subtitle: string | null;
    description: string | null;
    coverUrl: string | null;
    hlsUrl: string;
  }[],
): LiveRecentEntry[] {
  const out = [...list];
  for (const st of summaries) {
    const idx = out.findIndex((item) => item.id === st.id);
    if (idx >= 0) {
      const prev = out[idx];
      const merged: LiveRecentEntry = {
        ...prev,
        url: st.hlsUrl, // url 可随部署变，服务端权威
        source: 'service',
      };
      // 只补空白：prev 已有值（用户填过或上轮下发）一律保旧
      const server: Record<string, string | null> = {
        title: st.name,
        subtitle: st.subtitle,
        description: st.description,
        cover: st.coverUrl,
      };
      for (const key of ['title', 'subtitle', 'description', 'cover'] as const) {
        if (!prev[key] && server[key]) merged[key] = server[key] as string;
      }
      out[idx] = merged;
    } else {
      out.push({
        id: st.id,
        url: st.hlsUrl,
        title: st.name || undefined,
        subtitle: st.subtitle ?? undefined,
        description: st.description ?? undefined,
        cover: st.coverUrl ?? undefined,
        source: 'service',
        addedAtMs: Date.now(),
      });
    }
  }
  return out;
}
