/**
 * IPC 契约入口。
 *
 * `generated/` 由 `npm run gen:types` 从 Rust 生成（**不要手改**），
 * 其余是薄封装：客户端、错误归一化、动态边界的 zod schema、事件订阅。
 */
export * from './generated/ApiError';
export * from './generated/ApiErrorCode';
export * from './generated/AppInfo';
export * from './generated/AudioError';
export * from './generated/AudioErrorCode';
export * from './generated/AudioModeEvent';
export * from './generated/AudioModeInfo';
export * from './generated/DesiredOutput';
export * from './generated/Diagnostics';
export * from './generated/LibraryStats';
export * from './generated/MediaKeysInfo';
export * from './generated/PlaybackStatus';
export * from './generated/PlayerState';
export * from './generated/PowerInfo';
export * from './generated/QueueSnapshot';
export * from './generated/RepeatMode';
export * from './generated/SpectrumToggle';
export * from './generated/StreamsInfo';
export * from './generated/TrackAudio';
export * from './generated/TrackDetail';
export * from './generated/TrackPage';
export * from './generated/TrackRef';
export * from './generated/TrackRow';

export { ipc, toApiError, isRetryable } from './client';
export type { TrackQueryInput, DynamicJson } from './client';
export { settingsSchema, uiStateSchema, devValidate } from './schema';
export type { Settings, UiState } from './schema';
export { events } from './events';
export type { AudioModeEvent } from './events';
