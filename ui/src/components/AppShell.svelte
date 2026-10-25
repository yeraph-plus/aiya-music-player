<script lang="ts">
  /**
   * 应用外壳：版面 + 跨视图的容器行为。
   *
   * 边界判据（批次 1 定的，批次 3 收紧了一档）：
   * - **版面归壳**：`.app` 网格、侧栏、主区、侧面板、批量条、播放条的位置与层级；
   * - **数据归 store**：壳直接读 `ui` / `library` / `player` / `selection` /
   *   `list` / `playlists` / `overlay`，依赖方向仍是 components → stores → ipc；
   * - **不收 prop**：批次 1 时壳还接了 12 个回调（理由是"要 App 的本地状态"），
   *   批次 3 把那些状态搬进 store 之后，**壳自己就能闭环**——切视图是
   *   `list.openBuiltinView`、面板开关是 `ui.togglePanel`、批量操作是
   *   `list.batchPlay`。留着回调等于在壳和 App 之间留一排空管道。
   *
   * 唯一 `children`：主区内容（`views/*`）。壳不该知道当前是哪个视图。
   */
  import type { Snippet } from 'svelte';
  import Sidebar from './Sidebar.svelte';
  import PlayerBar from './PlayerBar.svelte';
  import QueuePanel from './QueuePanel.svelte';
  import LyricsPanel from './LyricsPanel.svelte';
  import SpectrumPanel from './SpectrumPanel.svelte';
  import ImportPanel from './ImportPanel.svelte';
  import BatchBar from './BatchBar.svelte';
  import type { SpectrumSource } from '../lib/spectrum-source';
  import { errorMessage } from '../lib/errors';
  import { library } from '../stores/library.svelte';
  import { list } from '../stores/list.svelte';
  import { notify } from '../stores/notify.svelte';
  import { overlay } from '../stores/overlay.svelte';
  import { player } from '../stores/player.svelte';
  import { playlists } from '../stores/playlists.svelte';
  import { selection } from '../stores/selection.svelte';
  import { ui } from '../stores/ui.svelte';

  type Props = {
    /** 主区内容（`views/*`），由 App 决定挂哪个视图 */
    children: Snippet;
  };

  let { children }: Props = $props();

  const view = $derived(ui.view);
  const sidePanel = $derived(ui.sidePanel);
  const settings = $derived(ui.settings);
  const playerState = $derived(player.state);

  /**
   * 频谱数据源：**只在面板真的打开时才创建**。
   *
   * `RemoteSpectrumSource` 构造时会订阅 `spectrum:frame` 并打开引擎的
   * tap —— 那是给音频链加一条采样复制的路径。提前建等于让所有用户
   * 常驻一条他们没在看的回调。
   *
   * 放在这里而不是 App：面板是壳的一部分，源的生命周期跟着面板走。
   */
  let spectrumSource = $state<SpectrumSource | null>(null);
  $effect(() => {
    if (sidePanel !== 'spectrum' || !settings.spectrum) {
      spectrumSource = null;
      return;
    }
    try {
      spectrumSource = player.getSpectrumSource();
    } catch (err) {
      // 拿不到源不致命：面板会不渲染，控制台留一句
      console.warn('spectrum source failed:', errorMessage(err));
      spectrumSource = null;
    }
  });

  /** 队列里点某一行：失败要写错误条（这是用户显式动作） */
  async function playQueueAt(index: number): Promise<void> {
    try {
      await player.playTracks(player.queueIds, index);
    } catch (err) {
      notify.showError(errorMessage(err));
    }
  }

  /** 批量「移除」：先把选中集换成 id，再开确认框（数量要显示给用户） */
  async function askRemoveSelection(): Promise<void> {
    const ids = await list.selectionIds();
    if (ids.length > 0) overlay.dialog = { kind: 'removeTracks', ids };
  }
</script>

<div class="app" class:side-open={sidePanel !== null}>
  <Sidebar
    {view}
    playlists={ui.playlists}
    stats={library.stats}
    dataDir={library.dataDir}
    scanning={library.scanning}
    onview={(kind) => list.openBuiltinView(kind)}
    onplaylist={(id) => list.openPlaylist(id)}
    oncreate={() => (overlay.dialog = { kind: 'createPlaylist' })}
    onplaylistmenu={(event, playlist) =>
      overlay.openPlaylistMenu(event.clientX, event.clientY, playlist)}
    onsettings={() => void ui.openSettings()}
  />

  <main class="main">
    {@render children()}
  </main>

  {#if sidePanel === 'queue'}
    <QueuePanel
      ids={player.queueIds}
      index={playerState?.index ?? null}
      onplay={(index) => void playQueueAt(index)}
      onremove={(index) => player.removeAt(index)}
      onmove={(from, to) => player.move(from, to)}
      onclear={() => player.clear()}
      onclose={() => (ui.sidePanel = null)}
    />
  {:else if sidePanel === 'lyrics'}
    <LyricsPanel
      lyrics={ui.lyrics}
      loading={ui.lyricsLoading}
      trackTitle={playerState?.current?.title ?? null}
      positionMs={playerState?.positionMs ?? 0}
      durationMs={playerState?.durationMs ?? 0}
      playing={playerState?.status === 'playing'}
      onseek={(ms) => player.seek(ms)}
      onclose={() => (ui.sidePanel = null)}
    />
  {:else if sidePanel === 'spectrum' && spectrumSource}
    <SpectrumPanel
      source={spectrumSource}
      engine={settings.spectrumEngine}
      onclose={() => (ui.sidePanel = null)}
    />
  {:else if sidePanel === 'import'}
    <ImportPanel onclose={() => (ui.sidePanel = null)} onsynced={() => void list.reload()} />
  {/if}

  {#if selection.size > 0}
    <BatchBar
      count={selection.size}
      playlists={ui.playlists}
      onplay={() => void list.batchPlay()}
      onenqueue={() => void list.batchEnqueue()}
      onaddto={(playlistId) => void playlists.addSelection(playlistId)}
      onremove={() => void askRemoveSelection()}
      onclear={() => selection.clear()}
    />
  {/if}

  <PlayerBar
    player={playerState}
    queueOpen={sidePanel === 'queue'}
    lyricsOpen={sidePanel === 'lyrics'}
    spectrumEnabled={settings.spectrum}
    spectrumOpen={sidePanel === 'spectrum'}
    ontoggle={() => player.toggle()}
    onnext={() => player.next()}
    onprev={() => player.prev()}
    onseek={(ms) => player.seek(ms)}
    onvolume={(value) => player.setVolume(value)}
    onmute={() => player.toggleMute()}
    onrate={(rate) => player.setRate(rate)}
    onrepeat={() => player.cycleRepeat()}
    onshuffle={() => player.setShuffle(!(playerState?.shuffle ?? false))}
    onqueue={() => ui.togglePanel('queue')}
    ontogglelyrics={() => ui.togglePanel('lyrics', playerState?.current?.id ?? null)}
    ontogglespectrum={() => ui.togglePanel('spectrum')}
  />
</div>
