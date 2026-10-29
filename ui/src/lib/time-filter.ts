/**
 * 列表筛选器（docs/27 + 2026-10-28 多轮重构）：
 * - 入库时间：**日期区间**（YYYY-MM-DD，本地时区；五轮拍板——固定预设档
 *   换成区间输入，"7 天内"这类滚动窗口用户自己选日期表达不了）。上界含
 *   当天（查询时换算成次日 0 点的**开区间**，与后端 addedToMs 语义对齐）。
 * - 字段条件 `conds`：与动态歌单**同一批键**（genre/artist/album/year/
 *   playCount × eq/contains/gte/lte），但只在本次查询生效、**不落库**——
 *   歌单的规则是持久语义，筛选器是临时意图，共用解析、分道保存。
 *   （筛选 UI 只暴露 genre/artist/album/year；playCount 是歌单规则用键，
 *   2026-10-28 五轮拍板：常听统计已有"最近播放"入口，筛选器不需要它。）
 * - 「上次播放」档位已删（2026-10-28 拍板）：它与曲库页「最近播放」tab
 *   功能重叠；播放记录的保留周期收敛到设置项 `historyDays`，由后端
 *   裁剪 history 行统一表达，筛选器不再自带一级时间轴。
 *
 * 纯函数可表驱动单测。
 */

import type { SmartField, SmartOp } from './smart';

/** 筛选器临时条件（与动态歌单同键；value 文本字段 string、数字字段 number） */
export interface FilterCond {
  field: SmartField;
  op: SmartOp;
  value: string | number;
}

export interface TrackTimeFilter {
  /** 入库时间区间下界（含，当天 0 点起）。null = 不限 */
  addedFrom: string | null;
  /** 入库时间区间上界（**含当天**）。null = 不限 */
  addedTo: string | null;
  conds: FilterCond[];
}

export const DEFAULT_TIME_FILTER: TrackTimeFilter = {
  addedFrom: null,
  addedTo: null,
  conds: [],
};

export function timeFilterActive(f: TrackTimeFilter): boolean {
  return f.addedFrom !== null || f.addedTo !== null || f.conds.length > 0;
}

/** 'YYYY-MM-DD' → 本地时区当天 0 点毫秒；格式非法或日期不存在返回 null */
export function localDayMs(s: string): number | null {
  const m = /^(\d{4})-(\d{2})-(\d{2})$/.exec(s);
  if (!m) return null;
  const d = new Date(Number(m[1]), Number(m[2]) - 1, Number(m[3]));
  // Date 自动进位（2 月 30 日 → 3 月 1 日）：input[type=date] 不会给非法值，
  // 但防御手改 URL/旧数据，进位结果与原文不符就不收
  if (
    d.getFullYear() !== Number(m[1]) ||
    d.getMonth() !== Number(m[2]) - 1 ||
    d.getDate() !== Number(m[3])
  ) {
    return null;
  }
  return d.getTime();
}

/**
 * 筛选器 → 查询入参片段。日期只在两端都合法时才产生时间字段；
 * conds 原样透传（后端用与动态歌单同一套解析，未知键明确报错）。
 */
export function timeFilterToQuery(
  f: TrackTimeFilter,
): Pick<import('./ipc/client').TrackQueryInput, 'addedFromMs' | 'addedToMs' | 'conds'> {
  const out: ReturnType<typeof timeFilterToQuery> = {};
  const from = f.addedFrom ? localDayMs(f.addedFrom) : null;
  if (from !== null) out.addedFromMs = from;
  const to = f.addedTo ? localDayMs(f.addedTo) : null;
  // 上界含当天：+1 天取次日 0 点，配后端开区间语义（addedToMs 是 < 比较）
  if (to !== null) out.addedToMs = to + 86_400_000;
  if (f.conds.length > 0) {
    out.conds = f.conds.map((c) => ({ ...c }));
  }
  return out;
}
