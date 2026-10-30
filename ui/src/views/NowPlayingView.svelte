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
  import { nav } from '../stores/nav.svelte';
  import { ui } from '../stores/ui.svelte';

  /**
   * 正在播放页（docs/25 §4 T4a 两栏沉浸）。
   *
   * 内容层（第 4 层）= 左列封面信息 + 右列歌词，**去卡片化**浮在背景上；
   * 第 1-2 层（固定渐变 / 山形环境光）由 AppShell 根部的 AmbientBackdrop
   * 全应用提供（T4c app 化），本页**不画自己的背景**：侧栏在这一页整体退出、
   * 主区铺满（docs/46 §5.2），背景层因此连续地铺满整个窗口。
   *
   * 页内**可视化**（docs/26 §3，形态由设置「频谱形态」控制：
   * 镜像曲线 / 径向环形 / 中轴波纹）垫在两栏内容之后——
   * 与背景层共享同一 SpectrumSource 实例（start/stop 引用计数叠放），
   * 数据源按 `npSpectrum` 开关派生：关了只影响本页，背景照常。
   */
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
  <!-- 页内**不放返回键**（2026-10-30 用户拍板）：顶栏自带一级返回，
       播放条的歌曲区是同一个开关（封面角标箭头朝下 = 已展开，再点收起），
       Esc 也走同一条历史栈（lib/keyboard.ts）。三个入口语义一致，
       页内再放一个圆形按钮就是第四个重复动作。 -->
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
        {#if current.album && current.albumId}
          <button
            class="np-line np-album"
            title={`查看专辑 ${current.album}`}
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
            title={`查看歌手 ${current.artist ?? '未知艺人'}`}
            onclick={() => current?.artistId && nav.go({ kind: 'artist', id: current.artistId })}
          >
            {current.artist ?? '未知艺人'}
          </button>
        {:else}
          <p class="np-line">{current.artist ?? '未知艺人'}</p>
        {/if}
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
