<script lang="ts">
  import type { Snippet } from 'svelte';
  import Button from './ui/Button.svelte';
  import EmptyState from './ui/EmptyState.svelte';
  import PlaylistList from './PlaylistList.svelte';
  import TrackList from './TrackList.svelte';
  import { errorMessage } from '../lib/errors';
  import { SELECTION_CAP } from '../lib/selection';
  import { list } from '../stores/list.svelte';
  import { nav } from '../stores/nav.svelte';
  import { notify } from '../stores/notify.svelte';
  import { overlay } from '../stores/overlay.svelte';
  import { player } from '../stores/player.svelte';
  import { playlists } from '../stores/playlists.svelte';
  import { selection } from '../stores/selection.svelte';
  import { ui } from '../stores/ui.svelte';

  /**
   * 列表区（docs/28）：全部 store 接线收在这里，供
   * LibraryView（曲库/歌单/搜索）与 AlbumView/ArtistView（详情页）共用。
   *
   * 为什么要抽：docs/27 把批量动作接进列表组件后， LibraryView 的
   * 列表标签带上了 25 个 prop + 6 个 handler——拆详情页时
   * 原样复制两份是 3×250 行的接线抄写，漏一个 prop 的症状是"某个页面
   * 的批量按钮点了没反应"。现在接线只有这一份，页面只声明**差异**：
   * 排序可用性、删除按钮文案、筛选器有无与空态。
   *
   * 2026-10-28 拆分后本组件还负责**选表**：歌单视图渲染 `PlaylistList`
   * （可拖放排序，moveItem 也归这里），其余视图渲染 `TrackList`
   * （管理列表）。接线只有一份、组件选对了，行为分支就不散落在页面里。
   *
   * 失败/启动中的兜底空态也归这里（错误条里有原因 → "重试"；否则页面
   * 自己的 `empty` snippet）——这两个状态与页面无关，各页抄一遍只会漂移。
   */

  interface Props {
    canSort: boolean;
    /** 移除按钮文案随视图变：曲库/详情页=从曲库中删除，歌单=从播放列表移除 */
    removeLabel: string;
    /** 筛选器按钮是否出现（曲库页开；歌单/详情页是固定上下文不给筛） */
    showFilter: boolean;
    /**
     * 拖放排序回调。**传了就渲染 PlaylistList（可拖表），不传渲染
     * TrackList（管理表）**——判据由页面给：手动歌单的顺序是用户语义
     * 可拖；动态歌单是查询结果、曲库/详情页是查询语义，都不可拖。
     */
    dragSort?: (from: number, to: number) => void;
    /**
     * 详情页把自己的列链接废掉（docs/28）：专辑页上"查看专辑"指向的就是
     * 当前页，nav 会当成"刷新"重拉列表——滚动位置凭空丢一次，还让人以为
     * 点错了。艺人页同理禁歌手列。
     */
    disableArtistLink?: boolean;
    disableAlbumLink?: boolean;
    /** 列表为空且无错误时的页面级空态（空库/空歌单/无搜索结果） */
    empty: Snippet;
  }

  let {
    canSort,
    removeLabel,
    showFilter,
    dragSort,
    disableArtistLink = false,
    disableAlbumLink = false,
    empty,
  }: Props = $props();

  const playerState = $derived(player.state);

  function handleSelect(index: number, event: MouseEvent | KeyboardEvent): void {
    if (selection.handleSelect(index, event) === 'capped') {
      notify.showToast(`单次最多选择 ${SELECTION_CAP} 首，已截取刚点中的一段`);
    }
  }

  /** 勾选框：点=切换、Shift=连选（与 ctrl+点行的语义不同） */
  function handleCheck(index: number, event: MouseEvent | KeyboardEvent): void {
    if (selection.handleCheck(index, event) === 'capped') {
      notify.showToast(`单次最多选择 ${SELECTION_CAP} 首，已截取刚点中的一段`);
    }
  }

  /**
   * 批量操作开关（docs/27 第 2 点）：关 = 隐藏勾选列，同时清掉选择集——
   * 勾选列都看不见了还留着选择会让人以为操作会落到那几行上。
   */
  function setBatchMode(on: boolean): void {
    ui.batchMode = on;
    if (!on) selection.clear();
  }

  /** 工具条「加入歌单」：选择集 → ID 后开选择器对话框 */
  function addSelectionToPlaylist(): void {
    void (async () => {
      const ids = await list.selectionIds();
      if (ids.length === 0) return;
      overlay.dialog = { kind: 'addToPlaylist', ids };
    })();
  }

  /** 工具条「加入播放队列」：选择集追加到队尾 */
  function enqueueSelection(): void {
    void (async () => {
      const ids = await list.selectionIds();
      if (ids.length === 0) return;
      try {
        await player.enqueue(ids);
        notify.showToast(`已加入队列 ${ids.length} 首`);
        selection.clear();
      } catch (error) {
        notify.showError(errorMessage(error));
      }
    })();
  }

  /**
   * 工具条「从曲库中删除 / 从播放列表移除」：语义随视图。
   * 曲库走确认对话框（只删索引不动磁盘）；歌单按下标倒序删。
   */
  function removeSelection(): void {
    if (ui.view.kind === 'playlist') {
      void playlists.removeItemsAt(selection.sortedIndices());
      return;
    }
    void (async () => {
      const ids = await list.selectionIds();
      if (ids.length === 0) return;
      overlay.dialog = { kind: 'removeTracks', ids };
    })();
  }

  /** 歌单视图拖拽排序的落点（PlaylistList 专用）：手动歌单的顺序是用户语义 */
  function movePlaylistItem(from: number, to: number): void {
    dragSort?.(from, to);
  }

  /** 两个列表组件共用的 props（接线只有这一份，页面只声明差异） */
  const shared = $derived({
    store: list.trackWindow,
    version: list.version,
    total: list.total,
    query: ui.search,
    sort: ui.sort,
    direction: ui.direction,
    canSort,
    onsort: (field: (typeof ui)['sort']) => list.setSort(field),
    currentId: playerState?.current?.id ?? null,
    selectedIndex: selection.selectedIndex,
    selection: selection.selection,
    restoreScrollTop: list.restoreScrollTop,
    rowHeight: ui.rowHeight,
    playing: playerState?.status === 'playing',
    onplay: (index: number) => void list.playIndex(index),
    onselect: handleSelect,
    oncheck: handleCheck,
    oncheckall: () => selection.toggleAll(list.total),
    onmenu: (event: MouseEvent, index: number, track: Parameters<typeof overlay.openTrackMenu>[3]) => {
      // 右键的行不在选择集里 → 先单选它（handleSelect 裸点语义），
      // 在选择集里则整组保留；菜单按 multi 对整组生效（docs/27 第 6 点）
      if (!selection.selection.has(index)) handleSelect(index, event);
      overlay.openTrackMenu(event.clientX, event.clientY, index, track, selection.size > 1);
    },
    onscroll: (top: number) => list.setScroll(top),
    onrestored: () => (list.restoreScrollTop = null),
    onretry: () => void list.reload(),
    batchMode: ui.batchMode,
    onbatchmode: setBatchMode,
    onplayall: () => void list.playAll(),
    onenqueue: enqueueSelection,
    onaddto: addSelectionToPlaylist,
    onremove: removeSelection,
    removeLabel,
    filterCount: showFilter ? (list.timeFilterOn ? 1 : 0) : undefined,
    onfilteropen: () => (overlay.dialog = { kind: 'trackFilter' }),
    onopenartist: (id: string) => nav.go({ kind: 'artist', id }),
    onopenalbum: (id: string) => nav.go({ kind: 'album', id }),
    disableArtistLink,
    disableAlbumLink,
  });
</script>

{#if list.total > 0}
  {#if dragSort}
    <PlaylistList {...shared} onmove={movePlaylistItem} />
  {:else}
    <TrackList {...shared} />
  {/if}
{:else if !list.booted}
  <EmptyState fill title="正在启动…" />
{:else if notify.error}
  <!-- 查询失败时 total 也是 0，但这时候说"曲库还是空的"是错的：
       用户会去设置页重新扫描，而真正该做的是重试查询 -->
  <EmptyState fill title="列表加载失败" hint="上方错误条里有原因">
    {#snippet action()}
      <Button onclick={() => void list.reload()}>重试</Button>
    {/snippet}
  </EmptyState>
{:else}
  {@render empty()}
{/if}
