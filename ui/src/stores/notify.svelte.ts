/**
 * 全局提示：错误条 + 轻提示（toast）。
 *
 * 抽出来的理由：批次 3 之后，写错误的地方从「App.svelte 里十几个 catch」
 * 扩散到 4 个 store（列表 / 歌单 / 键盘 / 浮层）。继续用 App 局部状态
 * 就得层层传回调，而这类"给用户看一眼"的东西没有任何视图语义。
 *
 * 两条通道的语义不同，别合并：
 * - `error`：**留在屏幕上**直到用户关掉。用户显式动作失败（删除/移动/播放）走这条。
 * - `toast`：2.6 秒自动消失。成功反馈走这条——成功不需要用户确认。
 */
class NotifyStore {
  error = $state<string | null>(null);
  toast = $state<string | null>(null);

  private toastTimer: ReturnType<typeof setTimeout> | undefined;

  /** 清空错误条（下次 reload 会调；用户点「关闭」也调） */
  clearError(): void {
    this.error = null;
  }

  /** 显示错误条。传 `ApiError` 的 message 或任意文案 */
  showError(message: string): void {
    this.error = message;
  }

  showToast(message: string, ms = 2600): void {
    this.toast = message;
    clearTimeout(this.toastTimer);
    this.toastTimer = setTimeout(() => (this.toast = null), ms);
  }

  dispose(): void {
    clearTimeout(this.toastTimer);
  }
}

export const notify = new NotifyStore();
