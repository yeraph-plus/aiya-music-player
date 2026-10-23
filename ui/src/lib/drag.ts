/**
 * 行拖拽排序（指针实现）。
 *
 * 为什么不用 svelte-dnd-action：队列与播放列表都是"固定行高 + 虚拟化"，
 * 通用库会把节点真正移动 DOM，虚拟列表下等于把整个滚动窗口重建一次，
 * 500 项以上明显掉帧。这里只做两件事：算目标下标、画一条插入线，
 * 松手时交给主进程改顺序，UI 跟着队列版本号刷新。
 *
 * 纯计算部分（`dropIndex`）单独导出，可单测。
 */

export function dropIndex(from: number, deltaY: number, rowHeight: number, count: number): number {
  if (count <= 0) return -1;
  const steps = Math.round(deltaY / rowHeight);
  return Math.min(Math.max(from + steps, 0), count - 1);
}

export interface RowDragOptions {
  event: PointerEvent;
  index: number;
  count: number;
  rowHeight: number;
  scroller: HTMLElement | null;
  /** 按下后移动超过这个距离才算拖拽，否则仍然是一次点击 */
  threshold?: number;
  onState: (state: { from: number; to: number; active: boolean } | null) => void;
  onDrop: (from: number, to: number) => void;
}

const EDGE = 32;
const SCROLL_STEP = 10;

/**
 * 拖拽激活（移动超过阈值）后松手，浏览器仍会在同一元素上派发一次 click。
 * 这里用短窗口负缓存让行组件能识别并忽略这次"假点击"：
 * finish(true) 且激活过时置位，CLICK_SUPPRESS_MS 内的 click 视为拖拽残留。
 * 下一次 pointerdown（startRowDrag 入口）会重置，不影响之后的正常点击。
 */
const CLICK_SUPPRESS_MS = 300;
let dragClickUntil = 0;

/** 本次 click 是否是拖拽残留；命中即消费（只拦一次） */
export function consumeClickSuppressed(): boolean {
  if (Date.now() >= dragClickUntil) return false;
  dragClickUntil = 0;
  return true;
}

export function startRowDrag(options: RowDragOptions): void {
  const { event, index, count, rowHeight, scroller, onState, onDrop } = options;
  const target = event.target as HTMLElement | null;
  // 行内的按钮（移除、播放）不参与拖拽
  if (target?.closest('button, input, a')) return;
  if (event.button !== 0) return;

  const startY = event.clientY;
  const threshold = options.threshold ?? 6;
  const startScrollTop = scroller?.scrollTop ?? 0;
  dragClickUntil = 0;
  let active = false;
  let to = index;
  let scrollOffset = 0;
  let raf = 0;
  let lastClientY = startY;

  const totalDelta = (): number => lastClientY - startY + scrollOffset;

  const tick = (): void => {
    raf = 0;
    if (!scroller || !active) return;
    const rect = scroller.getBoundingClientRect();
    if (lastClientY < rect.top + EDGE) scroller.scrollTop -= SCROLL_STEP;
    else if (lastClientY > rect.bottom - EDGE) scroller.scrollTop += SCROLL_STEP;
    scrollOffset = scroller.scrollTop - startScrollTop;
  };

  const onMove = (moveEvent: PointerEvent): void => {
    lastClientY = moveEvent.clientY;
    const delta = totalDelta();
    if (!active && Math.abs(delta) < threshold) return;
    if (!active) {
      active = true;
      document.body.classList.add('is-dragging');
    }
    moveEvent.preventDefault();
    to = dropIndex(index, delta, rowHeight, count);
    onState({ from: index, to, active: true });

    if (!raf && scroller) {
      const rect = scroller.getBoundingClientRect();
      if (lastClientY < rect.top + EDGE || lastClientY > rect.bottom - EDGE) {
        raf = requestAnimationFrame(tick);
      }
    }
  };

  const finish = (commit: boolean): void => {
    window.removeEventListener('pointermove', onMove);
    window.removeEventListener('pointerup', onUp);
    window.removeEventListener('pointercancel', onCancel);
    document.body.classList.remove('is-dragging');
    if (raf) cancelAnimationFrame(raf);
    if (commit && active) dragClickUntil = Date.now() + CLICK_SUPPRESS_MS;
    if (commit && active && to !== index) onDrop(index, to);
    onState(null);
  };

  const onUp = (): void => finish(true);
  const onCancel = (): void => finish(false);

  window.addEventListener('pointermove', onMove, { passive: false });
  window.addEventListener('pointerup', onUp);
  window.addEventListener('pointercancel', onCancel);
}
