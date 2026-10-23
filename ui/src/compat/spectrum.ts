import { listen } from '@tauri-apps/api/event';
import type { SpectrumSource } from '../lib/spectrum-source.js';

/**
 * 远程频谱源（docs/07 §2.5 + ADR-0006）。
 *
 * Rust sidecar 在 PCM 链内做 FFT → 48 对数分带 → u8[48]，经 Tauri 事件
 * `spectrum:frame` 推送；这里只做归一化，不碰任何音频节点。
 *
 * 引擎未就绪（P2 之前）时永远收不到事件：`live` 为 false、分带恒 0，
 * SpectrumPanel 呈现静态空态 —— 这就是预期的降级，不是 bug。
 * pro 分支（audioMotion）要求 `audioNode`，本实现恒为 undefined，
 * SpectrumPanel 已有"audioNode 缺失静默降级"逻辑，无需改动。
 */
const FRAME_EVENT = 'spectrum:frame';

export class RemoteSpectrumSource implements SpectrumSource {
  readonly bandCount: number;
  /** 恒 undefined：WebView 内没有音频图，pro 引擎据此自动降级 */
  readonly audioNode: undefined = undefined;

  private bands: Float32Array;
  private running = false;
  private disposed = false;
  private unlisten: (() => void) | null = null;

  constructor(bandCount = 48) {
    this.bandCount = bandCount;
    this.bands = new Float32Array(bandCount);
  }

  get live(): boolean {
    return this.running && this.unlisten !== null;
  }

  start(): void {
    if (this.running || this.disposed) return;
    this.running = true;
    // listen 返回 Promise；按旧 API 约定包成同步 unsubscribe。
    // 非 Tauri 环境（浏览器预览）listen 会拒绝 —— 吞掉，保持空态。
    void listen<number[]>(FRAME_EVENT, (event) => {
      const frame = event.payload;
      if (!this.running || !Array.isArray(frame)) return;
      const n = Math.min(this.bandCount, frame.length);
      for (let i = 0; i < n; i++) this.bands[i] = (frame[i] ?? 0) / 255;
    })
      .then((un) => {
        if (this.disposed || !this.running) un();
        else this.unlisten = un;
      })
      .catch(() => {
        /* 浏览器预览：无 Tauri 事件通道，保持空态 */
      });
  }

  stop(): void {
    this.running = false;
    this.unlisten?.();
    this.unlisten = null;
    this.bands.fill(0);
  }

  /** 取一帧：调用方 rAF 驱动，与旧 AnalyserSource 同约定 */
  frame(): Float32Array {
    return this.bands;
  }

  dispose(): void {
    this.disposed = true;
    this.stop();
  }
}
