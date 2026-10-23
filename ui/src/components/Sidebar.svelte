<script lang="ts">
  import type { LibraryStats, PlaylistSummary } from '@aiya/contracts';
  import { artworkUrl } from '@aiya/contracts';
  import type { View } from '../lib/view';
  import { shortenPath } from '../lib/format';
  import Icon from './Icon.svelte';

  interface Props {
    view: View;
    playlists: PlaylistSummary[];
    stats: LibraryStats | null;
    dataDir: string | null;
    scanning: boolean;
    onview: (kind: 'library' | 'recent' | 'mostPlayed') => void;
    onplaylist: (id: string) => void;
    oncreate: () => void;
    onplaylistmenu: (event: MouseEvent, playlist: PlaylistSummary) => void;
    onsettings: () => void;
  }

  let {
    view,
    playlists,
    stats,
    dataDir,
    scanning,
    onview,
    onplaylist,
    oncreate,
    onplaylistmenu,
    onsettings,
  }: Props = $props();
</script>

<aside class="sidebar">
  <div class="brand"><strong>AIYA</strong><span>MUSIC</span></div>

  <nav class="nav">
    <button class="nav-item" class:active={view.kind === 'library'} onclick={() => onview('library')}>
      <Icon name="list" size={15} />
      <span>全部曲目</span>
    </button>
    <button class="nav-item" class:active={view.kind === 'recent'} onclick={() => onview('recent')}>
      <Icon name="clock" size={15} />
      <span>最近播放</span>
    </button>
    <button class="nav-item" class:active={view.kind === 'mostPlayed'} onclick={() => onview('mostPlayed')}>
      <Icon name="flame" size={15} />
      <span>常听</span>
    </button>
  </nav>

  <div class="side-section">
    <div class="side-head">
      <span>播放列表</span>
      <button class="icon-btn tiny" onclick={oncreate} title="新建播放列表" aria-label="新建播放列表">
        <Icon name="plus" size={13} />
      </button>
    </div>

    <div class="playlist-list">
      {#each playlists as playlist (playlist.id)}
        <button
          class="nav-item playlist-item"
          class:active={view.kind === 'playlist' && view.id === playlist.id}
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
            <span class="cover xs placeholder">♪</span>
          {/if}
          <span class="pl-name">{playlist.name}</span>
          <span class="pl-count">{playlist.trackCount}</span>
        </button>
      {:else}
        <div class="side-empty">还没有播放列表</div>
      {/each}
    </div>
  </div>

  <div class="sidebar-footer">
    {#if stats}
      {stats.tracks} 首 · {stats.albums} 张专辑<br />
      {stats.artists} 位艺人
      {#if stats.missingLocations > 0}
        <br /><span class="warn">{stats.missingLocations} 个位置缺失</span>
      {/if}
    {:else}
      尚未建立曲库
    {/if}
    {#if dataDir}
      <br /><span class="dim path" title={dataDir}>{shortenPath(dataDir, 2)}</span>
    {/if}
    <button class="nav-item settings-entry" onclick={onsettings} disabled={scanning}>
      <Icon name="settings" size={14} />
      <span>设置与诊断</span>
    </button>
  </div>
</aside>
