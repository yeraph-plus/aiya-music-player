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
   * 艺人详情页（docs/28 拆出）：与 AlbumView 同构——路由参数
   * （`ui.artistId`）直接可寻址，曲目列表 = 曲库查询 + artistId 筛选，
   * 列表接线共用 TrackListPanel。头部的聚合数据可能被启动恢复直接命中，
   * 缓存为空时这里补拉一次。
   */
  const artist = $derived(facets.artistById(ui.artistId ?? ''));

  $effect(() => {
    if (facets.artists === null) void facets.loadArtists();
  });

  const header = $derived.by(() => {
    const ar = artist;
    return {
      title: ar?.name ?? ui.artistId ?? '艺人',
      meta:
        ar === null
          ? `${list.total} 首`
          : `${ar.trackCount} 首 · ${ar.albumCount} 张专辑` +
            (ar.totalDurationMs > 0 ? ` · ${formatDurationLong(ar.totalDurationMs)}` : ''),
      cover: ar?.artworkId ? (artworkUrl(ar.artworkId, 320) ?? null) : null,
    };
  });
</script>

<div class="page-pad">
  <Header kicker="艺人" title={header.title} meta={header.meta} cover={header.cover}>
    {#snippet actions()}
      <Button variant="primary" onclick={() => void list.playAll()} disabled={list.total === 0}>
        {#snippet icon()}<Icon name="play" size={12} />{/snippet}
        播放全部
      </Button>
    {/snippet}
  </Header>
</div>

<!--
  艺人页把"歌手"列链接废掉（disableArtistLink）：点了指向的是当前页，
  nav 视为刷新，只会白丢一次滚动位置。专辑列保持可点。
-->
<TrackListPanel canSort={true} removeLabel="从曲库中删除" showFilter={false} disableArtistLink={true}>
  {#snippet empty()}
    <div class="empty">
      <strong>这位艺人没有曲目</strong>
      <span>曲目可能已被移出曲库</span>
      <Button onclick={() => nav.go({ kind: 'browse', view: 'artists' })}>返回艺人</Button>
    </div>
  {/snippet}
</TrackListPanel>
