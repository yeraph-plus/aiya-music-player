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

  const header = $derived.by(() => {
    const al = album;
    return {
      title: al?.title ?? ui.albumId ?? '专辑',
      subtitle: al?.artist ?? undefined,
      meta:
        al === null
          ? `${list.total} 首`
          : `${al.trackCount} 首` +
            (al.year ? ` · ${al.year}` : '') +
            (al.totalDurationMs > 0 ? ` · ${formatDurationLong(al.totalDurationMs)}` : ''),
      cover: al?.artworkId ? (artworkUrl(al.artworkId, 320) ?? null) : null,
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
