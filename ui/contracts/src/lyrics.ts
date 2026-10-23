import type { LyricLine } from './domain.js';

/**
 * 当前应该高亮的歌词行。
 *
 * 放在 contracts 里是因为渲染层每 500ms 调一次，而它只依赖 LyricLine；
 * 让渲染层为了这个函数去依赖整个解析包没有意义。
 * 二分查找：一首歌几百行，线性扫描也不慢，但没必要。
 */
export function findActiveLine(lines: readonly LyricLine[], positionMs: number): number {
  if (lines.length === 0) return -1;
  let low = 0;
  let high = lines.length - 1;
  let found = -1;
  while (low <= high) {
    const mid = (low + high) >> 1;
    const line = lines[mid];
    if (!line) break;
    if (line.timeMs <= positionMs) {
      found = mid;
      low = mid + 1;
    } else {
      high = mid - 1;
    }
  }
  return found;
}
