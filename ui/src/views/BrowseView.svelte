<script lang="ts">
  import { untrack } from 'svelte';
  import { Disc, Mic2, Search, User } from '@lucide/svelte';
  import Header from '../components/content/Header.svelte';
  import Button from '../components/ui/Button.svelte';
  import Card from '../components/ui/Card.svelte';
  import EmptyState from '../components/ui/EmptyState.svelte';
  import Skeleton from '../components/ui/Skeleton.svelte';
  import type { AlbumFacet, ArtistFacet } from '../lib/ipc/generated';
  import { artworkUrl } from '../lib/artwork';
  import { formatDurationLong } from '../lib/format';
  import { facets } from '../stores/facets.svelte';
  import { nav } from '../stores/nav.svelte';
  import { ui } from '../stores/ui.svelte';

  /**
   * 专辑 / 艺人宫格浏览页（B4-3b，docs/24 §3.2 路由表 albums/artists）。
   *
   * docs/28：升为侧栏一级项（曲库页 tab 撤掉后宫格需要一级入口），
   * 宫格页上的 tab 条随之撤掉——侧栏就是这层导航，页内再放一组
   * "专辑/艺人互切"是同一个功能的第二次出现。卡片进详情页
   * （`album`/`artist` 带参数路由，docs/28 拆出的独立视图）。
   *
   * 两个模式共用一个组件：形状完全一样（宫格卡片），差别只有数据源、
   * 文案与卡片副标题——分开写两个视图得到的是两份一模一样的拉取/空态/
   * 搜索逻辑。模式由 `ui.page` 单真相派生（'albums' | 'artists'）。
   *
   * 搜索框是**页内的**（过滤本页名称），不是顶栏的全局搜索——顶栏搜索
   * 搜的是曲目，两者语义不同，也不共用防抖 store。
   */
  const mode = $derived(ui.page === 'artists' ? 'artists' : 'albums');

  let search = $state('');
  /** 输入防抖 200ms 后才发查询（聚合本身毫秒级，防的是连打刷 IPC） */
  let searchTimer: ReturnType<typeof setTimeout> | undefined;

  const rows = $derived(mode === 'artists' ? facets.artists : facets.albums);
  /** 过滤后总数（后端截断到 500 条；total > rows.length = 还有没画出来的） */
  const total = $derived(mode === 'artists' ? facets.artistsTotal : facets.albumsTotal);
  const truncated = $derived(rows !== null && total > rows.length);

  $effect(() => {
    // 只追踪 mode：search 变化走 onSearchInput 的显式重拉（防抖控制在那里）。
    // 不 untrack 的话每次击键都会先触发一遍 effect 直拉，防抖形同虚设。
    const m = mode;
    untrack(() => void (m === 'artists' ? facets.loadArtists(search || undefined) : facets.loadAlbums(search || undefined)));
    return () => clearTimeout(searchTimer);
  });

  function onSearchInput(value: string): void {
    search = value;
    clearTimeout(searchTimer);
    searchTimer = setTimeout(
      () => void (mode === 'artists' ? facets.loadArtists(search || undefined) : facets.loadAlbums(search || undefined)),
      200,
    );
  }

  const heading = $derived(
    mode === 'artists'
      ? { kicker: '曲库', title: '艺人', subtitle: '按名称浏览曲库里的全部艺人' }
      : { kicker: '曲库', title: '专辑', subtitle: '按名称浏览曲库里的全部专辑' },
  );

  const meta = $derived(
    rows === null
      ? ''
      : `${rows.length}${truncated ? ` / 共 ${total}` : ''} ${mode === 'artists' ? '位艺人' : '张专辑'}`,
  );

  /** 专辑卡片属性。艺人卡片不用封面（docs/28 修订：user 图标，见模板内分支） */
  function albumCardProps(item: AlbumFacet): { title: string; subtitle: string; cover: string | null } {
    return {
      title: item.title,
      subtitle:
        `${item.trackCount} 首` +
        (item.artist ? ` · ${item.artist}` : '') +
        (item.totalDurationMs > 0 ? ` · ${formatDurationLong(item.totalDurationMs)}` : ''),
      cover: item.artworkId ? artworkUrl(item.artworkId, 256) : null,
    };
  }
</script>

<div class="page-pad browse">
  <Header kicker={heading.kicker} title={heading.title} subtitle={heading.subtitle} {meta}>
    {#snippet actions()}
      <div class="search" data-tauri-drag-region="false">
        <Search size={14} aria-hidden="true" />
        <input
          type="search"
          placeholder={mode === 'artists' ? '按艺人名过滤' : '按专辑名过滤'}
          aria-label={mode === 'artists' ? '按艺人名过滤' : '按专辑名过滤'}
          value={search}
          oninput={(e) => onSearchInput(e.currentTarget.value)}
        />
      </div>
    {/snippet}
  </Header>

  {#if facets.loading && rows === null}
    <Skeleton kind="row" count={10} />
  {:else if rows && rows.length > 0}
    <div class="grid">
      {#each rows as item (item.id)}
        {#if 'name' in item}
          <!-- 艺人卡片：user 图标（artists 表没有封面数据，不做图片获取逻辑） -->
          <Card
            title={item.name}
            subtitle={`${item.trackCount} 首 · ${item.albumCount} 张专辑`}
            onclick={() => nav.go({ kind: 'artist', id: item.id })}
          >
            {#snippet icon()}<User size={22} />{/snippet}
          </Card>
        {:else}
          <Card
            {...albumCardProps(item)}
            onclick={() => nav.go({ kind: 'album', id: item.id })}
          />
        {/if}
      {/each}
      {#if truncated}
        <p class="trunc-hint" role="note">
          共 {total} 个，仅显示名称最靠前的 {rows.length} 个——用上方搜索框按名称收窄
        </p>
      {/if}
    </div>
  {:else if search.trim()}
    <EmptyState title="没有匹配的结果" hint="换个关键词，或清除过滤条件">
      {#snippet icon()}<Search size={26} />{/snippet}
      {#snippet action()}
        <Button onclick={() => onSearchInput('')}>清除过滤</Button>
      {/snippet}
    </EmptyState>
  {:else}
    <EmptyState
      title={mode === 'artists' ? '还没有可浏览的艺人' : '还没有可浏览的专辑'}
      hint="扫描入库时没有读到足够的标签信息；先去曲库确认曲目，再回来看"
    >
      {#snippet icon()}
        {#if mode === 'artists'}<Mic2 size={26} />{:else}<Disc size={26} />{/if}
      {/snippet}
      {#snippet action()}
        <Button onclick={() => nav.go({ kind: 'builtin', view: 'library' })}>去曲库看看</Button>
      {/snippet}
    </EmptyState>
  {/if}
</div>

<style>
  .browse {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
    height: 100%;
  }

  .search {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 0 10px;
    height: 30px;
    min-width: 200px;
    border: 1px solid var(--line);
    border-radius: var(--radius-pill);
    background: var(--surface-input);
    color: var(--text-3);
  }

  .search:focus-within {
    border-color: var(--line-strong);
  }

  .search input {
    flex: 1;
    min-width: 0;
    border: none;
    background: none;
    color: var(--text-1);
    font-size: var(--fs-sm);
  }

  .search input:focus {
    outline: none;
  }

  /* 卡片是横排形状，宫格里两行文本对齐 */
  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(240px, 1fr));
    gap: var(--space-2);
    align-content: start;
    overflow-y: auto;
    padding-bottom: var(--space-4);
    min-height: 0;
  }

  /* 截断提示：grid 末尾占满整行 */
  .trunc-hint {
    grid-column: 1 / -1;
    margin: 0;
    padding: var(--space-2) 0;
    font-size: var(--fs-sm);
    color: var(--text-3);
  }
</style>
