<script lang="ts">
  import type { PlaylistSummary } from '@aiya/contracts';
  import { artworkUrl } from '@aiya/contracts';
  import Icon from './Icon.svelte';

  interface Props {
    title: string;
    count: number;
    playlists: PlaylistSummary[];
    onpick: (playlistId: string) => void;
    oncreate: () => void;
    onclose: () => void;
  }

  let { title, count, playlists, onpick, oncreate, onclose }: Props = $props();
</script>

<div class="modal-backdrop" role="presentation" onclick={onclose}></div>
<div class="modal picker" role="dialog" aria-label={title}>
  <header>
    <strong>{title}</strong>
    <span class="dim">{count} 首</span>
    <button class="icon-btn" onclick={onclose} aria-label="关闭"><Icon name="close" size={14} /></button>
  </header>

  <div class="picker-list">
    {#each playlists as playlist (playlist.id)}
      <button class="picker-row" onclick={() => onpick(playlist.id)}>
        {#if playlist.artworkId}
          <img class="cover sm" src={artworkUrl(playlist.artworkId, 96) ?? ''} alt="" loading="lazy" />
        {:else}
          <span class="cover sm placeholder">♪</span>
        {/if}
        <span class="picker-meta">
          <span>{playlist.name}</span>
          <span class="dim">{playlist.trackCount} 首</span>
        </span>
      </button>
    {:else}
      <div class="side-empty">还没有播放列表</div>
    {/each}
  </div>

  <footer class="modal-actions">
    <button class="btn" onclick={onclose}>取消</button>
    <button class="btn primary" onclick={oncreate}><Icon name="plus" size={12} /> 新建播放列表</button>
  </footer>
</div>
