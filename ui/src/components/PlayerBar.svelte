<script lang="ts">
  import type { PlayerState } from '@aiya/contracts';
  import { artworkUrl } from '@aiya/contracts';
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
  } from '@aiya/contracts';
  import { formatTime, repeatLabel } from '../lib/format';
  import Icon from './Icon.svelte';

  interface Props {
    player: PlayerState | null;
    queueOpen: boolean;
    lyricsOpen: boolean;
    /** 频谱功能是否在设置里开启；关闭时按钮整个不出现 */
    spectrumEnabled: boolean;
    spectrumOpen: boolean;
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
    ontogglelyrics: () => void;
    ontogglespectrum: () => void;
    onreveal: () => void;
  }

  let {
    player,
    queueOpen,
    lyricsOpen,
    spectrumEnabled,
    spectrumOpen,
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
    ontogglelyrics,
    ontogglespectrum,
    onreveal,
  }: Props = $props();

  let dragging = $state(false);
  let dragMs = $state(0);
  let rateOpen = $state(false);
  let rateBox = $state<HTMLDivElement | null>(null);

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

  function onRateInput(event: Event): void {
    onrate(Number((event.currentTarget as HTMLInputElement).value));
  }

  function onRatePointerDown(event: PointerEvent): void {
    // 点击面板外部时收起；面板内部（滑块/按钮）不算
    const box = rateBox;
    if (!box || !rateOpen) return;
    if (event.target instanceof Node && box.contains(event.target)) return;
    rateOpen = false;
  }

  function onRateKeydown(event: KeyboardEvent): void {
    if (event.key === 'Escape') rateOpen = false;
  }
</script>

<svelte:window onpointerdown={onRatePointerDown} onkeydown={onRateKeydown} />

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
          <button class="link artist" onclick={onreveal} title="在文件夹中显示">
            {player.current.artist ?? '未知艺人'}
          </button>
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
      <div class="rate-box" bind:this={rateBox}>
        <button
          class="rate-chip"
          class:on={rate !== 1}
          class:open={rateOpen}
          onclick={() => (rateOpen = !rateOpen)}
          ondblclick={() => onrate(1)}
          aria-label="播放速度"
          title={`播放速度 ${formatRate(rate)}（双击复位 1.0×）`}
        >
          {formatRate(rate)}
        </button>
        {#if rateOpen}
          <div class="rate-pop" role="dialog" aria-label="播放速度">
            <div class="rate-head">
              <span>播放速度</span>
              <button class="rate-reset" onclick={() => onrate(1)}>复位</button>
            </div>
            <input
              class="rate-slider"
              type="range"
              min={AUDIO_RATE_MIN}
              max={AUDIO_RATE_MAX}
              step={AUDIO_RATE_STEP}
              value={rate}
              oninput={onRateInput}
              aria-label="播放速度"
            />
            <div class="rate-scale">
              <span>{formatRate(AUDIO_RATE_MIN)}</span>
              <span>1.0×</span>
              <span>{formatRate(AUDIO_RATE_MAX)}</span>
            </div>
          </div>
        {/if}
      </div>
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
      class:on={lyricsOpen}
      onclick={ontogglelyrics}
      aria-label="歌词"
      title="歌词"
    >
      <Icon name="lyrics" />
    </button>
    {#if spectrumEnabled}
      <button
        class="icon-btn"
        class:on={spectrumOpen}
        onclick={ontogglespectrum}
        aria-label="频谱"
        title="频谱"
      >
        <Icon name="spectrum" />
      </button>
    {/if}
    <button
      class="icon-btn"
      class:on={queueOpen}
      onclick={onqueue}
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
