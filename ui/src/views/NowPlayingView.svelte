<script lang="ts">
  import { AudioLines, ListMusic } from '@lucide/svelte';
  import LyricsPanel from '../components/LyricsPanel.svelte';
  import SpectrumPanel from '../components/SpectrumPanel.svelte';
  import Button from '../components/ui/Button.svelte';
  import { artworkUrl } from '../lib/artwork';
  import { formatAudioSpec } from '../lib/audio-format';
  import { errorMessage } from '../lib/errors';
  import type { SpectrumSource } from '../lib/spectrum-source';
  import { lyrics } from '../stores/lyrics.svelte';
  import { player } from '../stores/player.svelte';
  import { ui } from '../stores/ui.svelte';

  const current = $derived(player.state?.current ?? null);
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
</script>

<div class="now-playing-page">
  <header class="now-playing-hero">
    {#if current?.artworkId}
      <img src={artworkUrl(current.artworkId, 320) ?? ''} alt={`《${current.title}》封面`} />
    {:else}
      <span class="now-playing-cover"><ListMusic size={42} strokeWidth={1.3} /></span>
    {/if}
    <div class="now-playing-meta">
      <span class="page-kicker">NOW PLAYING</span>
      <h1>{current?.title ?? '未在播放'}</h1>
      <p>{current ? `${current.artist ?? '未知艺人'}${current.album ? ` · ${current.album}` : ''}` : '从首页或歌单选择一首歌开始'}</p>
      {#if current?.audio}<span class="audio-spec">{formatAudioSpec(current.audio)}</span>{/if}
    </div>
    <AudioLines class="now-playing-watermark" size={76} strokeWidth={1} />
  </header>

  <div class="now-playing-grid">
    <LyricsPanel
      lyrics={lyrics.doc}
      loading={lyrics.loading}
      trackTitle={current?.title ?? null}
      positionMs={player.state?.positionMs ?? 0}
      durationMs={player.state?.durationMs ?? 0}
      playing={player.state?.status === 'playing'}
      onseek={(ms) => player.seek(ms)}
    />
    {#if ui.settings.spectrum && spectrumSource}
      <SpectrumPanel source={spectrumSource} engine={ui.settings.spectrumEngine} />
    {:else}
      <section class="spectrum-panel spectrum-disabled">
        <header><strong>频谱</strong><span class="dim">已关闭</span></header>
        <div>
          <AudioLines size={32} />
          <p>频谱可视化当前已关闭</p>
          <Button size="sm" onclick={() => ui.applySettings({ ...ui.settings, spectrum: true })}>
            开启频谱
          </Button>
        </div>
      </section>
    {/if}
  </div>
</div>
