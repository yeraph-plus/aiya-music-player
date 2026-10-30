<script lang="ts">
  import { themeColor } from '../lib/accent';
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
   * 三形态（设置「频谱形态」切换；背景的形态由 `bgSpectrumStyle` 单独控制，
   * 两者互不影响）：
   * - `mirror` 镜像曲线：水平中线上一条平滑贝塞尔曲线随频谱呼吸，上下
   *   对称，描边发光 + 半透明透镜填充（T4h 用户拍板的首发形态）；
   * - `radial` 径向环形：频谱绕圆一周向外呼吸，慢速旋转，圆环结构感强；
   * - `ripple` 中轴波纹：**一条**居中曲线随频谱起伏、缓缓横移（画法见
   *   `drawRipple` 上方的注释）。上一版 `wave` 心电图已退役；同月地形版
   *   （22 层等深线铺满下半屏）也已从页内撤掉——那一套现在只做背景层，
   *   见 `lib/render/layers/ripple.ts`。
   *
   * 共同画法：48 带时间平滑（起音跟手、衰减绵长）→ 中点二次贝塞尔平滑 →
   * accent 色 shadowBlur 发光描边 + 低透明度填充（不抢歌词可读性）。
   * 全页 behind 内容（z-index 0，内容席位 z-index 1）。
   *
   * 数据源为 null（频谱设置关闭/引擎失败）时不渲染；reduced-motion 不渲染。
   *
   * 尺寸跟随（2026-10-30 修）：backing store 的宽/高**必须各自**跟 CSS 尺寸
   * × DPR 比较。旧代码只比了 `width`，于是窗口只在纵向变大（或分屏、最大化
   * 只改高度）时 canvas 的 `height` 属性留在旧值，浏览器把旧位图纵向拉伸 ——
   * 观感就是"图表不跟尺寸重排，糊了/错位了"，而背景层走的是 WebGL 渲染器
   * （有自己的 resize 回调），所以看起来正常。现在每帧按当前 DPR 重算两个
   * 维度，DPR 变化（拖到不同缩放比的显示器）也一并跟上。
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

  /**
   * 中轴波纹（ripple）。
   *
   * 2026-10-30 定版：页内**只画一条居中曲线**。上一版这里是"地形"——48 带当
   * 水面截面、沿透视深度叠 22 层等深线（画法见 `lib/render/layers/ripple.ts`）。
   * 那套搬进背景层之后，页内再画一遍就变成"同一屏两片水"：内容层被夹在中间，
   * 而且页内那块 canvas 只覆盖内容区（限宽 1180），水面到画布边缘就断，和背景
   * 那片铺满窗口的地形还不同源。所以页内降为一条线：背景负责场面，页内只留
   * 主视觉。
   *
   * 一条线要读得懂，靠三件事：
   * - **横向连续**：48 带按"带"为单位浮点采样（循环 + 线性插值）。直接按
   *   数组下标取会让线在带边界处折角，48 个点在 1180px 上就是 48 个硬拐点。
   * - **缓慢横移**：0.35 带/秒。线像水面一样缓缓淌；再快就成了"左右抖动的
   *   心电图"，那正是 `wave` 退役的原因。
   * - **振幅吃能量**：整体能量进 0.45–1.0 区间缩放振幅。安静段几乎是一条
   *   静水线，副歌才抬起浪——但最低留 0.45，否则静音时线彻底塌成直线，
   *   看着像画错了。
   *
   * 为什么不像镜像那样填"透镜"：镜像有上下两条曲线，靠间距读出音量；单线
   * 只有高度一个自由度，填充只会把中间那句歌词压成灰底。发声由 `shadowBlur`
   * 的发光描边承担，透明度与镜像的描边对齐，两种形态切换时亮度不跳。
   */
  function drawRipple(
    ctx: CanvasRenderingContext2D,
    vals: number[],
    width: number,
    height: number,
    accent: string,
    timeMs: number,
  ): void {
    const n = vals.length;
    const cols = 72;
    const cy = height / 2;
    const t = timeMs / 1000;

    // 整体能量（0–1）：与背景层同一口径（均值 0.32 视为满浪）
    let sum = 0;
    for (let i = 0; i < n; i += 1) sum += vals[i] as number;
    const energy = Math.min(1, sum / n / 0.32);

    /** 按"带"为单位的浮点位置采样（循环 + 线性插值），横移后仍连续 */
    const sample = (pos: number): number => {
      const p = ((pos % n) + n) % n;
      const i0 = Math.floor(p);
      const f = p - i0;
      const a = vals[i0] as number;
      const b = vals[(i0 + 1) % n] as number;
      return a + (b - a) * f;
    };

    // 振幅以**画布高度**为基准：宽度只影响采样密度，不该让线变陡。
    const amp = height * 0.115 * (0.45 + 0.55 * energy);
    const offset = t * 0.35;

    const pts: Array<[number, number]> = [];
    for (let c = 0; c <= cols; c += 1) {
      const xr = c / cols;
      pts.push([xr * width, cy - sample(xr * n + offset) * amp]);
    }

    // 左右各 7% 淡出：canvas 是整页宽（inset:0），线直接顶到边缘像被裁断，
    // 渐隐后读成"延伸到画面之外"
    const stroke = ctx.createLinearGradient(0, 0, width, 0);
    stroke.addColorStop(0, 'transparent');
    stroke.addColorStop(0.07, accent);
    stroke.addColorStop(0.93, accent);
    stroke.addColorStop(1, 'transparent');

    ctx.lineJoin = 'round';
    ctx.lineCap = 'round';
    ctx.lineWidth = 1.8;
    ctx.strokeStyle = stroke;
    ctx.shadowColor = accent;
    ctx.shadowBlur = 16;
    ctx.beginPath();
    smoothPath(ctx, pts);
    ctx.stroke();
    ctx.shadowBlur = 0;
  }

  /**
   * CSS 尺寸的缓存值。由 ResizeObserver 维护，绘制循环只读。
   *
   * 为什么不每帧读 `clientWidth`：那是布局查询，在 rAF 里逐帧读会把绘制
   * 循环和样式/布局绑在一起（页面别处一改样式就多一次强制布局）。RO 只在
   * 尺寸真的变了时回调一次，正好是我们要的时机。
   */
  let cssW = 0;
  let cssH = 0;

  function measure(target: HTMLCanvasElement): void {
    cssW = target.clientWidth;
    cssH = target.clientHeight;
  }

  function draw(): void {
    const target = canvas;
    if (!target || !source) return;
    const ctx = target.getContext('2d');
    if (!ctx) return;

    // 尺寸没量到（首帧 / 页面切换中）时继续排队，别在这一帧把 canvas 清零
    if (cssW === 0 || cssH === 0) {
      measure(target);
      if (cssW === 0 || cssH === 0) {
        frameId = requestAnimationFrame(draw);
        return;
      }
    }

    // 两个维度**各自**比：旧代码只比 width，窗口只在纵向变大时 height 属性
    // 不更新，浏览器把旧位图纵向拉伸（详见组件头部注释）
    const dpr = Math.min(2, window.devicePixelRatio || 1);
    const bw = Math.round(cssW * dpr);
    const bh = Math.round(cssH * dpr);
    if (target.width !== bw || target.height !== bh) {
      target.width = bw;
      target.height = bh;
    }
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, cssW, cssH);

    const bands = source.frame();
    const n = bands.length;
    if (!display || display.length !== n) display = new Float32Array(n);
    for (let i = 0; i < n; i += 1) {
      display[i] = display[i] + ((bands[i] as number) - display[i]) * 0.25;
    }

    // 主题色取缓存值：每帧 getComputedStyle 会强制一次样式解析（见 accent.ts）
    const accent = themeColor('--accent');
    const vals = Array.from(display, (v) => Math.max(0, v));
    const now = performance.now();

    if (style === 'radial') {
      // 环形直接用 48 带（绕圆一周，首尾相接天然对称，无需镜像）
      drawRadial(ctx, vals, cssW, cssH, accent, now);
    } else if (style === 'ripple') {
      // 中轴波纹：一条横贯页面中线的曲线（见 drawRipple）
      drawRipple(ctx, vals, cssW, cssH, accent, now);
    } else {
      // 镜像带序列：左半倒序 + 右半正序（中心带不重复）→ 水平居中对称
      const mirrored: number[] = [];
      for (let i = n - 1; i >= 0; i -= 1) mirrored.push(vals[i] as number);
      for (let i = 1; i < n; i += 1) mirrored.push(vals[i] as number);
      drawMirror(ctx, mirrored, cssW, cssH, accent);
    }

    frameId = requestAnimationFrame(draw);
  }

  // 订阅与绘制循环同生命周期；source 由父级按设置派生（null = 不画）。
  // style 故意不进依赖：形态切换只换画笔，不重启订阅（tap 是引用计数的全局开关）。
  $effect(() => {
    if (!source || reducedMotion) return;
    // canvas 是 $state（bind:this），首帧可能还没绑定——赋值后 effect 会重跑
    const target = canvas;
    if (!target) return;
    measure(target);
    // 尺寸跟随：侧栏塌陷、窗口缩放、最大化都走这里。RO 只改缓存值，
    // 真正的位图重分配在下一帧的 draw 里做（避免在回调里读改布局）
    const ro = new ResizeObserver(() => measure(target));
    ro.observe(target);
    source.start();
    frameId = requestAnimationFrame(draw);
    return () => {
      ro.disconnect();
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
    /*
     * 这里**不加 mask**（2026-10-30 用户拍板撤掉横向渐隐）。
     * 上一版为了让波形别横穿 30px 的歌词，在 34%–56% 处渐隐到透明——
     * 结果是右半边的频谱条整体被削没了，看着像"画了一半"。
     * 现在的做法是把对比度交给内容层：当前句底下垫一块渐隐黑椭圆
     * （见 pages.css 的 .np-lyrics），频谱保持全宽，谁也不切谁。
     */
  }
</style>
