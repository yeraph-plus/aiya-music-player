import { events, ipc } from './ipc';

/**
 * 频谱数据源接口 —— UI 只认这个接口，不认具体实现（ADR-037）。
 *
 * 约定：调用方用 rAF 驱动，每帧调一次 `frame()` 取当前分带值（0–1）后绘制。
 * 30fps 的数据**不进 Svelte 响应式**（docs/18 §4.2）：`frame()` 返回同一个
 * Float32Array 实例就地覆盖，不做 `$state` 写入，否则整棵组件树每秒 30 次脏检查。
 */
export interface SpectrumSource {
  /** 分带数量（创建后不变，UI 据此分配绘制宽度） */
  readonly bandCount: number;
  /** 是否有实时数据（暂停/无数据时 UI 显示衰减或静态降级态） */
  readonly live: boolean;
  start(): void;
  stop(): void;
  /** 取一帧：返回当前分带值（0–1）。未运行时返回全 0 */
  frame(): Float32Array;
}

/**
 * 远程频谱源（ADR-0006：音频在 Rust sidecar）。
 *
 * Rust 在 PCM 链内做 FFT → 48 对数分带 → u8[48]，经 `spectrum:frame` 推送；
 * 这里只做 0-255 → 0-1 的归一化，不碰任何音频节点。
 *
 * 订阅是**按需 + 引用计数**的：引擎 tap（`spectrum_set`）是**全局**开关，
 * 而消费方可能有多个（AppShell 的背景环境层、正在播放页的可视化），共享
 * 同一个实例。`start()/stop()` 按引用计数叠放——第一个 start 打开 tap，
 * 最后一个 stop 才关闭；否则页面层卸载会把背景层的 tap 一起带走（T4h）。
 *
 * 引擎未就绪时永远收不到帧：`live` 为 false、分带恒 0，面板呈现静态空态 ——
 * 这是预期的降级，不是 bug。
 */
export class RemoteSpectrumSource implements SpectrumSource {
  readonly bandCount: number;

  private bands: Float32Array;
  private running = false;
  private refs = 0;
  private disposed = false;
  private unsubscribe: (() => void) | null = null;

  constructor(bandCount = 48) {
    this.bandCount = bandCount;
    this.bands = new Float32Array(bandCount);
  }

  get live(): boolean {
    return this.running && this.unsubscribe !== null;
  }

  start(): void {
    if (this.disposed) return;
    this.refs += 1;
    if (this.running) return;
    this.running = true;
    // tap 开关失败不影响空态：面板照样打开，只是没有数据
    void ipc.app.spectrumSet(true).catch(() => undefined);
    this.unsubscribe = events.spectrumFrame((frame) => {
      if (!this.running || !Array.isArray(frame)) return;
      const n = Math.min(this.bandCount, frame.length);
      for (let i = 0; i < n; i += 1) this.bands[i] = (frame[i] ?? 0) / 255;
    });
  }

  stop(): void {
    this.refs = Math.max(0, this.refs - 1);
    // 还有别的消费方挂着：只减计数，不动 tap
    if (this.refs > 0) return;
    // 只在曾 start 过时退订，避免面板没开就发一条多余的 IPC
    if (this.running) void ipc.app.spectrumSet(false).catch(() => undefined);
    this.running = false;
    this.unsubscribe?.();
    this.unsubscribe = null;
    this.bands.fill(0);
  }

  frame(): Float32Array {
    return this.bands;
  }

  dispose(): void {
    this.disposed = true;
    this.stop();
  }
}
