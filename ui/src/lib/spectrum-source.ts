import { computeBandRanges, mapBands, smoothBands, type BandRange } from './spectrum.js';

/**
 * 频谱数据源接口 —— UI 只认这个接口，不认具体实现（ADR-037）。
 *
 * 现在的实现是渲染层 AnalyserNode（音频图在 AudioSink 里，源只负责读）。
 * WASAPI 时代渲染层拿不到音频数据，将换成本进程推送的回环实现——
 * 换实现时 UI 零改动，与播放内核 AudioSink 的解耦思路同构。
 *
 * 帧循环约定：调用方用 rAF 驱动，每帧调一次 `frame()`：
 * 先 `sample()` 取数（内部完成 FFT 读取 + 分带 + 平滑），再读 `bands` 绘制。
 */
export interface SpectrumSource {
  /** 分带数量（创建后不变，UI 据此分配绘制宽度） */
  readonly bandCount: number;
  /** 是否有实时数据（暂停/无数据时 UI 显示衰减或静态降级态） */
  readonly live: boolean;
  /**
   * 底层音频节点（可 absent）——供第三方渲染引擎挂接。
   *
   * AnalyserNode 是直通节点：第三方引擎连到它下面只读数据、不改音频路径，
   * 且必须自行保证不重复连 destination（audioMotion 用 connectSpeakers: false）。
   */
  readonly audioNode?: AudioNode;
  start(): void;
  stop(): void;
  /** 取一帧：读取 + 分带 + 平滑。返回当前分带值（0–1） */
  frame(): Float32Array;
}

/**
 * AnalyserNode 实现。
 *
 * AnalyserNode 本体由 AudioSink 创建并挂在音频图上（图的所有权在出口），
 * 这里只消费它的 getByteFrequencyData —— 不持有也不创建图。
 */
export class AnalyserSource implements SpectrumSource {
  readonly bandCount: number;
  /** 音频图里的 AnalyserNode，供专业引擎（audioMotion）挂接 */
  readonly audioNode: AudioNode;
  private readonly analyser: AnalyserNode;
  private readonly ranges: BandRange[];
  private readonly fft: Uint8Array<ArrayBuffer>;
  private readonly smoothed: Float32Array;
  private running = false;

  constructor(analyser: AnalyserNode, bandCount = 48) {
    this.analyser = analyser;
    this.audioNode = analyser;
    this.bandCount = bandCount;
    // fftSize 是 2 的幂；AnalyserNode 默认 2048 / 48000Hz，bin ≈ 23.4Hz，
    // 40Hz 起步覆盖到 16kHz 足够可视化
    this.ranges = computeBandRanges(analyser.fftSize, analyser.context.sampleRate, bandCount);
    this.fft = new Uint8Array(analyser.frequencyBinCount);
    this.smoothed = new Float32Array(this.ranges.length);
  }

  get live(): boolean {
    return this.running;
  }

  start(): void {
    this.running = true;
  }

  stop(): void {
    this.running = false;
    // 停止后清空状态：下次打开不带着上一首的残影
    this.smoothed.fill(0);
  }

  frame(): Float32Array {
    if (!this.running) return this.smoothed;
    this.analyser.getByteFrequencyData(this.fft);
    const bands = mapBands(this.fft, this.ranges);
    const smoothed = smoothBands(this.smoothed, bands);
    this.smoothed.set(smoothed);
    return this.smoothed;
  }
}
