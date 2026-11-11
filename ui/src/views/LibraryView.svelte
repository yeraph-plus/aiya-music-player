<script lang="ts">
  import { ArrowDown, ArrowUp } from '@lucide/svelte';
  import Icon from '../components/Icon.svelte';
  import TextTabs from '../components/ui/TextTabs.svelte';
import Progress from '../components/ui/Progress.svelte';
  import Header from '../components/content/Header.svelte';
  import Button from '../components/ui/Button.svelte';
  import Card from '../components/ui/Card.svelte';
  import Toolbar from '../components/content/Toolbar.svelte';
  import TrackListPanel from '../components/TrackListPanel.svelte';
  import EmptyState from '../components/ui/EmptyState.svelte';
  import { artworkUrl } from '../lib/artwork';
  import { formatDurationLong } from '../lib/format';
  import { t } from '../lib/i18n.svelte';
  import { DEFAULT_TIME_FILTER } from '../lib/time-filter';
  import { library } from '../stores/library.svelte';
  import { list } from '../stores/list.svelte';
  import { facets } from '../stores/facets.svelte';
  import { nav } from '../stores/nav.svelte';
  import { overlay } from '../stores/overlay.svelte';
  import { playlists } from '../stores/playlists.svelte';
  import { scan } from '../stores/scan.svelte';
  import { ui } from '../stores/ui.svelte';

  /**
   * 曲库主区：只负责**曲库 / 歌单 / 搜索**三个列表页。
   * 专辑/艺人详情页在 AlbumView/ArtistView（参数路由直达）；
   * 常听的入口在推荐页 top-15。
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
        kicker: 'PLAYLIST',
        title: playlist?.name ?? t('library.playlistFallback'),
        // 动态歌单是规则聚合，手动歌单是顺序编排——描述跟着种类说实话
        subtitle: (playlist?.kind === 'smart'
          ? t('library.playlistSmartSubtitle')
          : t('library.playlistManualSubtitle')) as string | undefined,
        meta: playlist
          ? t('library.playlistMeta', { n: playlist.trackCount, duration: formatDurationLong(playlist.durationMs) })
          : '',
        cover: playlist?.artworkId ? (artworkUrl(playlist.artworkId, 256) ?? null) : null,
        playDisabled: !playlist || playlist.trackCount === 0,
      };
    }
    if (isSearchPage) {
      return {
        kicker: 'SEARCH',
        title: ui.search.trim() ? t('library.searchTitle', { q: ui.search.trim() }) : t('library.searchTitleEmpty'),
        subtitle: t('library.searchSubtitle'),
        meta: t('library.searchMeta', { n: list.total }),
        cover: undefined as string | null | undefined,
        playDisabled: list.total === 0,
      };
    }
    return {
      // 标题随 tab 走（最近播放/最近添加是曲库页的 tab 索引）
      kicker: 'LIBRARY',
      title: LIB_TABS.find((tab) => tab.key === ui.libraryTab)?.label ?? t('library.tabAll'),
      subtitle: t('library.subtitle'),
      meta: t('common.trackCount', { n: list.total }),
      cover: undefined as string | null | undefined,
      playDisabled: list.total === 0,
    };
  });

  /* 搜索结果页 */

  const isSearchPage = $derived(ui.page === 'search');

  /**
   * 曲库页 tab 索引：「全部曲目」旁的三个小标题 tab。排序控件只在「全部曲目」给——
   * 另两个 tab 的顺序是语义（默认倒序），再点一次整 tab 翻转正倒序
   * （`list.openLibraryTab`），列头排序对它们没有意义。
   */
  const LIB_TABS: { key: 'all' | 'added' | 'recent'; label: string }[] = $derived([
    { key: 'all', label: t('library.tabAll') },
    { key: 'added', label: t('library.tabAdded') },
    { key: 'recent', label: t('library.tabRecent') },
  ]);

  /**
   * 排序控件开关：曲库页非 all tab 的顺序是语义，不给列头排序；手动歌单的顺序
   * 也是语义（拖拽排序 + `pi.position`），同样不吃列头；动态歌单物化后的顺序
   * 等于入库序（首行插入在 0），与手动歌单同一套语义。列头排序只服务曲库页。
   */
  const canSort = $derived(view.kind === 'library' && ui.libraryTab === 'all');

  /**
   * 搜索页的聚合段：曲目列表之外再给专辑/艺人两段（路由表
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

  /** 卡片段只在查询词与当前展示一致时渲染（防抖期间保留上一次结果）。
   *  搜索命中多少给多少，不截断——换行网格放得下，后端聚合自带上限 */
  const searchAlbums = $derived(
    isSearchPage && facetQuery ? facets.albums : null,
  );
  const searchArtists = $derived(
    isSearchPage && facetQuery ? facets.artists : null,
  );

  /**
   * 时间筛选 chip：筛选器模态框应用后在这里给一个可清除的入口。
   * 只在曲库页显示——艺人/专辑的入口是详情页路由（非筛选动作），
   * 曲库页只剩时间筛选这一种可清除状态。
   */
  const filterChips = $derived(
    ui.page === 'library' && list.timeFilterOn
      ? [{ label: t('library.filterChip'), onclear: () => list.setTimeFilter({ ...DEFAULT_TIME_FILTER, conds: [] }) }]
      : [],
  );
</script>

<!-- 根容器：搜索页整页滚动。卡片段换行后可能很高，若仍由 .list 内部独扛滚动，卡片段会把列表压到没法用（flex 收缩，滚动"失效"）。非搜索页 display 行为与直接挂在
.main 下完全一致（list 内部滚）；搜索页整页滚，列表区保底高度不被挤没（见 .lib-root.scroll）。 -->
<div class="lib-root" class:scroll={isSearchPage}>
<div class="page-pad">
  <Header
    kicker={header.kicker}
    title={header.title}
    subtitle={header.subtitle}
    meta={header.meta}
    cover={header.cover}
  >
    {#snippet actions()}
      {#if ui.page === 'library'}
        <!-- tab 索引：小标题形态贴着大标题，当前项高亮。只在曲库页给—— 搜索页复用 library view，但三个 tab 在那里是"同库换排序"，语义不成立。语义归 TextTabs（nav +
        aria-current）：套完整的 tablist/tab 角色而没有 tabpanel，会让读屏软件等待一个不存在的面板。 -->
        <TextTabs
          items={LIB_TABS.map((tab) => ({
            id: tab.key,
            label: tab.label,
            title:
              tab.key === 'all'
                ? t('library.tabAllTip')
                : t('library.tabSortTip', { dir: ui.direction === 'desc' ? t('library.dirDesc') : t('library.dirAsc') }),
          }))}
          value={ui.libraryTab}
          onchange={(key) => list.openLibraryTab(key as (typeof LIB_TABS)[number]['key'])}
          label={t('library.tabsLabel')}
        >
          {#snippet trailing(tab)}
            <!-- 方向箭头只画在活跃 tab 上：ui.direction 是全局共享状态，
                 非活跃 tab 上的箭头会与实际顺序不符 -->
            {#if ui.libraryTab === tab.id && tab.id !== 'all'}
              {#if ui.direction === 'desc'}
                <ArrowDown size={11} strokeWidth={2.4} aria-hidden="true" />
              {:else}
                <ArrowUp size={11} strokeWidth={2.4} aria-hidden="true" />
              {/if}
            {/if}
          {/snippet}
        </TextTabs>
      {/if}
      {#if view.kind === 'playlist'}
        <Button variant="primary" onclick={() => void list.playAll()} disabled={header.playDisabled}>
          {#snippet icon()}<Icon name="play" size={12} />{/snippet}
          {t('library.playAll')}
        </Button>
        {#if currentPlaylist}
          {#if currentPlaylist.kind === 'smart'}
            <!-- 物化模型：刷新重跑规则，只追加新命中的曲目 -->
            <Button
              onclick={() => {
                if (currentPlaylist) void playlists.refreshSmart(currentPlaylist);
              }}
              disabled={library.scanning}
            >
              {#snippet icon()}<Icon name="refresh" size={12} />{/snippet}
              {t('library.refresh')}
            </Button>
          {/if}
          <Button
            onclick={() => {
              if (currentPlaylist) overlay.dialog = { kind: 'playlistEdit', playlist: currentPlaylist };
            }}
          >
            {#snippet icon()}<Icon name="edit" size={12} />{/snippet}
            {t('library.edit')}
          </Button>
          <Button
            disabled={currentPlaylist.trackCount === 0}
            onclick={() => {
              if (currentPlaylist) void playlists.clear(currentPlaylist);
            }}
          >
            {t('library.clear')}
          </Button>
          <Button
            variant="danger"
            onclick={() => {
              if (currentPlaylist) overlay.dialog = { kind: 'deletePlaylist', playlist: currentPlaylist };
            }}
          >
            {#snippet icon()}<Icon name="trash" size={12} />{/snippet}
            {t('common.delete')}
          </Button>
        {/if}
      {/if}
    {/snippet}
  </Header>

  <!--
    曲库页的第一层 tab（全部曲目/专辑/艺人）与分隔线撤掉
    专辑/艺人升侧栏一级项后 tab 只剩"自己指自己"，chips 行在无筛选时
    整条不渲染（Toolbar 的行为），不留孤立的分隔线。
  -->
  <Toolbar filters={filterChips} />
</div>

{#if library.scan && library.scanning}
  {@const progress = library.scan}
  <!-- 扫描时的环境指示：只报进度不带按钮。全应用唯一的取消入口在设置页
       「扫描入库」的执行状态卡，不另造第二份取消 -->
  <div class="scan-bar">
    <Progress value={progress.total ? Math.min(100, Math.round((progress.scanned / progress.total) * 100)) : 8} label={t('library.scanProgressLabel')} />
    <span>
      {progress.phase === 'walking'
        ? t('library.scanWalking')
        : t('library.scanLive', { scanned: progress.scanned, total: progress.total ?? '?', imported: progress.imported, skipped: progress.skipped, failed: progress.failed })}
    </span>
  </div>
{/if}

{#if isSearchPage && (searchAlbums?.length || searchArtists?.length)}
  <div class="facet-rows">
    {#if searchAlbums?.length}
      <section class="facet-row">
        <h2>{t('library.sectionAlbums')}</h2>
        <div class="facet-grid">
          {#each searchAlbums as al (al.id)}
            <Card
              title={al.title}
              subtitle={al.artist
                ? `${al.artist} · ${t('common.trackCount', { n: al.trackCount })}`
                : t('common.trackCount', { n: al.trackCount })}
              cover={al.artworkId ? artworkUrl(al.artworkId, 256) : null}
              onclick={() => nav.go({ kind: 'album', id: al.id })}
            />
          {/each}
        </div>
      </section>
    {/if}
    {#if searchArtists?.length}
      <section class="facet-row">
        <h2>{t('library.sectionArtists')}</h2>
        <div class="facet-grid">
          {#each searchArtists as ar (ar.id)}
            <Card
              title={ar.name}
              subtitle={t('browse.artistCardMeta', { n: ar.trackCount, albums: ar.albumCount })}
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
  <EmptyState fill title={t('library.searchEmptyTitle')} hint={t('library.searchEmptyHint')} />
{:else}
  <!-- 搜索页的曲目小标题：与上面的专辑/艺人两段同一级，明确隔开。
       其他页面没有这段 -->
  {#if isSearchPage}
    <div class="facet-rows tracks-head"><h2>{t('library.sectionTracks')}</h2></div>
  {/if}
  <TrackListPanel
    {canSort}
    removeLabel={view.kind === 'playlist' ? t('library.removeFromPlaylist') : t('library.deleteFromLibrary')}
    showFilter={ui.page === 'library'}
    dragSort={
      view.kind === 'playlist' ? (from, to) => void playlists.moveItem(from, to) : undefined
    }
  >
    {#snippet empty()}
      {#if ui.search.trim()}
        <EmptyState title={t('library.noMatchTitle')} hint={t('library.noMatchHint')}>
          {#snippet action()}
            <Button onclick={() => list.clearSearch()}>{t('shell.titlebar.searchClear')}</Button>
          {/snippet}
        </EmptyState>
      {:else if list.timeFilterOn}
        <!-- 筛选到 0 行 ≠ 空库：文案必须分开，否则用户会去重新扫描 -->
        <EmptyState title={t('library.filteredEmptyTitle')} hint={t('library.filteredEmptyHint')}>
          {#snippet action()}
            <Button onclick={() => list.setTimeFilter({ ...DEFAULT_TIME_FILTER, conds: [] })}>
              {t('library.clearFilter')}
            </Button>
          {/snippet}
        </EmptyState>
      {:else if view.kind === 'playlist'}
        <EmptyState title={t('library.playlistEmptyTitle')} hint={t('library.playlistEmptyHint')}>
          {#snippet action()}
            <Button onclick={() => nav.go({ kind: 'builtin', view: 'library' })}>{t('library.goToLibrary')}</Button>
          {/snippet}
        </EmptyState>
      {:else if view.kind === 'library' && ui.libraryTab === 'recent' && (library.stats?.tracks ?? 0) > 0}
        <!-- 「最近播放」空 ≠ 空库：库里有歌只是还没播过，"添加曲库"文案会把用户引去重扫。库真空时才走下面的"曲库还是空的" —— 那才是该给的动作 -->
        <EmptyState title={t('library.recentEmptyTitle')} hint={t('library.recentEmptyHint')}>
          {#snippet action()}
            <Button onclick={() => list.openLibraryTab('all')}>{t('library.recentEmptyAction')}</Button>
          {/snippet}
        </EmptyState>
      {:else}
        <EmptyState title={t('library.emptyTitle')} hint={t('library.emptyHint')}>
          {#snippet action()}
            <Button variant="primary" onclick={() => void scan.pickRootAndScan()} disabled={library.scanning}>
              {t('library.chooseFolder')}
            </Button>
          {/snippet}
        </EmptyState>
      {/if}
    {/snippet}
  </TrackListPanel>
{/if}
</div><!-- /lib-root -->

<style>
  .lib-root {
    flex: 1;
    min-height: 0;
    min-width: 0;
    display: flex;
    flex-direction: column;
  }

  /* 搜索页整页滚动：卡片段随页滚走；曲目区保底 340px——卡片段再高，
     列表也不会被 flex 收缩挤到没有可用视口（那就是"滚动失效"） */
  .lib-root.scroll {
    overflow-y: auto;
  }

  .lib-root.scroll :global(.list-shell) {
    min-height: 340px;
  }

  /* 搜索结果页的专辑/艺人段：换行网格，不做横滚——结果不该藏在滚动方向
     后面。每个卡 200px 起，自动换行 */
  .facet-rows {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
    padding: 0 22px var(--space-3);
  }

  .facet-row h2,
  .tracks-head h2 {
    margin: 0 0 var(--space-2);
    font-size: var(--fs-sm);
    font-weight: 600;
    color: var(--text-2);
  }

  .facet-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(200px, 1fr));
    gap: var(--space-2);
  }

  /* 曲目小标题行（与卡片段同款 h2），给单曲列表一个明确的段落起点 */
  .tracks-head {
    padding-bottom: 0;
  }
</style>
