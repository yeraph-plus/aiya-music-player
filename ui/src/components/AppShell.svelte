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
  import { errorMessage } from '../lib/errors';
  import type { SpectrumSource } from '../lib/spectrum-source';
  import { eq } from '../stores/eq.svelte';
  import { list } from '../stores/list.svelte';
  import { nav } from '../stores/nav.svelte';
  import { notify } from '../stores/notify.svelte';
  import { overlay } from '../stores/overlay.svelte';
  import { player } from '../stores/player.svelte';
  import { ui } from '../stores/ui.svelte';

  type Props = { children: Snippet };
  let { children }: Props = $props();

  const playerState = $derived(player.state);

  /**
   * 全应用背景的频谱源（docs/25 §5 T4c app 化）：设置开着才建
   * （tap 按需订阅，关着引擎零开销）。原先在 NowPlayingView——背景
   * 升为全应用层后跟着搬进来。
   *
   * 2026-10-30（docs/46 §6.2）：`spectrum` 拆成两个开关，这一层只认
   * `bgSpectrum`；正在播放页的开关由 NowPlayingView 自己派生。二者任一为真
   * 时 tap 都开着（`RemoteSpectrumSource` 的引用计数负责叠放）。
   * `glEffects` 是**能力闸门**：关掉时连源都不建，引擎侧也不订阅。
   */
  let spectrumSource = $state<SpectrumSource | null>(null);

  const bgSpectrumOn = $derived(ui.settings.glEffects && ui.settings.bgSpectrum);

  $effect(() => {
    if (!bgSpectrumOn) {
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

  /**
   * 正在播放页：侧栏**整体退出**、主区铺满（docs/46 §5.2 用户拍板）。
   *
   * 为什么不叠覆盖层：背景是 `.app` 的全窗口绝对定位层，挡住它的只是侧栏
   * 那块 82% 不透明的面板。侧栏一退，背景连续地铺满整个窗口——正在播放页
   * **零额外背景绘制**，也就省掉了最贵的那块全屏合成。
   */
  const npOpen = $derived(ui.page === 'nowPlaying');

  async function playQueueAt(index: number): Promise<void> {
    try {
      await player.playTracks(player.queueIds, index);
    } catch (err) {
      notify.showError(errorMessage(err));
    }
  }
</script>

<div class="app" class:np-open={npOpen}>
  <!-- 背景四层（1-3 层，T4c 起 app 化；T4d 起第 1 层是固定渐变，不跟随封面）：
       绝对定位在 .app 根部、四区席位之下。全应用只有这一套背景——
       正在播放页靠"侧栏退出 + 主区铺满"露出它，而不是再叠一层。 -->
  <AmbientBackdrop
    source={spectrumSource}
    enabled={bgSpectrumOn}
    shape={ui.settings.bgSpectrumStyle}
    glow={ui.settings.ridgeGlow}
    paused={playerState?.status !== 'playing'}
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

  <div class="player-dock">
    <PlayerBar
      player={playerState}
      queueOpen={ui.queueOpen}
      eqOpen={ui.eqOpen}
      eqEnabled={eq.enabled}
      rateDraft={player.rateDraft}
      ontoggle={() => player.toggle()}
      onnext={() => player.next()}
      onprev={() => player.prev()}
      onseek={(ms) => player.seek(ms)}
      onvolume={(value) => player.setVolume(value)}
      onmute={() => player.toggleMute()}
      onrate={(rate) => player.dragRate(rate)}
      onmode={() => player.cyclePlayMode()}
      onqueueopen={(open) => ui.setQueue(open)}
      oneqopen={(open) => ui.setEqDrawer(open)}
      onshownowplaying={() => {
        // 播放条这块是**开关**：已经在这一页就退回上一页（与顶栏返回、Esc
        // 走同一条历史栈），不要在栈里塞第二个自己
        if (npOpen) nav.exitNowPlaying();
        else nav.go({ kind: 'page', page: 'nowPlaying' });
      }}
      {npOpen}
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
