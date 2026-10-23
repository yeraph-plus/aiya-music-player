/**
 * 多选模型。
 *
 * 只存"视图下标"，不存 ID —— 列表是虚拟化的，能拿到 ID 的只有已加载的
 * 那一小段；批量操作时把下标交给主进程换算，全选 5 万首也不需要把整库拉下来。
 *
 * 上限与主进程的 MAX_BATCH 对齐：两边不一致时，界面能选出一个"看起来合法、
 * 一操作就报错"的选择集 —— 那比直接不让选更糟。
 */

export type Selection = ReadonlySet<number>;

/** 一次批量操作的上限，与 apps/desktop/src/main/ipc.ts 的 MAX_BATCH 一致 */
export const SELECTION_CAP = 5000;

export const EMPTY_SELECTION: Selection = new Set<number>();

export function toggleSelection(selection: Selection, index: number): Set<number> {
  const next = new Set(selection);
  if (next.has(index)) next.delete(index);
  else next.add(index);
  return next;
}

/**
 * Shift 连选：含两端，方向无关。
 *
 * 超过上限时保留"靠近刚点的那一行"的一段，而不是从锚点开始截断 ——
 * 用户拖着 Shift 往下点，想选的显然是刚点到的位置和它上方的一段。
 */
export function rangeSelection(
  anchor: number,
  index: number,
  cap = SELECTION_CAP,
): Set<number> {
  const next = new Set<number>();
  if (anchor < 0 || index < 0) {
    if (index >= 0) next.add(index);
    return next;
  }

  const start = Math.min(anchor, index);
  const end = Math.max(anchor, index);
  if (end - start + 1 > cap) {
    const from = index >= anchor ? index - cap + 1 : index;
    const to = index >= anchor ? index : index + cap - 1;
    for (let i = from; i <= to; i += 1) next.add(i);
    return next;
  }

  for (let i = start; i <= end; i += 1) next.add(i);
  return next;
}

/** 全选：上限保护，避免 10 万行的视图把数组撑爆 */
export function selectAll(total: number, cap = SELECTION_CAP): Set<number> {
  const next = new Set<number>();
  const limit = Math.min(Math.max(0, total), cap);
  for (let i = 0; i < limit; i += 1) next.add(i);
  return next;
}
