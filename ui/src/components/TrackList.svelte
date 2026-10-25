<script lang="ts">
  import { Check, Minus } from '@lucide/svelte';
  import { Checkbox } from 'bits-ui';
  import type { TrackRow } from '../lib/ipc';
  import type { TrackSort } from '../lib/query';
  import { artworkUrl } from '../lib/artwork';
  import { startRowDrag, consumeClickSuppressed } from '../lib/drag';
  import { computeRange } from '../lib/virtual';
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
    onplay: (index: number) => void;
    /** Ctrl / Shift / ⌘ 点击时走选择而不是播放 */
    onselect: (index: number, event: MouseEvent | KeyboardEvent) => void;
    /** 勾选框：点=切换、Shift=连选（与 ctrl+点击的语义不同） */
    oncheck: (index: number, event: MouseEvent) => void;
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
   * 表头三态：全选的上限与 `lib/selection.ts` 的 SELECTION_CAP 一致，
   * 否则 5 万首的视图点全选后表头还显示"半选"（只选了前 5000）。
   */
  const selectableTotal = $derived(Math.min(total, SELECTION_CAP));
  const allSelected = $derived(selectableTotal > 0 && selection.size >= selectableTotal);
  const someSelected = $derived(selection.size > 0 && !allSelected);

  let scroller = $state<HTMLDivElement | null>(null);
  let viewportHeight = $state(640);
  let scrollTop = $state(0);
  let dragState = $state<{ from: number; to: number; active: boolean } | null>(null);

  const range = $derived(
    computeRange({
      scrollTop,
      viewportHeight,
      rowHeight,
      total,
      overscan: OVERSCAN,
    }),
  );

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

  $effect(() => {
    if (!scroller) return;
    const observer = new ResizeObserver(() => {
      viewportHeight = scroller?.clientHeight ?? 640;
    });
    observer.observe(scroller);
    viewportHeight = scroller.clientHeight;
    return () => observer.disconnect();
  });

  // 恢复滚动位置：等 total 就位后只执行一次
  $effect(() => {
    if (restoreScrollTop === null || !scroller || total === 0) return;
    scroller.scrollTop = restoreScrollTop;
    scrollTop = scroller.scrollTop;
    onrestored();
  });

  // 键盘选中项变化时把它滚进视野
  $effect(() => {
    if (!scroller || selectedIndex < 0) return;
    const top = selectedIndex * ROW_HEIGHT;
    const bottom = top + ROW_HEIGHT;
    if (top < scroller.scrollTop) scroller.scrollTop = top;
    else if (bottom > scroller.scrollTop + scroller.clientHeight) {
      scroller.scrollTop = bottom - scroller.clientHeight;
    }
  });

  function handleScroll(): void {
    if (!scroller) return;
    scrollTop = scroller.scrollTop;
    onscroll(scrollTop, Math.floor(scrollTop / ROW_HEIGHT));
  }

  function handlePointerDown(event: PointerEvent, index: number): void {
    if (!onmove) return;
    startRowDrag({
      event,
      index,
      count: total,
      rowHeight,
      scroller,
      onState: (next) => (dragState = next),
      onDrop: (from, to) => onmove?.(from, to),
    });
  }
</script>

<div
  class="list"
  class:with-plays={showPlays}
  class:dragging={dragState?.active}
  bind:this={scroller}
  onscroll={handleScroll}
  role="grid"
  aria-rowcount={total}
>
  <div class="grid-head" role="row">
    <!-- 点击处理放在包装层而不是 Checkbox 上：bits-ui 自己的 click 处理器会同时跑，
         内部 checked 与我们的受控 prop 各翻一次，结果正好反相（实测：勾了的行
         显示未勾）。键盘可达性由内层 button 提供，这两条 a11y 提示是误报。 -->
    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <!-- tabindex=-1：可编程聚焦但不进 Tab 序列，真正的键盘入口是内层 button -->
    <span class="check" role="columnheader" tabindex="-1" onclick={oncheckall}>
      <Checkbox.Root
        class="tt-box"
        checked={allSelected}
        indeterminate={someSelected}
        aria-label={allSelected ? '取消全选' : '全选'}
        disabled={total === 0}
      >
        {#snippet children({ checked, indeterminate })}
          {#if indeterminate}<Minus size={11} />{:else if checked}<Check size={11} />{/if}
        {/snippet}
      </Checkbox.Root>
    </span>
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
            // 拖拽激活后松手会派发一次残留 click，吞掉避免误播放
            if (consumeClickSuppressed()) {
              event.stopPropagation();
              return;
            }
            // 勾选列里的点击不是"播放"。**这个判断不能省**：Svelte 5 把 click
            // 委托到根节点统一派发，`stopPropagation` 拦不住同一批委托 handler，
            // 所以由行自己认出"点的是勾选框"才是可靠的那道闸。
            if ((event.target as HTMLElement | null)?.closest('.check')) return;
            if (event.ctrlKey || event.metaKey || event.shiftKey) onselect(row.index, event);
            else onplay(row.index);
          }}
          onkeydown={(event) => {
            // 键盘与鼠标单击同语义：Enter/Space 播放（选择走 Ctrl/Shift+点击或全局快捷键）
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
          <!-- 同上：点选框由包装层接管，键盘可达性在 button 上 -->
          <!-- svelte-ignore a11y_click_events_have_key_events -->
          <span class="check" role="cell" tabindex="-1" onclick={(event) => oncheck(row.index, event)}>
            <Checkbox.Root
              class="tt-box"
              checked={selection.has(row.index)}
              aria-label={`选择第 ${row.index + 1} 行`}
            >
              {#snippet children({ checked })}
                {#if checked}<Check size={11} />{/if}
              {/snippet}
            </Checkbox.Root>
          </span>
          <span class="idx">
            {#if row.track.id === currentId}
              <span class="bars" class:animated={playing} aria-label="正在播放">
                <i></i><i></i><i></i>
              </span>
            {:else}
              {row.index + 1}
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
          <span class="check"></span>
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
          <span class="check"></span>
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
