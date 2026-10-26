<script lang="ts">
  import Icon from '../components/Icon.svelte';
  import Header from '../components/content/Header.svelte';
  import Button from '../components/ui/Button.svelte';
  import Card from '../components/ui/Card.svelte';
  import Toolbar from '../components/content/Toolbar.svelte';
  import TrackList from '../components/TrackList.svelte';
  import { artworkUrl } from '../lib/artwork';
  import { formatDurationLong } from '../lib/format';
  import { SELECTION_CAP } from '../lib/selection';
  import { LIB_TABS, viewCanSort, viewTitle } from '../lib/view';
  import { history } from '../stores/history.svelte';
  import { library } from '../stores/library.svelte';
  import { list } from '../stores/list.svelte';
  import { facets } from '../stores/facets.svelte';
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
   * 专辑/艺人详情页（B4-3b）的头部数据。曲目列表与曲库页共用一条查询路径
   * （source=library + albumId/artistId 筛选，见 `ui.enterFacet`），差别只在头部。
   *
   * 详情页可能被启动恢复直接命中（facets 缓存还是空），所以这里负责补拉
   * 一次聚合列表再查单条；查不到（条目刚被移除）时 title 用 id 兜底。
   */
  const facet = $derived.by(() => {
    if (ui.page === 'album') {
      return { kind: 'album' as const, data: facets.albumById(ui.albumId ?? '') };
    }
    if (ui.page === 'artist') {
      return { kind: 'artist' as const, data: facets.artistById(ui.artistId ?? '') };
    }
    return null;
  });

  $effect(() => {
    if (ui.page === 'album' && facets.albums === null) void facets.loadAlbums();
    if (ui.page === 'artist' && facets.artists === null) void facets.loadArtists();
  });

  /**
   * 标题区数据。
   *
   * 三个视图的头部形状原本分成两套（歌单有一张 92px 封面，统计只有文字），
   * 分别写在 ViewHeader 的两个 `{#if}` 分支里。现在归一到 Header 的
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
    if (facet) {
      if (facet.kind === 'album') {
        const al = facet.data;
        return {
          kicker: '专辑',
          title: al?.title ?? ui.albumId ?? '专辑',
          subtitle: al?.artist ?? undefined,
          meta:
            al === null
              ? `${list.total} 首`
              : `${al.trackCount} 首` +
                (al.year ? ` · ${al.year}` : '') +
                (al.totalDurationMs > 0 ? ` · ${formatDurationLong(al.totalDurationMs)}` : ''),
          cover: al?.artworkId ? (artworkUrl(al.artworkId, 320) ?? null) : null,
          playDisabled: list.total === 0,
        };
      }
      const ar = facet.data;
      return {
        kicker: '艺人',
        title: ar?.name ?? ui.artistId ?? '艺人',
        subtitle: undefined,
        meta:
          ar === null
            ? `${list.total} 首`
            : `${ar.trackCount} 首 · ${ar.albumCount} 张专辑` +
              (ar.totalDurationMs > 0 ? ` · ${formatDurationLong(ar.totalDurationMs)}` : ''),
        cover: ar?.artworkId ? (artworkUrl(ar.artworkId, 320) ?? null) : null,
        playDisabled: list.total === 0,
      };
    }
    if (isSearchPage) {
      return {
        kicker: '搜索',
        title: ui.search.trim() ? `搜索「${ui.search.trim()}」` : '搜索',
        subtitle: '曲目、专辑与艺人同时匹配',
        meta: `${list.total} 首`,
        cover: undefined as string | null | undefined,
        playDisabled: list.total === 0,
      };
    }
    return {
      kicker: '曲库',
      title: '全部曲目',
      meta: `${list.total} 首`,
      cover: undefined as string | null | undefined,
      playDisabled: list.total === 0,
    };
  });

  /* ---------------- 搜索结果页（B4-3c） ---------------- */

  const isSearchPage = $derived(ui.page === 'search');

  /**
   * 搜索页的聚合段：曲目列表之外再给专辑/艺人两段（docs/24 §3.2 路由表
   * "分段：曲目 / 专辑 / 艺人"）。独立 200ms 防抖——曲目查询的 120ms 防抖
   * 在 list store 里，两边互不依赖；聚合是毫秒级，防的只是连打刷 IPC。
   * 空词不拉（空搜索页显示提示，不是全库）。
   */
  let facetSearchTimer: ReturnType<typeof setTimeout> | undefined;
  let facetQuery = $state('');
  $effect(() => {
    if (!isSearchPage) return;
    const q = ui.search.trim();
    clearTimeout(facetSearchTimer);
    facetSearchTimer = setTimeout(
      () => {
        facetQuery = q;
        if (!q) return;
        void facets.loadAlbums(q);
        void facets.loadArtists(q);
      },
      200,
    );
    return () => clearTimeout(facetSearchTimer);
  });

  /** 卡片段只在查询词与当前展示一致时渲染（防抖期间的旧结果显示旧段） */
  const searchAlbums = $derived(
    isSearchPage && facetQuery ? facets.albums?.slice(0, 8) ?? null : null,
  );
  const searchArtists = $derived(
    isSearchPage && facetQuery ? facets.artists?.slice(0, 8) ?? null : null,
  );

  function handleSelect(index: number, event: MouseEvent | KeyboardEvent): void {
    if (selection.handleSelect(index, event) === 'capped') {
      notify.showToast(`单次最多选择 ${SELECTION_CAP} 首，已截取刚点中的一段`);
    }
  }

  /**
   * 曲库页的 tab（docs/25 §3 重排后）。最近播放 / 常听已升级为侧栏一级页，
   * 不再寄居曲库 tab 行；专辑 / 艺人是导航式 tab（进 browse 宫格页）。
   * tab 条只在曲库页显示——专辑/艺人**宫格页**的 tab 条在 BrowseView 里
   * （共用 LIB_TABS）；歌单/详情页不显示（标题已经说明在哪）。
   */
  const showBuiltinTabs = $derived(ui.page === 'library');

  /**
   * 筛选 chip 只在曲库页显示。专辑/艺人详情页的"筛选"就是页面本身
   * （ui.albumId/ui.artistId 是路由参数），显示"专辑筛选 ×"chip 等于
   * 让用户能把当前页面拆掉——点 × 会清掉路由参数却不换页。
   */
  const filterChips = $derived(
    ui.page === 'library'
      ? [
          ...(ui.artistId ? [{ label: '艺人筛选', onclear: () => list.clearFilter() }] : []),
          ...(ui.albumId ? [{ label: '专辑筛选', onclear: () => list.clearFilter() }] : []),
        ]
      : [],
  );

  function switchView(id: string): void {
    if (id === 'library') {
      nav.go({ kind: 'builtin', view: 'library' });
    } else if (id === 'albums' || id === 'artists') {
      nav.go({ kind: 'browse', view: id });
    }
  }

  /** 勾选框：点=切换、Shift=连选（与 ctrl+点行的语义不同） */
  function handleCheck(index: number, event: MouseEvent | KeyboardEvent): void {
    if (selection.handleCheck(index, event) === 'capped') {
      notify.showToast(`单次最多选择 ${SELECTION_CAP} 首，已截取刚点中的一段`);
    }
  }
</script>

<div class="page-pad">
  <Header
    kicker={header.kicker}
    title={header.title}
    subtitle={header.subtitle}
    meta={header.meta}
    cover={header.cover}
  >
    {#snippet actions()}
      <Button variant="primary" onclick={() => void list.playAll()} disabled={header.playDisabled}>
        {#snippet icon()}<Icon name="play" size={12} />{/snippet}
        播放全部
      </Button>
      {#if view.kind === 'playlist' && currentPlaylist}
        <Button
          onclick={() => {
            if (currentPlaylist) overlay.dialog = { kind: 'renamePlaylist', playlist: currentPlaylist };
          }}
        >
          {#snippet icon()}<Icon name="edit" size={12} />{/snippet}
          重命名
        </Button>
        <Button
          disabled={currentPlaylist.trackCount === 0}
          onclick={() => {
            if (currentPlaylist) void playlists.clear(currentPlaylist);
          }}
        >
          清空
        </Button>
        <Button
          variant="danger"
          onclick={() => {
            if (currentPlaylist) overlay.dialog = { kind: 'deletePlaylist', playlist: currentPlaylist };
          }}
        >
          {#snippet icon()}<Icon name="trash" size={12} />{/snippet}
          删除
        </Button>
      {:else if view.kind === 'recent'}
        <Button onclick={() => void history.clear()}>清空历史</Button>
      {/if}
    {/snippet}
  </Header>

  <Toolbar
    tabs={showBuiltinTabs ? [...LIB_TABS] : undefined}
    active={ui.page}
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
    <Button size="sm" onclick={() => scan.cancel()}>取消</Button>
  </div>
{/if}

{#if isSearchPage && (searchAlbums?.length || searchArtists?.length)}
  <div class="facet-rows">
    {#if searchAlbums?.length}
      <section class="facet-row">
        <h2>专辑</h2>
        <div class="facet-scroll">
          {#each searchAlbums as al (al.id)}
            <Card
              title={al.title}
              subtitle={al.artist ? `${al.artist} · ${al.trackCount} 首` : `${al.trackCount} 首`}
              cover={al.artworkId ? artworkUrl(al.artworkId, 256) : null}
              onclick={() => nav.go({ kind: 'album', id: al.id })}
            />
          {/each}
        </div>
      </section>
    {/if}
    {#if searchArtists?.length}
      <section class="facet-row">
        <h2>艺人</h2>
        <div class="facet-scroll">
          {#each searchArtists as ar (ar.id)}
            <Card
              title={ar.name}
              subtitle={`${ar.trackCount} 首 · ${ar.albumCount} 张专辑`}
              cover={ar.artworkId ? artworkUrl(ar.artworkId, 256) : null}
              onclick={() => nav.go({ kind: 'artist', id: ar.id })}
            />
          {/each}
        </div>
      </section>
    {/if}
  </div>
{/if}

{#if isSearchPage && !ui.search.trim()}
  <div class="empty">
    <strong>在上方搜索框输入关键词</strong>
    <span>曲目、专辑与艺人会同时匹配</span>
  </div>
{:else if list.total > 0}
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
    rowHeight={ui.rowHeight}
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
      <Button onclick={() => void list.reload()}>重试</Button>
    {:else if ui.search.trim()}
      <strong>没有匹配的曲目</strong>
      <span>换个关键词，或清除搜索条件</span>
      <Button onclick={() => list.clearSearch()}>清除搜索</Button>
    {:else if view.kind === 'recent'}
      <strong>还没有播放记录</strong>
      <span>听几首歌，这里会按时间倒序记录</span>
    {:else if view.kind === 'mostPlayed'}
      <strong>还没有常听曲目</strong>
      <span>播放次数会从零开始积累</span>
    {:else if ui.page === 'album'}
      <strong>这张专辑里没有曲目</strong>
      <span>曲目可能已被移出曲库</span>
      <Button onclick={() => nav.go({ kind: 'browse', view: 'albums' })}>返回专辑</Button>
    {:else if ui.page === 'artist'}
      <strong>这位艺人没有曲目</strong>
      <span>曲目可能已被移出曲库</span>
      <Button onclick={() => nav.go({ kind: 'browse', view: 'artists' })}>返回艺人</Button>
    {:else if view.kind === 'playlist'}
      <strong>这个播放列表还是空的</strong>
      <span>在曲库里选中歌曲，右键或批量操作加入这里</span>
      <Button onclick={() => nav.go({ kind: 'builtin', view: 'library' })}>去曲库看看</Button>
    {:else}
      <strong>曲库还是空的</strong>
      <span>选择一个音乐目录，AIYA 会扫描并建立索引</span>
      <Button variant="primary" onclick={() => void scan.pickRootAndScan()} disabled={library.scanning}>
        选择音乐目录
      </Button>
    {/if}
  </div>
{:else}
  <div class="empty"><strong>正在启动…</strong></div>
{/if}

<style>
  /* 搜索结果页的专辑/艺人段（B4-3c）。横向卡片行：各最多 8 张，横向滚动，
     主区留给曲目列表（虚拟滚动的重心） */
  .facet-rows {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
    padding: 0 22px var(--space-3);
  }

  .facet-row h2 {
    margin: 0 0 var(--space-2);
    font-size: var(--fs-sm);
    font-weight: 600;
    color: var(--text-2);
  }

  .facet-scroll {
    display: grid;
    grid-auto-flow: column;
    grid-auto-columns: minmax(200px, 1fr);
    gap: var(--space-2);
    overflow-x: auto;
    padding-bottom: 2px;
  }
</style>
