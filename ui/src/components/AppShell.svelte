<script lang="ts">
  import type { Snippet } from 'svelte';
  import AmbientBackdrop from './AmbientBackdrop.svelte';
  import PlayerBar from './shell/PlayerBar.svelte';
  import Sidebar from './shell/Sidebar.svelte';
  import TitleBar from './shell/TitleBar.svelte';
  import EqPanel from './EqPanel.svelte';
  import Icon from './Icon.svelte';
  import InlineError from './ui/InlineError.svelte';
  import QueuePanel from './QueuePanel.svelte';
  import SideDrawer from './ui/SideDrawer.svelte';
  import { errorMessage } from '../lib/errors';
  import { t } from '../lib/i18n.svelte';
  import type { AudioErrorCode } from '../lib/ipc/generated/AudioErrorCode';
  import type { SpectrumSource } from '../lib/spectrum-source';
  import { eq } from '../stores/eq.svelte';
  import { drop } from '../stores/drop.svelte';
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
   * 运行时播放错误的分档（报错三档通道，2026-11 拍板）：
   * - **阻断级**（deviceLost / unknown）：设备或引擎出了状况，不处理就没法继续
   *   听 → 弹窗，带「跳到下一首」动作；
   * - **不阻断**（文件缺失、解码、格式不支持等单曲问题）：跳过即可 → 播放条吐司。
   *   曲名不进文案：播放条歌曲区正显示着它，重复一遍只会把吐司撑长。
   */
  const BLOCKING_PLAYBACK_CODES: ReadonlySet<AudioErrorCode> = new Set(['deviceLost', 'unknown']);

  /**
   * 去重键：error 状态会随状态帧持续存在，同一次失败只报一遍。
   * 状态离开 error 时清空（见下方 effect）：键本身没有时间维度，
   * 不复位的话同曲同错重试后再失败会被当成上一笔旧账吞掉。
   */
  let lastPlaybackErrorKey: string | null = null;

  $effect(() => {
    // 先落到局部量再判空：$derived 是 getter，每次访问重新求值，TS 无法跨访问收窄
    const ps = playerState;
    // 播放器任何一次离开 error（重试起播必经 loading/playing/paused）都复位
    // 去重键，下一段同名错误才有机会再次提示；持续 error 的状态帧不进这个
    // 分支，同一次失败仍只报一遍。
    if (ps?.status !== 'error') {
      lastPlaybackErrorKey = null;
      return;
    }
    if (!ps.error || !ps.current) return;
    const err = ps.error;
    const key = `${ps.current.id}|${err.code}|${err.message}`;
    if (key === lastPlaybackErrorKey) return;
    lastPlaybackErrorKey = key;
    if (BLOCKING_PLAYBACK_CODES.has(err.code)) {
      overlay.showAlert({
        title: t('shell.notify.playbackFailed'),
        message: t('shell.notify.playbackBlocked', {
          title: ps.current.title ?? t('shell.notify.unknownTrack'),
          message: err.message,
        }),
        action: { label: t('shell.notify.skipNext'), run: () => player.next() },
      });
    } else {
      notify.showToast(t('shell.notify.playbackFailedToast', { message: err.message }), 4000);
    }
  });

  /**
   * 全应用背景的频谱源：仅在设置开启时创建，tap 按需订阅
   * 关闭时引擎侧零开销。
   *
   * `spectrum` 拆为两个开关后，本层只读取 `bgSpectrum`；
   * 正在播放页的开关由 NowPlayingView 自行派生。任一为真时 tap 均开启
   * （`RemoteSpectrumSource` 通过引用计数处理叠加）。
   * `glEffects` 为能力闸门：关闭时不创建源，引擎侧也不订阅。
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
   * 正在播放页：侧栏整体退出，主区铺满。
   *
   * 不使用覆盖层：背景是 `.app` 的全窗口绝对定位层，遮挡它的只有侧栏那块
   * 不透明面板。侧栏退出后背景连续铺满窗口，正在播放页因此产生零额外背景
   * 绘制，省去最昂贵的全屏合成。
   */
  const npOpen = $derived(ui.page === 'nowPlaying');

  async function playQueueAt(index: number): Promise<void> {
    try {
      await player.playTracks(player.queueIds, index);
    } catch (err) {
      // 起播失败不阻断运行：走播放条吐司（报错三档，2026-11 拍板）
      notify.showToast(errorMessage(err));
    }
  }
</script>

<div class="app" class:np-open={npOpen}>
  <!-- 背景：绝对定位在 .app 根部、四区席位之下。第 1 层为固定渐变，不跟随封面。全应用只有这一套背景；正在播放页通过"侧栏退出 +
  主区铺满"露出它，不另叠一层，否则两套频谱会同时消耗 GPU。 -->
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
    <!-- 运行时播放错误不在这里：按阻断级分档走弹窗/播放条吐司，见上方 effect -->

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
    队列 / EQ 抽屉：窗口右缘滑出的悬浮 SideDrawer 宿主
    （上下让出顶栏/播放条栏高再留呼吸边）。原先播放条上的 bits-ui
    Popover 气泡——floating-ui 测量+碰撞翻转让开合位置有可见抖动；
    抽屉几何确定（贴右缘、定宽、定边距），没有测量参与。
    互斥仍由 ui store 的 setQueue/setEqDrawer 负责，这里只做壳。
  -->
  <SideDrawer open={ui.queueOpen} label={t('shell.drawer.queue')} onclose={() => ui.closeQueue()}>
    <QueuePanel
      ids={player.queueIds}
      transient={player.queueTransient}
      index={playerState?.index ?? null}
      playing={playerState?.status === 'playing'}
      onplay={(index) => void playQueueAt(index)}
      onremove={(index) => player.removeAt(index)}
      onmove={(from, to) => player.move(from, to)}
      onclear={() => player.clear()}
      onclose={() => ui.closeQueue()}
    />
  </SideDrawer>

  <SideDrawer open={ui.eqOpen} label={t('shell.drawer.eq')} onclose={() => ui.setEqDrawer(false)}>
    <div class="eq-drawer-body">
      <EqPanel />
    </div>
  </SideDrawer>

  <!--
    拖放蒙版：整窗都是落点，所以这一层也铺满整窗。
    `pointer-events: none` —— 原生拖放由宿主接管，蒙版只负责显示，
    绝不能拦住指针（否则 enter/over/leave 的判定会抖）。

    文案分两行：第一行说"松开做什么"，第二行是 enter 时分类器的预告
    （`drop.hint`）。**文件夹不识别**这条规则就靠第二行在松手前说清，
    而不是等落点后弹一句"失败"。
  -->
  {#if drop.hovering}
    <div class="drop-veil">
      <div class="drop-card">
        <span class="drop-icon"><Icon name="folder" size={22} /></span>
        <strong>{t('shell.drop.title')}</strong>
        <span class="drop-hint">{drop.hint ?? t('shell.drop.hintFallback')}</span>
      </div>
    </div>
  {/if}
</div>

<style>
  /*
   * 拖放蒙版。
   *
   * 层级压在所有常驻壳层之上、吐司（z-index 60）之下：拖拽期间不该压住
   * 提示，但必须压住播放条与抽屉。
   *
   * 背景用 `--surface-1` 72% 半透明而不是纯黑：整个应用在这一刻还在
   * 正常渲染（背景频谱也可能在动），盖一层死黑会让窗口像"卡住了"。
   */
  .drop-veil {
    position: absolute;
    inset: 0;
    z-index: var(--z-pop);
    display: grid;
    place-items: center;
    padding: 24px;
    background: color-mix(in srgb, var(--surface-1) 72%, transparent);
    /* 指针必须穿透：原生拖放由宿主接管，蒙版只是显示 */
    pointer-events: none;
  }

  .drop-card {
    display: grid;
    justify-items: center;
    gap: 8px;
    max-width: min(520px, 80vw);
    padding: 26px 34px;
    border-radius: var(--radius-lg);
    border: 1px dashed var(--accent);
    background: color-mix(in srgb, var(--surface-3) 92%, transparent);
    box-shadow: var(--shadow-4);
    text-align: center;
  }

  .drop-icon {
    display: grid;
    place-items: center;
    width: 44px;
    height: 44px;
    border-radius: var(--radius-pill);
    background: var(--accent-weak);
    color: var(--accent-text);
  }

  .drop-card strong {
    font-size: var(--fs-lg);
    font-weight: 550;
    color: var(--text-1);
  }

  .drop-hint {
    font-size: var(--fs-sm);
    color: var(--text-2);
    /* 预告里可能带"（走设置 → 曲库导入）"，允许折行 */
    line-height: 1.5;
  }
</style>
