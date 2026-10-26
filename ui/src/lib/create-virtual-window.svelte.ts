import { computeRange, type VirtualRange } from './virtual';

/**
 * 固定行高虚拟列表的滚动水管：scroller 绑定、视口测量、可见区间推导。
 *
 * `TrackList`（曲库/歌单，B4-1 前后都是主力列表）与 `QueuePanel`（播放队列）
 * 原来各抄了一遍 ResizeObserver effect / scrollTop 状态 / pad 计算 —— 逐字
 * 相同的 30 行在两处漂移，改一处漏一处只是时间问题。区间数学本身在
 * `lib/virtual.ts`（纯函数、可单测），这里只负责把 DOM 测量接上去。
 *
 * **用法**（`.svelte.ts` 里的 runes 在组件初始化时调用才合法）：
 *
 * ```svelte
 * <script lang="ts">
 *   const vw = createVirtualWindow({
 *     rowHeight: () => ROW_HEIGHT, // getter：行高随密度设置变化
 *     total: () => ids.length,
 *   });
 * </script>
 * <div bind:this={vw.scroller} onscroll={vw.handleScroll}>
 *   <div style="height:{vw.range.padTop}px"></div>
 *   …
 * ```
 *
 * 不提供 `onScroll` 回调选项：回调是 prop 时在创建时捕获会拿到**首次的
 * 函数实例**（Svelte 模板内联箭头每次渲染都是新的），行为对但来源隐蔽。
 * 需要链式处理的组件在自己的 `onscroll` 里先调 `vw.handleScroll()` 再取
 * `vw.scrollTop`，依赖关系显式。
 *
 * **引用语义**：`scroller` / `scrollTop` 用显式 getter/setter 暴露，而不是
 * 把 DOM 节点塞进 `$state` 代理 —— 代理包装 DOM 节点没有意义还会干扰
 * `bind:this` 的赋值路径。`bind:this={vw.scroller}` 走 setter 写回。
 */
export interface VirtualWindowOptions {
  /** 行高。传 getter：行高可能随密度设置在运行时变化 */
  rowHeight: () => number;
  /** 行数（响应式读取） */
  total: () => number;
  /** 视口外多渲染几行，滚动时补页的缓冲 */
  overscan?: number;
  /** 视口测量就位前的兜底高度（QueuePanel 抽屉 ≈520，主列表 ≈640） */
  fallbackViewport?: number;
}

export interface VirtualWindow {
  /** 绑定滚动容器：`bind:this={vw.scroller}` */
  scroller: HTMLElement | null;
  /** 当前滚动位置（与 DOM 同步；外部重置位置时写这里保持状态一致） */
  scrollTop: number;
  /** 测量到的视口高度（滚动定位数学要用） */
  readonly viewportHeight: number;
  /** 可见区间 + 上下补位（模板直接用） */
  readonly range: VirtualRange;
  /** 挂到容器的 `onscroll` 上 */
  handleScroll(): void;
}

export function createVirtualWindow(options: VirtualWindowOptions): VirtualWindow {
  const fallback = options.fallbackViewport ?? 520;
  let scroller = $state<HTMLElement | null>(null);
  let viewportHeight = $state(fallback);
  let scrollTop = $state(0);

  const rowHeight = $derived(options.rowHeight());
  const range = $derived(
    computeRange({
      scrollTop,
      viewportHeight,
      rowHeight,
      total: options.total(),
      overscan: options.overscan,
    }),
  );

  // 视口尺寸随容器变化（窗口缩放、抽屉开合、分栏拖动都会改高度）。
  // 依赖 scroller：重新绑定时换观察目标。
  $effect(() => {
    if (!scroller) return;
    const observer = new ResizeObserver(() => {
      viewportHeight = scroller?.clientHeight ?? fallback;
    });
    observer.observe(scroller);
    viewportHeight = scroller.clientHeight;
    return () => observer.disconnect();
  });

  return {
    get scroller(): HTMLElement | null {
      return scroller;
    },
    set scroller(value: HTMLElement | null) {
      scroller = value;
    },
    get scrollTop(): number {
      return scrollTop;
    },
    set scrollTop(value: number) {
      scrollTop = value;
    },
    get viewportHeight(): number {
      return viewportHeight;
    },
    get range(): VirtualRange {
      return range;
    },
    handleScroll(): void {
      if (!scroller) return;
      scrollTop = scroller.scrollTop;
    },
  };
}
