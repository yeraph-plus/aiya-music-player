/**
 * 全应用唯一的 WebGL 舞台。
 *
 * 唯一性的依据：
 *
 * 1. WebGL 上下文是稀缺资源。Chromium 每进程约提供 16 个上下文，超出后静默丢弃
 *    最旧的上下文，表现为某处画面突然变黑，且难以归因——哪一块变黑取决于上下文
 *    被丢弃的顺序，与代码执行顺序无关。
 * 2. 每层一个 canvas 会使合成器多切一层。流动渐变、山形、页面频谱若各占一个
 *    canvas，同一帧即产生多次独立合成事务；同一 GL canvas 内顺序绘制，脏区只需
 *    一次提交。
 * 3. 舞台是分辨率、DPR 与上下文丢失等问题的唯一处理点。图层只负责绘制，不各自
 *    处理 `devicePixelRatio` 变化与 `webglcontextlost`。
 *
 * 同一帧先按登记顺序绘制全窗图层，再绘制区域图层。区域绘制受 viewport/scissor
 * 限制，随后恢复全窗状态。共享画布只在帧入口清除一次，DOM 内容保持在其上方。
 */

import { WebGLRenderer } from 'three';
import { detectCapability } from './capability';
import { RegionTracker } from './region';

export interface Frame {
  /** 累积的有效绘制秒数；隐藏与上下文丢失期间不累加。 */
  time: number;
  /**
   * 距上一帧的秒数，已截断到 [0, 0.1]。
   *
   * 必须截断：窗口最小化/切后台再回来时，两帧间隔可能是几十秒，任何
   * `v += speed * dt` 的写法都会让画面瞬移一大段（山形/流动渐变都会"跳一下"）。
   */
  dt: number;
  /** CSS 像素尺寸（不含 DPR），即图层应该按多大的逻辑画布来布局 */
  width: number;
  height: number;
}

export interface GlLayer {
  /** 诊断/日志用 */
  readonly name: string;
  /** 舞台逻辑尺寸变化时调用（含首帧）。与尺寸相关的资源在这里重建 */
  resize?(width: number, height: number): void;
  /** 每帧调用一次。画到 `stage.renderer` 当前绑定的 framebuffer 上 */
  draw(stage: GlStage, frame: Frame): void;
  dispose(): void;
}

/** DPR 上限：4K@2x 是 8M 像素，再高对全屏 shader 不划算 */
const MAX_DPR = 2;

export class GlStage {
  readonly renderer: WebGLRenderer;
  private readonly layers: GlLayer[] = [];
  private readonly regions = new Map<GlLayer, RegionTracker>();
  private readonly regionSizes = new Map<GlLayer, { width: number; height: number }>();
  private readonly canvas: HTMLCanvasElement;
  private width = 0;
  private height = 0;
  private dpr = 0;
  private elapsed = 0;
  private lastFrameAt: number | null = null;
  private lost = false;
  private disposed = false;

  constructor(canvas: HTMLCanvasElement) {
    this.canvas = canvas;
    this.renderer = new WebGLRenderer({
      canvas,
      // 背景由 CSS 渐变承担，画布须透明，否则会覆盖第 1 层
      alpha: true,
      // 全屏 shader 自身以平滑函数做抗锯齿，MSAA 在此无收益
      antialias: false,
      depth: false,
      stencil: false,
      powerPreference: 'high-performance',
      // 画布内容按预乘 alpha 输出：着色器里写的是 col*alpha，浏览器按预乘
      // 合成才对得上；关闭它会让低 alpha 区域发白（边缘一圈灰）
      premultipliedAlpha: true,
    });
    // 多图层共用一个 framebuffer：每一层自己决定要不要清
    this.renderer.autoClear = false;
    this.renderer.setClearColor(0x000000, 0);

    canvas.addEventListener('webglcontextlost', this.handleLost);
    canvas.addEventListener('webglcontextrestored', this.handleRestored);
    document.addEventListener('visibilitychange', this.resetClock);
  }

  private resetClock = (): void => { this.lastFrameAt = null; };

  /**
   * 上下文丢失与恢复。
   *
   * 上下文丢失属正常事件，驱动更新、GPU 重置、休眠唤醒均可触发。只有调用
   * `preventDefault()` 才具备恢复资格；未处理时浏览器不发送 restored，该画布将
   * 保持黑屏直到整页重载。
   */
  private handleLost = (event: Event): void => {
    event.preventDefault();
    this.lost = true;
    this.resetClock();
    console.warn('[render] webgl context lost; pausing until restored');
  };

  private handleRestored = (): void => {
    this.lost = false;
    this.resetClock();
    // three 自己会在 restored 后重建 program/texture（它监听同一个事件），
    // 但尺寸状态要重新下发一次，否则恢复后画布是旧大小
    this.dpr = 0;
    this.width = 0;
    this.height = 0;
    this.regionSizes.clear();
    console.info('[render] webgl context restored');
  };

  add(layer: GlLayer, region?: HTMLElement): void {
    this.layers.push(layer);
    if (region) this.regions.set(layer, new RegionTracker(this.canvas, region));
    // 已经有尺寸时立刻补一次 resize，避免"后加的图层第一帧画在 0×0 上"
    if (this.width > 0 && !region) layer.resize?.(this.width, this.height);
  }

  remove(layer: GlLayer): void {
    const index = this.layers.indexOf(layer);
    if (index < 0) return;
    this.layers.splice(index, 1);
    this.regions.get(layer)?.dispose();
    this.regions.delete(layer);
    this.regionSizes.delete(layer);
    layer.dispose();
  }

  /** 舞台是否在正常工作（上下文丢失期间为 false） */
  get usable(): boolean {
    return !this.lost && !this.disposed;
  }

  /**
   * 每帧调用一次（由 `subscribeRender` 驱动，别自己排 rAF）。
   *
   * 尺寸从 `clientWidth/clientHeight` 现取而不是缓存的：窗口缩放、
   * 侧栏退出、devtools 停靠都会改它，而没有任何一个事件能可靠覆盖全部来源。
   * 读一次布局属性（非几何）不会强制 reflow。
   */
  render(nowMs: number): void {
    if (!this.usable || document.hidden) return;

    const w = this.canvas.clientWidth;
    const h = this.canvas.clientHeight;
    if (w === 0 || h === 0) { this.resetClock(); return; }

    const dpr = Math.min(MAX_DPR, window.devicePixelRatio || 1);
    if (dpr !== this.dpr) {
      this.dpr = dpr;
      this.renderer.setPixelRatio(dpr);
    }
    if (w !== this.width || h !== this.height) {
      this.width = w;
      this.height = h;
      // 第三个参数 false：CSS 尺寸由样式表管（100%），three 别去写 style
      this.renderer.setSize(w, h, false);
      for (const layer of this.layers) if (!this.regions.has(layer)) layer.resize?.(w, h);
    }

    const rawDt = this.lastFrameAt === null ? 0 : (nowMs - this.lastFrameAt) / 1000;
    this.lastFrameAt = nowMs;
    const dt = Math.min(0.1, Math.max(0, rawDt));
    this.elapsed += dt;
    const frame: Frame = {
      time: this.elapsed,
      dt,
      width: w,
      height: h,
    };

    this.restoreViewport();
    this.renderer.clear();
    // 全窗层先绘制，区域层最后绘制，形态切换不改变前后景顺序。
    for (let pass = 0; pass < 2; pass += 1) {
      for (const layer of this.layers) {
        const tracker = this.regions.get(layer);
        if (Boolean(tracker) !== (pass === 1)) continue;
        try {
          let localFrame = frame;
          if (tracker) {
            const rect = tracker.measure();
            if (!rect) continue;
            const size = this.regionSizes.get(layer);
            if (!size || size.width !== rect.width || size.height !== rect.height) {
              layer.resize?.(rect.width, rect.height);
              this.regionSizes.set(layer, { width: rect.width, height: rect.height });
            }
            this.renderer.setViewport(rect.x, rect.y, rect.width, rect.height);
            this.renderer.setScissor(rect.clipX, rect.clipY, rect.clipWidth, rect.clipHeight);
            this.renderer.setScissorTest(true);
            localFrame = { ...frame, width: rect.width, height: rect.height };
          }
          layer.draw(this, localFrame);
        } catch (error) {
          console.error(`[render] layer "${layer.name}" failed:`, error);
        } finally {
          this.restoreViewport();
        }
      }
    }
  }

  private restoreViewport(): void {
    this.renderer.setRenderTarget(null);
    this.renderer.setViewport(0, 0, this.width, this.height);
    this.renderer.setScissor(0, 0, this.width, this.height);
    this.renderer.setScissorTest(false);
  }

  /**
   * 卸载。需要调用 `forceContextLoss()`：`dispose()` 只释放 three 侧资源，GL
   * 上下文要等 GC 才回收；HMR 每次热更都会重建舞台，不主动释放会在数次热更后
   * 达到上下文数量上限。
   */
  dispose(): void {
    if (this.disposed) return;
    this.disposed = true;
    document.removeEventListener('visibilitychange', this.resetClock);
    this.canvas.removeEventListener('webglcontextlost', this.handleLost);
    this.canvas.removeEventListener('webglcontextrestored', this.handleRestored);
    for (const layer of this.layers) {
      try {
        layer.dispose();
      } catch (error) {
        console.error(`[render] layer "${layer.name}" dispose failed:`, error);
      }
    }
    this.layers.length = 0;
    for (const region of this.regions.values()) region.dispose();
    this.regions.clear();
    this.regionSizes.clear();
    this.renderer.dispose();
    this.renderer.forceContextLoss();
  }
}

/**
 * 仅在硬件 WebGL2 可用时创建舞台；失败时保留 CSS 静态背景。
 */
export function createGlStage(canvas: HTMLCanvasElement): GlStage | null {
  const capability = detectCapability();
  if (capability.tier < 2) return null;
  try {
    return new GlStage(canvas);
  } catch (error) {
    // 驱动黑名单或上下文名额耗尽时，CSS 渐变继续显示。
    console.warn('[render] gl stage unavailable:', error);
    return null;
  }
}
