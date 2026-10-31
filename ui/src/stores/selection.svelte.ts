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
  /**
   * 「把 DOM 焦点交给焦点行」的请求计数（发布前 UI 审查 P0-1）。
   *
   * 只有键盘 move 意图（keyboard.ts）会调 `requestRowFocus()` 置位：方向键
   * 移动的不只是高亮 class，DOM 焦点必须跟过去——否则 Space 播的是焦点
   * 所在的旧行、Enter 播的是高亮的新行。点选、"定位到正在播放"等编程路径
   * **不**置位（点选时浏览器已把焦点给到行上；定位不该抢用户焦点）。
   * 行组件盯着这个计数执行 focus；store 字段，重挂载不清零。
   */
  focusNonce = $state(0);

  requestRowFocus(): void {
    this.focusNonce++;
  }

  /**
   * 列表最近一次「跟随滚动」到过的焦点下标。普通字段即可：只做闸门，不进渲染。
   *
   * 为什么必须有：列表组件的「焦点行滚进视野」effect 会在**组件重挂载、
   * 视口尺寸变化（抽屉开合/窗口缩放）、行高变化**时重跑，而不只是焦点变化。
   * 双击播放后 `clear()` 只清选择集、`selectedIndex` 停在播放行上——没有这个
   * 闸，任何一次重跑都会把用户正在浏览的位置拽回正在播放的行（实测：
   * 播放中滚动/切页/开关队列抽屉，列表反复跳回播放条目）。
   * 闸门存 store 里而不是组件局部变量，因为重挂载会把局部变量清零、闸就失效了。
   */
  followedIndex = -1;

  get size(): number {
    return this.selection.size;
  }

  /**
   * 点行 / Ctrl / Shift 点行统一入口（B4-1 交互语义）。
   *
   * 裸点=只选这一行（清掉别的）、Ctrl=切换、Shift=连选；**裸点不再播放** ——
   * 播放归双击 / hover 播放键 / Enter。与 `handleCheck` 分开是因为语义不同：
   * 点行是"把注意力给这一行"（裸点清掉别的），勾选框是"往集合里加减"。
   *
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

  /**
   * 点勾选框：**不加修饰键也是"切换"**，不是"只选这一行"。
   *
   * 勾选框的常规预期是逐条累加，清空请用表头或批量条的 ✕。两套混在一起
   * 的症状是"勾第二首时第一首自己掉了"。
   */
  handleCheck(index: number, event: MouseEvent | KeyboardEvent): 'capped' | 'ok' {
    if (event.shiftKey) {
      const start = this.anchor < 0 ? this.selectedIndex : this.anchor;
      const span = Math.abs(index - start) + 1;
      this.selection = rangeSelection(start, index);
      this.selectedIndex = index;
      return span > SELECTION_CAP ? 'capped' : 'ok';
    }
    this.selection = toggleSelection(this.selection, index);
    this.anchor = index;
    this.selectedIndex = index;
    return 'ok';
  }

  /** 表头三态：已全选（或已达上限）就清空，否则全选 */
  toggleAll(total: number): void {
    const cap = Math.min(Math.max(0, total), SELECTION_CAP);
    if (cap > 0 && this.selection.size >= cap) this.clear();
    else this.selectAll(total);
  }

  /** 升序下标数组；主进程按同一查询换算成 ID */
  sortedIndices(): number[] {
    return [...this.selection].sort((a, b) => a - b);
  }
}

export const selection = new SelectionStore();
