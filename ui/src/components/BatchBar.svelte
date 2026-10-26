<script lang="ts">
  import type { PlaylistSummary } from '../lib/ipc';
  import Icon from './Icon.svelte';
  import Button from './ui/Button.svelte';
  import IconButton from './ui/IconButton.svelte';

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
  <Button size="sm" onclick={onplay}>
    {#snippet icon()}<Icon name="play" size={12} />{/snippet}
    播放
  </Button>
  <Button size="sm" onclick={onenqueue}>加入队列</Button>
  <div class="batch-picker">
    <Button size="sm" onclick={() => (pickerOpen = !pickerOpen)} disabled={playlists.length === 0}>
      加入播放列表
    </Button>
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
  <Button variant="danger" size="sm" onclick={onremove}>
    {#snippet icon()}<Icon name="trash" size={12} />{/snippet}
    移出曲库
  </Button>
  <IconButton label="取消选择" size="sm" onclick={onclear}>
    <Icon name="close" size={13} />
  </IconButton>
</div>
