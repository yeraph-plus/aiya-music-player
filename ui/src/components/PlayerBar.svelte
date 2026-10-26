<script lang="ts">
  import type { PlayerState } from '../lib/ipc/generated/PlayerState';
  import { artworkUrl } from '../lib/artwork';
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
  } from '../lib/audio-format';
  import { formatTime, repeatLabel } from '../lib/format';
  import Icon from './Icon.svelte';
  import Popover from './ui/Popover.svelte';
  import Slider from './ui/Slider.svelte';
  import { AudioLines } from '@lucide/svelte';

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
    onqueue: () => void;
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
    onqueue,
    onshownowplaying,
  }: Props = $props();

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

  function onSeekInput(event: Event): void {
    dragging = true;
    dragMs = Number((event.currentTarget as HTMLInputElement).value);
  }

  function onSeekCommit(): void {
    if (!dragging) return;
    dragging = false;
    onseek(dragMs);
  }

  /** 拖动即生效：倍速变化用户期待立刻听到，不需要松手才提交 */
  function onRateInput(value: number): void {
    onrate(value);
  }

  // 面板的"点外面关闭 / Esc 关闭 / 焦点还原"全交给 Popover（bits-ui），
  // 这里不再有 window 级监听 —— 原来那两行是每个浮层都要抄一遍的东西。
</script>

<footer class="player">
  <div class="now">
    {#if player?.current?.artworkId}
      <img class="cover lg" src={artworkUrl(player.current.artworkId, 96) ?? ''} alt="" />
    {:else}
      <span class="cover lg placeholder">♪</span>
    {/if}
    <div class="meta">
      <div class="title" title={player?.current?.title ?? ''}>
        {player?.current?.title ?? '未在播放'}
      </div>
      <div class="line2">
        {#if player?.current}
          <span class="artist">{player.current.artist ?? '未知艺人'}</span>
          {#if badge}
            <span class="fmt-badge" class:hires={audio ? isHiRes(audio) : false} title={tooltip ?? ''}>
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
      <button
        class="icon-btn"
        class:on={player?.shuffle}
        onclick={onshuffle}
        aria-label="随机播放"
        title={player?.shuffle ? '随机播放：开' : '随机播放：关'}
      >
        <Icon name="shuffle" />
      </button>
      <button class="icon-btn" onclick={onprev} aria-label="上一首" disabled={!hasTrack}>
        <Icon name="prev" size={18} />
      </button>
      <button
        class="icon-btn play"
        class:loading
        onclick={ontoggle}
        aria-label={playing ? '暂停' : '播放'}
        disabled={!hasTrack && (player?.queueLength ?? 0) === 0}
      >
        {#if loading}
          <span class="spinner"></span>
        {:else if playing}
          <Icon name="pause" size={16} />
        {:else}
          <Icon name="play" size={16} />
        {/if}
      </button>
      <button class="icon-btn" onclick={onnext} aria-label="下一首" disabled={!hasTrack}>
        <Icon name="next" size={18} />
      </button>
      <button
        class="icon-btn"
        class:on={player?.repeat !== 'off'}
        onclick={onrepeat}
        aria-label="循环模式"
        title={repeatLabel(player?.repeat ?? 'off')}
      >
        <Icon name={player?.repeat === 'one' ? 'repeatOne' : 'repeat'} />
      </button>
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
            oninput={onRateInput}
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
      <input
        type="range"
        min="0"
        max={durationMs || 1}
        step="100"
        value={positionMs}
        oninput={onSeekInput}
        onchange={onSeekCommit}
        onpointerup={onSeekCommit}
        aria-label="播放进度"
        disabled={!hasTrack || durationMs === 0}
      />
      <span class="t">{formatTime(durationMs)}</span>
    </div>
  </div>

  <div class="right">
    <button
      class="icon-btn"
      class:on={nowPlayingActive}
      onclick={onshownowplaying}
      aria-label="正在播放"
      title="正在播放：歌词与频谱"
    >
      <AudioLines size={17} />
    </button>
    <button
      class="icon-btn"
      class:on={queueOpen}
      onclick={onqueue}
      data-queue-trigger
      aria-label="播放队列"
      title={`播放队列（${player?.queueLength ?? 0} 首）`}
    >
      <Icon name="queue" />
    </button>
    <button class="icon-btn" onclick={onmute} aria-label="静音" title={player?.muted ? '取消静音' : '静音'}>
      <Icon name={player?.muted || (player?.volume ?? 0) === 0 ? 'mute' : 'volume'} />
    </button>
    <input
      class="volume-slider"
      type="range"
      min="0"
      max="1"
      step="0.01"
      value={player?.muted ? 0 : (player?.volume ?? 0.85)}
      oninput={(event) => onvolume(Number((event.currentTarget as HTMLInputElement).value))}
      aria-label="音量"
    />
  </div>
</footer>

<style>
  /* ---------------- 倍速面板 ----------------
   * 从 `styles/player.css` 搬来。唯一改动：不再自带
   * `position:absolute; bottom; right` —— 定位由 Popover（bits-ui）做，
   * 留着那三行会和浮层的 transform 叠在一起，面板会飘到窗口外。
   */

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
