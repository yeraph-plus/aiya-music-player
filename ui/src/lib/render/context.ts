/**
 * 全应用唯一的 WebGL 舞台（docs/46 §7，P1 落地）。
 *
 * 为什么"唯一"：
 *
 * 1. **上下文是稀缺资源**。Chromium 每进程只给 ~16 个 WebGL 上下文，超出后
 *    它会静默丢掉最老的那个 —— 症状是"某处画面突然黑了"，而且极难归因
 *    （哪块黑取决于谁的上下文先被丢，与代码执行顺序无关）。
 * 2. **一层一个 canvas 会让合成器多切一层**。流动渐变、山形、将来的页面频谱
 *    如果各占一个 canvas，同一帧里就是三次独立的合成事务。同一个 GL canvas
 *    内按顺序画完，脏区一次提交。
 * 3. 舞台是"分辨率 / DPR / 上下文丢失"这类脏活的唯一落点。图层只管画，不用
 *    各自处理 `devicePixelRatio` 变化和 `webglcontextlost`。
 *
 * 图层按 `add()` 顺序**在同一帧里依次画**（`autoClear = false`），所以后加的
 * 图层叠在先加的之上，等价于 DOM 里的 z 序。想调整叠放顺序就调 `add` 的顺序，
 * 不要靠 z-index —— 它们共享同一张画布，CSS 层级对它们无效。
 */

import { WebGLRenderer } from 'three';
import { detectCapability } from './capability';

export interface Frame {
  /** 舞台启动至今的秒数（不是 `performance.now()/1000`，从 0 起算） */
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
  private readonly canvas: HTMLCanvasElement;
  private width = 0;
  private height = 0;
  private dpr = 0;
  private startedAt = 0;
  private lastFrameAt = 0;
  private lost = false;

  constructor(canvas: HTMLCanvasElement) {
    this.canvas = canvas;
    this.renderer = new WebGLRenderer({
      canvas,
      // 背景是 CSS 渐变，画布必须透明，否则会把第 1 层整个盖掉
      alpha: true,
      // 全屏 shader 靠自身做抗锯齿（平滑函数），MSAA 在这里是纯浪费
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
  }

  /**
   * 上下文丢失/恢复。
   *
   * 丢失是**正常的**：驱动更新、GPU 重置、休眠唤醒都会触发，不是异常。
   * 必须 `preventDefault()` 才有资格被恢复；不拦的话浏览器不会发 restored，
   * 这块画布就永久黑着（直到整页重载）。
   */
  private handleLost = (event: Event): void => {
    event.preventDefault();
    this.lost = true;
    console.warn('[render] webgl context lost; pausing until restored');
  };

  private handleRestored = (): void => {
    this.lost = false;
    // three 自己会在 restored 后重建 program/texture（它监听同一个事件），
    // 但尺寸状态要重新下发一次，否则恢复后画布是旧大小
    this.dpr = 0;
    this.width = 0;
    this.height = 0;
    console.info('[render] webgl context restored');
  };

  add(layer: GlLayer): void {
    this.layers.push(layer);
    // 已经有尺寸时立刻补一次 resize，避免"后加的图层第一帧画在 0×0 上"
    if (this.width > 0) layer.resize?.(this.width, this.height);
  }

  remove(layer: GlLayer): void {
    const index = this.layers.indexOf(layer);
    if (index >= 0) this.layers.splice(index, 1);
  }

  /** 舞台是否在正常工作（上下文丢失期间为 false） */
  get usable(): boolean {
    return !this.lost;
  }

  /**
   * 每帧调用一次（由 `subscribeRender` 驱动，别自己排 rAF）。
   *
   * 尺寸从 `clientWidth/clientHeight` 现取而不是缓存的：窗口缩放、
   * 侧栏退出、devtools 停靠都会改它，而没有任何一个事件能可靠覆盖全部来源。
   * 读一次布局属性（非几何）不会强制 reflow。
   */
  render(nowMs: number): void {
    if (this.lost) return;

    const w = this.canvas.clientWidth;
    const h = this.canvas.clientHeight;
    if (w === 0 || h === 0) return;

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
      for (const layer of this.layers) layer.resize?.(w, h);
    }

    if (this.startedAt === 0) {
      this.startedAt = nowMs;
      this.lastFrameAt = nowMs;
    }
    const rawDt = (nowMs - this.lastFrameAt) / 1000;
    this.lastFrameAt = nowMs;
    const frame: Frame = {
      time: (nowMs - this.startedAt) / 1000,
      dt: Math.min(0.1, Math.max(0, rawDt)),
      width: w,
      height: h,
    };

    this.renderer.clear();
    for (const layer of this.layers) {
      try {
        layer.draw(this, frame);
      } catch (error) {
        // 单层画挂不该连带别的层一起黑
        console.error(`[render] layer "${layer.name}" failed:`, error);
      }
    }
  }

  /**
   * 卸载。`forceContextLoss()` 是必须的：`dispose()` 只释放 three 侧的
   * 资源，GL 上下文本身要到 GC 才回收；而 HMR 每次热更都会重建舞台，
   * 不主动丢就会在几次热更后撞上上下文上限。
   */
  dispose(): void {
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
    this.renderer.dispose();
    this.renderer.forceContextLoss();
  }
}

/**
 * 建舞台。**能力不足时返回 null**，调用方据此完全不建 canvas。
 *
 * 只在档 2（硬件加速）建：档 1 的机器是软件光栅，全屏 fragment shader 在
 * 那种环境下的表现没有实测数据（docs/46 §2.3 只测了三种 shader 的软栅
 * 单帧 1.3ms，但不确定性太大），保持"软件光栅 → 用现有 Canvas 2D"这条
 * 已经验证过的退路更稳。
 */
export function createGlStage(canvas: HTMLCanvasElement): GlStage | null {
  const capability = detectCapability();
  if (capability.tier < 2) return null;
  try {
    return new GlStage(canvas);
  } catch (error) {
    // 创建失败（驱动黑名单、上下文名额耗尽）不该让背景整个消失：
    // 退回 2D 路径，静态渐变仍然在
    console.warn('[render] gl stage unavailable:', error);
    return null;
  }
}
