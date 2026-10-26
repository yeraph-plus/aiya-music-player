<script lang="ts">
  import type { Snippet } from 'svelte';
  import PlayerBar from './shell/PlayerBar.svelte';
  import Sidebar from './shell/Sidebar.svelte';
  import TitleBar from './shell/TitleBar.svelte';
  import QueuePanel from './QueuePanel.svelte';
  import BatchBar from './BatchBar.svelte';
  import { errorMessage } from '../lib/errors';
  import { list } from '../stores/list.svelte';
  import { nav } from '../stores/nav.svelte';
  import { notify } from '../stores/notify.svelte';
  import { overlay } from '../stores/overlay.svelte';
  import { player } from '../stores/player.svelte';
  import { playlists } from '../stores/playlists.svelte';
  import { selection } from '../stores/selection.svelte';
  import { isListPage, ui } from '../stores/ui.svelte';

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

  async function askRemoveSelection(): Promise<void> {
    const ids = await list.selectionIds();
    if (ids.length > 0) overlay.dialog = { kind: 'removeTracks', ids };
  }
</script>

<div class="app">
  <TitleBar />
  <Sidebar />

  <main class="main">
    {#if notify.error}
      <div class="error" aria-live="assertive">
        {notify.error}
        <button class="link" onclick={() => notify.clearError()}>关闭</button>
      </div>
    {/if}
    {#if playerState?.status === 'error' && playerState.error}
      <div class="error">
        《{playerState.current?.title ?? '未知曲目'}》{playerState.error.message}
        <button class="link" onclick={() => player.next()}>跳到下一首</button>
      </div>
    {/if}

    <div class="page-content">
      {@render children()}
    </div>
  </main>

  {#if isListPage(ui.page) && selection.size > 0}
    <BatchBar
      count={selection.size}
      playlists={playlists.items}
      onplay={() => void list.batchPlay()}
      onenqueue={() => void list.batchEnqueue()}
      onaddto={(playlistId) => void playlists.addSelection(playlistId)}
      onremove={() => void askRemoveSelection()}
      onclear={() => selection.clear()}
    />
  {/if}

  <div class="player-dock">
    {#if ui.queueOpen}
      <QueuePanel
        ids={player.queueIds}
        index={playerState?.index ?? null}
        onplay={(index) => void playQueueAt(index)}
        onremove={(index) => player.removeAt(index)}
        onmove={(from, to) => player.move(from, to)}
        onclear={() => player.clear()}
        onclose={() => ui.closeQueue()}
      />
    {/if}
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
      onqueue={() => ui.toggleQueue()}
      onshownowplaying={() => nav.go({ kind: 'page', page: 'nowPlaying' })}
    />
  </div>
</div>
