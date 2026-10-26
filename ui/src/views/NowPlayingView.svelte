<script lang="ts">
  import { ListMusic } from '@lucide/svelte';
  import LyricsPanel from '../components/LyricsPanel.svelte';
  import NowPlayingSpectrum from '../components/NowPlayingSpectrum.svelte';
  import { artworkUrl } from '../lib/artwork';
  import { audioBadge, audioTooltip, formatAudioSpec } from '../lib/audio-format';
  import { errorMessage } from '../lib/errors';
  import type { SpectrumSource } from '../lib/spectrum-source';
  import { player } from '../stores/player.svelte';
  import { lyrics } from '../stores/lyrics.svelte';
  import { ui } from '../stores/ui.svelte';

  /**
   * 正在播放页（docs/25 §4 T4a 两栏沉浸）。
   *
   * 内容层（第 4 层）= 左列封面信息 + 右列歌词，**去卡片化**浮在背景上；
   * 第 1-3 层（固定渐变 / 山形环境光 / 可读性遮罩）由 AppShell 根部的
   * AmbientBackdrop 全应用提供（T4c app 化），本页无遮罩档。
   * 页内**居中式可视化**（docs/26 §3，形态由设置「频谱风格」控制：
   * 镜像曲线 / 径向环形 / 示波线）垫在两栏内容之后——
   * 与背景层共享同一 SpectrumSource 实例（start/stop 引用计数叠放），
   * 数据源按全局频谱设置派生：关了设置这里和背景一起静默。
   */
  const current = $derived(player.state?.current ?? null);
  const spectrumStyle = $derived(ui.settings.spectrumStyle);

  let spectrumSource = $state<SpectrumSource | null>(null);

  $effect(() => {
    // 「关闭」= 本页不画居中可视化（只剩背景环境光）：source 为 null 时
    // 组件整个不渲染，tap 引用计数也不会被本页持有
    if (!ui.settings.spectrum || ui.settings.spectrumStyle === 'off') {
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
</script>

<div class="now-playing-page">
  <NowPlayingSpectrum source={spectrumSource} style={spectrumStyle} />

  <div class="np-info">
    {#if current?.artworkId}
      <img
        class="np-cover"
        src={artworkUrl(current.artworkId, 512) ?? ''}
        alt={`《${current.title}》封面`}
      />
    {:else}
      <span class="np-cover np-cover-empty"><ListMusic size={64} strokeWidth={1.3} /></span>
    {/if}
    <div class="np-meta">
      {#if current?.audio}
        {@const badge = audioBadge(current.audio.codec, current.audio.container)}
        {@const spec = formatAudioSpec(current.audio)}
        {#if badge || spec}
          <span class="audio-spec" title={audioTooltip(current.audio) ?? undefined}>
            {[badge, spec].filter(Boolean).join(' · ')}
          </span>
        {/if}
      {/if}
      <h1>{current?.title ?? '未在播放'}</h1>
      {#if current}
        {#if current.album}<p class="np-line np-album">{current.album}</p>{/if}
        <p class="np-line">{current.artist ?? '未知艺人'}</p>
      {:else}
        <p class="np-line">从推荐页或歌单选择一首歌开始</p>
      {/if}
    </div>
  </div>

  <div class="np-lyrics">
    <LyricsPanel
      lyrics={lyrics.doc}
      loading={lyrics.loading}
      trackTitle={current?.title ?? null}
      positionMs={player.state?.positionMs ?? 0}
      durationMs={player.state?.durationMs ?? 0}
      playing={player.state?.status === 'playing'}
      onseek={(ms) => player.seek(ms)}
    />
  </div>
</div>
