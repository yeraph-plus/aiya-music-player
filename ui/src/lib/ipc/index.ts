/**
 * IPC 契约入口。
 *
 * `generated/` 由 `npm run gen:types` 从 Rust 生成（**不要手改**，含其中的
 * `index.ts` 桶文件）；其余是薄封装：客户端、错误归一化、动态边界的 zod
 * schema、事件订阅。
 *
 * 全部 54 个命令都是 `Result<T, ApiError>`，返回值类型全部来自 Rust。
 * 参数里的 query / settings 仍是动态形状——理由见 docs/17 §3.2。
 */
export * from './generated';

export { ipc, toApiError, isRetryable } from './client';
export type { TrackQueryInput, DynamicJson } from './client';
export { settingsSchema, uiStateSchema, devValidate } from './schema';
export type { Settings, UiState } from './schema';
export { events } from './events';
