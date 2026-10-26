<script lang="ts">
  import type { SpectrumSource } from '../lib/spectrum-source';

  interface Props {
    /** 频谱数据源（48 带 tap）；null 或 spectrumOn=false 时环境层不渲染 */
    source: SpectrumSource | null;
    spectrumOn: boolean;
    /**
     * 沉浸档：正在播放页用浅遮罩让背景放光进来；其余页面用重遮罩
     * 保列表可读性（背景只是"隐约透出"，不是主视觉）。
     */
    immersive: boolean;
  }

  let { source, spectrumOn, immersive }: Props = $props();

  /**
   * 全应用背景（docs/25 §5 T4c app 化 + T4d 固定渐变）。
   *
   * 三层（第 4 层是界面布局本身，在四区席位里）：
   * - 第 1 层 背景：固定 CSS 渐变（`--ambient-bg`，tokens.css 按
   *   `html[data-ambient]` 定义，设置页成套切换）。**不再跟随封面**——
   *   预模糊封面与封面主色提取已按用户拍板移除（T4d）。
   * - 第 2 层 频谱环境光：低分辨率 buffer 画柱 → `drawImage` 放大 +
   *   ctx.filter 大半径模糊——柔化在画布内部完成，频谱自己就是"叠在
   *   背景上的光"。刻意不用 backdrop-filter 垫在动画 canvas 上（合成器
   *   每帧全屏重模糊，方案 B 红线）。
   * - 第 3 层 可读性遮罩：只在 standard 档渲染。**沉浸档整个撤销**
   *   （T4d 用户反馈：正在播放页看不到频谱，就是被这层 72% 的底色
   *   压没了）——固定渐变自身已带可读性设计。
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

    if (immersive) {
      // 沉浸档（正在播放页）：**不虚化，直出**（用户反馈：关掉背景虚化，
      // 直接显示频谱层）。柱子贴底满幅、不透明度拉高——它是本页的主视觉，
      // 不是环境光。standard 档（列表页）才是"柔化成光"的画法。
      ctx.filter = 'none';
      ctx.globalAlpha = 0.9;
      ctx.drawImage(off, 0, height * 0.3, width, height * 0.7);
      ctx.filter = 'none';
      ctx.globalAlpha = 1;
    } else {
      // 大半径模糊 + 透明度 + 略微出血：频谱变成环境光而不是一排柱子
      ctx.filter = 'blur(26px)';
      ctx.globalAlpha = 0.55;
      ctx.drawImage(off, -width * 0.05, height * 0.25, width * 1.1, height * 0.85);
      ctx.filter = 'none';
      ctx.globalAlpha = 1;
    }

    frameId = requestAnimationFrame(draw);
  }

  // 数据源订阅与绘制循环同生命周期：有源就 start（开引擎 tap），无源 stop
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

<div class="ambient-bg" class:immersive>
  <div class="ambient-art"></div>
  {#if active}
    <canvas bind:this={canvas} class="ambient-spectrum"></canvas>
  {/if}
  {#if !immersive}
    <div class="ambient-veil"></div>
  {/if}
</div>

<style>
  .ambient-bg {
    position: absolute;
    inset: 0;
    z-index: 0;
    overflow: hidden;
    pointer-events: none;
  }

  /* 第 1 层：固定渐变（--ambient-bg 随 html[data-ambient] 切换）。
   * standard 档收着点（0.6）——列表页背景是氛围不是主角；沉浸档放满。 */
  .ambient-art {
    position: absolute;
    inset: 0;
    background: var(--ambient-bg);
    opacity: 0.6;
  }

  .immersive .ambient-art {
    opacity: 1;
  }

  .ambient-spectrum {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
  }

  /* 第 3 层可读性遮罩：仅 standard 档存在（沉浸档不渲染——频谱直接可见）。
   * 近乎均匀地压一层 surface-1，正文对比度优先，背景在缝隙里透出即可。 */
  .ambient-veil {
    position: absolute;
    inset: 0;
    background: linear-gradient(
      to top,
      color-mix(in srgb, var(--surface-1) 88%, transparent),
      color-mix(in srgb, var(--surface-1) 74%, transparent) 40%,
      color-mix(in srgb, var(--surface-1) 58%, transparent) 78%,
      color-mix(in srgb, var(--surface-1) 46%, transparent)
    );
  }
</style>
