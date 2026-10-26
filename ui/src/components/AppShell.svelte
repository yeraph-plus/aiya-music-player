<script lang="ts">
  import type { Snippet } from 'svelte';
  import AmbientBackdrop from './AmbientBackdrop.svelte';
  import PlayerBar from './shell/PlayerBar.svelte';
  import Sidebar from './shell/Sidebar.svelte';
  import TitleBar from './shell/TitleBar.svelte';
  import EqPanel from './EqPanel.svelte';
  import InlineError from './ui/InlineError.svelte';
  import QueuePanel from './QueuePanel.svelte';
  import SideDrawer from './ui/SideDrawer.svelte';
  import BatchBar from './BatchBar.svelte';
  import { errorMessage } from '../lib/errors';
  import type { SpectrumSource } from '../lib/spectrum-source';
  import { batch } from '../stores/batch.svelte';
  import { eq } from '../stores/eq.svelte';
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

  /**
   * 全应用背景的频谱源（docs/25 §5 T4c app 化）：设置开着才建
   * （tap 按需订阅，关着引擎零开销）。原先在 NowPlayingView——背景
   * 升为全应用层后跟着搬进来。
   */
  let spectrumSource = $state<SpectrumSource | null>(null);

  $effect(() => {
    if (!ui.settings.spectrum) {
      spectrumSource = null;
      return;
    }
    try {
      spectrumSource = player.getSpectrumSource();
    } catch (error) {
      console.warn('spectrum source failed:', errorMessage(error));
      spectrumSource = null;
    }
  });

  async function playQueueAt(index: number): Promise<void> {
    try {
      await player.playTracks(player.queueIds, index);
    } catch (err) {
      notify.showError(errorMessage(err));
    }
  }
</script>

<div class="app">
  <!-- 背景四层（1-3 层，T4c 起 app 化；T4d 起第 1 层是固定渐变，不跟随封面）：
       绝对定位在 .app 根部、四区席位之下；沉浸档只在正在播放页 -->
  <AmbientBackdrop
    source={spectrumSource}
    spectrumOn={ui.settings.spectrum}
    immersive={ui.page === 'nowPlaying'}
  />

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
      eqOpen={ui.eqOpen}
      eqEnabled={eq.enabled}
      ontoggle={() => player.toggle()}
      onnext={() => player.next()}
      onprev={() => player.prev()}
      onseek={(ms) => player.seek(ms)}
      onvolume={(value) => player.setVolume(value)}
      onmute={() => player.toggleMute()}
      onrate={(rate) => player.setRate(rate)}
      onmode={() => player.cyclePlayMode()}
      onqueueopen={(open) => ui.setQueue(open)}
      oneqopen={(open) => ui.setEqDrawer(open)}
      onshownowplaying={() => nav.go({ kind: 'page', page: 'nowPlaying' })}
    />
  </div>

  <!--
    队列 / EQ 抽屉（docs/25 §6）：窗口右缘滑出的悬浮 SideDrawer 宿主
    （上下让出顶栏/播放条栏高再留呼吸边）。原先播放条上的 bits-ui
    Popover 气泡——floating-ui 测量+碰撞翻转让开合位置有可见抖动；
    抽屉几何确定（贴右缘、定宽、定边距），没有测量参与。
    互斥仍由 ui store 的 setQueue/setEqDrawer 负责，这里只做壳。
  -->
  <SideDrawer open={ui.queueOpen} label="播放队列" onclose={() => ui.closeQueue()}>
    <QueuePanel
      ids={player.queueIds}
      index={playerState?.index ?? null}
      onplay={(index) => void playQueueAt(index)}
      onremove={(index) => player.removeAt(index)}
      onmove={(from, to) => player.move(from, to)}
      onclear={() => player.clear()}
      onclose={() => ui.closeQueue()}
    />
  </SideDrawer>

  <SideDrawer open={ui.eqOpen} label="均衡器" onclose={() => ui.setEqDrawer(false)}>
    <div class="eq-drawer-body">
      <EqPanel />
    </div>
  </SideDrawer>
</div>
