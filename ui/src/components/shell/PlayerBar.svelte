<script lang="ts">
  import type { PlayerState } from '../../lib/ipc/generated/PlayerState';
  import { artworkUrl } from '../../lib/artwork';
  import {
    AUDIO_RATE_MAX,
    AUDIO_RATE_MIN,
    AUDIO_RATE_STEP,
    audioBadge,
    audioTooltip,
    clampRate,
    formatAudioSpec,
    formatRate,
    isHiRes,
  } from '../../lib/audio-format';
  import { formatTime, isTransientId, repeatLabel, transientOrigin } from '../../lib/format';
  import { livePhase } from '../../lib/live';
  import { overlay } from '../../stores/overlay.svelte';
  import { ChevronUp, SlidersHorizontal } from '@lucide/svelte';
  import Icon from '../Icon.svelte';
  import Cover from '../ui/Cover.svelte';
  import Button from '../ui/Button.svelte';
  import IconButton from '../ui/IconButton.svelte';
  import Popover from '../ui/Popover.svelte';
  import Slider from '../ui/Slider.svelte';

  /**
   * 底部播放条：唯一常驻的播放控制面。
   *
   * 进度与音量的提交时机不同：进度使用 `oncommit`，音量使用 `oninput`。
   * 拖动中逐帧 seek 会使 Rust 侧反复 flush 输出缓冲（实测产生咔哒声）；
   * 音量则相反，用户预期拖动过程中即可听到变化。
   *
   * 按键布局：
   * - 左侧歌曲区域同时是正在播放页的入口；
   * - 主控五键为加入歌单、上一首、播放、下一首、播放模式单键循环；
   * - 队列与 EQ 由 AppShell 宿主的 `SideDrawer` 承担，此处只保留触发键，
   *   避免 Popover 依赖 floating-ui 测量与翻转导致的定位抖动；
   * - 倍速仍用贴键的 Popover（小面板，无定位抖动问题），位于右侧键区。
   */

  interface Props {
    player: PlayerState | null;
    queueOpen: boolean;
    eqOpen: boolean;
    /** EQ 是否启用（启用时触发键打点，不打开抽屉也能看出状态） */
    eqEnabled: boolean;
    /** 倍速滑杆的本地草稿（player store 防抖对账，见 dragRate）；null = 跟随后端 */
    rateDraft: number | null;
    ontoggle: () => void;
    onnext: () => void;
    onprev: () => void;
    onseek: (ms: number) => void;
    onvolume: (value: number) => void;
    onmute: () => void;
    onrate: (rate: number) => void;
    /** 播放模式单键循环（列表循环→单曲循环→随机→顺序） */
    onmode: () => void;
    /** 队列抽屉开合（ui store 唯一入口；SideDrawer 宿主在 AppShell） */
    onqueueopen: (open: boolean) => void;
    /** EQ 抽屉开合（与队列互斥，ui store 负责清理） */
    oneqopen: (open: boolean) => void;
    /** 跳正在播放页 / 从正在播放页退回（左侧歌曲区域的点击目标，是个开关） */
    onshownowplaying: () => void;
    /** 正在播放页是否开着：决定封面角标折线箭头的朝向 */
    npOpen: boolean;
  }

  let {
    player,
    queueOpen,
    eqOpen,
    eqEnabled,
    rateDraft,
    ontoggle,
    onnext,
    onprev,
    onseek,
    onvolume,
    onmute,
    onrate,
    onmode,
    onqueueopen,
    oneqopen,
    onshownowplaying,
    npOpen,
  }: Props = $props();

  /**
   * 拖动中的进度镜像。
   *
   * 拖动时不能直接显示 `player.positionMs`：播放引擎每 ~250ms 推一帧位置，
   * 拿它渲染会让拇指在跟手与跳回之间来回。松手（`oncommit`）才 seek，
   * 之后 `dragging=false`，位置重新以后端为准。
   */
  let dragging = $state(false);
  let dragMs = $state(0);
  let rateOpen = $state(false);

  const positionMs = $derived(dragging ? dragMs : (player?.positionMs ?? 0));

  /**
   * 时长三态：`null` = 未知（直播流，或本地文件尚未解析完成）
   * `0` = 空文件，正值 = 可拖动。
   *
   * 使用 `?? 0` 合并会把"未知"与"零秒"压成同一个值：直播流的时长位渲染为
   * `0:00`，进度条 `max` 经 `durationMs || 1` 兜底为 1 毫秒，表现为"时长为零的
   * 损坏文件"，而非"这是直播"。
   */
  const durationMs = $derived(player?.durationMs ?? null);
  /** 仅有正时长时才允许拖动：直播流不会进入该分支 */

  const seekable = $derived(durationMs !== null && durationMs > 0);
  /**
   * 直播流（`AudioSource` 的 `stream` 取值）。
   *
   * 语义：暂停即断开连接，播放即重新连接。因此徽章分三态
   * 不能统一显示"直播"——暂停状态下连接已不存在。
   */
  const isStream = $derived(player?.source === 'stream');
  /**
   * bits-ui Slider 的吸附行为（真机可复现进度条冻结，mock 后端无法复现）：
   * value 不在 step 网格上时，SliderRootState 内部的 watch 会将其吸附到最近
   * 网格点；该写入经由 `onValueChange` 通道，因此程序化的位置更新同样会触发
   * `oninput`，`dragging` 被判为 true，`positionMs` 从此固定为 `dragMs`
   * （引擎仍上报位置，但拖动镜像覆盖之）。
   *
   * 播放位置的毫秒值对 step=100 几乎总是非网格值，故该路径必然触发；mock 后端
   * 的位置定时器按 100ms 步进推进（见 mock/backend.ts 的 `tickPosition`），恰好
   * 始终落在网格上，e2e 因此全绿。处理方式：传入前先吸附到网格，内部 watch
   * 读取的值始终合法，不再触发该写入。
   */
  const sliderMs = $derived(Math.round(positionMs / 100) * 100);
  const playing = $derived(player?.status === 'playing');
  const loading = $derived(player?.status === 'loading');
  const hasTrack = $derived(Boolean(player?.current));

  // 滑杆防抖期间显示本地草稿（即时跟手），草稿作废后回落到后端状态
  const rate = $derived(clampRate(rateDraft ?? (player?.rate ?? 1)));
  const audio = $derived(player?.current?.audio ?? null);
  const badge = $derived(audioBadge(audio?.codec ?? null, audio?.container ?? null));
  const spec = $derived(formatAudioSpec(audio));
  const tooltip = $derived(audioTooltip(audio));

  /** 直播徽章三态（文案口径与直播页共用 `lib/live.ts`，避免两处各写一遍） */
  const phase = $derived(livePhase(player?.status));

  /**
   * 副标题来源：直播流取主机名，拖入的本地文件取所在目录。
   * 库内曲目两者均为 `null`，回落到艺人名。
   */
  const transient = $derived(isTransientId(player?.current?.id));
  const origin = $derived(transientOrigin(player?.current?.id));

  const volume = $derived(player?.muted ? 0 : (player?.volume ?? 0.85));

  /**
   * 播放模式（单键循环四态）。后端为 repeat 与 shuffle 两个正交开关，此处
   * 合成为单一"模式"取值：shuffle 开启时一律显示随机，与 repeat 取值无关。
   */
  type PlayMode = 'all' | 'one' | 'shuffle' | 'sequential';
  const mode = $derived<PlayMode>(
    player?.shuffle ? 'shuffle' : (player?.repeat ?? 'off') === 'one' ? 'one' : (player?.repeat ?? 'off') === 'all' ? 'all' : 'sequential',
  );
  const modeLabel = $derived(
    mode === 'shuffle' ? '随机播放' : repeatLabel(mode === 'sequential' ? 'off' : mode),
  );
  /** PlayMode → Icon 语义名（all 用 repeat 图标，one 用 repeatOne） */
  const modeIcon = $derived(
    mode === 'all' ? 'repeat' : mode === 'one' ? 'repeatOne' : mode,
  );

  /** 当前曲目加入歌单：复用全局 addToPlaylist 对话框（OverlayHost 渲染） */
  function addCurrentToPlaylist(): void {
    const id = player?.current?.id;
    if (id) overlay.dialog = { kind: 'addToPlaylist', ids: [id] };
  }
</script>

<footer class="player">
  <!--
    歌曲区域即正在播放页的开关。该区域本身展示当前播放内容
    作为入口比经由侧栏更直接。无曲目时仍可进入，由页面显示空态（"未在播放"），
    不设为 disabled。

    封面悬停提示：整块封面覆盖暗色蒙版与居中折线箭头，仅悬停或键盘聚焦时
    出现。箭头朝上表示可展开，朝下表示已展开、再次点击收起。页面内不设返回键，
    退出经此处或顶栏返回。
  -->
  <button
    class="now"
    class:open={npOpen}
    onclick={onshownowplaying}
    title={npOpen ? '收起正在播放' : '打开正在播放'}
    aria-label={npOpen ? '收起正在播放页' : '打开正在播放页'}
    aria-expanded={npOpen}
  >
    <span class="now-cover">
      <Cover
        src={player?.current?.artworkId ? artworkUrl(player.current.artworkId, 96) : null}
        size="md"
      />
      <span class="now-cue" aria-hidden="true">
        <span class="now-cue-icon">
          <ChevronUp size={20} strokeWidth={2.4} />
        </span>
      </span>
    </span>

    <div class="meta">
      <div class="title" title={player?.current?.title ?? ''}>
        {player?.current?.title ?? '未在播放'}
      </div>
      <div class="line2">
        {#if player?.current}
          {#if transient}
            <!-- 临时项没有艺人：直播给主机名、拖入的文件给所在目录 -->
            <span class="artist">{origin ?? (isStream ? '网络直播流' : '外部音频文件')}</span>
          {:else}
            <span class="artist">{player.current.artist ?? '未知艺人'}</span>
          {/if}
          {#if badge}
            <span
              class="fmt-badge"
              class:hires={audio ? isHiRes(audio) : false}
              title={tooltip ?? ''}
            >
              {badge}
            </span>
          {/if}
          {#if spec}
            <span class="fmt-spec" title={tooltip ?? ''}>{spec}</span>
          {/if}
        {:else}
          选择一首歌开始
        {/if}
      </div>
    </div>
  </button>

  <div class="transport">
    <div class="controls">
      <!-- 临时项（拖入的文件 / 直播流）不在曲库里，id 是 `file:`/`stream:`
           前缀串——写进歌单就是一行永远查不到的坏数据，直接禁掉 -->
      <IconButton
        label={transient ? '临时项不能加入播放列表' : '把当前曲目加入播放列表'}
        round
        disabled={!hasTrack || transient}
        onclick={addCurrentToPlaylist}
      >
        <Icon name="heart" />
      </IconButton>

      <IconButton label="上一首" round disabled={!hasTrack} onclick={onprev}>
        <Icon name="prev" size={18} />
      </IconButton>

      <IconButton
        label={playing ? (isStream ? '断开直播' : '暂停') : isStream ? '连接直播' : '播放'}
        size="lg"
        variant="solid"
        round
        {loading}
        disabled={!hasTrack && (player?.queueLength ?? 0) === 0}
        onclick={ontoggle}
      >
        {#if playing}
          <Icon name="pause" size={16} />
        {:else}
          <Icon name="play" size={16} />
        {/if}
      </IconButton>

      <IconButton label="下一首" round disabled={!hasTrack} onclick={onnext}>
        <Icon name="next" size={18} />
      </IconButton>

      <!-- 模式单键循环：列表循环 → 单曲循环 → 随机 → 顺序 -->
      <IconButton label={`播放模式：${modeLabel}`} round pressed={mode !== 'sequential'} onclick={onmode}>
        <Icon name={modeIcon} />
      </IconButton>
    </div>

    <div class="timeline" class:live={isStream}>
      <!-- 左 = 已收听时长。直播没有"当前位置"这个概念，但 `positionMs` 在 stream 下单调递增（Actor 从 0 起算、断开即归零），直接当已收听时长用就对。 -->
      <span class="t" title={isStream ? '已收听' : undefined}>{formatTime(positionMs)}</span>

      {#if isStream}
        <!--
          直播：进度条换成一条**呼吸的横线**。
          纯 CSS 动画，**不另起 rAF**（守 单 rAF 约定）；
          - 不是 `<input>`，也不留 disabled 滑块：直播没有可拖的刻度，
            给一个"拖不动"的滑块只会让人以为是坏的；
          - 断开时（paused = 已断连）动画停下、变暗，和徽章三态对齐。
        -->
        <span class="live-bar" class:on={playing} aria-hidden="true"></span>
      {:else}
        <Slider
          label="播放进度"
          value={sliderMs}
          min={0}
          max={Math.max(durationMs ?? 0, 1)}
          step={100}
          disabled={!hasTrack || !seekable}
          oninput={(value) => {
            dragging = true;
            dragMs = value;
          }}
          oncommit={(value) => {
            if (!dragging) return;
            dragging = false;
            onseek(value);
          }}
        />
      {/if}

      {#if isStream}
        <span
          class="t live-badge"
          class:on={phase.tone === 'on'}
          class:wait={phase.tone === 'wait'}
          title={phase.tone === 'on'
            ? '直播流 · 暂停会断开连接，再播放即重连'
            : phase.tone === 'wait'
              ? '正在连接直播流…'
              : '已断开连接 · 按播放重新连接'}
        >
          <span class="live-dot" aria-hidden="true"></span>{phase.text}
        </span>
      {:else}
        <span class="t">{seekable ? formatTime(durationMs) : '--:--'}</span>
      {/if}
    </div>
  </div>

  <div class="right">
    <!--
      队列 / EQ 抽屉触发键：内容宿主是 AppShell 的 SideDrawer
      （窗口右缘滑出，几何确定不抖）。这里只是普通开关键：开合态走 IconButton
      的 expanded（aria-expanded + accent 底），外点/Esc/焦点还原全部由抽屉负责。
      EQ 的"生效中"打点走 dot。
    -->
    <IconButton
      label={`播放队列（${player?.queueLength ?? 0} 首）`}
      expanded={queueOpen}
      onclick={() => onqueueopen(!queueOpen)}
    >
      <Icon name="queue" />
    </IconButton>
    <IconButton
      label={`均衡器（${eqEnabled ? '已启用' : '已关闭'}）`}
      expanded={eqOpen}
      dot={eqEnabled}
      onclick={() => oneqopen(!eqOpen)}
    >
      <SlidersHorizontal size={16} />
    </IconButton>

    <!-- 倍速：保持气泡（小面板贴键无抖动问题），不进抽屉 -->
    <Popover
      open={rateOpen}
      onopenchange={(value) => (rateOpen = value)}
      side="top"
      align="end"
      sideOffset={12}
    >
      {#snippet trigger(props)}
        <button
          {...props}
          class={`rate-chip${rate !== 1 ? ' on' : ''}`}
          ondblclick={() => onrate(1)}
          aria-label="播放速度"
          title={`播放速度 ${formatRate(rate)}（双击复位 1.0×）`}
        >
          {formatRate(rate)}
        </button>
      {/snippet}

      <div class="rate-panel">
        <div class="rate-head">
          <span>播放速度</span>
          <Button variant="link" onclick={() => onrate(1)}>复位</Button>
        </div>
        <Slider
          label="播放速度"
          value={rate}
          min={AUDIO_RATE_MIN}
          max={AUDIO_RATE_MAX}
          step={AUDIO_RATE_STEP}
          oninput={(value) => onrate(value)}
          width="100%"
        />
        <div class="rate-scale">
          <span>{formatRate(AUDIO_RATE_MIN)}</span>
          <span>1.0×</span>
          <span>{formatRate(AUDIO_RATE_MAX)}</span>
        </div>
      </div>
    </Popover>

    <IconButton label={player?.muted ? '取消静音' : '静音'} round onclick={onmute}>
      <Icon name={player?.muted || (player?.volume ?? 0) === 0 ? 'mute' : 'volume'} />
    </IconButton>
    <div class="volume">
      <Slider
        label="音量"
        value={volume}
        min={0}
        max={1}
        step={0.01}
        oninput={(value) => onvolume(value)}
        width="100%"
      />
    </div>
  </div>
</footer>

<style>
  /* 整条 */

  .player {
    display: grid;
    /* 左中右三段：左侧曲目信息固定下限、中间传输区占大头、右侧控制固定下限。
       三段的 minmax 让窄窗时先压缩左右，传输区不会被挤没。 */
    grid-template-columns: minmax(170px, 1fr) minmax(300px, 2.1fr) minmax(170px, 1fr);
    align-items: center;
    gap: 16px;
    height: var(--player-h);
    padding: 0 18px;
    /* 不设边线：与主区的分隔由上抛阴影承担。播放条是全应用最实的一层，
     * 阴影既给出分界，也表明它悬浮于背景之上（方向朝上，光源在上）。 */
    box-shadow: var(--shadow-up);
    /* 半透明 95%（与顶栏同档，接近不透明）：应用级背景从下层透出。底色取
     * surface-3，顶栏取 surface-2，同档不同阶，形成轻微色差。
     * 不使用 backdrop-filter：持久壳层覆盖在动画频谱 canvas 之上时，每帧都
     * 需对整条重新模糊。 */
    background: color-mix(in srgb, var(--surface-3) 95%, transparent);
  }

  /* 左：当前曲目（= 正在播放页入口） */

  .now {
    display: flex;
    align-items: center;
    gap: 10px;
    min-width: 0;
    padding: 4px 6px;
    margin: -4px -6px; /* 抵消内距：视觉占位与原 div 一致，只多出悬停热区 */
    border-radius: 10px;
    text-align: left;
    transition: background var(--dur-fast) var(--ease);
  }

  .now:hover {
    background: var(--surface-hover);
  }

  /* 封面外面只多一层定位盒，供 hover 提示蒙版精确铺满封面 */
  .now-cover {
    position: relative;
    flex: none;
    display: block;
    border-radius: var(--radius-md);
  }

  /* 折线箭头表示「还有一页」，同时反映开合状态：朝上为未展开（可进入），朝下为已展开（再次点击收起）。形态为覆盖整块封面的暗色蒙版加居中箭头。角标式小徽章在静止时同样显示，会在封面角落切出一块且尺寸过小难以辨认。蒙版仅在 hover
     或键盘聚焦时淡入：48px 的封面不宜常驻装饰，而"此处可点击"这一信息只在用户准备操作时需要。黑色蒙版与白箭头在任何封面上对比度一致，不随主题变化。 */
  .now-cue {
    position: absolute;
    inset: 0;
    display: grid;
    place-items: center;
    border-radius: var(--radius-md);
    background: rgba(0, 0, 0, 0.5);
    color: #fff;
    opacity: 0;
    pointer-events: none;
    transition: opacity var(--dur-fast) var(--ease);
  }

  .now:hover .now-cue,
  .now:focus-visible .now-cue {
    opacity: 1;
  }

  /* 箭头朝"将要去的那一侧"偏 1.5px：朝上 = 这页在上面。图标包一层自己的 span，而不是选 `.now-cue svg`——svg 是
     ChevronUp 组件渲染的，Svelte 的作用域规则选不到它（check 会报未使用选择器）。 */
  .now-cue-icon {
    display: grid;
    place-items: center;
    /* 箭头压在亮封面上也要立得住：极轻的一圈投影就够了，多了显脏 */
    filter: drop-shadow(0 1px 2px rgba(0, 0, 0, 0.55));
    transition: transform var(--dur-fast) var(--ease);
  }

  .now:hover .now-cue-icon,
  .now:focus-visible .now-cue-icon {
    transform: translateY(-1.5px);
  }

  /* 已展开 = 箭头朝下（"按下去收起"）。旋转在前、位移在后，
     位移跟着进旋转后的坐标系 → 展开时箭头向下偏，和朝向一致 */
  .now.open .now-cue-icon {
    transform: rotate(180deg);
  }

  .now.open:hover .now-cue-icon,
  .now.open:focus-visible .now-cue-icon {
    transform: rotate(180deg) translateY(-1.5px);
  }

  .now:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: -2px;
  }

  .meta {
    min-width: 0;
    /* container-type 会让内容尺寸被抑制（contain: inline-size），flex 里 flex-basis:auto 就成了 0 —— 必须显式
       flex:1 占位，否则 .title（自身 overflow:hidden）宽度 0 被整行裁掉 */
    flex: 1;
    /* 窄到放不下时隐藏规格文字，徽章保留（见下面 @container） */
    container-type: inline-size;
  }

  .title {
    font-size: var(--fs-base);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .line2 {
    display: flex;
    align-items: center;
    gap: 6px;
    margin-top: 2px;
    font-size: var(--fs-sm);
    color: var(--text-2);
    overflow: hidden;
    white-space: nowrap;
  }

  .artist {
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .fmt-badge {
    flex: none;
    padding: 0 4px;
    border-radius: 3px;
    border: 1px solid var(--line-strong);
    font-size: var(--fs-2xs);
    line-height: 14px;
    letter-spacing: 0.04em;
    color: var(--text-2);
  }

  .fmt-badge.hires {
    border-color: var(--accent);
    color: var(--accent-text);
  }

  .fmt-spec {
    flex: none;
    font-size: var(--fs-2xs);
    color: var(--text-3);
    font-variant-numeric: tabular-nums;
  }

  @container (max-width: 250px) {
    .fmt-spec {
      display: none;
    }
  }

  /* 中：传输 + 进度 */

  .transport {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 6px;
    min-width: 0;
  }

  .controls {
    display: flex;
    align-items: center;
    gap: 12px;
  }


  .timeline {
    width: 100%;
    display: flex;
    align-items: center;
    gap: 10px;
    font-size: var(--fs-xs);
    color: var(--text-2);
    font-variant-numeric: tabular-nums;
  }

  .timeline .t {
    flex: none;
    width: 38px;
  }

  .timeline .t:last-child {
    text-align: right;
  }

  /* 直播 */

  /*
   * 呼吸条：替代进度滑块。纯 CSS 动画，**不另起 rAF**（守
   * 单循环约定）；也不是 `<input>`——直播没有可拖的刻度，摆一个拖不动的
   * 滑块只会让人以为是坏的。
   *
   * `.on` = 正在播。断开（paused）时摘掉它：暂停在本产品里就是**真的断连**，
   * 界面不该继续"流动"骗人。
   */
  .live-bar {
    position: relative;
    flex: 1;
    height: 3px;
    border-radius: var(--radius-pill);
    background: var(--surface-active);
    overflow: hidden;
  }

  .live-bar::after {
    content: '';
    position: absolute;
    inset: 0;
    background: linear-gradient(
      90deg,
      transparent 0%,
      color-mix(in srgb, var(--accent) 75%, transparent) 50%,
      transparent 100%
    );
    animation: live-breathe 2.8s ease-in-out infinite;
  }

  /* 断开态：静止灰线，不播放动画、不加渐变 */
  .live-bar:not(.on)::after {
    animation: none;
    background: none;
  }

  @keyframes live-breathe {
    0%,
    100% {
      transform: translateX(-60%);
      opacity: 0.3;
    }
    50% {
      transform: translateX(60%);
      opacity: 1;
    }
  }

  /* 徽章：`直播` / `连接中` / `已断开`。宽度自适应（时长位那个 38px 装不下） */
  .timeline .t.live-badge {
    width: auto;
    min-width: 38px;
    display: inline-flex;
    align-items: center;
    justify-content: flex-end;
    gap: 4px;
    padding-left: 4px;
    letter-spacing: 0.02em;
    color: var(--text-3);
  }

  .timeline .t.live-badge.on {
    color: var(--accent-text);
  }

  .timeline .t.live-badge.wait {
    color: var(--text-2);
  }

  .live-dot {
    flex: none;
    width: 5px;
    height: 5px;
    border-radius: 50%;
    background: currentColor;
  }

  .live-badge.on .live-dot {
    animation: live-pulse 1.8s ease-in-out infinite;
  }

  @keyframes live-pulse {
    50% {
      opacity: 0.25;
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .live-bar::after,
    .live-badge.on .live-dot {
      animation: none;
    }
  }

  /* 右：队列 / EQ / 倍速 / 音量 */

  .right {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: 6px;
    min-width: 0;
  }

  .volume {
    flex: none;
    width: 92px;
  }

  /* 倍速面板（Popover 内容） */



  .rate-chip {
    flex: none;
    height: 22px;
    min-width: 44px;
    padding: 0 7px;
    border-radius: var(--radius-pill);
    font-size: var(--fs-xs);
    font-variant-numeric: tabular-nums;
    color: var(--text-2);
    border: 1px solid transparent;
    display: grid;
    place-items: center;
  }

  .rate-chip:hover {
    color: var(--text-1);
    background: var(--surface-hover);
  }

  /* 非 1× 高亮：和模式键的开启态同一套语义 */
  .rate-chip.on {
    color: var(--accent);
    background: var(--accent-weak);
  }

  .rate-panel {
    width: 186px;
  }

  .rate-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    font-size: var(--fs-sm);
    margin-bottom: 10px;
  }

  .rate-reset {
    font-size: var(--fs-sm);
    color: var(--text-2);
    padding: 1px 6px;
    border-radius: 4px;
  }

  .rate-reset:hover {
    color: var(--text-1);
    background: var(--surface-hover);
  }

  .rate-scale {
    display: flex;
    justify-content: space-between;
    font-size: var(--fs-2xs);
    color: var(--text-3);
    font-variant-numeric: tabular-nums;
    margin-top: 6px;
  }
</style>
