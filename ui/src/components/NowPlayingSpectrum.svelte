<script lang="ts">
  import type { SpectrumSource } from '../lib/spectrum-source';

  interface Props {
    /** 频谱数据源（与背景层共享同一实例，start/stop 引用计数叠放） */
    source: SpectrumSource | null;
  }

  let { source }: Props = $props();

  /**
   * 正在播放页·居中式镜像对称曲线（docs/26 §3 方案 A，T4h 用户拍板）。
   *
   * 与背景层（AmbientBackdrop 山形）分工：背景是"氛围"，这里是**主视觉**——
   * 水平中线上一条平滑贝塞尔曲线随频谱呼吸，上下镜像对称，描边发光 +
   * 半透明填充。不从底部伸出（那是仪表语义），居中悬浮（舞台语义）。
   *
   * 画法：
   * - 48 带时间平滑（起音跟手、衰减绵长）→ 镜像成 2n-1 个点（左半倒序
   *   + 右半正序，水平居中对称）→ 中点二次贝塞尔连成平滑曲线；
   * - 上曲线 = cy − v·A，下曲线镜像；两曲线之间填垂直渐变（边缘浓、
   *   中线透），描边 accent + shadowBlur 发光；
   * - 全页 behind 内容（z-index 0，内容席位 z-index 1），低透明度不抢
   *   歌词可读性。
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

    // 镜像带序列：左半倒序 + 右半正序（中心带不重复）→ 水平居中对称
    const vals: number[] = [];
    for (let i = n - 1; i >= 0; i -= 1) vals.push(display[i] as number);
    for (let i = 1; i < n; i += 1) vals.push(display[i] as number);

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

    frameId = requestAnimationFrame(draw);
  }

  // 订阅与绘制循环同生命周期；source 由父级按设置派生（null = 不画）
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
