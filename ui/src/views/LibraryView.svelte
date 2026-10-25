<script lang="ts">
  import TrackList from '../components/TrackList.svelte';
  import ViewHeader from '../components/ViewHeader.svelte';
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
   * 曲库主区：视图头 + 筛选/扫描/错误条 + 曲目表（或空态）。
   * 顶栏（搜索）已上移到 AppShell，全局唯一；扫描与导入入口在设置页。
   */

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

  /** 勾选框：点=切换、Shift=连选（与 ctrl+点行的语义不同） */
  function handleCheck(index: number, event: MouseEvent): void {
    if (selection.handleCheck(index, event) === 'capped') {
      notify.showToast(`单次最多选择 ${SELECTION_CAP} 首，已截取刚点中的一段`);
    }
  }
</script>

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
    oncheck={handleCheck}
    oncheckall={() => selection.toggleAll(list.total)}
    onmenu={(event, index, track) => overlay.openTrackMenu(event.clientX, event.clientY, index, track)}
    onscroll={(top) => list.setScroll(top)}
    onrestored={() => (list.restoreScrollTop = null)}
    onretry={() => void list.reload()}
    onmove={view.kind === 'playlist' ? (from, to) => void playlists.moveItem(from, to) : undefined}
  />
{:else if list.booted}
  <div class="empty">
    {#if notify.error}
      <!-- 查询失败时 total 也是 0，但这时候说"曲库还是空的"是错的：
           用户会去设置页重新扫描，而真正该做的是重试查询 -->
      <strong>列表加载失败</strong>
      <span>上方错误条里有原因</span>
      <button class="btn" onclick={() => void list.reload()}>重试</button>
    {:else if ui.search.trim()}
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
