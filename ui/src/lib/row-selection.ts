import { rangeSelection, SELECTION_CAP } from './selection';

/**
 * 行选择的**公共语义**层（B4-1 定稿，B4-2 抽出）。
 *
 * 两个实现共用这里的分流与上限规则：
 * - **下标版**（主列表/歌单）：`stores/selection.svelte.ts` + `lib/selection.ts`
 *   纯函数 —— 选择集存视图下标，批量操作时由主进程换算 ID（虚拟列表拿不到
 *   全量 ID，ADR-020）。
 * - **ID 版**（首页紧凑表）：本文件的 `applyIdSelection` —— 选择集直接存
 *   曲目 ID（三段各自是独立查询，下标跨段无意义）。
 *
 * 语义一致是硬要求：两套表格上"点行/点勾选框"的手感必须相同，否则用户在
 * 首页学会的习惯到曲库就失效（反之亦然）。
 */

/** `MouseEvent` / `KeyboardEvent` 里与选择有关的公共形状 */
export interface Mods {
  shiftKey: boolean;
  ctrlKey: boolean;
  metaKey: boolean;
}

export type RowIntent =
  /** 裸点：只选这一行，清掉别的 */
  | 'single'
  /** Ctrl/⌘：把这一行加进/移出选择集 */
  | 'toggle'
  /** Shift：从锚点连选到这一行（含两端） */
  | 'range';

/**
 * 点行的意图分流（B4-1 交互语义）。
 *
 * 注意 Shift 在**没有锚点**时的处理由存储侧决定：下标版回退到键盘焦点行
 * （`selection.anchor < 0 ? selectedIndex : anchor`），ID 版回退成 toggle
 * （紧凑表没有键盘焦点概念）。这里只分类，不猜锚点。
 */
export function classifyRowClick(mods: Mods): RowIntent {
  if (mods.shiftKey) return 'range';
  if (mods.ctrlKey || mods.metaKey) return 'toggle';
  return 'single';
}

/**
 * 点勾选框的意图分流：**Shift=连选，其余一律切换累加**。
 *
 * 与点行的差别在裸点：勾选框的常规预期是逐条累加，清空请用表头全选或
 * 批量条的 ✕。混在一起的症状是"勾第二首时第一首自己掉了"。
 */
export function classifyRowCheck(mods: Mods): RowIntent {
  return mods.shiftKey ? 'range' : 'toggle';
}

export interface IdSelectionResult {
  /** 应用意图后的新选择集（直接交给受控 `onchange`） */
  ids: Set<string>;
  /** 新锚点（下标，仅本表内有意义） */
  anchor: number;
  /** 触发上限截断（调用方据此提示） */
  capped: boolean;
}

/**
 * 在 **ID 选择集**上应用一个行意图。
 *
 * `anchor < 0` 时的 'range' 回退成 'toggle'：紧凑表没有键盘焦点行，
 * Shift 裸点没有起点，按"用户其实想勾这一条"处理（与点勾选框的
 * Shift 无锚点行为一致）。
 *
 * 连选走 `rangeSelection`（下标区间）再映射成 ID：上限截断语义与下标版
 * 完全同源 —— 超限时保留"靠近刚点的那一行"的一段，不是从锚点截断。
 */
export function applyIdSelection(
  selected: ReadonlySet<string>,
  rows: ReadonlyArray<{ id: string }>,
  anchor: number,
  intent: RowIntent,
  index: number,
  cap = SELECTION_CAP,
): IdSelectionResult {
  const idAt = (i: number): string | undefined => rows[i]?.id;

  if (intent === 'range' && anchor >= 0) {
    const span = Math.abs(index - anchor) + 1;
    const out = new Set<string>();
    for (const i of rangeSelection(anchor, index, cap)) {
      const id = idAt(i);
      if (id) out.add(id);
    }
    return { ids: out, anchor, capped: span > cap };
  }

  const id = idAt(index);
  if (!id) return { ids: new Set(selected), anchor, capped: false };

  if (intent === 'toggle') {
    const out = new Set(selected);
    if (out.has(id)) out.delete(id);
    else out.add(id);
    return { ids: out, anchor: index, capped: false };
  }

  // 'single'，以及 range 无锚点的回退
  return { ids: new Set([id]), anchor: index, capped: false };
}
