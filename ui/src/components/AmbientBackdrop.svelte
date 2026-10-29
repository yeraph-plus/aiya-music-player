<script lang="ts">
  import { themeColor } from '../lib/accent';
  import type { SpectrumSource } from '../lib/spectrum-source';

  interface Props {
    /** 频谱数据源（48 带 tap）；null 或 spectrumOn=false 时环境层不渲染 */
    source: SpectrumSource | null;
    spectrumOn: boolean;
  }

  let { source, spectrumOn }: Props = $props();

  /**
   * 全应用背景（docs/25 §5 T4c app 化；T4h 起背景频谱改山形曲线）。
   *
   * **单一档位**（用户拍板，撤销 standard/immersive 双档）：原本列表页
   * 走"重遮罩保可读性"（veil 压 surface-1 渐变、氛围 0.6、频谱更虚），
   * 正在播放页走沉浸档（无遮罩、氛围满档、频谱 alpha 0.75/blur 14）。
   * 用户反馈沉浸档的透明度观感更好 —— 现在所有页面统一走这一档，
   * 可读性遮罩（第 3 层）整个撤销。
   *
   * 三层（第 4 层是界面布局本身，在四区席位里）：
   * - 第 1 层 背景：固定 CSS 渐变（`--ambient-bg`，tokens.css 按
   *   `html[data-ambient]` 定义，设置页成套切换）。**不跟随封面**。
   * - 第 2 层 频谱环境光：**山形曲线**（docs/26 调研）：48 带先做时间
   *   平滑，再经 Catmull-Rom 式中点二次贝塞尔连成一条起伏山脊线，
   *   闭合到底边填充渐变——远山一层（低幅低透明）+ 近山一层（主形）。
   *   画在低分辨率 buffer（160×90）上再放大，理由是**性能**：大半径
   *   模糊的代价 ∝ 像素数，低分辨率上做 blur 便宜一个数量级，放大本身
   *   的双线性插值还免费送一层柔化（T4g 曾把这层插值当"虚化" bug 修掉，
   *   现在它正是我们要的虚化）。blur 14px（虚化但保留山的可辨识度）。
   *
   * `prefers-reduced-motion: reduce` 时环境层整体不渲染（静态背景保留）。
   */
  let canvas = $state<HTMLCanvasElement | null>(null);
  let frameId = 0;

  const reducedMotion =
    typeof matchMedia !== 'undefined' && matchMedia('(prefers-reduced-motion: reduce)').matches;

  const active = $derived(Boolean(source) && spectrumOn && !reducedMotion);

  /** 低分辨率离屏 buffer：山形画在这里，主画布只做放大绘制 */
  let off: HTMLCanvasElement | null = null;

  /** 时间平滑后的显示值（48 带）：起音快、衰减慢，山才不会跟着帧抖 */
  let display: Float32Array | null = null;

  /**
   * 在 buffer 上画一条山脊线并填充到底边。
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
    if (!octx) {
      frameId = requestAnimationFrame(draw);
      return;
    }
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

    // **每帧只允许排一个下一帧**——这里若出现两次 requestAnimationFrame，
    // 调度数会按 2^n 指数增长，几秒内主线程被回调淹没、整个 UI 假死
    // （2026-10-28 实际发生过的回归：点击全部失效就是它）。
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

<div class="ambient-bg">
  <div class="ambient-art"></div>
  {#if active}
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

  .ambient-spectrum {
    position: absolute;
    /* T4h 回退让位尝试：背景层就是**整个窗口**（用户拍板"不用改了"）——
       山形经 blur 后边缘发散，让位与否视觉上几乎不可辨 */
    inset: 0;
    width: 100%;
    height: 100%;
  }
</style>
