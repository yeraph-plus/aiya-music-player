<script lang="ts">
  /**
   * 应用外壳：只负责「版面」。
   *
   * 边界约定（批次 1 落地，批次 3 继续沿用的判据）：
   * - **版面归壳**：`.app` 网格、侧栏、主区、侧面板、批量条、播放条的位置与层级；
   * - **数据归 store**：壳直接读 `ui` / `library` / `player` / `selection`，
   *   不通过 App 中转（壳是容器，依赖方向仍是 components → stores → ipc）；
   * - **行为归 App**：凡是需要 App 本地状态的（列表重载、选中集解析成 id、
   *   面板开关的落库防抖），一律由 App 以回调注入，壳里不复制一份逻辑。
   *
   * 为什么现在只搬版面：批次 3 才把 `views/` 拆出来，那时主区内容会整体变成
   * 一个 snippet 之外的独立组件；先把壳定下来，后续加视图不用再动 App.svelte
   * 的布局部分。
   */
  import type { Snippet } from 'svelte';
  import Sidebar from './Sidebar.svelte';
  import PlayerBar from './PlayerBar.svelte';
  import QueuePanel from './QueuePanel.svelte';
  import LyricsPanel from './LyricsPanel.svelte';
  import SpectrumPanel from './SpectrumPanel.svelte';
  import ImportPanel from './ImportPanel.svelte';
  import BatchBar from './BatchBar.svelte';
  import { player } from '../stores/player.svelte';
  import { ui } from '../stores/ui.svelte';
  import { library } from '../stores/library.svelte';
  import { selection } from '../stores/selection.svelte';
  import type { SpectrumSource } from '../lib/spectrum-source';

  type SidePanel = NonNullable<typeof ui.sidePanel>;

  type Props = {
    /** 主区内容（视图区），批次 3 会换成 `views/*` 组件 */
    children: Snippet;
    /* 侧栏：视图切换与歌单操作要重载列表，只能由 App 提供 */
    onview: (kind: 'library' | 'recent' | 'mostPlayed') => void;
    onplaylist: (id: string) => void;
    oncreate: () => void;
    onplaylistmenu: (event: MouseEvent, playlist: (typeof ui.playlists)[number]) => void;
    /* 队列：点某一行要播「当前队列的第 N 首」，失败要写进 App 的错误条 */
    onqueueplay: (index: number) => void;
    /* 批量条：选中集 → id 列表的解析要 App 的查询上下文 */
    onbatchplay: () => void;
    onbatchenqueue: () => void;
    onbatchaddto: (playlistId: string) => void;
    onbatchremove: () => void;
    /* 面板开关：切换要顺带落库滚动位置，防抖器在 App 里 */
    ontogglepanel: (panel: SidePanel) => void;
    /* 导入完成后曲库变了，需要 App 重载当前视图 */
    onsynced: () => void;
    /** 频谱数据源由 App 管理（建图 = 音频路径切换，只能在面板打开时创建） */
    spectrumSource: SpectrumSource | null;
  };

  let {
    children,
    onview,
    onplaylist,
    oncreate,
    onplaylistmenu,
    onqueueplay,
    onbatchplay,
    onbatchenqueue,
    onbatchaddto,
    onbatchremove,
    ontogglepanel,
    onsynced,
    spectrumSource,
  }: Props = $props();

  const view = $derived(ui.view);
  const sidePanel = $derived(ui.sidePanel);
  const settings = $derived(ui.settings);
  const playerState = $derived(player.state);
</script>

<div class="app" class:side-open={sidePanel !== null}>
  <Sidebar
    {view}
    playlists={ui.playlists}
    stats={library.stats}
    dataDir={library.dataDir}
    scanning={library.scanning}
    {onview}
    {onplaylist}
    {oncreate}
    {onplaylistmenu}
    onsettings={() => void ui.openSettings()}
  />

  <main class="main">
    {@render children()}
  </main>

  {#if sidePanel === 'queue'}
    <QueuePanel
      ids={player.queueIds}
      index={playerState?.index ?? -1}
      onplay={onqueueplay}
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
    <ImportPanel onclose={() => (ui.sidePanel = null)} {onsynced} />
  {/if}

  {#if selection.size > 0}
    <BatchBar
      count={selection.size}
      playlists={ui.playlists}
      onplay={onbatchplay}
      onenqueue={onbatchenqueue}
      onaddto={onbatchaddto}
      onremove={onbatchremove}
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
    onqueue={() => ontogglepanel('queue')}
    ontogglelyrics={() => ontogglepanel('lyrics')}
    ontogglespectrum={() => ontogglepanel('spectrum')}
  />
</div>
