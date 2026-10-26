<script lang="ts">
  import type { SpectrumSource } from '../lib/spectrum-source';

  interface Props {
    /** 当前曲目封面 URL（96px 足够——反正要糊掉）；null = 退主题底色 */
    artworkUrl: string | null;
    /** 频谱数据源（48 带 tap）；null 或 spectrumOn=false 时环境层不渲染 */
    source: SpectrumSource | null;
    spectrumOn: boolean;
  }

  let { artworkUrl, source, spectrumOn }: Props = $props();

  /**
   * 正在播放页的沉浸背景（docs/25 §5 T4b，方案 B：预模糊 + 频谱自发光 + 遮罩）。
   *
   * 三层各司其职，**没有一个全屏 backdrop-filter**：
   * - 第 1 层 背景：封面 `<img>` 自身 `filter: blur(64px)`——静态内容光栅化
   *   一次即被缓存，切歌才重算一次，零每帧成本（业界沉浸页的通用做法）。
   * - 第 2 层 频谱环境光：低分辨率 buffer 画柱 → `drawImage` 放大 + ctx.filter
   *   大半径模糊——柔化在**画布内部**完成，频谱自己就是"叠在背景上的光"，
   *   不依赖外层玻璃。这正是方案 B 的核心：backdrop-filter 背后若垫动画
   *   canvas，合成器每帧重建离屏 buffer 全屏重模糊（最贵用法），所以玻璃
   *   只给局部小块（.np-meta），整页用这套替代。
   * - 第 3 层 可读性遮罩：主题化渐变（--surface-1 从下往上压过来），
   *   深浅主题都成立。
   *
   * `prefers-reduced-motion: reduce` 时环境层整体不渲染（静态背景保留）。
   */
  let canvas = $state<HTMLCanvasElement | null>(null);
  let frameId = 0;

  const reducedMotion =
    typeof matchMedia !== 'undefined' && matchMedia('(prefers-reduced-motion: reduce)').matches;

  const active = $derived(Boolean(source) && spectrumOn && !reducedMotion);

  /** 低分辨率离屏 buffer：画柱在这里，主画布只做放大绘制 */
  let off: HTMLCanvasElement | null = null;

  function draw(): void {
    const target = canvas;
    if (!target || !source) return;
    const ctx = target.getContext('2d');
    if (!ctx) return;

    const dpr = Math.min(2, window.devicePixelRatio || 1);
    const width = target.clientWidth;
    const height = target.clientHeight;
    if (width === 0 || height === 0) {
      frameId = requestAnimationFrame(draw);
      return;
    }
    if (target.width !== Math.round(width * dpr)) {
      target.width = Math.round(width * dpr);
      target.height = Math.round(height * dpr);
    }
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, width, height);

    const BW = 128;
    const BH = 72;
    if (!off) off = document.createElement('canvas');
    if (off.width !== BW || off.height !== BH) {
      off.width = BW;
      off.height = BH;
    }
    const octx = off.getContext('2d');
    if (!octx) {
      frameId = requestAnimationFrame(draw);
      return;
    }
    octx.clearRect(0, 0, BW, BH);

    const bands = source.frame();
    const styles = getComputedStyle(target);
    const barColor = styles.getPropertyValue('--accent').trim() || '#7c9cff';
    octx.fillStyle = barColor;
    const count = bands.length;
    const slot = BW / count;
    for (let i = 0; i < count; i += 1) {
      const v = Math.min(1, bands[i] as number);
      const h = Math.max(1, v * BH);
      octx.fillRect(i * slot, BH - h, Math.max(1, slot - 0.5), h);
    }

    // 大半径模糊 + 低透明度 + 略微出血：频谱变成环境光而不是一排柱子
    ctx.filter = 'blur(26px)';
    ctx.globalAlpha = 0.5;
    ctx.drawImage(off, -width * 0.05, height * 0.25, width * 1.1, height * 0.85);
    ctx.filter = 'none';
    ctx.globalAlpha = 1;

    frameId = requestAnimationFrame(draw);
  }

  // 数据源订阅与绘制循环同生命周期：进页面 start（开引擎 tap），离开 stop
  $effect(() => {
    if (!active) return;
    source?.start();
    frameId = requestAnimationFrame(draw);
    return () => {
      cancelAnimationFrame(frameId);
      source?.stop();
    };
  });
</script>

<div class="ambient-bg">
  {#if artworkUrl}
    <img class="ambient-art" src={artworkUrl} alt="" />
  {/if}
  {#if active}
    <canvas bind:this={canvas} class="ambient-spectrum"></canvas>
  {/if}
  <div class="ambient-veil"></div>
</div>

<style>
  .ambient-bg {
    position: absolute;
    inset: 0;
    overflow: hidden;
    border-radius: 18px;
    pointer-events: none;
  }

  /* 出血 12%：blur 边缘会透出透明，拉大一点让边缘也糊满 */
  .ambient-art {
    position: absolute;
    inset: -12%;
    width: 124%;
    height: 124%;
    object-fit: cover;
    filter: blur(64px) saturate(1.4);
    opacity: 0.85;
  }

  .ambient-spectrum {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
  }

  /* 可读性遮罩：底部压得重（歌词/信息都在下半区呼吸），顶部放光进来 */
  .ambient-veil {
    position: absolute;
    inset: 0;
    background: linear-gradient(
      to top,
      color-mix(in srgb, var(--surface-1) 72%, transparent),
      color-mix(in srgb, var(--surface-1) 34%, transparent) 45%,
      transparent 82%
    );
  }
</style>
