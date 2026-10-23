import type { PlayerState, PlayerCommand, TrackQuery } from '@aiya/contracts';
import { AudioSink } from '../lib/audio-sink';
import type { SpectrumSource } from '../lib/spectrum-source';

/**
 * 播放器 store：状态订阅、队列快照、出口（AudioSink）与命令封装。
 *
 * `window.api.player` 的所有调用收敛在这里；组件只读 store、调动作，
 * 不再各自触碰 IPC。AudioSink 也归这里管——它需要 audio 元素，
 * 由 App 在 mount 时通过 `init(audioEl)` 注入。
 */
class PlayerStore {
  state = $state<PlayerState | null>(null);
  queueIds = $state<string[]>([]);
  /** 队列结构版本；变化时渲染层重拉队列快照 */
  version = -1;

  private sink: AudioSink | null = null;
  private audioEl: HTMLAudioElement | null = null;

  /**
   * 注入 audio 元素并开始订阅。
   * 返回清理函数（App 的 onMount 生命周期负责调用）。
   *
   * `onState` 额外回调用于 Media Session 等外部观察者；
   * store 自己的状态更新先于回调执行。
   */
  init(
    audioEl: HTMLAudioElement,
    hooks: { onState?: (state: PlayerState) => void } = {},
  ): () => void {
    this.audioEl = audioEl;
    const unsubscribeState = window.api.player.onState((next) => {
      this.state = next;
      hooks.onState?.(next);
      if (next.queueVersion !== this.version) void this.refreshQueue(next.queueVersion);
    });
    const unsubscribeCommand = window.api.player.onCommand((command) => {
      this.ensureSink()?.apply(command);
    });
    return () => {
      unsubscribeState();
      unsubscribeCommand();
      this.sink?.dispose();
      this.sink = null;
      this.audioEl = null;
    };
  }

  ensureSink(): AudioSink | null {
    if (this.sink || !this.audioEl) return this.sink;
    this.sink = new AudioSink(this.audioEl, {
      report: (report) => window.api.player.report(report),
    });
    return this.sink;
  }

  /** 频谱数据源（惰性建图，ADR-037）；只在面板打开时调用 */
  getSpectrumSource(bandCount = 48): SpectrumSource {
    const active = this.ensureSink();
    if (!active) throw new Error('audio element not ready');
    return active.getSpectrumSource(bandCount);
  }

  setNormalization(gainDb: number | null): void {
    this.ensureSink()?.setNormalization(gainDb);
  }

  alignVolume(volume: number, muted: boolean): void {
    this.ensureSink()?.setVolume(volume, muted);
  }

  async refreshQueue(version?: number): Promise<void> {
    if (version !== undefined && version === this.version) return;
    const result = await window.api.player.queue();
    if (result.ok) {
      this.queueIds = result.data.ids;
      this.version = result.data.version;
    }
  }

  /** 启动时的一次性状态快照（不走订阅） */
  async fetchState(): Promise<PlayerState | null> {
    const result = await window.api.player.state();
    return result.ok ? result.data : null;
  }

  /* ---------------- 命令 ---------------- */

  toggle(): void {
    void window.api.player.toggle();
  }

  next(): void {
    void window.api.player.next();
  }

  prev(): void {
    void window.api.player.prev();
  }

  seek(ms: number): void {
    void window.api.player.seek(Math.max(0, ms));
  }

  setVolume(value: number): void {
    void window.api.player.setVolume(Math.min(1, Math.max(0, value)));
  }

  setRate(value: number): void {
    void window.api.player.setRate(value);
  }

  toggleMute(): void {
    void window.api.player.toggleMute();
  }

  setShuffle(shuffle: boolean): void {
    void window.api.player.setShuffle(shuffle);
  }

  cycleRepeat(): void {
    void window.api.player.setRepeat('cycle');
  }

  clear(): void {
    void window.api.player.clear();
  }

  removeAt(index: number): void {
    void window.api.player.removeAt(index);
  }

  move(from: number, to: number): void {
    void window.api.player.move(from, to);
  }

  enqueue(trackIds: string[]): Promise<unknown> {
    return window.api.player.enqueue(trackIds);
  }

  insertNext(trackId: string): void {
    void window.api.player.insertNext(trackId);
  }

  playTracks(trackIds: string[], startIndex = 0): Promise<unknown> {
    return window.api.player.playTracks(trackIds, startIndex);
  }

  async playQuery(query: TrackQuery, startIndex = 0): Promise<{ ok: boolean; message?: string }> {
    const result = await window.api.player.playQuery(query, startIndex);
    return result.ok ? { ok: true } : { ok: false, message: result.error.message };
  }
}

export const player = new PlayerStore();
export type { PlayerCommand };
