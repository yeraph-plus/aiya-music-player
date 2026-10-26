<script lang="ts">
  import type { Snippet } from 'svelte';
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
  import { formatTime, repeatLabel } from '../../lib/format';
  import Icon from '../Icon.svelte';
  import Cover from '../ui/Cover.svelte';
  import IconButton from '../ui/IconButton.svelte';
  import Popover from '../ui/Popover.svelte';
  import Slider from '../ui/Slider.svelte';
  import { AudioLines, SlidersHorizontal } from '@lucide/svelte';

  /**
   * 底部播放条：**唯一常驻的播放控制面**。
   *
   * B3b-2 换皮时改了三处**实现**（外观口径不变）：
   * 1. 两个 `<input type="range">` → `Slider`（bits-ui）。顺带把全局
   *    `input[type=range]` 那套 `-webkit-slider-thumb` 伪元素样式删了 ——
   *    它在亮色主题下拇指是深色圆点、深色主题下又是浅色，两套值各写一遍。
   *    **进度用 `oncommit`、音量用 `oninput`**：拖动中每帧 seek 会让 Rust 侧
   *    反复 flush 输出缓冲（实测有咔哒声），音量则相反 —— 用户期待拖的时候就听到。
   * 2. 八个图标键 → `IconButton`（圆形 + `solid` 只给中间那颗播放键）。
   * 3. 封面 → `Cover size="md"`（48px，旧 `.cover.lg` 在曲库里也是 48 ——
   *    kit 的四档 22/34/48/92 里 92 是详情页头，播放条用不到）。
   *
   * 倍速面板仍是 `Popover` + `Slider`（B3b-1 已迁），这里不动。
   */

  interface Props {
    player: PlayerState | null;
    queueOpen: boolean;
    nowPlayingActive: boolean;
    ontoggle: () => void;
    onnext: () => void;
    onprev: () => void;
    onseek: (ms: number) => void;
    onvolume: (value: number) => void;
    onmute: () => void;
    onrate: (rate: number) => void;
    onrepeat: () => void;
    onshuffle: () => void;
    /** 队列抽屉开合（Popover 受控） */
    onqueueopen: (open: boolean) => void;
    /** 队列抽屉内容（关闭时不渲染——bits-ui 卸载 Content） */
    queuepanel: Snippet;
    /** EQ 抽屉开合（与队列互斥，ui store 负责清理，docs/25 §1） */
    eqOpen: boolean;
    /** EQ 是否启用（启用时触发键打点，不打开抽屉也能看出状态） */
    eqEnabled: boolean;
    oneqopen: (open: boolean) => void;
    /** EQ 抽屉内容 */
    eqpanel: Snippet;
    onshownowplaying: () => void;
  }

  let {
    player,
    queueOpen,
    nowPlayingActive,
    ontoggle,
    onnext,
    onprev,
    onseek,
    onvolume,
    onmute,
    onrate,
    onrepeat,
    onshuffle,
    onqueueopen,
    queuepanel,
    eqOpen,
    eqEnabled,
    oneqopen,
    eqpanel,
    onshownowplaying,
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
  const durationMs = $derived(player?.durationMs ?? 0);
  const playing = $derived(player?.status === 'playing');
  const loading = $derived(player?.status === 'loading');
  const hasTrack = $derived(Boolean(player?.current));

  const rate = $derived(clampRate(player?.rate ?? 1));
  const audio = $derived(player?.current?.audio ?? null);
  const badge = $derived(audioBadge(audio?.codec ?? null, audio?.container ?? null));
  const spec = $derived(formatAudioSpec(audio));
  const tooltip = $derived(audioTooltip(audio));

  const volume = $derived(player?.muted ? 0 : (player?.volume ?? 0.85));
</script>

<footer class="player">
  <div class="now">
    <Cover
      src={player?.current?.artworkId ? artworkUrl(player.current.artworkId, 96) : null}
      size="md"
    />
    <div class="meta">
      <div class="title" title={player?.current?.title ?? ''}>
        {player?.current?.title ?? '未在播放'}
      </div>
      <div class="line2">
        {#if player?.current}
          <span class="artist">{player.current.artist ?? '未知艺人'}</span>
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
  </div>

  <div class="transport">
    <div class="controls">
      <IconButton
        label={player?.shuffle ? '随机播放：开' : '随机播放：关'}
        round
        pressed={player?.shuffle ?? false}
        onclick={onshuffle}
      >
        <Icon name="shuffle" />
      </IconButton>

      <IconButton label="上一首" round disabled={!hasTrack} onclick={onprev}>
        <Icon name="prev" size={18} />
      </IconButton>

      <IconButton
        label={playing ? '暂停' : '播放'}
        size="lg"
        variant="solid"
        round
        disabled={!hasTrack && (player?.queueLength ?? 0) === 0}
        onclick={ontoggle}
      >
        {#if loading}
          <span class="spinner"></span>
        {:else if playing}
          <Icon name="pause" size={16} />
        {:else}
          <Icon name="play" size={16} />
        {/if}
      </IconButton>

      <IconButton label="下一首" round disabled={!hasTrack} onclick={onnext}>
        <Icon name="next" size={18} />
      </IconButton>

      <IconButton
        label={repeatLabel(player?.repeat ?? 'off')}
        round
        pressed={player?.repeat !== 'off'}
        onclick={onrepeat}
      >
        <Icon name={player?.repeat === 'one' ? 'repeatOne' : 'repeat'} />
      </IconButton>

      <Popover
        open={rateOpen}
        onopenchange={(value) => (rateOpen = value)}
        side="top"
        align="center"
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
            <button class="rate-reset" onclick={() => onrate(1)}>复位</button>
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
    </div>

    <div class="timeline">
      <span class="t">{formatTime(positionMs)}</span>
      <Slider
        label="播放进度"
        value={positionMs}
        min={0}
        max={durationMs || 1}
        step={100}
        disabled={!hasTrack || durationMs === 0}
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
      <span class="t">{formatTime(durationMs)}</span>
    </div>
  </div>

  <div class="right">
    <IconButton label="正在播放" round active={nowPlayingActive} onclick={onshownowplaying}>
      <AudioLines size={17} />
    </IconButton>
    <!--
      EQ 抽屉（docs/25 §1 T3）：与队列抽屉同一套壳（裸 button + Popover），
      互斥由 ui store 的 setEqDrawer/setQueue 负责。启用时键面右下打点——
      EQ 是"开着生效"的能力，不点开抽屉也该看得见状态。
    -->
    <Popover
      open={eqOpen}
      onopenchange={oneqopen}
      side="top"
      align="end"
      sideOffset={8}
      contentClass="queue-pop"
    >
      {#snippet trigger(props)}
        <button
          {...props}
          class="drawer-btn"
          class:on={eqEnabled}
          aria-label={`均衡器（${eqEnabled ? '已启用' : '已关闭'}）`}
          title={`均衡器（${eqEnabled ? '已启用' : '已关闭'}）`}
        >
          <SlidersHorizontal size={16} />
          {#if eqEnabled}<span class="drawer-dot" aria-hidden="true"></span>{/if}
        </button>
      {/snippet}
      {@render eqpanel()}
    </Popover>
    <!--
      队列抽屉（B5a）：kit Popover 整壳接管。触发键的 aria-expanded、Esc、
      外点关闭、关闭后焦点还原全部由 bits-ui 负责——取代 QueuePanel 里手写的
      svelte:window 两件套（外点 + Esc）。触发器不能复用 IconButton：
      bits-ui 的 props（含它的 onclick）必须原样展开到触发元素上，而 IconButton
      是固定签名组件；这里按 rate-chip 的先例用带 scoped 样式的裸 button。
    -->
    <Popover
      open={queueOpen}
      onopenchange={onqueueopen}
      side="top"
      align="end"
      sideOffset={8}
      contentClass="queue-pop"
    >
      {#snippet trigger(props)}
        <button
          {...props}
          class="drawer-btn"
          aria-label={`播放队列（${player?.queueLength ?? 0} 首）`}
          title={`播放队列（${player?.queueLength ?? 0} 首）`}
        >
          <Icon name="queue" />
        </button>
      {/snippet}
      {@render queuepanel()}
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
  /* ---------------- 整条 ---------------- */

  .player {
    display: grid;
    /* 左中右三段：左侧曲目信息固定下限、中间传输区占大头、右侧控制固定下限。
       三段的 minmax 让窄窗时先压缩左右，传输区不会被挤没。 */
    grid-template-columns: minmax(170px, 1fr) minmax(300px, 2.1fr) minmax(170px, 1fr);
    align-items: center;
    gap: 16px;
    height: var(--player-h);
    padding: 0 18px;
    border-top: 1px solid var(--line);
    /* 半透明（T4d，与顶栏同档 90%）：全应用背景从底下透出来。
     * 底色用 surface-3（顶栏是 surface-2）——同档不同阶，有一点色差。
     * 原 backdrop-filter 摘除——持久壳层垫着动画频谱 canvas 就是每帧
     * 全条重模糊（docs/25 §5 方案 B 红线）。 */
    background: color-mix(in srgb, var(--surface-3) 90%, transparent);
  }

  /* ---------------- 左：当前曲目 ---------------- */

  .now {
    display: flex;
    align-items: center;
    gap: 10px;
    min-width: 0;
  }

  .meta {
    min-width: 0;
    /* container-type 会让内容尺寸被抑制（contain: inline-size），
       flex 里 flex-basis:auto 就成了 0 —— 必须显式 flex:1 占位，
       否则 .title（自身 overflow:hidden）宽度 0 被整行裁掉 */
    flex: 1;
    /* 窄到放不下时隐藏规格文字，徽章保留（见下面 @container） */
    container-type: inline-size;
  }

  .title {
    font-size: 12.5px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .line2 {
    display: flex;
    align-items: center;
    gap: 6px;
    margin-top: 2px;
    font-size: 11px;
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
    font-size: 9.5px;
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
    font-size: 10px;
    color: var(--text-3);
    font-variant-numeric: tabular-nums;
  }

  @container (max-width: 250px) {
    .fmt-spec {
      display: none;
    }
  }

  /* ---------------- 中：传输 + 进度 ---------------- */

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

  .spinner {
    width: 12px;
    height: 12px;
    border-radius: 50%;
    border: 2px solid color-mix(in srgb, var(--play-fg) 30%, transparent);
    border-top-color: var(--play-fg);
    animation: spin 0.7s linear infinite;
  }

  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }

  .timeline {
    width: 100%;
    display: flex;
    align-items: center;
    gap: 10px;
    font-size: 10.5px;
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

  /* ---------------- 右：正在播放 / 队列 / 音量 ---------------- */

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

  /* ---------------- 倍速面板（Popover 内容） ---------------- */

  /* 抽屉触发键（队列/EQ 共用）：IconButton 是固定签名组件（bits-ui 的 props
     无法展开到它身上），按 rate-chip 的先例用裸 button + 同款外观（30px 圆形
     ghost 键） */
  .drawer-btn {
    position: relative;
    display: inline-grid;
    place-items: center;
    width: 30px;
    height: 30px;
    border-radius: var(--radius-pill);
    color: var(--text-2);
    flex: none;
    transition:
      background var(--dur-fast) var(--ease),
      color var(--dur-fast) var(--ease);
  }

  .drawer-btn:hover {
    background: var(--surface-hover);
    color: var(--text-1);
  }

  /* data-state 由 bits-ui 写：open = 抽屉开着（与旧 pressed 高亮同语义） */
  .drawer-btn[data-state='open'] {
    background: var(--accent-weak);
    color: var(--accent-text);
  }

  /* EQ 启用打点：不用 accent 底（那是"抽屉开着"），只点一颗小点表示"生效中" */
  .drawer-btn.on {
    color: var(--text-1);
  }

  .drawer-dot {
    position: absolute;
    right: 5px;
    bottom: 5px;
    width: 5px;
    height: 5px;
    border-radius: 50%;
    background: var(--accent);
  }

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

  /* 非 1× 高亮：和 shuffle / repeat 的开启态同一套语义 */
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
    font-size: 11.5px;
    margin-bottom: 10px;
  }

  .rate-reset {
    font-size: 11px;
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
    font-size: 10px;
    color: var(--text-3);
    font-variant-numeric: tabular-nums;
    margin-top: 6px;
  }
</style>
