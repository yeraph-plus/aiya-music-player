<script lang="ts">
  import { ListMusic } from '@lucide/svelte';
  import LyricsPanel from '../components/LyricsPanel.svelte';
  import NowPlayingSpectrum from '../components/NowPlayingSpectrum.svelte';
  import { artworkUrl } from '../lib/artwork';
  import { audioBadge, audioTooltip, formatAudioSpec } from '../lib/audio-format';
  import { t } from '../lib/i18n.svelte';
  import { errorMessage } from '../lib/errors';
  import type { SpectrumSource } from '../lib/spectrum-source';
  import { player } from '../stores/player.svelte';
  import { lyrics } from '../stores/lyrics.svelte';
  import { nav } from '../stores/nav.svelte';
  import { ui } from '../stores/ui.svelte';

  /** 封面与歌词位于共享 GL 画布之上，频谱只登记本页内容区域。 */
  const current = $derived(player.state?.current ?? null);
  const spectrumStyle = $derived(ui.settings.spectrumStyle);

  /** 本页频谱：全局 3D 闸门 + 本页开关（背景那个开关不参与判断） */
  const npSpectrumOn = $derived(ui.settings.glEffects && ui.settings.npSpectrum);

  let spectrumSource = $state<SpectrumSource | null>(null);

  $effect(() => {
    // 关闭 = 本页不画可视化（背景环境光不受影响）：source 为 null 时
    // 组件整个不渲染，tap 引用计数也不会被本页持有
    if (!npSpectrumOn) {
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
  <!-- 页内**不放返回键**：顶栏自带一级返回，播放条的歌曲区是同一个开关（封面角标箭头朝下 = 已展开，再点收起），Esc
  也走同一条历史栈（lib/keyboard.ts）。三个入口语义一致，页内再放一个圆形按钮就是第四个重复动作。 -->
  <NowPlayingSpectrum
    source={spectrumSource}
    style={spectrumStyle}
    paused={player.state?.status !== 'playing'}
  />

  <div class="np-info">
    {#if current?.artworkId}
      <img
        class="np-cover"
        src={artworkUrl(current.artworkId, 512) ?? ''}
        alt={t('now.np.coverAlt', { title: current.title })}
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
      <h1 title={current?.title}>{current?.title ?? t('shell.player.notPlaying')}</h1>
      {#if current}
        {#if current.album && current.albumId}
          <button
            class="np-line np-album"
            title={t('tracks.viewAlbum', { name: current.album })}
            onclick={() => current?.albumId && nav.go({ kind: 'album', id: current.albumId })}
          >
            {current.album}
          </button>
        {:else if current.album}
          <p class="np-line np-album">{current.album}</p>
        {/if}
        {#if current.artistId}
          <button
            class="np-line"
            title={t('tracks.viewArtist', { name: current.artist ?? t('shell.player.unknownArtist') })}
            onclick={() => current?.artistId && nav.go({ kind: 'artist', id: current.artistId })}
          >
            {current.artist ?? t('shell.player.unknownArtist')}
          </button>
        {:else}
          <p class="np-line">{current.artist ?? t('shell.player.unknownArtist')}</p>
        {/if}
      {:else}
        <p class="np-line">{t('now.np.emptyHint')}</p>
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
