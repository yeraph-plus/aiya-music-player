<script lang="ts">
  import { ListMusic } from '@lucide/svelte';
  import LyricsPanel from '../components/LyricsPanel.svelte';
  import { artworkUrl } from '../lib/artwork';
  import { formatAudioSpec } from '../lib/audio-format';
  import { lyrics } from '../stores/lyrics.svelte';
  import { player } from '../stores/player.svelte';

  /**
   * 正在播放页（docs/25 §4 T4a 两栏改版）：**去卡片化**。
   *
   * 左列 = 封面 + 标题 + 艺人·专辑 + 音频规格，无边框无底色浮在背景上
   * （可读性由背景遮罩层负责，见 §5 T4b）；右列 = 歌词，容器透明。
   * 原 hero 卡片 / SegmentedControl（歌词·频谱·EQ 三块可切）全部拆除：
   * - EQ 已迁播放条抽屉（§1 T3，ui.eqOpen）；
   * - 频谱转投背景环境层（§5 T4b）——SpectrumPanel 组件保留，
   *   入口随「沉浸模式」后补，本页不再渲染页内频谱块。
   */
  const current = $derived(player.state?.current ?? null);
</script>

<div class="now-playing-page">
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
      <span class="page-kicker">NOW PLAYING</span>
      <h1>{current?.title ?? '未在播放'}</h1>
      <p>
        {current
          ? `${current.artist ?? '未知艺人'}${current.album ? ` · ${current.album}` : ''}`
          : '从推荐页或歌单选择一首歌开始'}
      </p>
      {#if current?.audio}<span class="audio-spec">{formatAudioSpec(current.audio)}</span>{/if}
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
