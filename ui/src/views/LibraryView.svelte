<script lang="ts">
  import Icon from '../components/Icon.svelte';
  import PageHeader from '../components/PageHeader.svelte';
  import PageToolbar from '../components/PageToolbar.svelte';
  import TrackList from '../components/TrackList.svelte';
  import { artworkUrl } from '../lib/artwork';
  import { formatDurationLong } from '../lib/format';
  import { SELECTION_CAP } from '../lib/selection';
  import { viewCanSort, viewTitle } from '../lib/view';
  import { history } from '../stores/history.svelte';
  import { library } from '../stores/library.svelte';
  import { list } from '../stores/list.svelte';
  import { nav } from '../stores/nav.svelte';
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
    view.kind === 'playlist' ? playlists.byId(view.id) : null,
  );
  const showPlays = $derived(view.kind === 'recent' || view.kind === 'mostPlayed');
  const playerState = $derived(player.state);

  /**
   * 标题区数据。
   *
   * 三个视图的头部形状原本分成两套（歌单有一张 92px 封面，统计只有文字），
   * 分别写在 ViewHeader 的两个 `{#if}` 分支里。现在归一到 PageHeader 的
   * 「有封面 / 无封面」两态，差别只剩数据。
   */
  const header = $derived.by(() => {
    if (view.kind === 'playlist') {
      const playlist = currentPlaylist;
      return {
        kicker: '播放列表',
        title: playlist?.name ?? '播放列表',
        subtitle: undefined as string | undefined,
        meta: playlist
          ? `${playlist.trackCount} 首 · ${formatDurationLong(playlist.durationMs)}`
          : '',
        cover: playlist?.artworkId ? (artworkUrl(playlist.artworkId, 256) ?? null) : null,
        playDisabled: !playlist || playlist.trackCount === 0,
      };
    }
    if (view.kind === 'recent' || view.kind === 'mostPlayed') {
      return {
        kicker: '播放统计',
        title: viewTitle(view),
        subtitle:
          view.kind === 'recent'
            ? '按最后一次播放排序，只统计真正听过的曲目'
            : '按播放次数排序，用于快速回到最近常听的歌',
        meta: `${list.total} 首`,
        cover: undefined as string | null | undefined,
        playDisabled: list.total === 0,
      };
    }
    return {
      kicker: '曲库',
      title: '全部曲目',
      subtitle: ui.search.trim() ? `搜索「${ui.search.trim()}」` : undefined,
      meta: `${list.total} 首`,
      cover: undefined as string | null | undefined,
      playDisabled: list.total === 0,
    };
  });

  function handleSelect(index: number, event: MouseEvent | KeyboardEvent): void {
    if (selection.handleSelect(index, event) === 'capped') {
      notify.showToast(`单次最多选择 ${SELECTION_CAP} 首，已截取刚点中的一段`);
    }
  }

  /** 三个内置视图的 tab。歌单页不显示（它不是内置视图，标题已经说明在哪） */
  const BUILTIN_TABS = [
    { id: 'library', label: '全部曲目' },
    { id: 'recent', label: '最近播放' },
    { id: 'mostPlayed', label: '常听' },
  ];

  const filterChips = $derived([
    ...(ui.artistId ? [{ label: '艺人筛选', onclear: () => list.clearFilter() }] : []),
    ...(ui.albumId ? [{ label: '专辑筛选', onclear: () => list.clearFilter() }] : []),
  ]);

  function switchView(id: string): void {
    if (id === 'library' || id === 'recent' || id === 'mostPlayed') {
      nav.go({ kind: 'builtin', view: id });
    }
  }

  /** 勾选框：点=切换、Shift=连选（与 ctrl+点行的语义不同） */
  function handleCheck(index: number, event: MouseEvent): void {
    if (selection.handleCheck(index, event) === 'capped') {
      notify.showToast(`单次最多选择 ${SELECTION_CAP} 首，已截取刚点中的一段`);
    }
  }
</script>

<div class="page-pad">
  <PageHeader
    kicker={header.kicker}
    title={header.title}
    subtitle={header.subtitle}
    meta={header.meta}
    cover={header.cover}
  >
    {#snippet actions()}
      <button class="btn primary" onclick={() => void list.playAll()} disabled={header.playDisabled}>
        <Icon name="play" size={12} /> 播放全部
      </button>
      {#if view.kind === 'playlist' && currentPlaylist}
        <button
          class="btn"
          onclick={() => {
            if (currentPlaylist) overlay.dialog = { kind: 'renamePlaylist', playlist: currentPlaylist };
          }}
        >
          <Icon name="edit" size={12} /> 重命名
        </button>
        <button
          class="btn"
          disabled={currentPlaylist.trackCount === 0}
          onclick={() => {
            if (currentPlaylist) void playlists.clear(currentPlaylist);
          }}
        >
          清空
        </button>
        <button
          class="btn danger"
          onclick={() => {
            if (currentPlaylist) overlay.dialog = { kind: 'deletePlaylist', playlist: currentPlaylist };
          }}
        >
          <Icon name="trash" size={12} /> 删除
        </button>
      {:else if view.kind === 'recent'}
        <button class="btn" onclick={() => void history.clear()}>清空历史</button>
      {/if}
    {/snippet}
  </PageHeader>

  <PageToolbar
    tabs={view.kind === 'playlist' ? undefined : BUILTIN_TABS}
    active={view.kind}
    ontab={switchView}
    filters={filterChips}
  />
</div>

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
      <button class="btn" onclick={() => nav.go({ kind: 'builtin', view: 'library' })}>去曲库看看</button>
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
