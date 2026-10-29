<script lang="ts">
  import { User } from '@lucide/svelte';
  import Icon from '../components/Icon.svelte';
  import Header from '../components/content/Header.svelte';
  import Button from '../components/ui/Button.svelte';
  import EmptyState from '../components/ui/EmptyState.svelte';
  import TrackListPanel from '../components/TrackListPanel.svelte';
  import { formatDurationLong } from '../lib/format';
  import { facets } from '../stores/facets.svelte';
  import { list } from '../stores/list.svelte';
  import { nav } from '../stores/nav.svelte';
  import { ui } from '../stores/ui.svelte';

  /**
   * 艺人详情页（docs/28 拆出）：与 AlbumView 同构——路由参数
   * （`ui.artistId`）直接可寻址，曲目列表 = 曲库查询 + artistId 筛选，
   * 列表接线共用 TrackListPanel。
   *
   * **不做图片获取逻辑**（用户拍板）：artists 表的 artwork_id 扫描器从不
   * 填写，头部与宫格卡片一律 user 图标，`artworkUrl` 在本页没有位置。
   */
  const artist = $derived(facets.artistById(ui.artistId ?? ''));

  $effect(() => {
    if (facets.artists === null) void facets.loadArtists();
  });

  /**
   * 列表首行（AlbumView 同款，理由见彼处）：聚合缓存加载慢或失败时，
   * 标题回落到首行的歌手名——**绝不把路由参数（uuid）渲染出来**。
   */
  const firstRow = $derived.by(() => {
    void list.version;
    return list.trackWindow.get(0);
  });

  const header = $derived.by(() => {
    const ar = artist;
    const row = firstRow;
    return {
      title: ar?.name ?? row?.artist ?? '艺人',
      meta:
        ar === null
          ? `${list.total} 首`
          : `${ar.trackCount} 首 · ${ar.albumCount} 张专辑` +
            (ar.totalDurationMs > 0 ? ` · ${formatDurationLong(ar.totalDurationMs)}` : ''),
    };
  });
</script>

<div class="page-pad">
  <Header kicker="艺人" title={header.title} meta={header.meta}>
    {#snippet icon()}
      <div class="artist-badge"><User size={40} strokeWidth={1.6} /></div>
    {/snippet}
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
    <EmptyState title="这位艺人没有曲目" hint="曲目可能已被移出曲库">
      {#snippet action()}
        <Button onclick={() => nav.go({ kind: 'browse', view: 'artists' })}>返回艺人</Button>
      {/snippet}
    </EmptyState>
  {/snippet}
</TrackListPanel>

<style>
  /* 徽标本体在 Header 的 .ch-icon（尺寸/圆形/投影），这里只定 icon 大小 */
  .artist-badge {
    display: grid;
    place-items: center;
  }
</style>
