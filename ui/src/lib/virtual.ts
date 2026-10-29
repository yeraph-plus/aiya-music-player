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

/**
 * 让第 index 行进入视口（键盘上下选择时用），返回目标 scrollTop。
 *
 * `contentOffset` 是滚动内容里**行区之前的占位高度**（吸顶表头是 scroller
 * 内的流内元素、列表容器自己的 padding-top 也算）：不传的话行顶会被
 * 表头盖住一行的高度。
 */
export function scrollTopForRow(
  index: number,
  rowHeight: number,
  viewportHeight: number,
  scrollTop: number,
  contentOffset = 0,
): number {
  if (index < 0) return scrollTop;
  const rowTop = contentOffset + index * rowHeight;
  const rowBottom = rowTop + rowHeight;
  if (rowTop < scrollTop) return rowTop;
  if (rowBottom > scrollTop + viewportHeight) return rowBottom - viewportHeight;
  return scrollTop;
}

/**
 * 把第 index 行滚到视口中部（「定位到正在播放」按钮用）。
 *
 * 与 `scrollTopForRow` 的差别：那个是跟随语义——行已在视野内就不动、
 * 越界才拉回边缘；这个是跳转语义——目标通常在视野外很远，居中放置
 * 给用户上下文（能看到目标行前后各几行）。结果钳在合法滚动范围内。
 */
export function centeredScrollTopForRow(
  index: number,
  rowHeight: number,
  viewportHeight: number,
  total: number,
  contentOffset = 0,
): number {
  if (index < 0 || total <= 0 || rowHeight <= 0) return 0;
  const rowTop = contentOffset + index * rowHeight;
  const maxScroll = Math.max(0, contentOffset + total * rowHeight - viewportHeight);
  return clamp(rowTop + rowHeight / 2 - viewportHeight / 2, 0, maxScroll);
}

function clamp(value: number, min: number, max: number): number {
  return Math.min(Math.max(value, min), max);
}
