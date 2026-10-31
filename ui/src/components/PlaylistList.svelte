<script lang="ts">
  /**
   * 歌单视图专用：**可拖放排序**的曲目列表（2026-10-28 拆分）。
   *
   * 为什么从 TrackList 分叉而不是继续共用：TrackList 一个组件靠
   * `onmove?` / `canSort` / `filterCount?` 等散落开关编码"曲库/歌单/
   * 详情页"多种语境，越挂越多。现在三个分支各管各的——
   * `TrackTable`（纯榜单，首页）、`TrackList`（管理列表，曲库/详情页，
   * 无拖拽）、本组件（拖放排序，仅歌单）。行结构与 CSS 类名刻意保持
   * 一致（`.row` / `.grid-head`，见 lists.css），改动行渲染时两个
   * 文件要同步——头部这条注释就是提醒。
   *
   * 拖拽排序语义：按住行拖动（`lib/drag` 的 startRowDrag），松手时
   * onmove(from, to)；拖拽激活后的残留 click 被吞掉，避免误选中。
   */
  import { tick } from 'svelte';
  import { Play } from '@lucide/svelte';
  import type { TrackRow } from '../lib/ipc';
  import type { TrackSort } from '../lib/query';
  import { artworkUrl } from '../lib/artwork';
  import { startRowDrag, consumeClickSuppressed } from '../lib/drag';
  import { createVirtualWindow } from '../lib/create-virtual-window.svelte';
  import { centeredScrollTopForRow, scrollTopForRow } from '../lib/virtual';
  // 别名导入：组件已有 `selection` prop（选择集 ReadonlySet），同名会遮蔽 store
  import { selection as selectionStore } from '../stores/selection.svelte';
  import Button from './ui/Button.svelte';
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
    /**
     * 「定位到正在播放」的目标下标（父组件按当前查询解析）；`null` = 无事。
     * 一次性语义：滚过去就回报 `onlocated` 复位，**不做持续自动跟随**——
     * 播放中把用户正在浏览的位置反复拽回播放行是修掉的老 bug。
     */
    locateIndex: number | null;
    onlocated: () => void;
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
    /** 拖放排序（本组件存在的前提）：松手时报告 from → to */
    onmove: (from: number, to: number) => void;
    /* ---------------- 工具条（docs/27 内置，替代悬浮 BatchBar） ---------------- */
    batchMode: boolean;
    onbatchmode: (on: boolean) => void;
    /** 跳到正在播放的条目（解析下标在父组件，这里只透传给工具条） */
    onlocate: () => void;
    onplayall: () => void;
    onenqueue: () => void;
    onaddto: () => void;
    onremove: () => void;
    removeLabel: string;
    /** 时间筛选生效数（筛选键角标）；不给 = 不显示筛选键 */
    filterCount?: number;
    /** 打开筛选器模态框（与 filterCount 一起用） */
    onfilteropen: () => void;
    /** 歌手/专辑名点击跳转 */
    onopenartist: (id: string) => void;
    onopenalbum: (id: string) => void;
    /**
     * 详情页把自己的列链接废掉（docs/28）：在专辑页里点"专辑"跳的是当前页，
     * 渲染成纯文本（歌单视图用不到，接口与 TrackList 对齐省一层分支）。
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
    locateIndex,
    onlocated,
    onretry,
    onmove,
    batchMode,
    onbatchmode,
    onlocate,
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
  /**
   * 滚动内容里行区之前的占位高度：`.grid-head` 是 scroller 内 30px 的流内
   * 元素（sticky 不改变占位），虚拟数学（插线定位 / 滚动定位）都必须
   * 把它加回去。与 lists.css 的 `.grid-head { height: 30px }` 对应。
   */
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

  // roving tabindex 的锚（发布前 UI 审查 P0-1）：只有焦点行是 0，其余行 -1，
  // Tab 一次进列表、方向键在行间移动焦点。什么都没选时锚在第一个可见行——
  // 否则 Tab 永远进不了列表（全部 -1 = 没有入口）。
  const focusAnchor = $derived(selectedIndex >= 0 ? selectedIndex : (rows[0]?.index ?? -1));

  /**
   * 插入线的纵坐标。drop-line 是 scroller 内的绝对定位元素，随内容一起滚，
   * 所以这里是**内容坐标**：行边界 + 表头占位，与滚动位置无关——
   * 旧公式 `padTop + to * ROW_HEIGHT` 把区间起点混进了内容坐标，
   * 列表一滚插线就脱离目标行。
   */
  const dropLineTop = $derived(
    dragState && dragState.active ? HEAD_HEIGHT + dragState.to * ROW_HEIGHT : -1,
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

  // 焦点行（键盘/点选）变化时把它滚进视野。
  // **必须挡住重跑**：这个 effect 在组件重挂载、视口尺寸变化（抽屉开合/
  // 窗口缩放）、行高变化时都会重跑，而不只是焦点变化——双击播放后
  // selectedIndex 一直停在播放行上，重跑一次就把用户正在浏览的位置拽回
  // 播放行（实测：播放中滚动列表反复跳回正在播放的条目）。
  // 闸门 `selection.followedIndex` 记录「已经跟随到过的下标」，焦点没变
  // 就不滚；存 store 而非局部变量是因为重挂载会清零局部变量、闸就失效。
  $effect(() => {
    if (!vw.scroller || selectedIndex < 0) return;
    if (selectionStore.followedIndex === selectedIndex) return;
    selectionStore.followedIndex = selectedIndex;
    const target = scrollTopForRow(selectedIndex, ROW_HEIGHT, vw.viewportHeight, vw.scroller.scrollTop, HEAD_HEIGHT);
    if (target !== vw.scroller.scrollTop) vw.scroller.scrollTop = target;
  });

  // 键盘移动后把 DOM 焦点交给目标行（发布前 UI 审查 P0-1）：此前方向键只挪
  // 高亮 class、焦点原地不动——Space 播的是焦点所在旧行、Enter 播的是高亮
  // 新行，同一屏幕两个键播出两首歌。focusNonce 只由键盘 move 意图置位
  // （selection.requestRowFocus()），点选/定位到正在播放等编程路径不抢焦点。
  // preventScroll：滚动由上面的跟随效果负责，这里只管焦点。
  $effect(() => {
    if (selectionStore.focusNonce === 0 || selectedIndex < 0 || !vw.scroller) return;
    const idx = selectedIndex;
    void tick().then(() => {
      vw.scroller?.querySelector<HTMLElement>(`.row[data-index='${idx}']`)?.focus({ preventScroll: true });
    });
  });

  // 「定位到正在播放」：父组件解析出下标后滚一次（居中），回报复位。
  // 与上面的跟随闸门互不干扰：定位是显式动作，点几次滚几次。
  $effect(() => {
    if (locateIndex === null || !vw.scroller || total === 0) return;
    const target = centeredScrollTopForRow(locateIndex, ROW_HEIGHT, vw.viewportHeight, total, HEAD_HEIGHT);
    vw.scroller.scrollTop = target;
    vw.scrollTop = target;
    onlocated();
  });

  function handlePointerDown(event: PointerEvent, index: number): void {
    startRowDrag({
      event,
      index,
      count: total,
      rowHeight,
      scroller: vw.scroller,
      onState: (next) => (dragState = next),
      onDrop: (from, to) => onmove(from, to),
    });
  }

  /** 行内点击要排除的来源：勾选框 / 封面悬浮播放键 / 歌手·专辑跳转链接 */
  function isRowInternalTarget(target: EventTarget | null): boolean {
    return (target as HTMLElement | null)?.closest('.k-check, .play-btn, .link-cell') !== null;
  }
</script>

<!--
  外壳与 TrackList 同构（工具条 + 滚动区）。`.list` 是虚拟滚动的
  scroller（vw.scroller 绑在这里，几何不变）。
-->
<div class="list-shell" class:no-batch={!batchMode}>
  <ListToolbar
    {total}
    count={selection.size}
    {removeLabel}
    {batchMode}
    filterCount={filterCount ?? 0}
    canFilter={filterCount !== undefined}
    canLocate={currentId !== null}
    onlocate={onlocate}
    {onplayall}
    {onenqueue}
    {onaddto}
    {onremove}
    onfilteropen={onfilteropen}
    {onbatchmode}
  />

  <div
    class="list"
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
            class:drag-source={dragState?.active && dragState.from === row.index}
            style={`height:${rowHeight}px`}
            role="row"
            tabindex={row.index === focusAnchor ? 0 : -1}
            data-index={row.index}
            onpointerdown={(event) => handlePointerDown(event, row.index)}
            onclick={(event) => {
              // 拖拽激活后松手会派发一次残留 click，吞掉避免误选中
              if (consumeClickSuppressed()) {
                event.stopPropagation();
                return;
              }
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
                role="cell"
                checked={selection.has(row.index)}
                label={`选择第 ${row.index + 1} 行`}
                ontoggle={(event) => oncheck(row.index, event)}
              />
            {/if}
            <span class="idx">
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
            <span class="cover-slot">
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
            <span class="cell title" title={row.track.title}>
              {#each highlight(row.track.title, query) as part}
                {#if part.match}<mark>{part.text}</mark>{:else}{part.text}{/if}
              {/each}
            </span>
            <span class="cell dim">
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
            <span class="cell dim">
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
            <span class="cell num">{formatTime(row.track.durationMs)}</span>
          </div>
        {:else if row.error}
          <div class="row failed" role="row">
            {#if batchMode}<span class="k-check"></span>{/if}
            <span class="idx">{row.index + 1}</span>
            <span class="cover-slot"><span class="cover placeholder">!</span></span>
            <span class="cell">{row.error}</span>
            <span class="cell"></span>
            <span class="cell"></span>
            <span class="cell num"><Button variant="link" onclick={() => onretry(row.index)}>重试</Button></span>
          </div>
        {:else}
          <div class="row skeleton" role="row" aria-busy="true">
            {#if batchMode}<span class="k-check"></span>{/if}
            <span class="idx">{row.index + 1}</span>
            <span class="cover-slot"><span class="cover placeholder"></span></span>
            <span class="cell shimmer"></span>
            <span class="cell"></span>
            <span class="cell"></span>
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
</div>
