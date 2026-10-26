<script lang="ts">
  import Icon from '../components/Icon.svelte';
  import Header from '../components/content/Header.svelte';
  import Button from '../components/ui/Button.svelte';
  import Card from '../components/ui/Card.svelte';
  import Toolbar from '../components/content/Toolbar.svelte';
  import TrackListPanel from '../components/TrackListPanel.svelte';
  import { artworkUrl } from '../lib/artwork';
  import { formatDurationLong } from '../lib/format';
  import { DEFAULT_TIME_FILTER } from '../lib/time-filter';
  import { viewCanSort } from '../lib/view';
  import { library } from '../stores/library.svelte';
  import { list } from '../stores/list.svelte';
  import { facets } from '../stores/facets.svelte';
  import { nav } from '../stores/nav.svelte';
  import { overlay } from '../stores/overlay.svelte';
  import { playlists } from '../stores/playlists.svelte';
  import { scan } from '../stores/scan.svelte';
  import { ui } from '../stores/ui.svelte';

  /**
   * 曲库主区（docs/28 收窄）：只负责**曲库 / 歌单 / 搜索**三个列表页。
   * 专辑/艺人详情页拆去了 AlbumView/ArtistView（参数路由直达），
   * 最近播放/常听单页退役（常听收进推荐页 top-15）。
   * 列表接线在 TrackListPanel——这里只剩头部、筛选 chip 与搜索聚合段。
   */

  const view = $derived(ui.view);
  const currentPlaylist = $derived(
    view.kind === 'playlist' ? playlists.byId(view.id) : null,
  );

  /**
   * 标题区数据。歌单有一张 92px 封面，搜索/曲库只有文字——Header 的
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
      subtitle: undefined as string | undefined,
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

  /**
   * 时间筛选 chip（docs/27）：筛选器模态框应用后在这里给一个可清除的入口。
   * 只在曲库页显示——艺人/专辑"筛选"已升级为详情页路由（B4-3b），
   * 详情页拆出后曲库页只剩时间筛选这一种可清除状态。
   */
  const filterChips = $derived(
    ui.page === 'library' && list.timeFilterOn
      ? [{ label: '时间筛选', onclear: () => list.setTimeFilter({ ...DEFAULT_TIME_FILTER }) }]
      : [],
  );
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
      {/if}
    {/snippet}
  </Header>

  <!--
    docs/28：曲库页的第一层 tab（全部曲目/专辑/艺人）与分隔线撤掉——
    专辑/艺人升侧栏一级项后 tab 只剩"自己指自己"，chips 行在无筛选时
    整条不渲染（Toolbar 的行为），不留孤立的分隔线。
  -->
  <Toolbar filters={filterChips} />
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
{:else}
  <TrackListPanel
    canSort={viewCanSort(view)}
    removeLabel={view.kind === 'playlist' ? '从播放列表移除' : '从曲库中删除'}
    showFilter={ui.page === 'library'}
    onmove={view.kind === 'playlist' ? (from, to) => void playlists.moveItem(from, to) : undefined}
  >
    {#snippet empty()}
      {#if ui.search.trim()}
        <div class="empty">
          <strong>没有匹配的曲目</strong>
          <span>换个关键词，或清除搜索条件</span>
          <Button onclick={() => list.clearSearch()}>清除搜索</Button>
        </div>
      {:else if view.kind === 'playlist'}
        <div class="empty">
          <strong>这个播放列表还是空的</strong>
          <span>在曲库里选中歌曲，右键或批量操作加入这里</span>
          <Button onclick={() => nav.go({ kind: 'builtin', view: 'library' })}>去曲库看看</Button>
        </div>
      {:else}
        <div class="empty">
          <strong>曲库还是空的</strong>
          <span>选择一个音乐目录，AIYA 会扫描并建立索引</span>
          <Button variant="primary" onclick={() => void scan.pickRootAndScan()} disabled={library.scanning}>
            选择音乐目录
          </Button>
        </div>
      {/if}
    {/snippet}
  </TrackListPanel>
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
