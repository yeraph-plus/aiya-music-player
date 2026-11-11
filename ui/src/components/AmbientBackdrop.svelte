<script lang="ts">
  import { untrack } from 'svelte';
  import { themeColor } from '../lib/accent';
  import { detectCapability } from '../lib/render/capability';
  import { createGlStage, type GlStage } from '../lib/render/context';
  import { FlowLayer } from '../lib/render/layers/flow';
  import { RidgeLayer } from '../lib/render/layers/ridge';
  import { RippleLayer } from '../lib/render/layers/ripple';
  import { subscribeRender } from '../lib/render/scheduler';
  import { reducedMotion } from '../lib/motion.svelte';
  import type { BgSpectrumStyle, RidgeGlow } from '../lib/settings';
  import type { SpectrumSource } from '../lib/spectrum-source';

  interface Props {
    /** 频谱数据源（48 带 tap）；null 或 enabled=false 时环境层不渲染 */
    source: SpectrumSource | null;
    /** 背景动态频谱开关（`settings.bgSpectrum` 且全局 3D 闸门开着才为 true） */
    enabled: boolean;
    /** 背景频谱形态（山形 / 波纹，见 `BgSpectrumStyle`） */
    shape: BgSpectrumStyle;
    /** 背景频谱光效档位（山形与波纹**共用**同一档语义） */
    glow: RidgeGlow;
    /**
     * 是否处于暂停态。暂停时频谱静止并收平，而非凝固在暂停瞬间的起伏上。
     * 流动渐变不受影响：它属于常驻环境光，不是频谱。
     */
    paused: boolean;
  }

  let { source, enabled, shape, glow, paused }: Props = $props();

  /**
   * 全应用背景。
   *
   * 全应用只有一套背景：正在播放页侧栏退出、主区铺满后，本层天然覆盖整个
   * 窗口，不为任何单页另绘第二层，因此本组件不需要知道当前路由。
   *
   * 只有一种视觉档位：各页面共用同一透明度，可读性遮罩层不启用。
   *
   * 三层（第 4 层为界面布局本身，属四区席位）：
   * - 第 1 层 背景：固定 CSS 渐变（`--ambient-bg`，tokens.css 按
   *   `html[data-ambient]` 定义，设置页成套切换）。不跟随封面。
   * 第 2 层 流动渐变：WebGL 上缓慢流动的彩色光雾，叠在第 1 层
   *   之上做加色。`--ambient-bg` 仍是配色唯一来源，颜色由 `--accent` 派生。
   *   硬件 GL 不可用时整层不创建。
   * - 第 3 层 频谱环境光：形态由 `bgSpectrumStyle` 决定。`ridge` 山形
   *   48 带先做时间平滑，再重采样为平滑脊线并闭合到底边填充渐变
   *   分远山（低幅低透明）与近山（主形）两层。`ripple` 波纹：同一曲线作为水面
   *   截面，沿视线逐层绘制等深线铺满下半屏（画法见 `layers/ripple.ts`）。
   *
   * 两种形态互斥：使用同一块屏幕与同一份数据，同时绘制会互相削弱，两者均难以
   * 辨认。切换时整层重建——几何结构不同，无法通过 uniform 切换。
   *
   * 两条光栅路径（档位见 `lib/render/capability.ts`）：
   * - 档 2（硬件 GL）：两种形态绘制在同一 GL canvas 上，图层按 `add()` 顺序同帧
   *   叠放（`layers/ridge.ts`、`layers/ripple.ts`、`flow.ts`）。光晕由解析方式
   *   计算（按到脊线或线心的距离取指数衰减），无离屏缓冲与模糊核。
   * - 档 1（软件光栅或 GL 不可用）：仅山形，使用 Canvas 2D 实现（160×90 buffer
   *   加 `ctx.filter blur(14px)` 放大）。波纹不做 2D 兜底：22 层全窗口描边加
   *   shadowBlur 在软件光栅上开销最大，此类机器退回山形即可。
   *
   * 绘制循环挂全应用单例调度器（`lib/render/scheduler`），本组件不
   * 自行排 rAF。组件内多处 `requestAnimationFrame` 会使调度数按 2^n 增长，数秒
   * 内占满主线程并导致界面无响应。多图层场景需从结构上排除。
   *
   * `prefers-reduced-motion: reduce` 时环境层不渲染，静态背景保留。
   */
  let canvas = $state<HTMLCanvasElement | null>(null);

  /** tap 是否该开着（数据源 + 开关 + 非 reduced-motion） */
  const tapOn = $derived(Boolean(source) && enabled && !reducedMotion.value);

  /**
   * GL 舞台是否创建。
   *
   * 档位判断放在 canvas 元素渲染这一层，而非仅在 `createGlStage` 内：后者返回
   * null 时 `bind:this` 已渲染画布，会留下一个全屏透明 canvas 额外占用一层
   * 合成（软件光栅实测即为此情形：档 1，画布仍存在）。
   *
   * 只要求 `enabled`（已含 `glEffects` 闸门），不要求 `source`：流动渐变不依赖
   * 频谱数据，tap 失败时背景仍应保持动态。
   */
  const glActive = $derived(enabled && !reducedMotion.value && detectCapability().tier >= 2);

  /** 频谱层走 GL（需要数据源）；否则山形走 2D 兜底 */
  const specGl = $derived(glActive && Boolean(source));
  const ridge2d = $derived(tapOn && !glActive);

  let glCanvas = $state<HTMLCanvasElement | null>(null);
  let stage = $state<GlStage | null>(null);
  let ridgeLayer = $state<RidgeLayer | null>(null);
  let rippleLayer = $state<RippleLayer | null>(null);

  /**
   * 2D 兜底画布的 CSS 尺寸缓存。
   *
   * 直接在 draw() 里逐帧读 `clientWidth/clientHeight` 会把绘制循环与样式/布局
   * 绑定：页面任意处改样式都会追加一次强制布局。ResizeObserver 只在尺寸真正
   * 变化时回调一次，正是这里需要的时机。
   */
  let cssW = 0;
  let cssH = 0;
  $effect(() => {
    const target = canvas;
    if (!target) return;
    const measure = () => {
      cssW = target.clientWidth;
      cssH = target.clientHeight;
    };
    measure();
    const ro = new ResizeObserver(measure);
    ro.observe(target);
    return () => ro.disconnect();
  });

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

  // 频谱图层随"数据源 + 形态"挂载与卸载。形态进入依赖数组，因此切换时整层重建：山形为闭合填充，波纹为线阵，几何结构不同，无法像光效档位那样仅修改 uniform。重建在同一帧内完成（旧层
  // dispose 后新层 `add`），舞台不重建，不产生黑帧。光效档位不进入依赖：改档只修改 uniform，不重建图层。
  $effect(() => {
    const current = stage;
    const src = specGl ? source : null;
    const want = shape;
    // 初始档位在重建那一刻取一次就够（后续变化走下一条 effect），所以 untrack：
    // 否则切档会连带触发本 effect、把整层重建一遍
    const initialGlow = untrack(() => glow);
    if (!current || !src) {
      ridgeLayer = null;
      rippleLayer = null;
      return;
    }
    if (want === 'ripple') {
      const layer = new RippleLayer(src, initialGlow);
      current.add(layer);
      rippleLayer = layer;
      return () => {
        rippleLayer = null;
        current.remove(layer);
        layer.dispose();
      };
    }
    const layer = new RidgeLayer(src, initialGlow);
    current.add(layer);
    ridgeLayer = layer;
    return () => {
      ridgeLayer = null;
      current.remove(layer);
      layer.dispose();
    };
  });

  // 光效档位：两种形态共用（波纹按"光的多少 + 边缘锐度"映射到同一语义）
  $effect(() => {
    // 读 `glow` 建立依赖；图层可能还没建（首帧/档 1），null 直接跳过
    const level = glow;
    ridgeLayer?.setGlow(level);
    rippleLayer?.setGlow(level);
  });

  // 暂停：频谱静止 + 收平（不暂存暂停前的起伏）
  $effect(() => {
    const isPaused = paused;
    ridgeLayer?.setPaused(isPaused);
    rippleLayer?.setPaused(isPaused);
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
    // 尺寸读的是 RO 缓存（见上方注释），不再逐帧查布局
    const width = cssW;
    const height = cssH;
    // 尺寸为 0（布局尚未结算/页面切走）：本帧跳过，调度器下一帧还会来
    if (width === 0 || height === 0) return;
    // 宽高各自比较：仅比较宽度时，窗口只纵向变化（最大化、分屏、拖动下沿）会使 `height`
    // 属性停留在旧值，浏览器将旧位图纵向拉伸。NowPlayingSpectrum 的 2D 兜底路径曾出现该缺陷（同一处遗漏）。
    const bw = Math.round(width * dpr);
    const bh = Math.round(height * dpr);
    if (target.width !== bw || target.height !== bh) {
      target.width = bw;
      target.height = bh;
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
      // RemoteSpectrumSource.frame() 已归一化到 0–1（引擎 0–255 → /255），此处不得再次除以 255：二次归一化会把带值压到约
      // 0.4%，山形贴近底边成一条平线暂停时目标值取 0：山形在约 90ms 内落回底边，不保留暂停前的起伏
      const v = paused ? 0 : Math.min(1, Math.max(0, bands[i] as number));
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
    z-index: var(--z-bg);
    overflow: hidden;
    pointer-events: none;
  }

  /* 第 1 层：固定渐变（--ambient-bg 随 html[data-ambient] 切换）。
   * 单档，不做透明度衰减。 */
  .ambient-art {
    position: absolute;
    inset: 0;
    background: var(--ambient-bg);
    opacity: 1;
  }

  /* 第 2、3 层：GL 画布。不参与布局，尺寸由样式给定（100%）；
   * three 侧 setSize 传 updateStyle=false，两边不互相覆盖。 */
  .ambient-flow {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
  }

  .ambient-spectrum {
    position: absolute;
    /* 背景层覆盖整个窗口：山形经模糊后边缘向外发散，收窄与否在视觉上无法区分 */
    inset: 0;
    width: 100%;
    height: 100%;
  }
</style>
