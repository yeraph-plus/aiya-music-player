<script lang="ts">
  import type { Track, TrackSort } from '@aiya/contracts';
  import { artworkUrl } from '@aiya/contracts';
  import { startRowDrag } from '../lib/drag';
  import { computeRange } from '../lib/virtual';
  import { formatTime, highlight } from '../lib/format';
  import type { TrackWindow } from '../lib/track-window';

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
    onmenu: (event: MouseEvent, index: number, track: Track | null) => void;
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
    onmenu,
    onscroll,
    onrestored,
    onretry,
    onmove,
    rowHeight = 52,
  }: Props = $props();

  const ROW_HEIGHT = $derived(rowHeight);
  const OVERSCAN = 6;

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
    const out: Array<{ index: number; track: Track | undefined; error: string | null }> = [];
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
          tabindex="-1"
          onpointerdown={(event) => handlePointerDown(event, row.index)}
          onclick={(event) => {
            if (event.ctrlKey || event.metaKey || event.shiftKey) onselect(row.index, event);
            else onplay(row.index);
          }}
          onkeydown={(event) => {
            if (event.key === 'Enter') onplay(row.index);
            else if (event.key === ' ') {
              event.preventDefault();
              onselect(row.index, event);
            }
          }}
          oncontextmenu={(event) => {
            event.preventDefault();
            onselect(row.index, event);
            onmenu(event, row.index, row.track ?? null);
          }}
        >
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
          {#if showPlays}
            <span class="cell num">{row.track.playCount > 0 ? row.track.playCount : '—'}</span>
          {/if}
          <span class="cell num">{formatTime(row.track.durationMs)}</span>
        </div>
      {:else if row.error}
        <div class="row failed" role="row">
          <span class="idx">{row.index + 1}</span>
          <span class="cover placeholder">!</span>
          <span class="cell">{row.error}</span>
          <span class="cell"></span>
          <span class="cell"></span>
          {#if showPlays}<span class="cell"></span>{/if}
          <span class="cell num"><button class="link" onclick={() => onretry(row.index)}>重试</button></span>
        </div>
      {:else}
        <div class="row skeleton" role="row" aria-busy="true">
          <span class="idx">{row.index + 1}</span>
          <span class="cover placeholder"></span>
          <span class="cell shimmer"></span>
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
