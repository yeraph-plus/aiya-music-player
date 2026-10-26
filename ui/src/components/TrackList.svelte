<script lang="ts">
  import { Play } from '@lucide/svelte';
  import type { TrackRow } from '../lib/ipc';
  import type { TrackSort } from '../lib/query';
  import { artworkUrl } from '../lib/artwork';
  import { startRowDrag, consumeClickSuppressed } from '../lib/drag';
  import { createVirtualWindow } from '../lib/create-virtual-window.svelte';
  import { scrollTopForRow } from '../lib/virtual';
  import CheckCell from './ui/CheckCell.svelte';
  import { formatDay, formatTime, highlight } from '../lib/format';
  import type { TrackWindow } from '../lib/track-window';
  import { SELECTION_CAP } from '../lib/selection';

  interface Props {
    store: TrackWindow;
    /** store 内部变更时递增，用来驱动细粒度刷新 */
    version: number;
    total: number;
    query: string;
    sort: TrackSort;
    direction: 'asc' | 'desc';
    canSort: boolean;
    /** 最近/常听视图多显示一列"次数" */
    showPlays: boolean;
    onsort: (field: TrackSort) => void;
    currentId: string | null;
    selectedIndex: number;
    selection: ReadonlySet<number>;
    /** 一次性恢复位置；应用后由父组件置空 */
    restoreScrollTop: number | null;
    playing: boolean;
    /**
     * 播放。**只由双击 / hover 播放键 / 键盘 Enter 触发**（B4-1 交互语义，
     * docs/24 §6）：单击的职责是选中，与"点一下就切歌"的旧行为区分 ——
     * 单击播放时用户没法先"看一眼再操作"，也和多选模型打架。
     */
    onplay: (index: number) => void;
    /**
     * 选中。裸点、Ctrl、Shift **都走这里**（语义差别在
     * `selection.handleSelect` 内部分流）：裸点=只选这行、Ctrl=切换、
     * Shift=连选。
     */
    onselect: (index: number, event: MouseEvent | KeyboardEvent) => void;
    /** 勾选框：点=切换、Shift=连选（与 ctrl+点击的语义不同）；键盘 Space/Enter 也走这里 */
    oncheck: (index: number, event: MouseEvent | KeyboardEvent) => void;
    /** 表头三态全选 */
    oncheckall: () => void;
    onmenu: (event: MouseEvent, index: number, track: TrackRow | null) => void;
    onscroll: (scrollTop: number, firstVisible: number) => void;
    onrestored: () => void;
    onretry: (index: number) => void;
    /** 传入即开启拖拽排序（播放列表视图） */
    onmove?: (from: number, to: number) => void;
    /** 行高由密度设置决定；虚拟化与 CSS 必须用同一个值 */
    rowHeight?: number;
  }

  let {
    store,
    version,
    total,
    query,
    sort,
    direction,
    canSort,
    showPlays,
    onsort,
    currentId,
    selectedIndex,
    selection,
    restoreScrollTop,
    playing,
    onplay,
    onselect,
    oncheck,
    oncheckall,
    onmenu,
    onscroll,
    onrestored,
    onretry,
    onmove,
    rowHeight = 52,
  }: Props = $props();

  const ROW_HEIGHT = $derived(rowHeight);
  const OVERSCAN = 6;

  /**
   * 滚动水管抽到 `lib/create-virtual-window`（与 QueuePanel 共用）：
   * scroller 绑定、视口测量、可见区间都在那里，这里只接业务 effect。
   */
  const vw = createVirtualWindow({
    rowHeight: () => ROW_HEIGHT,
    total: () => total,
    overscan: OVERSCAN,
    fallbackViewport: 640,
  });

  /**
   * 表头三态：全选的上限与 `lib/selection.ts` 的 SELECTION_CAP 一致，
   * 否则 5 万首的视图点全选后表头还显示"半选"（只选了前 5000）。
   */
  const selectableTotal = $derived(Math.min(total, SELECTION_CAP));
  const allSelected = $derived(selectableTotal > 0 && selection.size >= selectableTotal);
  const someSelected = $derived(selection.size > 0 && !allSelected);

  let dragState = $state<{ from: number; to: number; active: boolean } | null>(null);

  const range = $derived(vw.range);

  const rows = $derived.by(() => {
    // 读一下版本号，让 store 更新时这段重新计算
    void version;
    const out: Array<{ index: number; track: TrackRow | undefined; error: string | null }> = [];
    for (let i = range.start; i < range.end; i += 1) {
      out.push({ index: i, track: store.get(i), error: store.errorAt(i) });
    }
    return out;
  });

  const dropLineTop = $derived(
    dragState && dragState.active ? range.padTop + dragState.to * ROW_HEIGHT : -1,
  );

  // 可见区间变了就补页。放在 effect 里而不是渲染函数里，
  // 避免在渲染过程中触发异步状态写入。
  $effect(() => {
    if (total > 0) store.ensureRange(range.start, range.end);
  });

  // 恢复滚动位置：等 total 就位后只执行一次（游标恢复时 total 刚就绪）
  $effect(() => {
    if (restoreScrollTop === null || !vw.scroller || total === 0) return;
    vw.scroller.scrollTop = restoreScrollTop;
    vw.scrollTop = vw.scroller.scrollTop;
    onrestored();
  });

  // 键盘选中项变化时把它滚进视野
  $effect(() => {
    if (!vw.scroller || selectedIndex < 0) return;
    const target = scrollTopForRow(selectedIndex, ROW_HEIGHT, vw.viewportHeight, vw.scroller.scrollTop);
    if (target !== vw.scroller.scrollTop) vw.scroller.scrollTop = target;
  });

  function handlePointerDown(event: PointerEvent, index: number): void {
    if (!onmove) return;
    startRowDrag({
      event,
      index,
      count: total,
      rowHeight,
      scroller: vw.scroller,
      onState: (next) => (dragState = next),
      onDrop: (from, to) => onmove?.(from, to),
    });
  }
</script>

<div
  class="list"
  class:with-plays={showPlays}
  class:dragging={dragState?.active}
  bind:this={vw.scroller}
  onscroll={() => {
    // 先让水管同步 scrollTop，再上报（沿用 B4-1 前的回调形状）
    vw.handleScroll();
    onscroll(vw.scrollTop, Math.floor(vw.scrollTop / ROW_HEIGHT));
  }}
  role="grid"
  aria-rowcount={total}
>
  <div class="grid-head" role="row">
    <CheckCell
      role="columnheader"
      checked={allSelected}
      indeterminate={someSelected}
      label={allSelected ? '取消全选' : '全选'}
      disabled={total === 0}
      ontoggle={() => oncheckall()}
    />
    <span class="idx">#</span>
    <span></span>
    <button class="sort-head" class:on={sort === 'title'} onclick={() => canSort && onsort('title')} disabled={!canSort}>
      标题{sort === 'title' && canSort ? (direction === 'asc' ? ' ↑' : ' ↓') : ''}
    </button>
    <button class="sort-head" class:on={sort === 'artist'} onclick={() => canSort && onsort('artist')} disabled={!canSort}>
      艺人{sort === 'artist' && canSort ? (direction === 'asc' ? ' ↑' : ' ↓') : ''}
    </button>
    <button class="sort-head" class:on={sort === 'album'} onclick={() => canSort && onsort('album')} disabled={!canSort}>
      专辑{sort === 'album' && canSort ? (direction === 'asc' ? ' ↑' : ' ↓') : ''}
    </button>
    <button class="sort-head" class:on={sort === 'added'} onclick={() => canSort && onsort('added')} disabled={!canSort}>
      添加{sort === 'added' && canSort ? (direction === 'asc' ? ' ↑' : ' ↓') : ''}
    </button>
    {#if showPlays}<span class="num">次数</span>{/if}
    <span class="num">时长</span>
  </div>

  {#if total > 0}
    <div style={`height:${range.padTop}px`} aria-hidden="true"></div>
    {#each rows as row (row.index)}
      {#if row.track}
        <div
          class="row"
          class:active={row.track.id === currentId}
          class:selected={selection.has(row.index)}
          class:focus={row.index === selectedIndex}
          class:drag-source={dragState?.active && dragState.from === row.index}
          style={`height:${rowHeight}px`}
          role="row"
          tabindex="0"
          onpointerdown={(event) => handlePointerDown(event, row.index)}
          onclick={(event) => {
            // 拖拽激活后松手会派发一次残留 click，吞掉避免误选中
            if (consumeClickSuppressed()) {
              event.stopPropagation();
              return;
            }
            // 勾选列与 hover 播放键里的点击不是"选中这一行"。**这个判断不能省**：
            // Svelte 5 把 click 委托到根节点统一派发，`stopPropagation` 拦不住
            // 同一批委托 handler，所以由行自己认出点击来源才是可靠的那道闸。
            if ((event.target as HTMLElement | null)?.closest('.k-check, .play-btn')) return;
            // 裸点/Ctrl/Shift 全部交给 onselect，语义在 store 里分流
            onselect(row.index, event);
          }}
          ondblclick={(event) => {
            // 双击 = 播放。前面的两次单击各自选中过一次（幂等），无害。
            if ((event.target as HTMLElement | null)?.closest('.k-check, .play-btn')) return;
            onplay(row.index);
          }}
          onkeydown={(event) => {
            // 焦点在行内的按钮（勾选框 / 播放键）上时，Enter/Space 归按钮自己：
            // 行级 preventDefault 会把按钮的原生激活吃掉，变成"按空格勾选
            // 框却切了歌"。
            if ((event.target as HTMLElement | null)?.closest('button')) return;
            // 键盘与双击同语义：Enter/Space 播放
            if (event.key === 'Enter' || event.key === ' ') {
              event.preventDefault();
              onplay(row.index);
            }
          }}
          oncontextmenu={(event) => {
            event.preventDefault();
            onselect(row.index, event);
            onmenu(event, row.index, row.track ?? null);
          }}
        >
          <CheckCell
            checked={selection.has(row.index)}
            label={`选择第 ${row.index + 1} 行`}
            ontoggle={(event) => oncheck(row.index, event)}
          />
          <span class="idx">
            {#if row.track.id === currentId}
              <span class="bars" class:animated={playing} aria-label="正在播放">
                <i></i><i></i><i></i>
              </span>
            {:else}
              <!-- hover 播放键：单击语义改成"选中"之后，行内保留一个直接起播的
                   入口（不想练双击的用户用它）。display 由 CSS 在行 hover 时切换。 -->
              <button
                class="play-btn"
                tabindex="-1"
                aria-label={`播放 ${row.track.title}`}
                onclick={() => onplay(row.index)}
              >
                <Play size={11} />
              </button>
              <span class="idx-num">{row.index + 1}</span>
            {/if}
          </span>
          {#if row.track.artworkId}
            <img class="cover" src={artworkUrl(row.track.artworkId, 96) ?? ''} alt="" loading="lazy" />
          {:else}
            <span class="cover placeholder">♪</span>
          {/if}
          <span class="cell title" title={row.track.title}>
            {#each highlight(row.track.title, query) as part}
              {#if part.match}<mark>{part.text}</mark>{:else}{part.text}{/if}
            {/each}
          </span>
          <span class="cell dim" title={row.track.artist ?? ''}>{row.track.artist ?? '未知艺人'}</span>
          <span class="cell dim" title={row.track.album ?? ''}>{row.track.album ?? '—'}</span>
          <span class="cell num" title={formatDay(row.track.addedAtMs)}>{formatDay(row.track.addedAtMs)}</span>
          {#if showPlays}
            <span class="cell num">{row.track.playCount > 0 ? row.track.playCount : '—'}</span>
          {/if}
          <span class="cell num">{formatTime(row.track.durationMs)}</span>
        </div>
      {:else if row.error}
        <div class="row failed" role="row">
          <span class="k-check"></span>
          <span class="idx">{row.index + 1}</span>
          <span class="cover placeholder">!</span>
          <span class="cell">{row.error}</span>
          <span class="cell"></span>
          <span class="cell"></span>
          <span class="cell"></span>
          {#if showPlays}<span class="cell"></span>{/if}
          <span class="cell num"><button class="link" onclick={() => onretry(row.index)}>重试</button></span>
        </div>
      {:else}
        <div class="row skeleton" role="row" aria-busy="true">
          <span class="k-check"></span>
          <span class="idx">{row.index + 1}</span>
          <span class="cover placeholder"></span>
          <span class="cell shimmer"></span>
          <span class="cell"></span>
          <span class="cell"></span>
          <span class="cell"></span>
          {#if showPlays}<span class="cell"></span>{/if}
          <span class="cell num"></span>
        </div>
      {/if}
    {/each}
    <div style={`height:${range.padBottom}px`} aria-hidden="true"></div>
    {#if dropLineTop >= 0}
      <div class="drop-line" style={`top:${dropLineTop}px`} aria-hidden="true"></div>
    {/if}
  {/if}
</div>
