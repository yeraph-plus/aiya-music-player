/**
 * 事件订阅（docs/17 §4）：**组件不直接 listen**，只在 store 层订阅。
 *
 * 载荷类型全部来自 Rust（`generated/`），除了两条动态增量事件：
 * `scan:progress` 与 `beets:import` —— 它们的形状会演化，走 zod（后续页面接
 * 入时补 schema），这里先给 `DynamicJson` 让调用方显式处理。
 *
 * 实际通道来自 `transport()`：Tauri 用真 `listen`，浏览器预览用 mock 的
 * 内存事件总线（`spectrum:frame` / `player:state` 都能跑）。
 */
import type { UnlistenFn } from '@tauri-apps/api/event';
import type { AudioModeEvent } from './generated/AudioModeEvent';
import type { PlayerState } from './generated/PlayerState';
import type { DynamicJson } from './client';
import { transport } from './transport';

export type { AudioModeEvent };

/** 订阅辅助：统一 catch（事件通道不可用时静默降级，UI 走空态），返回同步 unsubscribe。 */
function sub<T>(event: string, cb: (payload: T) => void): () => void {
  let un: UnlistenFn | null = null;
  let closed = false;
  void transport()
    .listen<T>(event, (payload) => cb(payload))
    .then((u) => {
      if (closed) u();
      else un = u;
    })
    .catch(() => {
      /* 订阅失败：保持静默，调用方按"没有数据"处理 */
    });
  return () => {
    closed = true;
    un?.();
    un = null;
  };
}

export const events = {
  /** 播放状态快照，~10Hz。**高频**：订阅方必须合帧，不要每次事件都触发重渲染。 */
  playerState: (cb: (s: PlayerState) => void) => sub<PlayerState>('player:state', cb),

  /** 频谱 48 带（0-255），30fps。只有面板打开时才有数据（`ipc.spectrumSet(true)` 才订阅）。 */
  spectrumFrame: (cb: (bands: number[]) => void) => sub<number[]>('spectrum:frame', cb),

  /** 扫描进度，≤10Hz（宿主侧 100ms 节流）。形状随扫描器演化，动态处理。 */
  scanProgress: (cb: (p: DynamicJson) => void) => sub<DynamicJson>('scan:progress', cb),

  /** beets 导入：union 多态（progress / sessionEnd / librarySync*），终态不可丢。 */
  beetsImport: (cb: (e: DynamicJson) => void) => sub<DynamicJson>('beets:import', cb),

  /**
   * beets 运行时安装进度（docs/29 批 C）：progress / error / done / finished。
   * `finished` 带 `status`（与 `beets_status` 同形），UI 直接用它刷新状态块。
   */
  beetsInstall: (cb: (e: DynamicJson) => void) => sub<DynamicJson>('beets:install', cb),

  /** 音频实际输出状态（模式/时钟/缓冲/格式）。偶发，不可丢。 */
  audioMode: (cb: (e: AudioModeEvent) => void) => sub<AudioModeEvent>('audio:mode', cb),
};
