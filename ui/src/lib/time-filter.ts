/**
 * 列表筛选器的时间档位（docs/27）：**预设档**而非日期选择器——
 * 今天 / 7 天内 / 30 天内 / 今年，覆盖"最近收的歌、最近听的歌"两类高频意图，
 * 无需引入日期控件；后续 beets 标签字段按同样的「档位 → 查询参数」模式扩展。
 *
 * 纯函数可表驱动单测；档位在**查询时**换算成毫秒范围（而不是应用筛选时
 * 一次性快照）——「7 天内」随时间自然滚动，不要求用户重设。
 */

export type AddedPreset = 'any' | 'today' | 'week' | 'month' | 'year';
export type LastPlayedPreset = AddedPreset | 'never';

export interface TrackTimeFilter {
  added: AddedPreset;
  lastPlayed: LastPlayedPreset;
}

export const DEFAULT_TIME_FILTER: TrackTimeFilter = { added: 'any', lastPlayed: 'any' };

/** 设置页分段控件与筛选模态框共用的档位清单（顺序即展示顺序） */
export const ADDED_PRESETS: Array<{ value: AddedPreset; label: string }> = [
  { value: 'any', label: '全部' },
  { value: 'today', label: '今天' },
  { value: 'week', label: '7 天内' },
  { value: 'month', label: '30 天内' },
  { value: 'year', label: '今年' },
];

export const LAST_PLAYED_PRESETS: Array<{ value: LastPlayedPreset; label: string }> = [
  { value: 'any', label: '全部' },
  { value: 'never', label: '从未播放' },
  { value: 'today', label: '今天' },
  { value: 'week', label: '7 天内' },
  { value: 'month', label: '30 天内' },
  { value: 'year', label: '今年' },
];

export function timeFilterActive(f: TrackTimeFilter): boolean {
  return f.added !== 'any' || f.lastPlayed !== 'any';
}

/** 当天 0 点（本地时区） */
function startOfToday(now: Date): number {
  return new Date(now.getFullYear(), now.getMonth(), now.getDate()).getTime();
}

function startOfYear(now: Date): number {
  return new Date(now.getFullYear(), 0, 1).getTime();
}

/** 单个档位 → [fromMs, toMs)；to 省略 = 不设上界（后端 to 是开区间） */
function presetRange(preset: Exclude<LastPlayedPreset, 'any' | 'never'>, now: Date): {
  fromMs: number;
  toMs?: number;
} {
  switch (preset) {
    case 'today':
      // 到明天 0 点：把"今天 23:59 加进来的歌"也算进去（开区间语义）
      return { fromMs: startOfToday(now), toMs: startOfToday(now) + 86_400_000 };
    case 'week':
      return { fromMs: now.getTime() - 7 * 86_400_000 };
    case 'month':
      return { fromMs: now.getTime() - 30 * 86_400_000 };
    case 'year':
      return { fromMs: startOfYear(now) };
  }
}

/**
 * 档位筛选 → 查询入参片段。`lastPlayedNever` 与时间范围互斥，never 优先
 * （后端同规则，双保险）。`any` 档不产生任何字段。
 */
export function timeFilterToQuery(
  f: TrackTimeFilter,
  now: Date = new Date(),
): Pick<
  import('./ipc/client').TrackQueryInput,
  'addedFromMs' | 'addedToMs' | 'lastPlayedFromMs' | 'lastPlayedToMs' | 'lastPlayedNever'
> {
  const out: ReturnType<typeof timeFilterToQuery> = {};
  if (f.added !== 'any') {
    const { fromMs, toMs } = presetRange(f.added, now);
    out.addedFromMs = fromMs;
    if (toMs !== undefined) out.addedToMs = toMs;
  }
  if (f.lastPlayed === 'never') {
    out.lastPlayedNever = true;
  } else if (f.lastPlayed !== 'any') {
    const { fromMs, toMs } = presetRange(f.lastPlayed, now);
    out.lastPlayedFromMs = fromMs;
    if (toMs !== undefined) out.lastPlayedToMs = toMs;
  }
  return out;
}
