<script lang="ts">
  import type { SpectrumSource } from '../lib/spectrum-source';

  interface Props {
    /** 当前曲目封面 URL（96px 足够——反正要糊掉）；null = 退主题底色 */
    artworkUrl: string | null;
    /** 频谱数据源（48 带 tap）；null 或 spectrumOn=false 时环境层不渲染 */
    source: SpectrumSource | null;
    spectrumOn: boolean;
    /**
     * 沉浸档：正在播放页用浅遮罩让背景放光进来；其余页面用重遮罩
     * 保列表可读性（背景只是"隐约透出"，不是主视觉）。
     */
    immersive: boolean;
  }

  let { artworkUrl, source, spectrumOn, immersive }: Props = $props();

  /**
   * 全应用沉浸背景（docs/25 §5 T4b → T4c app 化，方案 B：预模糊 + 频谱自发光 + 遮罩）。
   *
   * 挂在 AppShell 根部、四区（顶栏/侧栏/主区/播放条）之下，整个应用共享一套
   * 四层结构：第 1 层预模糊封面、第 2 层频谱环境光、第 3 层可读性遮罩；
   * 第 4 层是界面布局本身。持久性壳层（顶栏/侧栏/播放条）一律**半透明底色、
   * 不用 backdrop-filter**——玻璃垫着动画 canvas 时合成器每帧重建离屏 buffer
   * 全屏重模糊（最贵用法），这是方案 B 的红线；真玻璃只留给瞬时浮层
   * （对话框/菜单/抽屉，打开时才付钱）和 .np-meta 一个展示性小块。
   *
   * 三层实现：
   * - 第 1 层 背景：封面 `<img>` 自身 `filter: blur(64px)`——静态内容光栅化
   *   一次即被缓存，切歌才重算一次，零每帧成本（业界沉浸页的通用做法）。
   * - 第 2 层 频谱环境光：低分辨率 buffer 画柱 → `drawImage` 放大 + ctx.filter
   *   大半径模糊——柔化在**画布内部**完成，频谱自己就是"叠在背景上的光"。
   * - 第 3 层 可读性遮罩：主题化渐变（--surface-1 从下往上压过来），深浅主题都
   *   成立；immersive 档压得轻（正在播放页），standard 档压得重（列表页）。
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
    z-index: 0;
    overflow: hidden;
    pointer-events: none;
  }

  /* 出血 12%：blur 边缘会透出透明，拉大一点让边缘也糊满。
   * 沉浸档放光（0.85），standard 档收着点（0.5）——列表页背景是氛围不是主角。 */
  .ambient-art {
    position: absolute;
    inset: -12%;
    width: 124%;
    height: 124%;
    object-fit: cover;
    filter: blur(64px) saturate(1.4);
    opacity: 0.5;
  }

  .immersive .ambient-art {
    opacity: 0.85;
  }

  .ambient-spectrum {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
  }

  /* 可读性遮罩（第 3 层）两档：
   * - standard（列表页）：近乎均匀地压一层 surface-1，正文对比度优先，
   *   背景在缝隙里透出即可；
   * - immersive（正在播放页）：底部压得重（信息/歌词在下半区呼吸），
   *   顶部放光进来。 */
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

  .immersive .ambient-veil {
    background: linear-gradient(
      to top,
      color-mix(in srgb, var(--surface-1) 72%, transparent),
      color-mix(in srgb, var(--surface-1) 34%, transparent) 45%,
      transparent 82%
    );
  }
</style>
