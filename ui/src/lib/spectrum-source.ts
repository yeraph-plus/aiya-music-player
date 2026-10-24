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
  /**
   * 底层音频节点（可 absent）——供第三方渲染引擎挂接。
   * WebView 内没有音频图，本实现恒为 undefined，`SpectrumPanel` 据此降级。
   */
  readonly audioNode?: AudioNode;
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
 * 订阅是**按需**的：`start()` 时才 `ipc.app.spectrumSet(true)` 打开引擎 tap
 * （关闭时引擎内零开销），`stop()` 时关掉。面板没打开就不该有 FFT 开销。
 *
 * 引擎未就绪时永远收不到帧：`live` 为 false、分带恒 0，面板呈现静态空态 ——
 * 这是预期的降级，不是 bug。
 */
export class RemoteSpectrumSource implements SpectrumSource {
  readonly bandCount: number;
  /** 恒 undefined：WebView 内没有音频图，pro 引擎据此自动降级 */
  readonly audioNode: undefined = undefined;

  private bands: Float32Array;
  private running = false;
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
    if (this.running || this.disposed) return;
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
