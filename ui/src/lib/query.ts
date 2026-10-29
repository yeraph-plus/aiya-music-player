// 同 ipc/schema.ts：先关 JIT，再构造 schema
import './zod-setup';
import { z } from 'zod';
import type { TrackQueryInput } from './ipc/client';

/**
 * 列表查询：排序、数据来源、分页协议（游标 / offset 双模式）。
 *
 * 2026-10-25 批次 4：从已删除的 `@aiya/contracts` 搬来。
 *
 * 为什么还留 zod：`TrackQuery` 是**命令入参**，Rust 侧收 `Json` 再手动解析
 * （docs/17 §3.2 的取舍），所以没有生成类型可依赖。这里的 schema 既做
 * 前端解析（补齐默认值、拒绝非法 mode），也是"入参形状"的唯一文字来源。
 * 返回值不走 zod —— 那是 `generated/` 的事。
 */

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
  /** 时间筛选（筛选器模态框，lib/time-filter.ts 换算）：毫秒，to 为开区间 */
  addedFromMs: z.number().int().nonnegative().optional(),
  addedToMs: z.number().int().nonnegative().optional(),
  /** 筛选器临时条件（与动态歌单同键，不落库）；旧 lastPlayed* 档已移除 */
  conds: z
    .array(
      z.object({
        field: z.enum(['genre', 'artist', 'album', 'year', 'playCount']),
        op: z.enum(['eq', 'contains', 'gte', 'lte']),
        value: z.union([z.string(), z.number()]),
      }),
    )
    .optional(),
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

/**
 * `TrackQuery` = 命令入参形状（`client.ts` 的 `TrackQueryInput`）。
 *
 * 不复用 `z.input<>`：那个推断在 `source` / `mode` 上会带出 zod 的
 * 可选化细节，而 `ipc.library.list()` 收的是手写结构类型。两者同构，
 * 但以 `client.ts` 为准可以避免"类型看着一样、赋值报错"的噪音。
 */
export type TrackQuery = TrackQueryInput;
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
