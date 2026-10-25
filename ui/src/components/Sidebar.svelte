<script lang="ts">
  import type { LibraryStats, PlaylistSummary } from '../lib/ipc';
  import type { AppPage } from '../stores/ui.svelte';
  import type { View } from '../lib/view';
  import { artworkUrl } from '../lib/artwork';
  import { shortenPath } from '../lib/format';
  import { AudioLines, House, ListMusic, Plus, Radio, Settings } from '@lucide/svelte';

  interface Props {
    page: AppPage;
    view: View;
    playlists: PlaylistSummary[];
    stats: LibraryStats | null;
    dataDir: string | null;
    onpage: (page: AppPage) => void;
    onplaylist: (id: string) => void;
    oncreate: () => void;
    onplaylistmenu: (event: MouseEvent, playlist: PlaylistSummary) => void;
  }

  let { page, view, playlists, stats, dataDir, onpage, onplaylist, oncreate, onplaylistmenu }: Props =
    $props();

  const pages = [
    { id: 'home' as const, label: '首页', icon: House },
    { id: 'nowPlaying' as const, label: '正在播放', icon: AudioLines },
    { id: 'radio' as const, label: 'FM 电台', icon: Radio },
    { id: 'settings' as const, label: '设置', icon: Settings },
  ];
</script>

<aside class="sidebar">
  <nav class="nav" aria-label="主导航">
    {#each pages as item (item.id)}
      <button
        class="nav-item"
        class:active={page === item.id}
        aria-current={page === item.id ? 'page' : undefined}
        onclick={() => onpage(item.id)}
      >
        <item.icon size={16} strokeWidth={1.8} />
        <span>{item.label}</span>
      </button>
    {/each}
  </nav>

  <div class="side-section">
    <div class="side-head">
      <span>歌单</span>
      <button class="icon-btn tiny" onclick={oncreate} title="新建歌单" aria-label="新建歌单">
        <Plus size={14} />
      </button>
    </div>

    <div class="playlist-list">
      {#each playlists as playlist (playlist.id)}
        <button
          class="nav-item playlist-item"
          class:active={page === 'home' && view.kind === 'playlist' && view.id === playlist.id}
          onclick={() => onplaylist(playlist.id)}
          oncontextmenu={(event) => {
            event.preventDefault();
            onplaylistmenu(event, playlist);
          }}
          title={`${playlist.name} · ${playlist.trackCount} 首`}
        >
          {#if playlist.artworkId}
            <img class="cover xs" src={artworkUrl(playlist.artworkId, 96) ?? ''} alt="" loading="lazy" />
          {:else}
            <span class="cover xs placeholder"><ListMusic size={12} /></span>
          {/if}
          <span class="pl-name">{playlist.name}</span>
          <span class="pl-count">{playlist.trackCount}</span>
        </button>
      {:else}
        <div class="side-empty">还没有歌单</div>
      {/each}
    </div>
  </div>

  <div class="sidebar-footer">
    {#if stats}
      <strong>{stats.tracks}</strong> 首曲目
      <span class="library-stats">{stats.albums} 张专辑 · {stats.artists} 位艺人</span>
      {#if stats.missingLocations > 0}
        <span class="warn">{stats.missingLocations} 个位置缺失</span>
      {/if}
    {:else}
      尚未建立曲库
    {/if}
    {#if dataDir}
      <span class="dim path" title={dataDir}>{shortenPath(dataDir, 2)}</span>
    {/if}
  </div>
</aside>
