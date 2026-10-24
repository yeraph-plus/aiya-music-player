import type { PlaybackStatus, RepeatMode, Track } from './domain.js';

/**
 * 播放状态由主进程独占：渲染层只是"扬声器"。
 *
 * 这样做的代价是每次操作多一次 IPC 往返，换来的是：
 * - 队列只有一个真相，多窗口 / 托盘 / Media Session 不会各说各话
 * - 连点 20 次下一首时，顺序由主进程串行决定，不存在跨进程竞态
 * - 状态落库与播放状态在同一事务边界内，重启恢复不会错位
 */
/**
 * 播放失败的分类（与 Rust `aiya_player::AudioErrorCode` 一致）。
 *
 * v1 这里是一句话（`error: string`），UI 无法判断"文件没了（跳过就行）"
 * 和"声卡拔了（必须提示）"，只能人肉看文案。
 */
export type AudioErrorCode =
  | 'fileMissing'
  | 'io'
  | 'decode'
  | 'deviceLost'
  | 'unsupportedFormat'
  | 'unknown';

export interface AudioError {
  code: AudioErrorCode;
  message: string;
}

export interface PlayerState {
  /**
   * 状态里**不放**队列本身：5 万首的 ID 数组在每秒一次的时间回报里
   * 会被重复序列化上百次。队列单独用 `player.queue()` 按版本号拉取。
   */
  queueLength: number;
  /** 队列结构的版本号，变化时渲染层重新拉一次队列 */
  queueVersion: number;
  /** 当前曲目在 queue 中的下标，-1 表示空队列 */
  index: number;
  /** 当前曲目在"播放顺序"里的位置（随机模式下与 index 不同） */
  cursor: number;
  repeat: RepeatMode;
  shuffle: boolean;
  status: PlaybackStatus;
  positionMs: number;
  durationMs: number | null;
  volume: number;
  muted: boolean;
  /**
   * 播放速率（0.5–2.0，0.1 网格；1 为原速）。
   *
   * 放在状态而不是曲目上：它是"这台设备这次的播放方式"，
   * 不是曲目属性 —— 进 `Track` 会让列表缓存与广播都背上它。
   */
  rate: number;
  /** 播放失败（结构化：code 决定 UI 该跳过还是提示）；正常为 null */
  error: AudioError | null;
  /** 当前曲目的展示字段快照，避免渲染层为了显示一首歌再查一次列表 */
  current: Track | null;
  /** 装载令牌：渲染层回传时带上，主进程据此丢弃过期回报 */
  token: number;
  /** 快照生成时间，毫秒 */
  updatedAtMs: number;
}

/**
 * 主进程 → 音频出口的播放指令。
 *
 * `load` 只给 `trackId`，**有意不带 URL**：怎么把一首歌变成声音（`media://`、
 * 本地文件、WASAPI + FFmpeg）是出口自己的事。把 `media://` 写进指令，等于让
 * 唯一的状态机知道当前用哪种传输，换引擎时连协议和测试都要跟着改。
 *
 * `load` 是唯一带 token 的指令：快速切歌时出口可能收到多个 load，
 * 只有最后一个的 token 与当前装载的一致，其余回报会被主进程忽略。
 */
export type PlayerCommand =
  | {
      kind: 'load';
      token: number;
      trackId: string;
      /** 起始位置（恢复上次进度时非 0） */
      startMs: number;
      autoplay: boolean;
    }
  | { kind: 'play'; token: number }
  | { kind: 'pause'; token: number }
  | { kind: 'seek'; token: number; ms: number }
  | { kind: 'volume'; volume: number; muted: boolean }
  | {
      // 不带 token，和 volume 同级：速率作用于出口本身而不是某一次装载。
      // Chromium 里改速率只发 ratechange 事件（实测），出口不会把它误报成
      // 暂停/跳转，所以不存在"过期指令污染状态"的问题。
      kind: 'rate';
      rate: number;
    }
  | { kind: 'stop' };

/** 渲染层 → 主进程的回报，fire-and-forget，不等待结果 */
export type PlayerReport =
  | { kind: 'started'; token: number }
  | { kind: 'paused'; token: number; positionMs: number }
  | { kind: 'ended'; token: number }
  | { kind: 'time'; token: number; positionMs: number }
  | { kind: 'duration'; token: number; durationMs: number }
  | { kind: 'error'; token: number; message: string };

/** 落库用的最小状态，不含 current / status / token */
export interface PersistedPlayer {
  trackIds: string[];
  index: number;
  repeat: RepeatMode;
  shuffle: boolean;
  positionMs: number;
  volume: number;
  muted: boolean;
  /** 与 volume 同构：重启后恢复上次的速率 */
  rate: number;
}

/** 队列内容快照，按版本号按需拉取 */
export interface QueueSnapshot {
  ids: string[];
  index: number;
  version: number;
}

export interface RevealResult {
  revealed: boolean;
  reason?: string;
}
