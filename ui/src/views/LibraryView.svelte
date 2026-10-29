<script lang="ts">
  import { ArrowDown, ArrowUp } from '@lucide/svelte';
  import Icon from '../components/Icon.svelte';
  import Header from '../components/content/Header.svelte';
  import Button from '../components/ui/Button.svelte';
  import Card from '../components/ui/Card.svelte';
  import Toolbar from '../components/content/Toolbar.svelte';
  import TrackListPanel from '../components/TrackListPanel.svelte';
  import EmptyState from '../components/ui/EmptyState.svelte';
  import { artworkUrl } from '../lib/artwork';
  import { formatDurationLong } from '../lib/format';
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
        // 2026-10-28 二次修订：与其他列表页同一标题级（大标题 + 统计），
        // kicker/subtitle 两层撤销
        kicker: undefined as string | undefined,
        title: ui.search.trim() ? `搜索「${ui.search.trim()}」` : '搜索',
        subtitle: undefined as string | undefined,
        meta: `${list.total} 首`,
        cover: undefined as string | null | undefined,
        playDisabled: list.total === 0,
      };
    }
    return {
      // 2026-10-28 用户拍板：列表页取消 kicker/描述三层标题，大标题 + 统计描述。
      // 标题随 tab 走（最近播放/最近添加已是曲库页的 tab 索引）
      kicker: undefined as string | undefined,
      title: LIB_TABS.find((t) => t.key === ui.libraryTab)?.label ?? '全部曲目',
      subtitle: undefined as string | undefined,
      meta: `${list.total} 首`,
      cover: undefined as string | null | undefined,
      playDisabled: list.total === 0,
    };
  });

  /* ---------------- 搜索结果页（B4-3c） ---------------- */

  const isSearchPage = $derived(ui.page === 'search');

  /** 列表页（非歌单非搜索）用大标题形态；搜索页 2026-10-28 起同档 */
  const isListPage = $derived(view.kind !== 'playlist');

  /**
   * 曲库页 tab 索引（2026-10-28：最近播放/最近添加从推荐页迁来，成为
   * 「全部曲目」旁的小标题 tab）。排序控件只在「全部曲目」给——
   * 另两个 tab 的顺序是语义（默认倒序），再点一次整 tab 翻转正倒序
   * （2026-10-30，list.openLibraryTab），sort 表头仍然没有意义。
   */
  const LIB_TABS = [
    { key: 'all', label: '全部曲目' },
    { key: 'added', label: '最近添加' },
    { key: 'recent', label: '最近播放' },
  ] as const;

  /**
   * 排序控件开关（2026-10-28 拍板扩展）：曲库页非 all tab 的顺序是语义；
   * 手动歌单的顺序也是语义（拖拽排序 + pi.position），不吃列头；
   * 动态歌单没有手动顺序（它是一条查询），排序交给列头——规则编辑器
   * 已不再暴露排序键。
   */
  // 动态歌单物化后（2026-10-30）顺序 = 入库序（首行插入在 0），与手动歌单
  // 同一套语义——列头排序只服务曲库页。
  const canSort = $derived(view.kind === 'library' && ui.libraryTab === 'all');

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

  /** 卡片段只在查询词与当前展示一致时渲染（防抖期间的旧结果显示旧段）。
   *  2026-10-28：横滚时代的「最多 8 张」截断撤销——换行网格放得下，
   *  搜索命中多少给多少（后端聚合自带上限） */
  const searchAlbums = $derived(
    isSearchPage && facetQuery ? facets.albums : null,
  );
  const searchArtists = $derived(
    isSearchPage && facetQuery ? facets.artists : null,
  );

  /**
   * 时间筛选 chip（docs/27）：筛选器模态框应用后在这里给一个可清除的入口。
   * 只在曲库页显示——艺人/专辑"筛选"已升级为详情页路由（B4-3b），
   * 详情页拆出后曲库页只剩时间筛选这一种可清除状态。
   */
  const filterChips = $derived(
    ui.page === 'library' && list.timeFilterOn
      ? [{ label: '筛选中', onclear: () => list.setTimeFilter({ ...DEFAULT_TIME_FILTER, conds: [] }) }]
      : [],
  );
</script>

<!--
  根容器（2026-10-28）：搜索页整页滚动——卡片段换行后可能很高，若仍由
  .list 内部独扛滚动，卡片段会把列表压到没法用（flex 收缩，滚动"失效"）。
  非搜索页 display 行为与直接挂在 .main 下完全一致（list 内部滚）；
  搜索页整页滚，列表区保底高度不被挤没（见 .lib-root.scroll）。
-->
<div class="lib-root" class:scroll={isSearchPage}>
<div class="page-pad">
  <Header
    kicker={header.kicker}
    title={header.title}
    subtitle={header.subtitle}
    meta={header.meta}
    cover={header.cover}
    big={isListPage}
  >
    {#snippet actions()}
      {#if ui.page === 'library'}
        <!-- tab 索引（2026-10-28）：小标题形态贴着大标题，当前项高亮。
             只在曲库页给——搜索页复用 library view，但三个 tab 在那里
             是"同库换排序"，语义不成立（2026-10-28 用户指出后收窄） -->
        <div class="lib-tabs" role="tablist" aria-label="曲库视图切换">
          {#each LIB_TABS as tab (tab.key)}
            <button
              class="lib-tab"
              class:on={ui.libraryTab === tab.key}
              role="tab"
              aria-selected={ui.libraryTab === tab.key}
              title={tab.key === 'all'
                ? '再点一次重新加载'
                : `再点一次切换最新/最旧顺序（当前：${ui.direction === 'desc' ? '倒序' : '正序'}）`}
              onclick={() => list.openLibraryTab(tab.key)}
            >
              {tab.label}
              <!-- 方向箭头只画在活跃 tab 上：ui.direction 是全局共享状态，
                   非活跃 tab 的箭头会说谎（2026-10-30） -->
              {#if ui.libraryTab === tab.key && tab.key !== 'all'}
                {#if ui.direction === 'desc'}
                  <ArrowDown size={11} strokeWidth={2.4} aria-hidden="true" />
                {:else}
                  <ArrowUp size={11} strokeWidth={2.4} aria-hidden="true" />
                {/if}
              {/if}
            </button>
          {/each}
        </div>
      {/if}
      {#if view.kind === 'playlist'}
        <Button variant="primary" onclick={() => void list.playAll()} disabled={header.playDisabled}>
          {#snippet icon()}<Icon name="play" size={12} />{/snippet}
          播放全部
        </Button>
        {#if currentPlaylist}
          {#if currentPlaylist.kind === 'smart'}
            <!-- 物化模型（2026-10-30）：刷新重跑规则，只追加新命中的曲目 -->
            <Button
              onclick={() => {
                if (currentPlaylist) void playlists.refreshSmart(currentPlaylist);
              }}
              disabled={library.scanning}
            >
              {#snippet icon()}<Icon name="refresh" size={12} />{/snippet}
              刷新
            </Button>
          {/if}
          <Button
            onclick={() => {
              if (currentPlaylist) overlay.dialog = { kind: 'playlistEdit', playlist: currentPlaylist };
            }}
          >
            {#snippet icon()}<Icon name="edit" size={12} />{/snippet}
            编辑
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
  <!-- 扫描时的环境指示（八轮拍板）：只报进度不带按钮——全应用唯一的
       取消入口在「扫描入库」页的执行状态卡，别再造第二份取消 -->
  <div class="scan-bar">
    <div class="progress">
      <i style={`width:${progress.total ? Math.min(100, Math.round((progress.scanned / progress.total) * 100)) : 8}%`}></i>
    </div>
    <span>
      {progress.phase === 'walking'
        ? '正在遍历目录…（到扫描入库页可取消）'
        : `${progress.scanned}/${progress.total ?? '?'} · 新增 ${progress.imported} · 跳过 ${progress.skipped} · 失败 ${progress.failed}`}
    </span>
  </div>
{/if}

{#if isSearchPage && (searchAlbums?.length || searchArtists?.length)}
  <div class="facet-rows">
    {#if searchAlbums?.length}
      <section class="facet-row">
        <h2>专辑</h2>
        <div class="facet-grid">
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
        <div class="facet-grid">
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
  <EmptyState fill title="在上方搜索框输入关键词" hint="曲目、专辑与艺人会同时匹配" />
{:else}
  <!-- 搜索页的曲目小标题（2026-10-28）：与上面的专辑/艺人两段同一级，
        明确隔开——其他页面没有这段，别渲染 -->
  {#if isSearchPage}
    <div class="facet-rows tracks-head"><h2>曲目</h2></div>
  {/if}
  <TrackListPanel
    {canSort}
    removeLabel={view.kind === 'playlist' ? '从播放列表移除' : '从曲库中删除'}
    showFilter={ui.page === 'library'}
    dragSort={
      view.kind === 'playlist' ? (from, to) => void playlists.moveItem(from, to) : undefined
    }
  >
    {#snippet empty()}
      {#if ui.search.trim()}
        <EmptyState title="没有匹配的曲目" hint="换个关键词，或清除搜索条件">
          {#snippet action()}
            <Button onclick={() => list.clearSearch()}>清除搜索</Button>
          {/snippet}
        </EmptyState>
      {:else if list.timeFilterOn}
        <!-- 筛选到 0 行 ≠ 空库：文案必须分开，否则用户会去重新扫描 -->
        <EmptyState title="筛选后没有匹配的曲目" hint="放宽时间范围，或删掉几个条件试试">
          {#snippet action()}
            <Button onclick={() => list.setTimeFilter({ ...DEFAULT_TIME_FILTER, conds: [] })}>
              清除筛选
            </Button>
          {/snippet}
        </EmptyState>
      {:else if view.kind === 'playlist'}
        <EmptyState title="这个播放列表还是空的" hint="在曲库里选中歌曲，右键或批量操作加入这里">
          {#snippet action()}
            <Button onclick={() => nav.go({ kind: 'builtin', view: 'library' })}>去曲库看看</Button>
          {/snippet}
        </EmptyState>
      {:else if view.kind === 'library' && ui.libraryTab === 'recent' && (library.stats?.tracks ?? 0) > 0}
        <!-- 「最近播放」空 ≠ 空库：库里有歌只是还没播过，"添加曲库"文案
             会把用户引去重扫（2026-10-30 分模式差分）。库真空的时候仍走
             下面的"曲库还是空的"——那才是该给的动作 -->
        <EmptyState title="还没有播放记录" hint="双击任意曲目开始播放，听过的会出现在这里">
          {#snippet action()}
            <Button onclick={() => list.openLibraryTab('all')}>先看全部曲目</Button>
          {/snippet}
        </EmptyState>
      {:else}
        <EmptyState title="曲库还是空的" hint="选择一个音乐目录，AIYA 会扫描并建立索引">
          {#snippet action()}
            <Button variant="primary" onclick={() => void scan.pickRootAndScan()} disabled={library.scanning}>
              选择音乐目录
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

  /* 曲库 tab 索引（2026-10-28）：小标题贴着大标题，当前项 accent 下划线。
     纯文字档（不做胶囊底）——它们是"换个排序看同一批歌"的轻索引 */
  .lib-tabs {
    display: flex;
    align-items: baseline;
    gap: 14px;
    margin-left: 18px;
  }

  .lib-tab {
    display: inline-flex;
    align-items: center;
    gap: 3px;
    padding: 2px 0;
    border: none;
    background: none;
    font-size: 12.5px;
    color: var(--text-2);
    border-bottom: 2px solid transparent;
  }

  .lib-tab:hover {
    color: var(--text-1);
  }

  .lib-tab.on {
    color: var(--text-1);
    font-weight: 600;
    border-bottom-color: var(--accent);
  }

  /* 搜索结果页的专辑/艺人段（B4-3c）。2026-10-28 二次修订：横滚撤掉，
     换行网格——结果不该藏在滚动方向后面；每个卡 200px 起，自动换行 */
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
