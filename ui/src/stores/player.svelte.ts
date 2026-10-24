import { events, ipc } from '../lib/ipc';
import type { PlayerState } from '../lib/ipc/generated/PlayerState';
import type { TrackQueryInput } from '../lib/ipc/client';
import { RemoteSpectrumSource, type SpectrumSource } from '../lib/spectrum-source';
import { errorMessage } from '../lib/errors';

/**
 * 播放器 store：状态订阅、队列快照、命令封装、频谱数据源。
 *
 * 与 v1 的三处结构差异（ADR-0006 / docs/18 §4.2）：
 * 1. **没有 AudioSink，也没有 `<audio>` 元素**。音频在 Rust sidecar 出声，
 *    UI 侧不存在音频图；旧代码里那条 `player:command` → `AudioSink.apply()`
 *    的路已经退化成 no-op 垫片，这里连同 `<audio>` 挂载点一起删掉。
 *    状态只从 `player:state` 事件来。
 * 2. **10Hz 事件用 rAF 合帧**：事件先写非响应式快照，再由 rAF 刷进 `$state`。
 *    10Hz 本身不贵，但 seek 拖动/连点下一首时事件会成串到达，
 *    "一帧最多一次脏检查"比"一条事件一次"稳。
 * 3. **命令失败不再静默**：新契约失败即 reject（返回 `ApiError`），
 *    这里统一 catch 成 warn —— 不弹错误条（播放控制的失败在状态里已经体现），
 *    但不能变成 unhandled rejection。
 */
class PlayerStore {
  state = $state<PlayerState | null>(null);
  queueIds = $state<string[]>([]);

  /** 队列结构版本；变化时渲染层重拉队列快照 */
  private version = -1;
  /** 非响应式快照：事件先落这里，rAF 再刷进 `state` */
  private pending: PlayerState | null = null;
  private frame: number | null = null;
  private spectrum: RemoteSpectrumSource | null = null;

  /**
   * 开始订阅。返回清理函数（App 的 onMount 生命周期负责调用）。
   *
   * `onState` 是给外部观察者的（系统媒体中心），**同步**调用而不是等 rAF：
   * 窗口隐藏时浏览器把 rAF 降到 0，但 OS 的媒体卡片必须立刻知道换歌了。
   * UI 侧的 `$state` 落后一帧没有观感影响，两者需求不一致，分开处理。
   */
  init(hooks: { onState?: (state: PlayerState) => void } = {}): () => void {
    const unsubscribe = events.playerState((next) => {
      this.pending = next;
      hooks.onState?.(next);
      if (next.queueVersion !== this.version) void this.refreshQueue(next.queueVersion);
      this.scheduleFlush();
    });
    return () => {
      unsubscribe();
      if (this.frame !== null) cancelAnimationFrame(this.frame);
      this.frame = null;
      this.pending = null;
      this.spectrum?.dispose();
      this.spectrum = null;
    };
  }

  private scheduleFlush(): void {
    if (this.frame !== null) return;
    this.frame = requestAnimationFrame(() => {
      this.frame = null;
      if (this.pending) this.state = this.pending;
    });
  }

  /** 频谱数据源（惰性建订阅；只在面板打开时调用） */
  getSpectrumSource(bandCount = 48): SpectrumSource {
    if (!this.spectrum) this.spectrum = new RemoteSpectrumSource(bandCount);
    return this.spectrum;
  }

  async refreshQueue(version?: number): Promise<void> {
    if (version !== undefined && version === this.version) return;
    try {
      const snapshot = await ipc.player.queue();
      this.queueIds = snapshot.ids;
      this.version = snapshot.version;
    } catch (e) {
      // 队列快照拉取失败只降级（保留旧 ids），不打断播放
      console.warn('refreshQueue failed:', errorMessage(e));
    }
  }

  /** 启动时的一次性状态快照（不走订阅） */
  async fetchState(): Promise<PlayerState | null> {
    try {
      return await ipc.player.state();
    } catch (e) {
      console.warn('player.state failed:', errorMessage(e));
      return null;
    }
  }

  /* ---------------- 命令 ----------------
   * 返回值故意丢弃：状态一律以 `player:state` 事件为准，
   * 命令的返回只是"后端已执行"的回执，两边都写会造成状态抖动。
   * 失败只记 warn（播放/暂停按不动不会让应用崩，但要能在日志里看到）。
   */

  private fail(op: string, e: unknown): void {
    console.warn(`player.${op} failed:`, errorMessage(e));
  }

  toggle(): void {
    void ipc.player.toggle().catch((e) => this.fail('toggle', e));
  }

  next(): void {
    void ipc.player.next().catch((e) => this.fail('next', e));
  }

  prev(): void {
    void ipc.player.prev().catch((e) => this.fail('prev', e));
  }

  seek(ms: number): void {
    void ipc.player.seek(Math.max(0, ms)).catch((e) => this.fail('seek', e));
  }

  setVolume(value: number): void {
    void ipc.player.setVolume(Math.min(1, Math.max(0, value))).catch((e) => this.fail('setVolume', e));
  }

  setRate(value: number): void {
    void ipc.player.setRate(value).catch((e) => this.fail('setRate', e));
  }

  toggleMute(): void {
    void ipc.player.toggleMute().catch((e) => this.fail('toggleMute', e));
  }

  setShuffle(shuffle: boolean): void {
    void ipc.player.setShuffle(shuffle).catch((e) => this.fail('setShuffle', e));
  }

  cycleRepeat(): void {
    void ipc.player.setRepeat('cycle').catch((e) => this.fail('setRepeat', e));
  }

  clear(): void {
    void ipc.player.clear().catch((e) => this.fail('clear', e));
  }

  removeAt(index: number): void {
    void ipc.player.removeAt(index).catch((e) => this.fail('removeAt', e));
  }

  move(from: number, to: number): void {
    void ipc.player.move(from, to).catch((e) => this.fail('move', e));
  }

  insertNext(trackId: string): void {
    void ipc.player.insertNext(trackId).catch((e) => this.fail('insertNext', e));
  }

  /* 下面三个会 reject 交给调用方：要弹错误条的是"播放失败"，不是全部命令 */

  enqueue(trackIds: string[]): Promise<PlayerState> {
    return ipc.player.enqueue(trackIds);
  }

  playTracks(trackIds: string[], startIndex = 0): Promise<PlayerState> {
    return ipc.player.playTracks(trackIds, startIndex);
  }

  playQuery(query: TrackQueryInput, startIndex = 0): Promise<PlayerState> {
    return ipc.player.playQuery(query, startIndex);
  }
}

export const player = new PlayerStore();
