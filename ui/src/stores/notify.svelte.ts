/**
 * 全局提示：错误条 + 轻提示（toast）。
 *
 * 抽出来的理由：批次 3 之后，写错误的地方从「App.svelte 里十几个 catch」
 * 扩散到 4 个 store（列表 / 歌单 / 键盘 / 浮层）。继续用 App 局部状态
 * 就得层层传回调，而这类"给用户看一眼"的东西没有任何视图语义。
 *
 * 通道归属按**报错三档**（2026-11 拍板）：
 * - `error`：**界面错误条**，留在屏幕上直到用户关掉。后端 / 数据域失败
 *   （查询、歌单、扫描、设置写盘、EQ）走这条——失败原因要能贴给人看。
 * - `toast`：**播放条吐司**（挂播放条上方），2.6 秒自动消失。成功反馈、
 *   前置校验提示、以及**不阻断运行**的播放报错（起播失败、单曲解码失败）
 *   走这条。
 * - 阻断级报错（设备丢失、未知引擎故障）→ `overlay.showAlert` 弹窗，
 *   归 overlay 不归这里。
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
