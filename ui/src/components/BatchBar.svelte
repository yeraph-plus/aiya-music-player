<script lang="ts">
  import type { PlaylistSummary } from '@aiya/contracts';
  import Icon from './Icon.svelte';

  interface Props {
    count: number;
    playlists: PlaylistSummary[];
    onplay: () => void;
    onenqueue: () => void;
    onaddto: (playlistId: string) => void;
    onremove: () => void;
    onclear: () => void;
  }

  let { count, playlists, onplay, onenqueue, onaddto, onremove, onclear }: Props = $props();

  let pickerOpen = $state(false);
</script>

<div class="batch-bar" role="toolbar" aria-label="批量操作">
  <span class="batch-count">已选 {count} 首</span>
  <button class="btn" onclick={onplay}><Icon name="play" size={12} /> 播放</button>
  <button class="btn" onclick={onenqueue}>加入队列</button>
  <div class="batch-picker">
    <button class="btn" onclick={() => (pickerOpen = !pickerOpen)} disabled={playlists.length === 0}>
      加入播放列表
    </button>
    {#if pickerOpen}
      <div class="batch-menu">
        {#each playlists as playlist (playlist.id)}
          <button
            class="menu-item"
            onclick={() => {
              pickerOpen = false;
              onaddto(playlist.id);
            }}
          >
            {playlist.name}
            <span class="dim">{playlist.trackCount}</span>
          </button>
        {/each}
      </div>
    {/if}
  </div>
  <button class="btn danger" onclick={onremove}><Icon name="trash" size={12} /> 移出曲库</button>
  <button class="icon-btn" onclick={onclear} aria-label="取消选择"><Icon name="close" size={13} /></button>
</div>
