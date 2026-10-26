<script lang="ts">
  import { ListMusic } from '@lucide/svelte';
  import AmbientBackdrop from '../components/AmbientBackdrop.svelte';
  import LyricsPanel from '../components/LyricsPanel.svelte';
  import { artworkUrl } from '../lib/artwork';
  import { formatAudioSpec } from '../lib/audio-format';
  import { errorMessage } from '../lib/errors';
  import type { SpectrumSource } from '../lib/spectrum-source';
  import { lyrics } from '../stores/lyrics.svelte';
  import { player } from '../stores/player.svelte';
  import { ui } from '../stores/ui.svelte';

  /**
   * 正在播放页（docs/25 §4 T4a 两栏 + §5 T4b 背景四层）。
   *
   * 内容层（第 4 层）= 左列封面信息 + 右列歌词，**去卡片化**浮在背景上；
   * 第 1-3 层（预模糊封面 / 频谱环境光 / 可读性遮罩）由 AmbientBackdrop
   * 负责——真正的 backdrop-filter 玻璃只给 .np-meta 这一个局部小块。
   * EQ 在播放条抽屉（§1 T3）；页内频谱块已拆，SpectrumPanel 的 pro 五形态
   * 入口随「沉浸模式」后补。
   */
  const current = $derived(player.state?.current ?? null);

  /** 背景频谱源：设置开着才建（tap 按需订阅，关着引擎零开销） */
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

  /** 背景用小图（96px）：反正要糊掉，省解码内存 */
  const bgArt = $derived(
    current?.artworkId ? artworkUrl(current.artworkId, 96) : null,
  );
</script>

<div class="now-playing-page">
  <AmbientBackdrop artworkUrl={bgArt} source={spectrumSource} spectrumOn={ui.settings.spectrum} />

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
