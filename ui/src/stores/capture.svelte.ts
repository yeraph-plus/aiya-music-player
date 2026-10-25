import type { ShortcutAction } from '../lib/shortcuts';

/**
 * 快捷键录制状态。
 *
 * 为什么必须是个 store 而不是组件里的局部 `$state`：录制控件挂在
 * `SettingsPanel` 的 `<svelte:window onkeydown>` 上，全局快捷键挂在
 * `App` 的 `<svelte:window onkeydown>` 上 —— **两个 listener 在同一个
 * `window` 上，冒泡阶段按注册顺序依次触发**（App 先挂载，先跑）。
 * 录制控件里 `event.stopPropagation()` 只能拦住"更外层祖先"的冒泡，
 * 拦不住同节点上的另一个 listener，所以按空格既录进了绑定、又触发了播放。
 *
 * 唯一可靠的修法：让全局那一侧**自己知道**正在录制。状态放这里，
 * 两个组件都不持有副本，也就不会出现"面板卸了标志还留着"的悬挂态
 * （`stop()` 由 `onDestroy` 兜底）。
 */
class CaptureStore {
  /** 正在录制哪个动作；`null` = 没在录制 */
  action = $state<ShortcutAction | null>(null);

  get active(): boolean {
    return this.action !== null;
  }

  start(action: ShortcutAction): void {
    this.action = action;
  }

  stop(): void {
    this.action = null;
  }
}

export const capture = new CaptureStore();
