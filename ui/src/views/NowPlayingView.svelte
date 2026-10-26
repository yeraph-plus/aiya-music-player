<script lang="ts">
  import { AudioLines, ListMusic } from '@lucide/svelte';
  import LyricsPanel from '../components/LyricsPanel.svelte';
  import SpectrumPanel from '../components/SpectrumPanel.svelte';
  import Button from '../components/ui/Button.svelte';
  import SegmentedControl from '../components/ui/SegmentedControl.svelte';
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

  /**
   * 两块可切（docs/24 B5a 三块 → docs/25 §1 T3 摘走 EQ）：歌词 / 频谱
   * 共占一个内容区。EQ 已迁往播放条抽屉（ui.eqOpen）。
   * 选中值存页面本地（不是设置项）：上次看歌词这次想看频谱，属页面内切换，
   * 记忆它反而要在设置 schema 里加一个纯 UI 字段。
   */
  type NowTab = 'lyrics' | 'spectrum';
  const TABS = [
    { value: 'lyrics', label: '歌词' },
    { value: 'spectrum', label: '频谱' },
  ] as const;
  let tab = $state<NowTab>('lyrics');
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

  <div class="now-playing-tabs">
    <SegmentedControl
      value={tab}
      options={TABS}
      label="正在播放页内容"
      onchange={(v) => (tab = v as NowTab)}
    />
  </div>

  <div class="now-playing-grid">
    {#if tab === 'lyrics'}
      <LyricsPanel
        lyrics={lyrics.doc}
        loading={lyrics.loading}
        trackTitle={current?.title ?? null}
        positionMs={player.state?.positionMs ?? 0}
        durationMs={player.state?.durationMs ?? 0}
        playing={player.state?.status === 'playing'}
        onseek={(ms) => player.seek(ms)}
      />
    {:else if tab === 'spectrum'}
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
    {/if}
  </div>
</div>
