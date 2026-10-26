<script lang="ts">
  import { ListMusic } from '@lucide/svelte';
  import LyricsPanel from '../components/LyricsPanel.svelte';
  import { artworkUrl } from '../lib/artwork';
  import { formatAudioSpec } from '../lib/audio-format';
  import { player } from '../stores/player.svelte';
  import { lyrics } from '../stores/lyrics.svelte';

  /**
   * 正在播放页（docs/25 §4 T4a 两栏沉浸）。
   *
   * 内容层（第 4 层）= 左列封面信息 + 右列歌词，**去卡片化**浮在背景上；
   * 第 1-3 层（预模糊封面 / 频谱环境光 / 可读性遮罩）由 AppShell 根部的
   * AmbientBackdrop 全应用提供（T4c app 化），本页进入沉浸档遮罩。
   * 真正的 backdrop-filter 玻璃只给 .np-meta 这一个局部小块。
   * EQ 在播放条抽屉（§1 T3）；页内频谱块已拆，SpectrumPanel 的 pro 五形态
   * 入口随「沉浸模式」后补。
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
