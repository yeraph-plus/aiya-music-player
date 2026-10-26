<script lang="ts">
  import type { PlaylistSummary } from '../lib/ipc';
  import { artworkUrl } from '../lib/artwork';
  import { Plus } from '@lucide/svelte';
  import Button from './ui/Button.svelte';
  import Cover from './ui/Cover.svelte';
  import Dialog from './ui/Dialog.svelte';

  /**
   * 「添加到播放列表」选择器。
   *
   * 三个对话框里唯一有主体的一个 —— 它演示了 `Dialog` 的正确用法：
   * 标题/说明走 props，列表走 children，按钮走 actions。
   *
   * 这里**不用 `Menu`**：选歌单是"看得到每个歌单有几首"的浏览行为，
   * 菜单是"我已经知道要点哪一项"的快捷路径。两者混用会让列表里出现
   * 一个既不像菜单也不像对话框的东西。
   */

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

<Dialog {title} description={`为 ${count} 首曲目选择一个播放列表`} width="md" onclose={onclose}>
  <div class="pick-list">
    {#each playlists as playlist (playlist.id)}
      <button class="pick-row" onclick={() => onpick(playlist.id)}>
        <Cover src={playlist.artworkId ? artworkUrl(playlist.artworkId, 96) : null} size="sm" />
        <span class="pick-name">{playlist.name}</span>
        <span class="pick-count">{playlist.trackCount} 首</span>
      </button>
    {:else}
      <p class="pick-empty">还没有播放列表，点右下角新建一个。</p>
    {/each}
  </div>

  {#snippet actions()}
    <Button onclick={onclose}>取消</Button>
    <Button variant="primary" onclick={oncreate}>
      {#snippet icon()}<Plus size={13} />{/snippet}
      新建播放列表
    </Button>
  {/snippet}
</Dialog>

<style>
  .pick-list {
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 8px;
  }

  .pick-row {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 6px 8px;
    border-radius: var(--radius-md);
    text-align: left;
    color: var(--text-1);
    font-size: 12.5px;
  }

  .pick-row:hover {
    background: var(--surface-hover);
  }

  .pick-name {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .pick-count {
    flex: none;
    font-size: 11px;
    color: var(--text-3);
    font-variant-numeric: tabular-nums;
  }

  .pick-empty {
    margin: 0;
    padding: 18px 10px;
    font-size: 12.5px;
    color: var(--text-2);
    text-align: center;
  }
</style>
