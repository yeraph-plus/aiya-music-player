import { z } from 'zod';

export const trackSortSchema = z.enum(['title', 'added', 'artist', 'album']);
export type TrackSort = z.infer<typeof trackSortSchema>;

export const sortDirectionSchema = z.enum(['asc', 'desc']);
export type SortDirection = z.infer<typeof sortDirectionSchema>;

export const MAX_PAGE_SIZE = 500;

/**
 * 列表的数据来源。
 *
 * 播放列表、最近播放、常听不是"另一种筛选"，而是另一种排序与连接方式：
 * 它们的顺序由 playlist_items.position / play_history 决定，跟标题、艺人无关。
 * 把它们塞进 `sort` 会让排序表达式变成一堆特例，所以单独建模，
 * 但共用同一套分页协议 —— 渲染层的 TrackWindow 不需要区分自己看的是什么。
 */
export const trackSourceSchema = z.discriminatedUnion('kind', [
  z.object({ kind: z.literal('library') }),
  z.object({ kind: z.literal('playlist'), playlistId: z.string().min(1).max(64) }),
  z.object({ kind: z.literal('recent') }),
  z.object({ kind: z.literal('mostPlayed') }),
]);

export type TrackSource = z.infer<typeof trackSourceSchema>;

const baseQuery = z.object({
  sort: trackSortSchema.default('title'),
  direction: sortDirectionSchema.default('asc'),
  /** 搜索词，最长 200，避免把超长串带进 LIKE */
  search: z.string().trim().max(200).optional(),
  artistId: z.string().max(64).optional(),
  albumId: z.string().max(64).optional(),
  source: trackSourceSchema.default({ kind: 'library' }),
  limit: z.number().int().min(1).max(MAX_PAGE_SIZE).default(100),
});

/**
 * 双模式查询：
 * - cursor：默认，只能向后翻页，O(1)
 * - offset：仅用于恢复滚动位置/拖动滚动条，必须由物化位置表支撑
 */
export const trackQuerySchema = z.discriminatedUnion('mode', [
  baseQuery.extend({
    mode: z.literal('cursor'),
    cursor: z.string().max(512).optional(),
  }),
  baseQuery.extend({
    mode: z.literal('offset'),
    offset: z.number().int().min(0),
  }),
]);

export type TrackQuery = z.input<typeof trackQuerySchema>;
export type TrackQueryParsed = z.output<typeof trackQuerySchema>;

/** 给缺省 mode 的调用方补齐默认值后再校验 */
export function parseTrackQuery(raw: unknown): TrackQueryParsed {
  const input =
    raw && typeof raw === 'object' && !('mode' in (raw as Record<string, unknown>))
      ? { ...(raw as Record<string, unknown>), mode: 'cursor' as const }
      : raw;
  return trackQuerySchema.parse(input);
}

/**
 * 游标 = base64url(JSON([sortValue, id]))。
 * 只编码排序键与 id，不含 offset，所以不会随插入而漂移。
 */
export type CursorPayload = [string | number | null, string];

/* WebView 里没有 Buffer；用 TextEncoder/atob 实现等价的 base64url */
function utf8ToBase64Url(text: string): string {
  const bytes = new TextEncoder().encode(text);
  let binary = '';
  for (const b of bytes) binary += String.fromCharCode(b);
  return btoa(binary).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');
}

function base64UrlToUtf8(text: string): string {
  const padded = text.replace(/-/g, '+').replace(/_/g, '/');
  const binary = atob(padded + '='.repeat((4 - (padded.length % 4)) % 4));
  const bytes = Uint8Array.from(binary, (c) => c.charCodeAt(0));
  return new TextDecoder().decode(bytes);
}

export function encodeCursor(payload: CursorPayload): string {
  return utf8ToBase64Url(JSON.stringify(payload));
}

export function decodeCursor(cursor: string): CursorPayload {
  let parsed: unknown;
  try {
    parsed = JSON.parse(base64UrlToUtf8(cursor));
  } catch {
    throw new Error('E_INVALID_CURSOR');
  }
  if (!Array.isArray(parsed) || parsed.length !== 2 || typeof parsed[1] !== 'string') {
    throw new Error('E_INVALID_CURSOR');
  }
  const [value, id] = parsed as [unknown, unknown];
  if (typeof id !== 'string') throw new Error('E_INVALID_CURSOR');
  if (value !== null && typeof value !== 'string' && typeof value !== 'number') {
    throw new Error('E_INVALID_CURSOR');
  }
  return [value as string | number | null, id];
}
