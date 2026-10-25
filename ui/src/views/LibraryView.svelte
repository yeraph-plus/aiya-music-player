<script lang="ts">
  import TopBar from '../components/TopBar.svelte';
  import TrackList from '../components/TrackList.svelte';
  import ViewHeader from '../components/ViewHeader.svelte';
  import { registerSearchFocus } from '../lib/focus-search';
  import { SELECTION_CAP } from '../lib/selection';
  import { viewCanSort } from '../lib/view';
  import { history } from '../stores/history.svelte';
  import { library } from '../stores/library.svelte';
  import { list } from '../stores/list.svelte';
  import { overlay } from '../stores/overlay.svelte';
  import { playlists } from '../stores/playlists.svelte';
  import { player } from '../stores/player.svelte';
  import { scan } from '../stores/scan.svelte';
  import { selection } from '../stores/selection.svelte';
  import { notify } from '../stores/notify.svelte';
  import { ui } from '../stores/ui.svelte';

  /**
   * 曲库主区：顶栏 + 视图头 + 筛选/扫描/错误条 + 曲目表（或空态）。
   *
   * 从 `App.svelte` 拆出来的（批次 3）。**这里不取数、不拼队列**——所有
   * 状态从 store 读，所有动作调 store 的方法。剩下真正属于"视图"的只有：
   * TopBar 实例（聚焦要 bind:this）、空态分支的文案、TrackList 的连线。
   *
   * 依赖方向：components → stores → ipc。本文件 import 了 9 个 store，
   * 但没有一个 store 反过来知道它存在。
   */

  let topBar = $state<{ focus: () => void } | null>(null);

  // 快捷键 focusSearch 需要一个"把焦点给输入框"的入口；实例是这里的 bind:this。
  $effect(() => {
    if (!topBar) return;
    const instance = topBar;
    return registerSearchFocus(() => instance.focus());
  });

  const view = $derived(ui.view);
  const currentPlaylist = $derived(
    view.kind === 'playlist' ? (ui.playlistById(view.id) ?? null) : null,
  );
  const showPlays = $derived(view.kind === 'recent' || view.kind === 'mostPlayed');
  const playerState = $derived(player.state);

  function handleSelect(index: number, event: MouseEvent | KeyboardEvent): void {
    if (selection.handleSelect(index, event) === 'capped') {
      notify.showToast(`单次最多选择 ${SELECTION_CAP} 首，已截取刚点中的一段`);
    }
  }
</script>

<TopBar
  bind:this={topBar}
  search={ui.search}
  sort={ui.sort}
  direction={ui.direction}
  canSort={viewCanSort(view)}
  total={list.total}
  scanning={library.scanning}
  onsearch={(value) => list.setSearch(value)}
  onclearsearch={() => list.clearSearch()}
  onsort={() => list.setSort('added')}
  onsettings={() => void ui.openSettings()}
  onchooseroot={() => void scan.pickRootAndScan()}
  onscan={() => void scan.start()}
  onimport={() => ui.togglePanel('import')}
/>

<ViewHeader
  {view}
  playlist={currentPlaylist}
  total={list.total}
  onplayall={() => void list.playAll()}
  onrename={() => {
    if (currentPlaylist) overlay.dialog = { kind: 'renamePlaylist', playlist: currentPlaylist };
  }}
  onclearitems={() => {
    if (currentPlaylist) void playlists.clear(currentPlaylist);
  }}
  ondelete={() => {
    if (currentPlaylist) overlay.dialog = { kind: 'deletePlaylist', playlist: currentPlaylist };
  }}
  onclearhistory={() => void history.clear()}
/>

{#if ui.artistId || ui.albumId}
  <div class="filters">
    {#if ui.artistId}<span class="filter">艺人筛选 <button class="link" onclick={() => list.clearFilter()}>清除</button></span>{/if}
    {#if ui.albumId}<span class="filter">专辑筛选 <button class="link" onclick={() => list.clearFilter()}>清除</button></span>{/if}
  </div>
{/if}

{#if library.scan && library.scanning}
  {@const progress = library.scan}
  <div class="scan-bar">
    <div class="progress">
      <i style={`width:${progress.total ? Math.min(100, Math.round((progress.scanned / progress.total) * 100)) : 8}%`}></i>
    </div>
    <span>
      {progress.phase === 'walking'
        ? '正在遍历目录…'
        : `${progress.scanned}/${progress.total ?? '?'} · 新增 ${progress.imported} · 跳过 ${progress.skipped} · 失败 ${progress.failed}`}
    </span>
    <button class="btn" onclick={() => scan.cancel()}>取消</button>
  </div>
{/if}

{#if notify.error}
  <div class="error" aria-live="assertive">
    {notify.error}
    <button class="link" onclick={() => notify.clearError()}>关闭</button>
  </div>
{/if}

{#if playerState?.status === 'error' && playerState.error}
  <div class="error">
    《{playerState.current?.title ?? '未知曲目'}》{playerState.error.message}
    <button class="link" onclick={() => player.next()}>跳到下一首</button>
  </div>
{/if}

{#if list.total > 0}
  <TrackList
    store={list.trackWindow}
    version={list.version}
    total={list.total}
    query={ui.search}
    sort={ui.sort}
    direction={ui.direction}
    canSort={viewCanSort(view)}
    {showPlays}
    onsort={(field) => list.setSort(field)}
    currentId={playerState?.current?.id ?? null}
    selectedIndex={selection.selectedIndex}
    selection={selection.selection}
    restoreScrollTop={list.restoreScrollTop}
    rowHeight={ui.settings.density === 'compact' ? 42 : 52}
    playing={playerState?.status === 'playing'}
    onplay={(index) => void list.playIndex(index)}
    onselect={handleSelect}
    onmenu={(event, index, track) => overlay.openTrackMenu(event.clientX, event.clientY, index, track)}
    onscroll={(top) => list.setScroll(top)}
    onrestored={() => (list.restoreScrollTop = null)}
    onretry={() => void list.reload()}
    onmove={view.kind === 'playlist' ? (from, to) => void playlists.moveItem(from, to) : undefined}
  />
{:else if list.booted}
  <div class="empty">
    {#if ui.search.trim()}
      <strong>没有匹配的曲目</strong>
      <span>换个关键词，或清除搜索条件</span>
      <button class="btn" onclick={() => list.clearSearch()}>清除搜索</button>
    {:else if view.kind === 'recent'}
      <strong>还没有播放记录</strong>
      <span>听几首歌，这里会按时间倒序记录</span>
    {:else if view.kind === 'mostPlayed'}
      <strong>还没有常听曲目</strong>
      <span>播放次数会从零开始积累</span>
    {:else if view.kind === 'playlist'}
      <strong>这个播放列表还是空的</strong>
      <span>在曲库里选中歌曲，右键或批量操作加入这里</span>
      <button class="btn" onclick={() => list.openBuiltinView('library')}>去曲库看看</button>
    {:else}
      <strong>曲库还是空的</strong>
      <span>选择一个音乐目录，AIYA 会扫描并建立索引</span>
      <button class="btn primary" onclick={() => void scan.pickRootAndScan()} disabled={library.scanning}>
        选择音乐目录
      </button>
    {/if}
  </div>
{:else}
  <div class="empty"><strong>正在启动…</strong></div>
{/if}
