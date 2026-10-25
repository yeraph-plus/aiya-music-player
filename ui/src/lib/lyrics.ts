import type { LyricLine } from './ipc/generated/LyricLine';

/**
 * 当前应该高亮的歌词行。
 *
 * 渲染层每 500ms 调一次，只依赖 `LyricLine`；二分查找：一首歌几百行，
 * 线性扫描也不慢，但没必要。
 *
 * 2026-10-25 批次 4：从已删除的 `@aiya/contracts` 搬来，
 * `LyricLine` 换成 Rust 生成的同名类型（字段一致：timeMs / text）。
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
