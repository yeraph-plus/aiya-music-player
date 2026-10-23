import type { PlayerCommand, PlayerReport } from '@aiya/contracts';
import type { SpectrumSource } from '../lib/spectrum-source.js';
import { RemoteSpectrumSource } from './spectrum.js';

/**
 * 远程音频出口（docs/07 §2.2 + ADR-0006）。
 *
 * 旧 `lib/audio-sink.ts` 把 PlayerCommand 翻译成 `<audio>` 操作；
 * Tauri 版音频在 Rust sidecar 出声，指令由后端直接执行，所以本类：
 * - `apply()` 是有意 no-op —— 绝不能把 load 转发给 <audio>，
 *   否则会出现第二条音频路径；
 * - 状态/位置不再由这里回报（transport 保留仅为签名兼容），
 *   后端经 `player:state` 事件直接驱动 store；
 * - `<audio>` 是 App.svelte 的死挂载点：不加载 src、不接 Web Audio。
 */
export interface AudioSinkTransport {
  report(report: PlayerReport): void;
}

export class AudioSink {
  private spectrum: RemoteSpectrumSource | null = null;
  private disposed = false;

  constructor(
    /** 死挂载点，仅保持旧签名；永不加载 src */
    private readonly _audio: HTMLAudioElement,
    private readonly _transport: AudioSinkTransport,
  ) {
    void this._audio;
    void this._transport;
  }

  /** 旧 API 保留；远程出口没有装载令牌语义（令牌在后端） */
  get currentToken(): number {
    return -1;
  }

  apply(_command: PlayerCommand): void {
    /* 后端已执行；见类注释 —— 不转发，防止第二条音频路径 */
  }

  /** ReplayGain 归一化已在后端 DSP 链内 */
  setNormalization(_gainDb: number | null): void {
    /* no-op */
  }

  /** 音量/静音已在后端；状态经 player:state 事件回到 UI */
  setVolume(_volume: number, _muted: boolean): void {
    /* no-op */
  }

  /** 频谱数据源：链内 tap 的事件消费者（P2 引擎就绪前为空态） */
  getSpectrumSource(bandCount = 48): SpectrumSource {
    if (!this.spectrum) this.spectrum = new RemoteSpectrumSource(bandCount);
    return this.spectrum;
  }

  dispose(): void {
    if (this.disposed) return;
    this.disposed = true;
    this.spectrum?.dispose();
    this.spectrum = null;
  }
}
