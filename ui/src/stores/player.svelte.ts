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
  /**
   * 倍速滑杆草稿：拖动中的本地即时显示（IPC 防抖期间画面跟手）。
   * `null` = 无草稿，显示值以后端状态为准（对账模式与 eq store 一致）。
   */
  rateDraft = $state<number | null>(null);

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
      clearTimeout(this.rateTimer);
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
      // 版本回退丢弃：暂停期间并发发出的旧请求后回来，会把队列覆盖回旧内容
      if (snapshot.version < this.version) return;
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
    // 显式设速（快捷键/复位键）直接作废滑杆草稿：防抖中的旧草稿不能盖住新值
    clearTimeout(this.rateTimer);
    this.rateDraft = null;
    void ipc.player.setRate(value).catch((e) => this.fail('setRate', e));
  }

  /** 防抖句柄（dragRate / setRate 共用） */
  private rateTimer: ReturnType<typeof setTimeout> | undefined;

  /**
   * 倍速滑杆入口：防抖 + 回写对账（与 eq store 的 apply 同一模式）。
   * 本地即时显示（rateDraft），停手 ~300ms 才发 IPC；响应里后端 clamp
   * 出差异、且用户此后没再动过滑杆时才回写显示值。
   */
  dragRate(value: number): void {
    this.rateDraft = value;
    clearTimeout(this.rateTimer);
    this.rateTimer = setTimeout(() => {
      const sent = this.rateDraft;
      if (sent !== null) void this.commitRate(sent);
    }, 300);
  }

  private async commitRate(sent: number): Promise<void> {
    try {
      const applied = await ipc.player.setRate(sent);
      if (applied.rate !== sent && this.rateDraft === sent) this.rateDraft = applied.rate;
    } catch (e) {
      // 下发失败回滚显示值（null = 退回后端状态），日志留痕
      this.rateDraft = null;
      this.fail('setRate', e);
    }
  }

  toggleMute(): void {
    void ipc.player.toggleMute().catch((e) => this.fail('toggleMute', e));
  }

  setShuffle(shuffle: boolean): void {
    void ipc.player.setShuffle(shuffle).catch((e) => this.fail('setShuffle', e));
  }

  /**
   * 播放模式（用户心智里的"一个键"）：列表循环 / 单曲循环 / 随机 / 顺序。
   *
   * 后端没有"模式"这个字段——它是 repeat + shuffle 两个正交开关的组合。
   * UI 侧组合成单键循环（docs/25 §6）：随机 = shuffle:true + repeat:'all'
   * （随机播完一轮还想循环）；顺序 = repeat:'off' + shuffle:false。
   * 两条命令并发下发，状态以 player:state 事件为准。
   */
  setMode(mode: 'all' | 'one' | 'shuffle' | 'sequential'): void {
    const shuffle = mode === 'shuffle';
    const repeat = mode === 'one' ? 'one' : mode === 'sequential' ? 'off' : 'all';
    if (shuffle !== (this.state?.shuffle ?? false)) this.setShuffle(shuffle);
    if (repeat !== (this.state?.repeat ?? 'off')) {
      void ipc.player.setRepeat(repeat).catch((e) => this.fail('setRepeat', e));
    }
  }

  /** 模式单键循环：列表循环 → 单曲循环 → 随机 → 顺序 → 列表循环 */
  cyclePlayMode(): void {
    const s = this.state;
    const current = s?.shuffle
      ? 'shuffle'
      : s?.repeat === 'one'
        ? 'one'
        : s?.repeat === 'all'
          ? 'all'
          : 'sequential';
    const ORDER = ['all', 'one', 'shuffle', 'sequential'] as const;
    const next = ORDER[(ORDER.indexOf(current) + 1) % ORDER.length]!;
    this.setMode(next);
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
