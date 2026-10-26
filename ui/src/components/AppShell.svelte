<script lang="ts">
  import type { Snippet } from 'svelte';
  import PlayerBar from './shell/PlayerBar.svelte';
  import Sidebar from './shell/Sidebar.svelte';
  import TitleBar from './shell/TitleBar.svelte';
  import InlineError from './ui/InlineError.svelte';
  import QueuePanel from './QueuePanel.svelte';
  import BatchBar from './BatchBar.svelte';
  import { errorMessage } from '../lib/errors';
  import { batch } from '../stores/batch.svelte';
  import { list } from '../stores/list.svelte';
  import { nav } from '../stores/nav.svelte';
  import { notify } from '../stores/notify.svelte';
  import { overlay } from '../stores/overlay.svelte';
  import { player } from '../stores/player.svelte';
  import { playlists } from '../stores/playlists.svelte';
  import { ui } from '../stores/ui.svelte';

  type Props = { children: Snippet };
  let { children }: Props = $props();

  const playerState = $derived(player.state);

  async function playQueueAt(index: number): Promise<void> {
    try {
      await player.playTracks(player.queueIds, index);
    } catch (err) {
      notify.showError(errorMessage(err));
    }
  }
</script>

<div class="app">
  <TitleBar />
  <Sidebar />

  <main class="main">
    {#if notify.error}
      <!-- 阻断类错误：没有可自救动作，只有「关闭」（InlineError 的两类之一） -->
      <InlineError message={notify.error} ondismiss={() => notify.clearError()} />
    {/if}
    {#if playerState?.status === 'error' && playerState.error}
      <!-- 可重试类错误：动作是「跳到下一首」，用户不用离开当前页 -->
      <InlineError
        message={`《${playerState.current?.title ?? '未知曲目'}》${playerState.error.message}`}
        action={{ label: '跳到下一首', onclick: () => player.next() }}
      />
    {/if}

    <div class="page-content">
      {@render children()}
    </div>
  </main>

  {#if batch.active}
    <!-- 批量条全应用只有这一个渲染点（B4-2b）：列表页与首页三段的选择
         都汇到 batch store，动作也只有一份实现 -->
    <BatchBar
      count={batch.count}
      playlists={playlists.items}
      onplay={() => void batch.play()}
      onenqueue={() => void batch.enqueue()}
      onaddto={(playlistId) => void batch.addTo(playlistId)}
      onremove={() => void batch.remove()}
      onclear={() => batch.clear()}
    />
  {/if}

  <div class="player-dock">
    <PlayerBar
      player={playerState}
      queueOpen={ui.queueOpen}
      nowPlayingActive={ui.page === 'nowPlaying'}
      ontoggle={() => player.toggle()}
      onnext={() => player.next()}
      onprev={() => player.prev()}
      onseek={(ms) => player.seek(ms)}
      onvolume={(value) => player.setVolume(value)}
      onmute={() => player.toggleMute()}
      onrate={(rate) => player.setRate(rate)}
      onrepeat={() => player.cycleRepeat()}
      onshuffle={() => player.setShuffle(!(playerState?.shuffle ?? false))}
      onqueueopen={(open) => ui.setQueue(open)}
      onshownowplaying={() => nav.go({ kind: 'page', page: 'nowPlaying' })}
    >
      {#snippet queuepanel()}
        <!-- 关闭时 bits-ui 卸载 Popover.Content：这里不再需要 {#if ui.queueOpen} -->
        <QueuePanel
          ids={player.queueIds}
          index={playerState?.index ?? null}
          onplay={(index) => void playQueueAt(index)}
          onremove={(index) => player.removeAt(index)}
          onmove={(from, to) => player.move(from, to)}
          onclear={() => player.clear()}
          onclose={() => ui.closeQueue()}
        />
      {/snippet}
    </PlayerBar>
  </div>
</div>
