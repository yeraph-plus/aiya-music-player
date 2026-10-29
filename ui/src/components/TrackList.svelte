<script lang="ts">
  /**
   * **管理列表**（2026-10-28 拆分）：虚拟滚动 + 多选批量 + 列头排序 +
   * 筛选器 + 右键菜单——曲库 / 搜索 / 艺人 / 专辑详情页用。
   *
   * 三个分支各管各的（勿再往回挂开关）：`TrackTable`（纯榜单，首页）、
   * 本组件（管理，无拖拽——这些视图的顺序是查询语义，不能手排）、
   * `PlaylistList`（可拖放排序，仅歌单——手动歌单的顺序是用户语义）。
   * 行结构与 CSS 类名与 PlaylistList 刻意保持一致（`.row` / `.grid-head`，
   * 见 lists.css），改动行渲染时两个文件要同步。
   */
  import { Play } from '@lucide/svelte';
  import type { TrackRow } from '../lib/ipc';
  import type { TrackSort } from '../lib/query';
  import { artworkUrl } from '../lib/artwork';
  import { createVirtualWindow } from '../lib/create-virtual-window.svelte';
  import { scrollTopForRow } from '../lib/virtual';
  import CheckCell from './ui/CheckCell.svelte';
  import ListToolbar from './ListToolbar.svelte';
  import { formatTime, highlight } from '../lib/format';
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
    onsort: (field: TrackSort) => void;
    currentId: string | null;
    selectedIndex: number;
    selection: ReadonlySet<number>;
    /** 一次性恢复位置；应用后由父组件置空 */
    restoreScrollTop: number | null;
    playing: boolean;
    /**
     * 播放。**只由双击 / 封面悬浮播放键 / 键盘 Enter 触发**（B4-1 交互语义，
     * docs/24 §6）：单击的职责是选中，与"点一下就切歌"的旧行为区分。
     */
    onplay: (index: number) => void;
    /**
     * 选中。裸点、Ctrl、Shift **都走这里**（语义差别在
     * `selection.handleSelect` 内部分流）：裸点=只选这行、Ctrl=切换、
     * Shift=连选。Ctrl 点行时若批量模式没开，组件会先打开它（docs/27）。
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
    /* ---------------- 工具条（docs/27 内置，替代悬浮 BatchBar） ---------------- */
    batchMode: boolean;
    onbatchmode: (on: boolean) => void;
    onplayall: () => void;
    onenqueue: () => void;
    onaddto: () => void;
    onremove: () => void;
    removeLabel: string;
    /** 时间筛选生效数（筛选键角标）；不给 = 不显示筛选键（歌单等固定顺序视图） */
    filterCount?: number;
    /** 打开筛选器模态框（与 filterCount 一起用） */
    onfilteropen: () => void;
    /** 歌手/专辑名点击跳转（docs/27：列表内直达详情页，不再绕右键菜单） */
    onopenartist: (id: string) => void;
    onopenalbum: (id: string) => void;
    /**
     * 详情页把自己的列链接废掉（docs/28）：在专辑页里点"专辑"跳的是当前页
     * （nav 视为刷新、滚动位置丢失），语义不成立——渲染成纯文本。
     */
    disableArtistLink?: boolean;
    disableAlbumLink?: boolean;
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
    batchMode,
    onbatchmode,
    onplayall,
    onenqueue,
    onaddto,
    onremove,
    removeLabel,
    filterCount,
    onfilteropen,
    onopenartist,
    onopenalbum,
    disableArtistLink = false,
    disableAlbumLink = false,
    rowHeight = 52,
  }: Props = $props();

  const ROW_HEIGHT = $derived(rowHeight);
  const OVERSCAN = 6;
  /** 行区前的表头占位（sticky 仍是流内元素），滚动定位要把这 30px 算进去 */
  const HEAD_HEIGHT = 30;

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
    const target = scrollTopForRow(selectedIndex, ROW_HEIGHT, vw.viewportHeight, vw.scroller.scrollTop, HEAD_HEIGHT);
    if (target !== vw.scroller.scrollTop) vw.scroller.scrollTop = target;
  });

  /** 行内点击要排除的来源：勾选框 / 封面悬浮播放键 / 歌手·专辑跳转链接 */
  function isRowInternalTarget(target: EventTarget | null): boolean {
    return (target as HTMLElement | null)?.closest('.k-check, .play-btn, .link-cell') !== null;
  }
</script>

<!--
  docs/27 结构：外壳（工具条 + 滚动区）。工具条不随列表滚动；
  `.list` 仍是虚拟滚动的 scroller（vw.scroller 绑在这里，几何不变）。
  `no-batch` 时勾选列整个不渲染，网格模板同步少一列（见 lists.css）。
-->
<div class="list-shell" class:no-batch={!batchMode}>
  <ListToolbar
    {total}
    count={selection.size}
    {removeLabel}
    {batchMode}
    filterCount={filterCount ?? 0}
    canFilter={filterCount !== undefined}
    {onplayall}
    {onenqueue}
    {onaddto}
    {onremove}
    onfilteropen={onfilteropen}
    {onbatchmode}
  />

  <div
    class="list"
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
      {#if batchMode}
        <CheckCell
          role="columnheader"
          checked={allSelected}
          indeterminate={someSelected}
          label={allSelected ? '取消全选' : '全选'}
          disabled={total === 0}
          ontoggle={() => oncheckall()}
        />
      {/if}
      <span class="idx">#</span>
      <span></span>
      <button class="sort-head" class:on={sort === 'title'} onclick={() => canSort && onsort('title')} disabled={!canSort}>
        标题{sort === 'title' && canSort ? (direction === 'asc' ? ' ↑' : ' ↓') : ''}
      </button>
      <button class="sort-head" class:on={sort === 'artist'} onclick={() => canSort && onsort('artist')} disabled={!canSort}>
        歌手{sort === 'artist' && canSort ? (direction === 'asc' ? ' ↑' : ' ↓') : ''}
      </button>
      <button class="sort-head" class:on={sort === 'album'} onclick={() => canSort && onsort('album')} disabled={!canSort}>
        专辑{sort === 'album' && canSort ? (direction === 'asc' ? ' ↑' : ' ↓') : ''}
      </button>
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
            style={`height:${rowHeight}px`}
            role="row"
            tabindex="0"
            onclick={(event) => {
              // 勾选列 / 播放键 / 跳转链接里的点击不是"选中这一行"。
              // Svelte 5 把 click 委托到根节点统一派发，stopPropagation 拦不住
              // 同一批委托 handler，由行自己认出来源才是可靠的那道闸。
              if (isRowInternalTarget(event.target)) return;
              // Ctrl+点行 = 批量意图：批量模式没开就顺手打开（docs/27 第 5 点），
              // 用户不用先去找工具条上的开关
              if ((event.ctrlKey || event.metaKey) && !batchMode) onbatchmode(true);
              onselect(row.index, event);
            }}
            ondblclick={(event) => {
              // 双击 = 播放。前面的两次单击各自选中过一次（幂等），无害。
              if (isRowInternalTarget(event.target)) return;
              onplay(row.index);
            }}
            onkeydown={(event) => {
              // 焦点在行内的按钮（勾选框 / 播放键 / 链接）上时，Enter/Space 归按钮自己
              if ((event.target as HTMLElement | null)?.closest('button')) return;
              if (event.key === 'Enter' || event.key === ' ') {
                event.preventDefault();
                onplay(row.index);
              }
            }}
            oncontextmenu={(event) => {
              event.preventDefault();
              // 右键的行不在选择集里 → 先单选它；在 selection 里则保持整组，
              // 菜单按 multi 语义对整组生效（docs/27 第 6 点）
              if (!selection.has(row.index)) onselect(row.index, event);
              onmenu(event, row.index, row.track ?? null);
            }}
          >
            {#if batchMode}
              <CheckCell
                checked={selection.has(row.index)}
                label={`选择第 ${row.index + 1} 行`}
                ontoggle={(event) => oncheck(row.index, event)}
              />
            {/if}
            <span class="idx" role="cell">
              {#if row.track.id === currentId}
                <span class="bars" class:animated={playing} aria-label="正在播放">
                  <i></i><i></i><i></i>
                </span>
              {:else}
                <span class="idx-num">{row.index + 1}</span>
              {/if}
            </span>
            <!-- 封面悬浮播放键（docs/27 第 4 点）：从 # 列搬到封面上，
                 行 hover 时以半透明底叠在封面中央 -->
            <span class="cover-slot" role="cell">
              {#if row.track.artworkId}
                <img class="cover" src={artworkUrl(row.track.artworkId, 96) ?? ''} alt="" loading="lazy" />
              {:else}
                <span class="cover placeholder">♪</span>
              {/if}
              <button
                class="play-btn"
                tabindex="-1"
                aria-label={`播放 ${row.track.title}`}
                onclick={() => onplay(row.index)}
              >
                <Play size={13} />
              </button>
            </span>
            <span class="cell title" role="cell" title={row.track.title}>
              {#each highlight(row.track.title, query) as part}
                {#if part.match}<mark>{part.text}</mark>{:else}{part.text}{/if}
              {/each}
            </span>
            <span class="cell dim" role="cell">
              {#if row.track.artistId && row.track.artist && !disableArtistLink}
                <button
                  class="link-cell"
                  title={`查看歌手 ${row.track.artist}`}
                  onclick={(event) => {
                    event.stopPropagation();
                    onopenartist(row.track!.artistId!);
                  }}
                >
                  {row.track.artist}
                </button>
              {:else}
                <span title={row.track.artist ?? ''}>{row.track.artist ?? '未知歌手'}</span>
              {/if}
            </span>
            <span class="cell dim" role="cell">
              {#if row.track.albumId && row.track.album && !disableAlbumLink}
                <button
                  class="link-cell"
                  title={`查看专辑 ${row.track.album}`}
                  onclick={(event) => {
                    event.stopPropagation();
                    onopenalbum(row.track!.albumId!);
                  }}
                >
                  {row.track.album}
                </button>
              {:else}
                <span title={row.track.album ?? ''}>{row.track.album ?? '—'}</span>
              {/if}
            </span>
            <span class="cell num" role="cell">{formatTime(row.track.durationMs)}</span>
          </div>
        {:else if row.error}
          <div class="row failed" role="row">
            {#if batchMode}<span class="k-check" role="cell"></span>{/if}
            <span class="idx" role="cell">{row.index + 1}</span>
            <span class="cover-slot" role="cell"><span class="cover placeholder">!</span></span>
            <span class="cell" role="cell">{row.error}</span>
            <span class="cell" role="cell"></span>
            <span class="cell" role="cell"></span>
            <span class="cell num" role="cell"><button class="link" onclick={() => onretry(row.index)}>重试</button></span>
          </div>
        {:else}
          <div class="row skeleton" role="row" aria-busy="true">
            {#if batchMode}<span class="k-check" role="cell"></span>{/if}
            <span class="idx" role="cell">{row.index + 1}</span>
            <span class="cover-slot" role="cell"><span class="cover placeholder"></span></span>
            <span class="cell shimmer" role="cell"></span>
            <span class="cell" role="cell"></span>
            <span class="cell" role="cell"></span>
            <span class="cell num" role="cell"></span>
          </div>
        {/if}
      {/each}
      <div style={`height:${range.padBottom}px`} aria-hidden="true"></div>
    {/if}
  </div>
</div>
