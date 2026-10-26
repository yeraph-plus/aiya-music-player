<script lang="ts">
  import type { SpectrumStyle } from '../lib/settings';
  import type { SpectrumSource } from '../lib/spectrum-source';

  interface Props {
    /** 频谱数据源（与背景层共享同一实例，start/stop 引用计数叠放） */
    source: SpectrumSource | null;
    /** 可视化形态（设置「频谱风格」，docs/26 §3）：切换即时生效，不重建数据订阅 */
    style: SpectrumStyle;
  }

  let { source, style }: Props = $props();

  /**
   * 正在播放页·居中式可视化（docs/26 §3）。
   *
   * 与背景层（AmbientBackdrop 山形）分工：背景是"氛围"，这里是**主视觉**——
   * 居中悬浮（舞台语义），不从底部伸出（那是仪表语义）。
   *
   * 三形态（设置「频谱风格」切换，背景环境光固定山形不受影响）：
   * - `mirror` 镜像曲线：水平中线上一条平滑贝塞尔曲线随频谱呼吸，上下
   *   对称，描边发光 + 半透明透镜填充（T4h 用户拍板的首发形态）；
   * - `radial` 径向环形：频谱绕圆一周向外呼吸，慢速旋转，圆环结构感强；
   * - `wave` 示波线：单条连续曲线沿频带起伏，技术感、最克制。
   *
   * 共同画法：48 带时间平滑（起音跟手、衰减绵长）→ 中点二次贝塞尔平滑 →
   * accent 色 shadowBlur 发光描边 + 低透明度填充（不抢歌词可读性）。
   * 全页 behind 内容（z-index 0，内容席位 z-index 1）。
   *
   * 数据源为 null（频谱设置关闭/引擎失败）时不渲染；reduced-motion 不渲染。
   */
  let canvas = $state<HTMLCanvasElement | null>(null);
  let frameId = 0;

  const reducedMotion =
    typeof matchMedia !== 'undefined' && matchMedia('(prefers-reduced-motion: reduce)').matches;

  /** 时间平滑后的显示值（48 带） */
  let display: Float32Array | null = null;

  /** 中点二次贝塞尔画一条经过 pts（[x,y] 数组）的开放曲线 */
  function smoothPath(ctx: CanvasRenderingContext2D, pts: Array<[number, number]>): void {
    const n = pts.length;
    ctx.moveTo(pts[0]![0], pts[0]![1]);
    for (let i = 1; i < n - 1; i += 1) {
      const midX = (pts[i]![0] + pts[i + 1]![0]) / 2;
      const midY = (pts[i]![1] + pts[i + 1]![1]) / 2;
      ctx.quadraticCurveTo(pts[i]![0], pts[i]![1], midX, midY);
    }
    ctx.quadraticCurveTo(pts[n - 1]![0], pts[n - 1]![1], pts[n - 1]![0], pts[n - 1]![1]);
  }

  /** 中点二次贝塞尔画一条闭合平滑曲线（首尾以中点衔接，无尖角接缝） */
  function smoothClosed(ctx: CanvasRenderingContext2D, pts: Array<[number, number]>): void {
    const n = pts.length;
    const mid = (a: [number, number], b: [number, number]): [number, number] => [
      (a[0] + b[0]) / 2,
      (a[1] + b[1]) / 2,
    ];
    const start = mid(pts[n - 1]!, pts[0]!);
    ctx.moveTo(start[0], start[1]);
    for (let i = 0; i < n; i += 1) {
      const cur = pts[i]!;
      const next = pts[(i + 1) % n]!;
      const m = mid(cur, next);
      ctx.quadraticCurveTo(cur[0], cur[1], m[0], m[1]);
    }
    ctx.closePath();
  }

  function drawMirror(
    ctx: CanvasRenderingContext2D,
    vals: number[],
    width: number,
    height: number,
    accent: string,
  ): void {
    const cy = height / 2;
    const amp = height * 0.16;
    const px = (i: number): number => (i / (vals.length - 1)) * width;
    const top: Array<[number, number]> = vals.map((v, i) => [px(i), cy - v * amp]);
    const bottom: Array<[number, number]> = vals.map((v, i) => [px(i), cy + v * amp]);

    // 两曲线之间的"透镜"区：垂直渐变（上下边缘浓、中线透），低透明度不抢正文
    const fill = ctx.createLinearGradient(0, cy - amp, 0, cy + amp);
    fill.addColorStop(0, accent);
    fill.addColorStop(0.5, 'transparent');
    fill.addColorStop(1, accent);
    ctx.globalAlpha = 0.16;
    ctx.fillStyle = fill;
    ctx.beginPath();
    smoothPath(ctx, top);
    // 沿下曲线反向（右 → 左）画回起点，闭合成"透镜"区
    const last = bottom.length - 1;
    ctx.lineTo(bottom[last]![0], bottom[last]![1]);
    for (let i = last; i >= 1; i -= 1) {
      const cur = bottom[i]!;
      const prev = bottom[i - 1]!;
      const midX = (cur[0] + prev[0]) / 2;
      const midY = (cur[1] + prev[1]) / 2;
      ctx.quadraticCurveTo(cur[0], cur[1], midX, midY);
    }
    ctx.lineTo(bottom[0]![0], bottom[0]![1]);
    ctx.closePath();
    ctx.fill();
    ctx.globalAlpha = 1;

    // 发光描边：上/下曲线各一条
    ctx.shadowColor = accent;
    ctx.shadowBlur = 16;
    ctx.lineWidth = 1.8;
    ctx.lineJoin = 'round';
    ctx.strokeStyle = accent;
    ctx.beginPath();
    smoothPath(ctx, top);
    ctx.stroke();
    ctx.beginPath();
    smoothPath(ctx, bottom);
    ctx.stroke();
    ctx.shadowBlur = 0;
  }

  function drawRadial(
    ctx: CanvasRenderingContext2D,
    vals: number[],
    width: number,
    height: number,
    accent: string,
    timeMs: number,
  ): void {
    const cx = width / 2;
    const cy = height / 2;
    const base = Math.min(width, height);
    const r0 = base * 0.2;
    const amp = base * 0.13;
    const n = vals.length;

    // 慢速旋转（~0.05 rad/s）：环形有了"活着"的转向感，又不干扰读歌词
    const spin = (timeMs / 1000) * 0.05;
    const pts: Array<[number, number]> = vals.map((v, i) => {
      const theta = spin + (i / n) * Math.PI * 2 - Math.PI / 2;
      const r = r0 + v * amp;
      return [cx + Math.cos(theta) * r, cy + Math.sin(theta) * r];
    });

    // 环带填充：内圈透明 → 峰值半径处 accent，整个圆盘径向渐变一次画完
    const fill = ctx.createRadialGradient(cx, cy, r0 * 0.92, cx, cy, r0 + amp);
    fill.addColorStop(0, 'transparent');
    fill.addColorStop(0.6, 'transparent');
    fill.addColorStop(1, accent);
    ctx.globalAlpha = 0.14;
    ctx.fillStyle = fill;
    ctx.beginPath();
    smoothClosed(ctx, pts);
    ctx.fill();
    ctx.globalAlpha = 1;

    // 内圈基线：细描边给环形一个"结构参照"，频谱静止时也不是空洞
    ctx.globalAlpha = 0.18;
    ctx.lineWidth = 1;
    ctx.strokeStyle = accent;
    ctx.beginPath();
    ctx.arc(cx, cy, r0 - 3, 0, Math.PI * 2);
    ctx.stroke();
    ctx.globalAlpha = 1;

    // 峰值环发光描边
    ctx.shadowColor = accent;
    ctx.shadowBlur = 16;
    ctx.lineWidth = 1.8;
    ctx.lineJoin = 'round';
    ctx.strokeStyle = accent;
    ctx.beginPath();
    smoothClosed(ctx, pts);
    ctx.stroke();
    ctx.shadowBlur = 0;
  }

  function drawWave(
    ctx: CanvasRenderingContext2D,
    vals: number[],
    width: number,
    height: number,
    accent: string,
  ): void {
    const cy = height / 2;
    const amp = height * 0.2;
    const n = vals.length;
    const px = (i: number): number => (i / (n - 1)) * width;
    const line: Array<[number, number]> = vals.map((v, i) => [px(i), cy - v * amp]);

    // 曲线下方的"声面"填充：曲线浓、向下淡出，给出单线的体积感
    const fill = ctx.createLinearGradient(0, cy - amp, 0, cy + amp * 0.4);
    fill.addColorStop(0, accent);
    fill.addColorStop(1, 'transparent');
    ctx.globalAlpha = 0.12;
    ctx.fillStyle = fill;
    ctx.beginPath();
    smoothPath(ctx, line);
    ctx.lineTo(width, cy + amp * 0.4);
    ctx.lineTo(0, cy + amp * 0.4);
    ctx.closePath();
    ctx.fill();
    ctx.globalAlpha = 1;

    // 中线基线（极淡）+ 发光主曲线
    ctx.globalAlpha = 0.12;
    ctx.lineWidth = 1;
    ctx.strokeStyle = accent;
    ctx.beginPath();
    ctx.moveTo(0, cy);
    ctx.lineTo(width, cy);
    ctx.stroke();
    ctx.globalAlpha = 1;

    ctx.shadowColor = accent;
    ctx.shadowBlur = 14;
    ctx.lineWidth = 1.8;
    ctx.lineJoin = 'round';
    ctx.strokeStyle = accent;
    ctx.beginPath();
    smoothPath(ctx, line);
    ctx.stroke();
    ctx.shadowBlur = 0;
  }

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

    const bands = source.frame();
    const n = bands.length;
    if (!display || display.length !== n) display = new Float32Array(n);
    for (let i = 0; i < n; i += 1) {
      display[i] = display[i] + ((bands[i] as number) - display[i]) * 0.25;
    }

    const styles = getComputedStyle(target);
    const accent = styles.getPropertyValue('--accent').trim() || '#7c9cff';

    if (style === 'radial') {
      // 环形直接用 48 带（绕圆一周，首尾相接天然对称，无需镜像）
      drawRadial(ctx, Array.from(display, (v) => Math.max(0, v)), width, height, accent, performance.now());
    } else if (style === 'wave') {
      drawWave(ctx, Array.from(display, (v) => Math.max(0, v)), width, height, accent);
    } else {
      // 镜像带序列：左半倒序 + 右半正序（中心带不重复）→ 水平居中对称
      const vals: number[] = [];
      for (let i = n - 1; i >= 0; i -= 1) vals.push(Math.max(0, display[i] as number));
      for (let i = 1; i < n; i += 1) vals.push(Math.max(0, display[i] as number));
      drawMirror(ctx, vals, width, height, accent);
    }

    frameId = requestAnimationFrame(draw);
  }

  // 订阅与绘制循环同生命周期；source 由父级按设置派生（null = 不画）。
  // style 故意不进依赖：形态切换只换画笔，不重启订阅（tap 是引用计数的全局开关）。
  $effect(() => {
    if (!source || reducedMotion) return;
    source.start();
    frameId = requestAnimationFrame(draw);
    return () => {
      cancelAnimationFrame(frameId);
      source.stop();
    };
  });
</script>

{#if source}
  <canvas bind:this={canvas} class="np-spectrum" aria-hidden="true"></canvas>
{/if}

<style>
  .np-spectrum {
    position: absolute;
    /* 整页居中：内容席位（.np-info/.np-lyrics）是 z-index 1，本层垫在其后 */
    inset: 0;
    z-index: 0;
    width: 100%;
    height: 100%;
    pointer-events: none;
  }
</style>
