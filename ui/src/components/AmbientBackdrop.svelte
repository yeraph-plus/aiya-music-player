<script lang="ts">
  import { themeColor } from '../lib/accent';
  import { detectCapability } from '../lib/render/capability';
  import { createGlStage, type GlStage } from '../lib/render/context';
  import { FlowLayer } from '../lib/render/layers/flow';
  import { RidgeLayer } from '../lib/render/layers/ridge';
  import { RippleLayer } from '../lib/render/layers/ripple';
  import { subscribeRender } from '../lib/render/scheduler';
  import type { BgSpectrumStyle, RidgeGlow } from '../lib/settings';
  import type { SpectrumSource } from '../lib/spectrum-source';

  interface Props {
    /** 频谱数据源（48 带 tap）；null 或 enabled=false 时环境层不渲染 */
    source: SpectrumSource | null;
    /** 背景动态频谱开关（`settings.bgSpectrum` 且全局 3D 闸门开着才为 true） */
    enabled: boolean;
    /** 背景频谱形态（山形 / 波纹，见 `BgSpectrumStyle`） */
    shape: BgSpectrumStyle;
    /** 山形光晕档位（docs/46 §3.2；只对 `shape='ridge'` 有效） */
    glow: RidgeGlow;
  }

  let { source, enabled, shape, glow }: Props = $props();

  /**
   * 全应用背景（docs/25 §5 T4c app 化；T4h 起背景频谱改山形曲线）。
   *
   * **单一档位**（用户拍板，撤销 standard/immersive 双档）：原本列表页
   * 走"重遮罩保可读性"（veil 压 surface-1 渐变、氛围 0.6、频谱更虚），
   * 正在播放页走沉浸档（无遮罩、氛围满档、频谱 alpha 0.75/blur 14）。
   * 用户反馈沉浸档的透明度观感更好 —— 现在所有页面统一走这一档，
   * 可读性遮罩（第 3 层）整个撤销。
   *
   * **正在播放页不再叠背景**（docs/46 §5.2，2026-10-30 用户拍板）：那一页侧栏
   * 整体退出、主区铺满，于是本层天然铺满整个窗口 —— 全应用只有这一套背景，
   * 不为任何单页再画第二层。本组件也因此不需要知道当前在哪个路由。
   *
   * 三层（第 4 层是界面布局本身，在四区席位里）：
   * - 第 1 层 背景：固定 CSS 渐变（`--ambient-bg`，tokens.css 按
   *   `html[data-ambient]` 定义，设置页成套切换）。**不跟随封面**。
   * - 第 2 层 流动渐变（docs/46 §3.1，P1）：WebGL 上的一层缓慢流动的彩色
   *   光雾，叠在第 1 层之上**加色**。它只加动态、不改配色资产——`--ambient-bg`
   *   仍是配色唯一来源，颜色从 `--accent` 派生。硬件 GL 不可用时整层不建。
   * - 第 3 层 频谱环境光：形态由设置 `bgSpectrumStyle` 决定（2026-10-30 起
   *   可分形态）。`ridge` 山形曲线（docs/26 调研）：48 带先做时间平滑，再
   *   重采样成平滑山脊线，闭合到底边填充渐变——远山一层（低幅低透明）+
   *   近山一层（主形）；`ripple` 波纹地形：同一份曲线当作水面截面，沿视线
   *   一层层画等深线，铺满下半屏（画法见 `layers/ripple.ts`）。
   *
   * **两种形态互斥**：它们争的是同一块屏幕、同一种数据，同时画会既看不清
   * 山也看不清水。切换时整层重建（几何结构不同，不是改个 uniform 能变的）。
   *
   * **两条光栅路径并存**（档位见 `lib/render/capability.ts`）：
   * - 档 2（硬件 GL）：两种形态都画在**同一个** GL canvas 上，图层按
   *   `add()` 顺序同帧叠放（`layers/ridge.ts` / `layers/ripple.ts` + `flow.ts`）。
   *   光晕都是解析的（到脊线/线心的像素距离取指数衰减），没有离屏、没有模糊核。
   * - 档 1（软件光栅 / GL 不可用）：只有山形，退回原来的 Canvas 2D 实现
   *   （160×90 buffer + `ctx.filter blur(14px)` 放大）。波纹**不**做 2D 兜底：
   *   那是 22 层全窗口描边 + shadowBlur，在软件光栅上正是最不该做的事；
   *   这类机器退回山形即可，它是唯一验证过的低级路径。
   *
   * 绘制循环挂全应用单例调度器（`lib/render/scheduler`，docs/46 §7）：
   * 这里**不再自己排 rAF**，由调度器每帧调用一次 `draw`。理由是 docs/26
   * 的真实事故——组件内多次 `requestAnimationFrame` 会让调度数按 2^n 增长，
   * 几秒内主线程被淹没、UI 假死。多图层时代这条必须从结构上杜绝。
   *
   * `prefers-reduced-motion: reduce` 时环境层整体不渲染（静态背景保留）。
   */
  let canvas = $state<HTMLCanvasElement | null>(null);

  const reducedMotion =
    typeof matchMedia !== 'undefined' && matchMedia('(prefers-reduced-motion: reduce)').matches;

  /** tap 是否该开着（数据源 + 开关 + 非 reduced-motion） */
  const tapOn = $derived(Boolean(source) && enabled && !reducedMotion);

  /**
   * GL 舞台是否该建。
   *
   * 档位判在**渲染 canvas 元素**这一层，而不是只在 `createGlStage` 里判：
   * 后者返回 null 时 `bind:this` 已经把画布渲染出来了，会留下一个全屏透明
   * canvas 白占一层合成（软件光栅实测就是这样：档 1、但画布还在）。
   *
   * 只看 `enabled`（已含 `glEffects` 闸门）不要求 `source`：流动渐变不需要
   * 频谱数据，tap 失败时背景仍该是活的。
   */
  const glActive = $derived(enabled && !reducedMotion && detectCapability().tier >= 2);

  /** 频谱层走 GL（需要数据源）；否则山形走 2D 兜底 */
  const specGl = $derived(glActive && Boolean(source));
  const ridge2d = $derived(tapOn && !glActive);

  let glCanvas = $state<HTMLCanvasElement | null>(null);
  let stage = $state<GlStage | null>(null);
  let ridgeLayer = $state<RidgeLayer | null>(null);
  let rippleLayer = $state<RippleLayer | null>(null);

  $effect(() => {
    const target = glCanvas;
    if (!target || !glActive) return;

    const created = createGlStage(target);
    if (!created) return;

    // 图层顺序 = 叠放顺序：流动渐变在下，山形在上
    created.add(new FlowLayer());
    stage = created;
    const stopTicking = subscribeRender((now) => created.render(now));
    return () => {
      stage = null;
      stopTicking();
      created.dispose();
    };
  });

  // 频谱图层随"数据源 + 形态"挂载/卸载。
  //
  // 形态进依赖 ⇒ 切换时整层重建：山形是闭合填充、波纹是线阵，几何结构不同，
  // 没法像光晕档位那样只改 uniform。重建发生在同一帧内（旧层 dispose 后新层
  // `add`），舞台不重建，所以不会有黑帧。
  //
  // 光晕档位**不进依赖**：它只对山形有效，改档时走下面单独的 effect 调
  // `setGlow`，避免"调一下光晕把整层重建一次"。
  $effect(() => {
    const current = stage;
    const src = specGl ? source : null;
    const want = shape;
    if (!current || !src) {
      ridgeLayer = null;
      rippleLayer = null;
      return;
    }
    if (want === 'ripple') {
      const layer = new RippleLayer(src);
      current.add(layer);
      rippleLayer = layer;
      return () => {
        rippleLayer = null;
        current.remove(layer);
        layer.dispose();
      };
    }
    const layer = new RidgeLayer(src);
    current.add(layer);
    ridgeLayer = layer;
    return () => {
      ridgeLayer = null;
      current.remove(layer);
      layer.dispose();
    };
  });

  $effect(() => {
    // 读 `glow` 建立依赖；图层可能还没建（首帧/档 1），null 直接跳过
    ridgeLayer?.setGlow(glow);
  });

  // 数据源订阅与绘制订阅同生命周期：有源就 start（开引擎 tap），无源 stop。
  $effect(() => {
    if (!tapOn) return;
    source?.start();
    return () => {
      source?.stop();
    };
  });

  /** 低分辨率离屏 buffer（2D 兜底路径专用）：山形画在这里，主画布只做放大绘制 */
  let off: HTMLCanvasElement | null = null;

  /** 时间平滑后的显示值（48 带）：起音快、衰减慢，山才不会跟着帧抖 */
  let display: Float32Array | null = null;

  /**
   * 在 buffer 上画一条山脊线并填充到底边（2D 兜底路径）。
   * ys 是已换算好的 y 像素序列；曲线用「中点二次贝塞尔」——控制点取
   * 数据点、锚点取相邻中点，得到 C1 连续的平滑山脊，没有折线尖角。
   */
  function ridge(octx: CanvasRenderingContext2D, ys: number[], BW: number, BH: number): void {
    const n = ys.length;
    const px = (i: number): number => (i / (n - 1)) * BW;
    octx.beginPath();
    octx.moveTo(0, BH);
    octx.lineTo(px(0), ys[0] as number);
    for (let i = 1; i < n - 1; i += 1) {
      const midX = (px(i) + px(i + 1)) / 2;
      const midY = ((ys[i] as number) + (ys[i + 1] as number)) / 2;
      octx.quadraticCurveTo(px(i), ys[i] as number, midX, midY);
    }
    octx.quadraticCurveTo(px(n - 1), ys[n - 1] as number, px(n - 1), ys[n - 1] as number);
    octx.lineTo(BW, BH);
    octx.closePath();
    octx.fill();
  }

  /** 由调度器每帧调用一次（签名收 `now`，本层用不到） */
  function draw(): void {
    const target = canvas;
    if (!target || !source) return;
    const ctx = target.getContext('2d');
    if (!ctx) return;

    const dpr = Math.min(2, window.devicePixelRatio || 1);
    const width = target.clientWidth;
    const height = target.clientHeight;
    // 尺寸为 0（布局尚未结算/页面切走）：本帧跳过，调度器下一帧还会来
    if (width === 0 || height === 0) return;
    if (target.width !== Math.round(width * dpr)) {
      target.width = Math.round(width * dpr);
      target.height = Math.round(height * dpr);
    }
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, width, height);

    const bands = source.frame();
    // 主题色取缓存值：每帧 getComputedStyle 会强制一次样式解析（见 accent.ts）
    const barColor = themeColor('--accent');

    // 低分辨率 buffer：模糊代价 ∝ 像素数，这里省一个数量级
    const BW = 160;
    const BH = 90;
    if (!off) off = document.createElement('canvas');
    if (off.width !== BW || off.height !== BH) {
      off.width = BW;
      off.height = BH;
    }
    const octx = off.getContext('2d');
    if (!octx) return;
    octx.clearRect(0, 0, BW, BH);

    // 时间平滑：display 每帧向目标值靠拢 30%（~60fps 下约 90ms 常数），
    // 起音跟手、衰减绵长，山形不会随帧闪烁
    const n = bands.length;
    if (!display || display.length !== n) display = new Float32Array(n);
    const eased: number[] = new Array(n);
    for (let i = 0; i < n; i += 1) {
      // RemoteSpectrumSource.frame() 已经归一化到 0–1（引擎 0–255 → /255），
      // 这里**不能**再除 255——T4h 曾二次归一化把带值压到 ~0.4%，山成贴底平线
      const v = Math.min(1, Math.max(0, bands[i] as number));
      display[i] = display[i] + (v - display[i]) * 0.3;
      // 轻微伽马压低持续偏热的低频带，山形重心回到中频
      eased[i] = Math.pow(display[i], 1.2);
    }

    // 填充渐变：山峰处最浓、向底边淡出——山是"光"不是实心块
    const grad = octx.createLinearGradient(0, 0, 0, BH);
    grad.addColorStop(0, barColor);
    grad.addColorStop(1, 'transparent');
    octx.fillStyle = grad;

    // 带值 → y 像素：近山全幅 0.82（顶部留白给渐变呼吸），
    // 远山取倒序带 + 0.5 倍振幅——两层错开有纵深
    const yOf = (v: number, amp: number): number => BH - Math.max(1, v * amp * BH);
    const near: number[] = eased.map((v) => yOf(v, 0.82));
    const far: number[] = eased.map((_, i) => yOf(eased[n - 1 - i] as number, 0.5));

    // 远山（低幅低透明）→ 近山（主形）
    octx.globalAlpha = 0.35;
    ridge(octx, far, BW, BH);
    octx.globalAlpha = 0.7;
    ridge(octx, near, BW, BH);
    octx.globalAlpha = 1;

    // 单一档位（原沉浸档参数，全页面统一）：blur 收窄保留山的可辨识度，
    // 透明度拉高——背景是主视觉氛围，不再区分"氛围页/内容页"
    ctx.filter = 'blur(14px)';
    ctx.globalAlpha = 0.75;
    ctx.drawImage(off, -width * 0.04, -height * 0.12, width * 1.08, height * 1.12);
    ctx.filter = 'none';
    ctx.globalAlpha = 1;
  }

  // 2D 兜底路径的绘制订阅：GL 路径下这个 tick 不该存在（否则每帧白跑一次）
  $effect(() => {
    if (!ridge2d) return;
    return subscribeRender(draw);
  });
</script>

<div class="ambient-bg">
  <div class="ambient-art"></div>
  {#if glActive}
    <!-- 第 2、3 层共用这一个画布：流动渐变先画，山形后画（叠在上面） -->
    <canvas bind:this={glCanvas} class="ambient-flow"></canvas>
  {/if}
  {#if ridge2d}
    <canvas bind:this={canvas} class="ambient-spectrum"></canvas>
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
   * 单一档位（用户拍板）：满档不衰减——原列表页的 0.6 随双档一起撤销。 */
  .ambient-art {
    position: absolute;
    inset: 0;
    background: var(--ambient-bg);
    opacity: 1;
  }

  /* 第 2、3 层：GL 画布。本身不参与布局，尺寸由样式给（100%），
   * three 那边 setSize 时传 updateStyle=false，所以两边不会互相踩。 */
  .ambient-flow {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
  }

  .ambient-spectrum {
    position: absolute;
    /* T4h 回退让位尝试：背景层就是**整个窗口**（用户拍板"不用改了"）——
       山形经 blur 后边缘发散，让位与否视觉上几乎不可辨 */
    inset: 0;
    width: 100%;
    height: 100%;
  }
</style>
