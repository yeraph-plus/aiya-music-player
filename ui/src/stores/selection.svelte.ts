import { rangeSelection, selectAll, toggleSelection, SELECTION_CAP } from '../lib/selection';

/**
 * 列表选择 store：Shift/Ctrl/Ctrl+Shift 的选择集、锚点、焦点行。
 *
 * 选择集是"视图下标"而非曲目 ID（ADR-020）：跨排序/筛选的语义由
 * 调用方（App）在把下标换成 ID 时处理。数量上限的提示通过返回值
 * 交给调用方展示，store 不持 UI 文案。
 */
class SelectionStore {
  selection = $state<Set<number>>(new Set());
  /** Shift 连选的锚点 */
  anchor = $state(-1);
  /** 当前焦点行（键盘上下移动） */
  selectedIndex = $state(-1);

  get size(): number {
    return this.selection.size;
  }

  /**
   * @returns 'capped' 表示触发单次上限、已截取（调用方据此提示）
   */
  handleSelect(index: number, event: MouseEvent | KeyboardEvent): 'capped' | 'ok' {
    if (event.shiftKey) {
      const start = this.anchor < 0 ? this.selectedIndex : this.anchor;
      const span = Math.abs(index - start) + 1;
      this.selection = rangeSelection(start, index);
      if (span > SELECTION_CAP) {
        this.selectedIndex = index;
        return 'capped';
      }
    } else if (event.ctrlKey || event.metaKey) {
      this.selection = toggleSelection(this.selection, index);
      this.anchor = index;
    } else {
      this.selection = new Set([index]);
      this.anchor = index;
    }
    this.selectedIndex = index;
    return 'ok';
  }

  clear(): void {
    this.selection = new Set();
    this.anchor = -1;
  }

  selectAll(total: number): void {
    this.selection = selectAll(total);
    this.anchor = 0;
  }

  /** 升序下标数组；主进程按同一查询换算成 ID */
  sortedIndices(): number[] {
    return [...this.selection].sort((a, b) => a - b);
  }
}

export const selection = new SelectionStore();
