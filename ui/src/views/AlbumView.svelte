<script lang="ts">
  import Icon from '../components/Icon.svelte';
  import Header from '../components/content/Header.svelte';
  import Button from '../components/ui/Button.svelte';
  import TrackListPanel from '../components/TrackListPanel.svelte';
  import { artworkUrl } from '../lib/artwork';
  import { formatDurationLong } from '../lib/format';
  import { facets } from '../stores/facets.svelte';
  import { list } from '../stores/list.svelte';
  import { nav } from '../stores/nav.svelte';
  import { ui } from '../stores/ui.svelte';

  /**
   * 专辑详情页（docs/28 拆出）：此前寄居 LibraryView，靠 `ui.page` 分支
   * 拼头部——现在页面各归各，路由参数（`ui.albumId`）直接可寻址，
   * 列表接线共用 TrackListPanel。
   *
   * 曲目列表与曲库页同一条查询路径（source=library + albumId 筛选），
   * 详情页可能被启动恢复直接命中（facets 缓存还是空），所以这里负责
   * 补拉一次聚合列表再查单条；查不到（条目刚被移除）时 title 用 id 兜底。
   */
  const album = $derived(facets.albumById(ui.albumId ?? ''));

  $effect(() => {
    if (facets.albums === null) void facets.loadAlbums();
  });

  /**
   * 列表首行。TrackWindow 是非响应式缓存，靠 `list.version` 计数器驱动重算
   * （与 TrackList 渲染同一套信号）。详情页打开时 `list.openFacet` 已把
   * 首页数据 prime 进窗口，这里几乎总是立即可用。
   */
  const firstRow = $derived.by(() => {
    void list.version;
    return list.trackWindow.get(0);
  });

  const header = $derived.by(() => {
    const al = album;
    const row = firstRow;
    // 名字回落链：聚合缓存 → 列表首行 → 通用文案。**绝不把路由参数（uuid）
    // 渲染出来**——那是一串 v5 uuid（docs/28 真机回归）：聚合缓存加载慢或
    // 失败时，首行的 album/artist 就是从 DB 读出的真名，和列表内容同源。
    const title = al?.title ?? row?.album ?? '专辑';
    // 封面回落链：聚合缓存的 artworkId → 首曲的 artworkId。扫描器给每条
    // 曲目都写了封面 id，专辑的首曲封面即专辑封面。
    const artworkId = al?.artworkId ?? row?.artworkId ?? null;
    return {
      title,
      subtitle: al?.artist ?? row?.artist ?? undefined,
      meta:
        al === null
          ? `${list.total} 首`
          : `${al.trackCount} 首` +
            (al.year ? ` · ${al.year}` : '') +
            (al.totalDurationMs > 0 ? ` · ${formatDurationLong(al.totalDurationMs)}` : ''),
      cover: artworkId ? (artworkUrl(artworkId, 320) ?? null) : null,
    };
  });
</script>

<div class="page-pad">
  <Header
    kicker="专辑"
    title={header.title}
    subtitle={header.subtitle}
    meta={header.meta}
    cover={header.cover}
  >
    {#snippet actions()}
      <Button variant="primary" onclick={() => void list.playAll()} disabled={list.total === 0}>
        {#snippet icon()}<Icon name="play" size={12} />{/snippet}
        播放全部
      </Button>
    {/snippet}
  </Header>
</div>

<!--
  专辑页把"专辑"列链接废掉（disableAlbumLink）：点了指向的是当前页，
  nav 视为刷新，只会白丢一次滚动位置。歌手列保持可点——那是真正能走的路。
-->
<TrackListPanel canSort={true} removeLabel="从曲库中删除" showFilter={false} disableAlbumLink={true}>
  {#snippet empty()}
    <div class="empty">
      <strong>这张专辑里没有曲目</strong>
      <span>曲目可能已被移出曲库</span>
      <Button onclick={() => nav.go({ kind: 'browse', view: 'albums' })}>返回专辑</Button>
    </div>
  {/snippet}
</TrackListPanel>
