/**
 * 错误呈现的唯一映射点（docs/18 §4.5）。
 *
 * 为什么不让组件各自 `e instanceof Error ? e.message : String(e)`：
 * 新契约的 reject 值是 `ApiError` **普通对象**（Tauri 序列化 `Err` 后直接 reject），
 * `instanceof Error` 为假，`String(e)` 会得到 `[object Object]`。
 * 这类"错误处理本身出错"的症状是界面出现一行看不懂的文字，比不显示更难查。
 */
import { toApiError } from './ipc/client';
import type { ApiError } from './ipc/generated/ApiError';

/** 给用户看的一句话。`detail` 是给日志的，别往 toast 里塞。 */
export function errorMessage(e: unknown): string {
  return toApiError(e).message;
}

/** 给开发者看的细节（可能为 null）。 */
export function errorDetail(e: unknown): string | null {
  return toApiError(e).detail;
}

export function asApiError(e: unknown): ApiError {
  return toApiError(e);
}

/**
 * 列表项过期是正常现象（别的窗口删了这首歌）：刷新列表即可，不弹 toast。
 */
export function isNotFound(e: unknown): boolean {
  return toApiError(e).code === 'notFound';
}

/**
 * 正在扫描 / 正在导入时后端会拒绝并发写：提示"稍后自动刷新"，
 * 而不是把 `busy` 当成故障报给用户。
 */
export function isBusy(e: unknown): boolean {
  return toApiError(e).code === 'busy';
}

/** 能不能给一个「重试」按钮，由后端 `retryable` 决定，UI 不自己猜。 */
export function isRetryable(e: unknown): boolean {
  return toApiError(e).retryable;
}
