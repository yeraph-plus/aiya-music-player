<script lang="ts">
  import type { PlaylistSummary } from '../lib/ipc';
  import { artworkUrl } from '../lib/artwork';
  import { formatDurationLong } from '../lib/format';
  import { viewTitle, type View } from '../lib/view';
  import Icon from './Icon.svelte';

  /**
   * 视图头部：播放列表的信息卡，或"最近播放 / 常听"的说明。
   *
   * 两种头部形状不同但位置一致，放在一个组件里是为了让 App 的模板
   * 不再出现"两段并列的 if 块 + 四五个按钮"这种结构噪声。
   */

  interface Props {
    view: View;
    playlist: PlaylistSummary | null;
    total: number;
    onplayall: () => void;
    onrename: () => void;
    onclearitems: () => void;
    ondelete: () => void;
    onclearhistory: () => void;
  }

  let {
    view,
    playlist,
    total,
    onplayall,
    onrename,
    onclearitems,
    ondelete,
    onclearhistory,
  }: Props = $props();

  const kicker = $derived(view.kind === 'playlist' ? '播放列表' : '播放统计');
  const description = $derived(
    view.kind === 'recent'
      ? '按最后一次播放排序，只统计真正听过的曲目'
      : '按播放次数排序，用于快速回到最近常听的歌',
  );
</script>

{#if view.kind === 'playlist' && playlist}
  <div class="view-head">
    {#if playlist.artworkId}
      <img class="view-cover" src={artworkUrl(playlist.artworkId, 256) ?? ''} alt="" />
    {:else}
      <span class="view-cover placeholder">♪</span>
    {/if}
    <div class="view-meta">
      <div class="view-kicker">{kicker}</div>
      <h1>{playlist.name}</h1>
      <div class="dim">
        {playlist.trackCount} 首 · {formatDurationLong(playlist.durationMs)}
      </div>
    </div>
    <div class="view-actions">
      <button class="btn primary" onclick={onplayall} disabled={playlist.trackCount === 0}>
        <Icon name="play" size={12} /> 播放全部
      </button>
      <button class="btn" onclick={onrename}>
        <Icon name="edit" size={12} /> 重命名
      </button>
      <button class="btn" onclick={onclearitems} disabled={playlist.trackCount === 0}>清空</button>
      <button class="btn danger" onclick={ondelete}>
        <Icon name="trash" size={12} /> 删除
      </button>
    </div>
  </div>
{:else if view.kind === 'recent' || view.kind === 'mostPlayed'}
  <div class="view-head compact">
    <div class="view-meta">
      <div class="view-kicker">{kicker}</div>
      <h1>{viewTitle(view)}</h1>
      <div class="dim">{description}</div>
    </div>
    <div class="view-actions">
      <button class="btn primary" onclick={onplayall} disabled={total === 0}>
        <Icon name="play" size={12} /> 播放全部
      </button>
      {#if view.kind === 'recent'}
        <button class="btn" onclick={onclearhistory}>清空历史</button>
      {/if}
    </div>
  </div>
{/if}
