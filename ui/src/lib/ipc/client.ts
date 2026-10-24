/**
 * 新 IPC 客户端（docs/17）：**类型来自 Rust**，错误是结构化对象。
 *
 * 与老 `compat/api.ts` 的区别：
 * - 老壳：`invoke` 后返回 `{ok,data}` 信封，错误在返回值里；
 * - 新壳：命令返回 `Result<T, ApiError>`，失败时 Tauri 直接 **reject**，
 *   这里把 reject 值归一化成 `ApiError`（它不是 `Error` 实例，没有 stack）。
 *
 * 只覆盖 B1（首屏必需）的命令；其余命令迁移到哪条就加哪条（docs/17 §5）。
 * 老 UI 仍走 `compat/api.ts`，两者共存到旧壳删除为止。
 */
import { invoke } from '@tauri-apps/api/core';
import type { ApiError } from './generated/ApiError';
import type { ApiErrorCode } from './generated/ApiErrorCode';
import type { AppInfo } from './generated/AppInfo';
import type { Diagnostics } from './generated/Diagnostics';
import type { LibraryStats } from './generated/LibraryStats';
import type { PlayerState } from './generated/PlayerState';
import type { QueueSnapshot } from './generated/QueueSnapshot';
import type { SpectrumToggle } from './generated/SpectrumToggle';
import type { TrackDetail } from './generated/TrackDetail';
import type { TrackPage } from './generated/TrackPage';

export type { ApiError, ApiErrorCode };

/** 命令参数里的 TrackQuery 形状由 Rust `TrackWindowQuery` 决定（B1 仍是动态 Value）。 */
export type TrackQueryInput = {
  sort?: 'title' | 'added' | 'artist' | 'album';
  direction?: 'asc' | 'desc';
  search?: string | null;
  artistId?: string | null;
  albumId?: string | null;
  source?: { kind: 'library' } | { kind: 'playlist'; playlistId: string } | { kind: 'recent' } | { kind: 'mostPlayed' };
  mode?: 'cursor' | 'offset';
  cursor?: string | null;
  offset?: number | null;
  limit?: number;
};

/** 没有固定形状的载荷（设置/UI 状态）：由 zod 兜住，见 `schema.ts`。 */
export type DynamicJson = Record<string, unknown>;

function isRecord(v: unknown): v is Record<string, unknown> {
  return typeof v === 'object' && v !== null;
}

/**
 * 把 Tauri 的 reject 值归一化成 `ApiError`。
 *
 * Tauri 2 把 `Err(ApiError)` 序列化后直接作为 Promise 的 reject 值，
 * 所以 `e instanceof Error` 为假、`e.message` 可能是 undefined。
 * 任何非契约形状（管道断了、命令不存在）都归到 `internal`。
 */
export function toApiError(e: unknown): ApiError {
  if (isRecord(e) && typeof e.code === 'string' && typeof e.message === 'string') {
    return {
      code: e.code as ApiErrorCode,
      message: e.message,
      detail: typeof e.detail === 'string' ? e.detail : null,
      retryable: e.retryable === true,
    };
  }
  const text = e instanceof Error ? e.message : typeof e === 'string' ? e : JSON.stringify(e ?? null);
  return { code: 'internal', message: '调用失败', detail: text, retryable: false };
}

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return await invoke<T>(cmd, args);
  } catch (e) {
    throw toApiError(e);
  }
}

export const ipc = {
  /* ---- app ---- */
  appInfo: () => call<AppInfo>('app_info'),
  diagnostics: () => call<Diagnostics>('app_diagnostics'),
  /** 动态形状：设置项会演化，调用方用 `settingsSchema` 校验 */
  getSettings: () => call<DynamicJson>('app_get_settings'),
  setSettings: (settings: DynamicJson) => call<DynamicJson>('app_set_settings', { settings }),
  getUiState: () => call<DynamicJson>('app_get_ui_state'),
  setUiState: (state: DynamicJson) => call<void>('app_set_ui_state', { state }),

  /* ---- library ---- */
  libraryList: (query: TrackQueryInput) => call<TrackPage>('library_list', { query }),
  libraryTracks: (ids: string[]) => call<TrackDetail[]>('library_tracks', { ids }),
  libraryStats: () => call<LibraryStats>('library_stats'),

  /* ---- player ---- */
  playerState: () => call<PlayerState>('player_state'),
  playerQueue: () => call<QueueSnapshot>('player_queue'),

  /* ---- 频谱 tap（面板打开才算订阅） ---- */
  spectrumSet: (enabled: boolean) => call<SpectrumToggle>('spectrum_set', { enabled }),
};

/** UI 处理约定（docs/17 §2.5）：只在这里判"能不能重试"，组件不重复判断。 */
export function isRetryable(e: unknown): boolean {
  return toApiError(e).retryable;
}
