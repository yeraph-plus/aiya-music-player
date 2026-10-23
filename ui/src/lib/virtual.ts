/** 固定行高虚拟列表的区间计算。抽成纯函数，这样滚动数学可以单测。 */
export interface VirtualRange {
  /** 包含 */
  start: number;
  /** 不包含 */
  end: number;
  padTop: number;
  padBottom: number;
}

export interface RangeInput {
  scrollTop: number;
  viewportHeight: number;
  rowHeight: number;
  total: number;
  overscan?: number;
}

export function computeRange(input: RangeInput): VirtualRange {
  const { scrollTop, viewportHeight, rowHeight, total } = input;
  const overscan = input.overscan ?? 6;

  if (total <= 0 || rowHeight <= 0) {
    return { start: 0, end: 0, padTop: 0, padBottom: 0 };
  }

  const top = Math.max(0, scrollTop);
  const firstVisible = Math.floor(top / rowHeight);
  const visibleCount = Math.ceil(Math.max(0, viewportHeight) / rowHeight);

  const start = clamp(firstVisible - overscan, 0, total);
  const end = clamp(firstVisible + visibleCount + overscan, start, total);

  return {
    start,
    end,
    padTop: start * rowHeight,
    padBottom: Math.max(0, (total - end) * rowHeight),
  };
}

/** 让第 index 行进入视口（键盘上下选择时用），返回目标 scrollTop */
export function scrollTopForRow(
  index: number,
  rowHeight: number,
  viewportHeight: number,
  scrollTop: number,
): number {
  if (index < 0) return scrollTop;
  const rowTop = index * rowHeight;
  const rowBottom = rowTop + rowHeight;
  if (rowTop < scrollTop) return rowTop;
  if (rowBottom > scrollTop + viewportHeight) return rowBottom - viewportHeight;
  return scrollTop;
}

function clamp(value: number, min: number, max: number): number {
  return Math.min(Math.max(value, min), max);
}
