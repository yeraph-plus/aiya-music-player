<script lang="ts">
  import { themeColor } from '../lib/accent';
  import { subscribeRender } from '../lib/render/scheduler';
  import { reducedMotion } from '../lib/motion.svelte';
  import type { SpectrumStyle } from '../lib/settings';
  import type { SpectrumSource } from '../lib/spectrum-source';

  interface Props {
    /** 频谱数据源（与背景层共享同一实例，start/stop 引用计数叠放） */
    source: SpectrumSource | null;
    /** 可视化形态（设置「频谱风格」）：切换即时生效，不重建数据订阅 */
    style: SpectrumStyle;
    /**
     * 是否暂停。暂停时本页频谱静止并收平，与背景层语义一致。
     */
    paused: boolean;
  }

  let { source, style, paused }: Props = $props();

  /**
   * 正在播放页的居中式可视化。
   *
   * 与背景层（AmbientBackdrop 山形）分工：背景承担氛围，本组件为主视觉——
   * 居中悬浮（舞台语义），不从底部伸出（后者属仪表语义）。
   *
   * 四形态，由设置中的「频谱形态」切换；背景形态由 `bgSpectrumStyle` 单独控制，
   * 两者互不影响：
   * - `mirror` 镜像：水平中线上一条平滑贝塞尔曲线随频谱呼吸，上下对称，
   *   描边发光加半透明透镜填充；
   * - `radial` 环带：频谱绕圆一周向外呼吸，慢速旋转，结构感强；
   * - `ripple` 残影：单条居中曲线 + 反馈式残影缓冲（画法见
   *   `drawRipple` 上方注释）。等深线地形形态仅用于背景层，见
   *   `lib/render/layers/ripple.ts`。
   * - `wave` 波纹：频谱曲线逐带交替成山与谷，振幅跨中线上下两段（画法见
   *   `drawWave` 上方注释）。
   *
   * 共同画法：48 带时间平滑（起音跟随、衰减绵长）→ 中点二次贝塞尔平滑 →
   * accent 色 shadowBlur 发光描边加低透明度填充（避免削弱歌词可读性）。
   * 全页位于内容之后（z-index 0，内容席位 z-index 1）。
   *
   * 数据源为 null（频谱设置关闭或引擎失败）时不渲染；reduced-motion 下不渲染。
   *
   * 尺寸跟随：backing store 的宽与高各自与 CSS 尺寸 × DPR 比较。仅比较 `width`
   * 时，窗口只纵向变化（最大化、分屏、拖动下沿）会使 canvas 的 `height` 属性
   * 停留在旧值，浏览器将旧位图纵向拉伸，观感为图表不随尺寸重排。背景层使用
   * WebGL 渲染器，具备独立的 resize 回调，因此不受影响。当前实现每帧按当前
   * DPR 重算两个维度，DPR 变化（拖至不同缩放比的显示器）同样跟随。
   */
  let canvas = $state<HTMLCanvasElement | null>(null);

  /** 时间平滑后的显示值（48 带） */
  let display: Float32Array | null = null;

  /**
   * 只在播放时推进的毫秒时钟（驱动波纹横移、环形慢转）。
   *
   * 不用 `performance.now()` 的原因是暂停语义：时间一直走的话，暂停画面里
   * 波纹还在漂、环形还在转——用户要的是"暂停即静止"。
   */
  let clock = 0;
  /** 收平系数：暂停时淡到 0，恢复时淡回 1（约 70ms 时间常数，不是硬切） */
  let gate = 1;
  /** 上一帧时间戳（秒），用来把"每帧 0.25"换算成与帧率无关的系数 */
  let lastAt = 0;

  /**
   * 波纹残影（反馈缓冲 + 帧步进节拍）：画布不清屏，残影由双缓冲乒乓维护。
   *
   * 主帧形状每 `RIPPLE_MAIN_EVERY` 秒步进一次（display 平滑与漂移时钟只在
   * 节拍帧推进）；每 `RIPPLE_ECHO_TICK_DIV` 个节拍把当前形状烙进缓冲成为
   * 残影（默认 2 = 残影节拍为主帧的一半），缓冲内容逐帧整体上飘、按时间
   * 常数衰减。衰减与上飘以真实 dt 连续推进，帧率无关。
   */
  const RIPPLE_MAIN_EVERY = 0.083;
  const RIPPLE_ECHO_TICK_DIV = 2;
  const RIPPLE_ECHO_TAU = 0.11;
  const RIPPLE_ECHO_RISE = 0.3;
  let echoRead: HTMLCanvasElement | null = null;
  let echoWrite: HTMLCanvasElement | null = null;
  let rippleAcc = 0;
  let rippleClock = 0;
  let rippleTickIndex = 0;
  let ripplePts: Array<[number, number]> | null = null;

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
    const amp = base * 0.15;
    const n = vals.length;

    // 慢速旋转（~0.05 rad/s）：环形有了"活着"的转向感，又不干扰读歌词
    const spin = (timeMs / 1000) * 0.05;
    // 环形映射把首尾频带直接相邻（低频贴着高频）：只有低音没有高音时，
    // band 0 与 band 47 的半径差会在衔接点形成一个尖角。把带值视为环状
    // 做两轮三点点平滑（[0.25, 0.5, 0.25]，环绕取样），衔接处的半径差
    // 在任何频带构成下都摊成跨数带的缓坡；代价是各带的峰值被轻微削低。
    let ring: Array<number> = Array.from(vals);
    for (let pass = 0; pass < 2; pass += 1) {
      const blurred: number[] = new Array(n);
      for (let i = 0; i < n; i += 1) {
        const prev = ring[(i - 1 + n) % n]!;
        const cur = ring[i]!;
        const next = ring[(i + 1) % n]!;
        blurred[i] = prev * 0.25 + cur * 0.5 + next * 0.25;
      }
      ring = blurred;
    }
    const pts: Array<[number, number]> = ring.map((v, i) => {
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
   * 页内只绘制一条居中曲线。等深线地形形态（48 带作为水面截面、沿透视深度叠
   * 22 层，画法见 `lib/render/layers/ripple.ts`）仅用于背景层：页内 canvas 只
   * 覆盖内容区（限宽 1180），水面在画布边缘截断，与铺满窗口的背景地形不同源，
   * 且内容层被夹在两层水面之间。页内因此只保留主视觉。
   *
   * 单线可读性依赖三点：
   * - 横向连续：48 带按带为单位做浮点采样（循环加线性插值）。按下标直接取值
   *   会使曲线在带边界折角，48 个点在 1180px 上形成 48 处硬拐点。
   * - 缓慢横移 0.35 带/秒，接近水面平移的速率；更快会呈现左右抖动，与已移除的
   *   心电图形态观感相近。
   * - 振幅随能量缩放，映射到 0.45–1.0 区间。安静段接近静态水面，强段抬升；
   *   下限保留 0.45，否则静音时曲线压为直线，读者会判为绘制异常。
   *
 * 不做透镜填充（镜像形态依赖上下两条曲线的间距表达音量）：单线只有高度一个
 * 自由度，填充会把中间的歌词压成灰底。发声感由 `shadowBlur` 发光描边承担，
 * 透明度与镜像描边对齐，两形态切换时亮度不跳变。
 *
 * 拖影：反馈式残影缓冲 + 帧步进节拍——主帧形状每 RIPPLE_MAIN_EVERY 秒
 * 步进一次，每 RIPPLE_ECHO_TICK_DIV 个节拍把当前形状烙进缓冲成为残影
 * （残影节拍为主帧的一半），缓冲内容逐帧整体上飘、按时间常数衰减。
 * 发光随残影一起烙进缓冲。暂停时主帧收平，存量残影按时间常数衰减
 * 殆尽后画面静止，与"暂停即静止"语义一致。
 */
  function drawRipple(
    ctx: CanvasRenderingContext2D,
    vals: number[],
    width: number,
    height: number,
    accent: string,
    timeMs: number,
    dt: number,
    isTick: boolean,
    bake: boolean,
  ): void {
    const n = vals.length;
    const cy = height / 2;

    // 左右各 7% 淡出：canvas 是整页宽（inset:0），线直接顶到边缘像被裁断，
    // 渐隐后读成"延伸到画面之外"
    const stroke = ctx.createLinearGradient(0, 0, width, 0);
    stroke.addColorStop(0, 'transparent');
    stroke.addColorStop(0.07, accent);
    stroke.addColorStop(0.93, accent);
    stroke.addColorStop(1, 'transparent');

    const drawLine = (g: CanvasRenderingContext2D, pts: Array<[number, number]>): void => {
      g.lineJoin = 'round';
      g.lineCap = 'round';
      g.lineWidth = 1.8;
      g.strokeStyle = stroke;
      g.shadowColor = accent;
      g.shadowBlur = 16;
      g.beginPath();
      smoothPath(g, pts);
      g.stroke();
      g.shadowBlur = 0;
    };

    // 缓冲演化：读缓冲整体上飘 + 衰减写入写缓冲（残影持续上飘渐隐）
    const cv = ctx.canvas;
    const dpr = cv.width / Math.max(1, width);
    if (!echoRead) echoRead = document.createElement('canvas');
    if (!echoWrite) echoWrite = document.createElement('canvas');
    for (const buf of [echoRead, echoWrite]) {
      if (buf.width !== cv.width || buf.height !== cv.height) {
        buf.width = cv.width;
        buf.height = cv.height;
      }
    }
    const wctx = echoWrite.getContext('2d');
    if (!wctx) return;
    wctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    wctx.clearRect(0, 0, width, height);
    wctx.globalAlpha = Math.exp(-dt / RIPPLE_ECHO_TAU);
    wctx.drawImage(
      echoRead,
      0,
      0,
      echoRead.width,
      echoRead.height,
      0,
      -height * RIPPLE_ECHO_RISE * dt,
      width,
      height,
    );
    wctx.globalAlpha = 1;

    // 节拍帧：主帧形状按当前带值重建；烙印帧（每 TICK_DIV 个节拍一次）
    // 把当前形状烙进缓冲成为残影
    if (isTick) {
      if (bake && ripplePts) drawLine(wctx, ripplePts);
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
      const amp = height * 0.13 * (0.45 + 0.55 * energy);
      const offset = t * 0.35;
      const cols = 72;
      const pts: Array<[number, number]> = [];
      for (let c = 0; c <= cols; c += 1) {
        const xr = c / cols;
        pts.push([xr * width, cy - sample(xr * n + offset) * amp]);
      }
      ripplePts = pts;
    }

    ctx.clearRect(0, 0, width, height);
    ctx.drawImage(echoWrite, 0, 0, echoWrite.width, echoWrite.height, 0, 0, width, height);
    if (ripplePts) drawLine(ctx, ripplePts);
    [echoRead, echoWrite] = [echoWrite, echoRead];
  }

  /**
   * 波纹（wave）。
   *
   * 频谱曲线本身占满中线上下两段：带值按序交替取向——偶数带向上成山、
   * 奇数带向下成谷，`cos(π·pos)` 在相邻带之间平滑过零，每个带的幅值完整
   * 呈现为山或谷（原单线只有山形，谷来自数据而非载波）。漂移沿用横移
   * 0.35 带/秒，方向随带序一起行进；能量档缩放整体振幅（均值 0.32 视为
   * 满浪，与残影同口径）。
   *
   * 画法与残影同语言：左右 7% 渐隐、发光描边、不填充（歌词可读性）。
   * 无残影缓冲：本形态每帧整帧重画，走与镜像/环带相同的清屏路径。
   */
  function drawWave(
    ctx: CanvasRenderingContext2D,
    vals: number[],
    width: number,
    height: number,
    accent: string,
    timeMs: number,
  ): void {
    const n = vals.length;
    const cy = height / 2;
    const t = timeMs / 1000;
    // 整体能量（0–1）：与残影同一口径（均值 0.32 视为满浪）
    let sum = 0;
    for (let i = 0; i < n; i += 1) sum += vals[i] as number;
    const energy = Math.min(1, sum / n / 0.32);
    /** 按"带"为单位的浮点位置采样（循环 + 线性插值），横向无带边界折角 */
    const sample = (pos: number): number => {
      const p = ((pos % n) + n) % n;
      const i0 = Math.floor(p);
      const f = p - i0;
      const a = vals[i0] as number;
      const b = vals[(i0 + 1) % n] as number;
      return a + (b - a) * f;
    };

    const amp = height * 0.13 * (0.45 + 0.55 * energy);
    const offset = t * 0.35;
    const cols = 96;
    const pts: Array<[number, number]> = [];
    for (let c = 0; c <= cols; c += 1) {
      const xr = c / cols;
      const pos = xr * n + offset;
      pts.push([xr * width, cy - sample(pos) * Math.cos(Math.PI * pos) * amp]);
    }

    // 左右各 7% 淡出：与残影同语言
    const strokeGrad = ctx.createLinearGradient(0, 0, width, 0);
    strokeGrad.addColorStop(0, 'transparent');
    strokeGrad.addColorStop(0.07, accent);
    strokeGrad.addColorStop(0.93, accent);
    strokeGrad.addColorStop(1, 'transparent');

    ctx.lineJoin = 'round';
    ctx.lineCap = 'round';
    ctx.lineWidth = 1.8;
    ctx.strokeStyle = strokeGrad;
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

  /** 由调度器每帧调用一次（`now` 是 rAF 时间戳，毫秒） */
  function draw(now: number): void {
    const target = canvas;
    if (!target || !source) return;
    const ctx = target.getContext('2d');
    if (!ctx) return;

    // 尺寸未测得（首帧或页面切换中）：本帧跳过，不清零 canvas，下一帧继续
    if (cssW === 0 || cssH === 0) {
      measure(target);
      if (cssW === 0 || cssH === 0) return;
    }

    // 与帧率无关的推进：dt 由调度器提供的时间戳计算，高刷屏不会推进过快，
    // 30fps 下也不产生拖尾（逐帧常数系数在两者间无法兼顾）
    const dt = lastAt === 0 ? 1 / 60 : Math.min(0.1, (now - lastAt) / 1000);
    lastAt = now;
    if (!paused) clock += dt * 1000;
    const targetGate = paused ? 0 : 1;
    gate += (targetGate - gate) * (1 - Math.exp(-dt / 0.07));
    if (Math.abs(gate - targetGate) < 0.002) gate = targetGate;

    // 两个维度各自比较：仅比较 width 时，窗口只纵向变化会使 height 属性不更新，
    // 浏览器将旧位图纵向拉伸（详见组件头部注释）
    const dpr = Math.min(2, window.devicePixelRatio || 1);
    const bw = Math.round(cssW * dpr);
    const bh = Math.round(cssH * dpr);
    if (target.width !== bw || target.height !== bh) {
      target.width = bw;
      target.height = bh;
    }
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    // 波纹的残影是画布自身的持久化（反馈缓冲），不能整帧清屏；其余形态每帧重画
    if (style !== 'ripple') ctx.clearRect(0, 0, cssW, cssH);

    const bands = source.frame();
    const n = bands.length;
    if (!display || display.length !== n) display = new Float32Array(n);

    // 波纹按主帧节拍步进（帧步进美学）：display 平滑与漂移时钟只在节拍帧
    // 推进，两拍之间主帧形状保持静止；其余形态逐帧平滑。
    // 平滑：0.25/帧 @60fps 换算成 dt 形式，帧率变了手感不变。
    // 暂停时目标值取 0——不是"停在上一帧"，而是收平（恢复播放从 0 长起来）
    let stepDt = dt;
    let tick = false;
    let bake = false;
    if (style === 'ripple') {
      rippleAcc += dt;
      if (rippleAcc >= RIPPLE_MAIN_EVERY) {
        tick = true;
        stepDt = rippleAcc;
        rippleAcc = 0;
        if (!paused) rippleClock += stepDt * 1000;
        rippleTickIndex += 1;
        bake = rippleTickIndex % RIPPLE_ECHO_TICK_DIV === 0;
      } else {
        stepDt = 0;
      }
    }
    if (stepDt > 0) {
      const smooth = 1 - Math.pow(1 - 0.25, stepDt * 60);
      for (let i = 0; i < n; i += 1) {
        const v = paused ? 0 : (bands[i] as number);
        display[i] = display[i] + (v - display[i]) * smooth;
      }
    }

    // 主题色取缓存值：每帧 getComputedStyle 会强制一次样式解析（见 accent.ts）
    const accent = themeColor('--accent');
    // gate 再压一道：暂停时形状要"拉平"，而不是留一条凝固的曲线
    const vals = Array.from(display, (v) => Math.max(0, v) * gate);

    if (style === 'radial') {
      // 环形直接用 48 带（绕圆一周，首尾相接天然对称，无需镜像）
      drawRadial(ctx, vals, cssW, cssH, accent, clock);
    } else if (style === 'ripple') {
      // 中轴波纹：一条横贯页面中线的曲线 + 反馈式残影（见 drawRipple）；
      // 主帧形状按 RIPPLE_MAIN_EVERY 节拍步进，残影每 TICK_DIV 个节拍烙一帧
      drawRipple(ctx, vals, cssW, cssH, accent, rippleClock, dt, tick, bake);
    } else if (style === 'wave') {
      // 波纹：频谱曲线逐带交替成山与谷，振幅跨中线上下两段（见 drawWave）
      drawWave(ctx, vals, cssW, cssH, accent, clock);
    } else {
      // 镜像带序列：左半倒序 + 右半正序（中心带不重复）→ 水平居中对称
      const mirrored: number[] = [];
      for (let i = n - 1; i >= 0; i -= 1) mirrored.push(vals[i] as number);
      for (let i = 1; i < n; i += 1) mirrored.push(vals[i] as number);
      drawMirror(ctx, mirrored, cssW, cssH, accent);
    }
  }

  // 绘制挂全应用单例调度器（条：全应用一个 rAF）。
  // 各自维护 rAF 使背景层与页内形成两个独立循环，统一到调度器后，后台与隐藏
  // 状态下的暂停也只在一处判断。
  // style / paused 刻意不进入依赖：形态或播放态变化只影响画笔，不重启订阅
  // （tap 为引用计数的全局开关，重新订阅会使频谱中断一帧）。
  $effect(() => {
    if (!source || reducedMotion.value) return;
    // canvas 是 $state（bind:this），首帧可能还没绑定——赋值后 effect 会重跑
    const target = canvas;
    if (!target) return;
    measure(target);
    // 尺寸跟随：侧栏塌陷、窗口缩放、最大化都走这里。RO 只改缓存值，
    // 真正的位图重分配在下一帧的 draw 里做（避免在回调里读改布局）
    const ro = new ResizeObserver(() => measure(target));
    ro.observe(target);
    source.start();
    const stop = subscribeRender(draw);
    return () => {
      ro.disconnect();
      stop();
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
    z-index: var(--z-bg);
    width: 100%;
    height: 100%;
    pointer-events: none;
    /* 不使用 mask。曾为避免波形横穿 30px 高的歌词行，在 34%–56%
       区间渐隐到透明，结果右半边频谱整体被削弱，观感为绘制不完整。当前将对比度交给内容层：当前歌词行下方垫一块渐隐黑椭圆（见 pages.css 的 .np-lyrics），频谱保持全宽，两者互不裁切。 */
  }
</style>
