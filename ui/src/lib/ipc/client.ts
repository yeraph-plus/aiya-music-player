/**
 * 新 IPC 客户端（docs/17）：**类型来自 Rust**，错误是结构化对象。
 *
 * 与老 `compat/api.ts` 的区别：
 * - 老壳：`invoke` 后返回 `{ok,data}` 信封，错误在返回值里；
 * - 新壳：命令返回 `Result<T, ApiError>`，失败时 Tauri 直接 **reject**，
 *   这里把 reject 值归一化成 `ApiError`（它不是 `Error` 实例，没有 stack）。
 *
 * 54 个命令全部覆盖。老 UI 仍走 `compat/api.ts`，两者共存到旧壳删除为止。
 *
 * 参数侧的约定（docs/17 §3.2）：`query` / `settings` / `options` / `decisions`
 * 这类复合对象在 Rust 侧收 `Json` 再手动解析，所以这里用结构类型而不是
 * 生成的 DTO —— 生成物只覆盖**返回值**。原因：参数反序列化失败发生在命令体
 * 之前，用强类型参数会让"参数不合法"变成 Tauri 自己的一句话 reject，
 * 绕开 `ApiError` 的分类。
 */
import { invoke } from '@tauri-apps/api/core';
import type {
  ApiError,
  ApiErrorCode,
  AppInfo,
  BeetsStatus,
  CancelRequested,
  ChosenRoot,
  Diagnostics,
  HistoryCleared,
  HistoryStats,
  JobStarted,
  LibraryStats,
  Lyrics,
  PlayerState,
  PlaylistAddResult,
  PlaylistDeleted,
  PlaylistSummary,
  QueueSnapshot,
  RemoveTracks,
  RepeatMode,
  ReportAck,
  RevealResult,
  ScanCancel,
  ScanResult,
  SpectrumToggle,
  TrackDetail,
  TrackPage,
} from './generated';

export type { ApiError, ApiErrorCode };

/**
 * 命令参数里的 TrackQuery 形状（Rust `TrackWindowQuery`，B1 起一直是动态 Value）。
 * 与 `@aiya/contracts` 的 `TrackQuery` 同构；新 UI 直接用它，不再引旧契约包。
 */
export type TrackQueryInput = {
  sort?: 'title' | 'added' | 'artist' | 'album';
  direction?: 'asc' | 'desc';
  search?: string | null;
  artistId?: string | null;
  albumId?: string | null;
  source?:
    | { kind: 'library' }
    | { kind: 'playlist'; playlistId: string }
    | { kind: 'recent' }
    | { kind: 'mostPlayed' };
  mode?: 'cursor' | 'offset';
  cursor?: string | null;
  offset?: number | null;
  limit?: number;
};

/** 批量选择请求（`library.idsForSelection`）。 */
export type SelectionIdsQuery = {
  query: TrackQueryInput;
  /** `'all'` = 当前视图全部；否则为 0 基下标 */
  indices: number[] | 'all';
};

/** 队列插入位置：`next` 插到当前曲之后，`end` 追加到队尾。 */
export type QueueInsertMode = { mode: 'next' | 'end' };

/** 扫描选项：不传 path 时后端按 settings.library.root → 系统 Music 目录取。 */
export type ScanOptions = { path?: string };

/**
 * beets 应用决策。`candidate` 是预览结果**原样回传**的深层对象——
 * 宿主不解读它（sidecar 的事），所以这里是 `unknown` 而不是具体类型。
 */
export type ApplyDecision = {
  album: string;
  action: 'apply' | 'apply_move' | 'apply_copy' | 'skip';
  paths: string[];
  candidate?: unknown;
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
  app: {
    info: () => call<AppInfo>('app_info'),
    diagnostics: () => call<Diagnostics>('app_diagnostics'),
    /** 动态形状：设置项会演化，调用方用 `settingsSchema` 校验 */
    getSettings: () => call<DynamicJson>('app_get_settings'),
    setSettings: (settings: DynamicJson) => call<DynamicJson>('app_set_settings', { settings }),
    getUiState: () => call<DynamicJson>('app_get_ui_state'),
    setUiState: (state: DynamicJson) => call<void>('app_set_ui_state', { state }),
    /** 资源管理器打开数据目录；打不开也算成功（不弹错误框） */
    openDataDir: () => call<void>('app_open_data_dir'),
    /** 频谱 tap 订阅开关：面板打开才 true，关闭时引擎内零开销 */
    spectrumSet: (enabled: boolean) => call<SpectrumToggle>('spectrum_set', { enabled }),
  },

  library: {
    list: (query: TrackQueryInput) => call<TrackPage>('library_list', { query }),
    /** 按 ID 批量取（含音频规格）；返回顺序与入参一致，缺失的跳过 */
    tracks: (ids: string[]) => call<TrackDetail[]>('library_tracks', { ids }),
    stats: () => call<LibraryStats>('library_stats'),
    /** 阻塞直到扫描结束（进度走 `scan:progress`）；并发调用得 `busy` */
    scan: (options?: ScanOptions) => call<ScanResult>('library_scan', { options }),
    scanCancel: () => call<ScanCancel>('library_scan_cancel'),
    /** 打开系统目录选择器；用户取消返回 null */
    chooseRoot: () => call<ChosenRoot | null>('library_choose_root'),
    /** 在文件管理器中定位；`revealed=false` 时看 `reason`，不是错误 */
    revealTrack: (trackId: string) => call<RevealResult>('library_reveal_track', { trackId }),
    /** 批量选择 → ID（全选 5 万首也只回一数组，UI 不必持有整库） */
    idsForSelection: (request: SelectionIdsQuery) =>
      call<string[]>('library_ids_for_selection', { request }),
    /** 从曲库移除（只删索引，不动磁盘文件） */
    removeTracks: (trackIds: string[]) => call<RemoveTracks>('library_remove_tracks', { trackIds }),
  },

  playlist: {
    list: () => call<PlaylistSummary[]>('playlist_list'),
    /** 不存在返回 null（详情页空态），不是错误 */
    detail: (id: string) => call<PlaylistSummary | null>('playlist_detail', { id }),
    create: (name: string, trackIds?: string[]) =>
      call<PlaylistSummary>('playlist_create', { name, trackIds }),
    rename: (id: string, name: string) => call<PlaylistSummary>('playlist_rename', { id, name }),
    remove: (id: string) => call<PlaylistDeleted>('playlist_delete', { id }),
    /** 侧栏拖动排序；返回排好序的完整列表（省一次 roundtrip） */
    reorder: (from: number, to: number) => call<PlaylistSummary[]>('playlist_reorder', { from, to }),
    addTracks: (id: string, trackIds: string[], mode?: 'append' | 'next') =>
      call<PlaylistAddResult>('playlist_add_tracks', { id, trackIds, mode }),
    removeAt: (id: string, position: number) =>
      call<PlaylistSummary>('playlist_remove_at', { id, position }),
    move: (id: string, from: number, to: number) =>
      call<PlaylistSummary>('playlist_move', { id, from, to }),
    clear: (id: string) => call<PlaylistSummary>('playlist_clear', { id }),
  },

  player: {
    state: () => call<PlayerState>('player_state'),
    /** 队列按版本号单独拉；5 万首的 ID 数组不该挤进每秒一次的状态广播 */
    queue: () => call<QueueSnapshot>('player_queue'),
    playTracks: (trackIds: string[], startIndex?: number) =>
      call<PlayerState>('player_play_tracks', { trackIds, startIndex }),
    /** 用"当前视图的查询"建队列：整库顺序必须由后端按同一份排序生成 */
    playQuery: (query: TrackQueryInput, startIndex?: number) =>
      call<PlayerState>('player_play_query', { query, startIndex }),
    toggle: () => call<PlayerState>('player_toggle'),
    next: () => call<PlayerState>('player_next'),
    prev: () => call<PlayerState>('player_prev'),
    seek: (positionMs: number) => call<PlayerState>('player_seek', { positionMs }),
    setVolume: (volume: number) => call<PlayerState>('player_set_volume', { volume }),
    toggleMute: () => call<PlayerState>('player_toggle_mute'),
    setRate: (rate: number) => call<PlayerState>('player_set_rate', { rate }),
    /** `cycle` = 顺序 → 列表循环 → 单曲循环 轮转（后端算下一档） */
    setRepeat: (repeat: RepeatMode | 'cycle') =>
      call<PlayerState>('player_set_repeat', { repeat }),
    setShuffle: (shuffle: boolean) => call<PlayerState>('player_set_shuffle', { shuffle }),
    enqueue: (trackIds: string[], options?: QueueInsertMode) =>
      call<PlayerState>('player_enqueue', { trackIds, options }),
    insertNext: (trackId: string) => call<PlayerState>('player_insert_next', { trackId }),
    removeAt: (index: number) => call<PlayerState>('player_remove_at', { index }),
    move: (from: number, to: number) => call<PlayerState>('player_move', { from, to }),
    clear: () => call<PlayerState>('player_clear'),
    /** v1 旧回报通道（`<audio>` 时代）；引擎在 Rust 后只剩诊断价值 */
    report: (report: DynamicJson) => call<ReportAck>('player_report', { report }),
  },

  lyrics: {
    /** 没找到回 `found=false`（UI 显示"暂无"），不当错误 */
    get: (trackId: string) => call<Lyrics>('lyrics_get', { trackId }),
  },

  history: {
    stats: () => call<HistoryStats>('history_stats'),
    clear: () => call<HistoryCleared>('history_clear'),
  },

  beets: {
    /** 探测最多 ~30s（逐个候选解释器）；设置页打开时调一次即可 */
    status: () => call<BeetsStatus>('beets_status'),
    /** 只读预览：进度走 `beets:import`；没装 beets 得 `beetsMissing` */
    preview: (paths: string[]) => call<JobStarted>('beets_preview', { paths }),
    apply: (decisions: ApplyDecision[]) => call<JobStarted>('beets_apply', { decisions }),
    cancel: () => call<CancelRequested>('beets_cancel'),
    /** 手动触发 beets → 自有读模型全量对齐；汇总走 `librarySynced` 事件 */
    sync: () => call<JobStarted>('beets_sync'),
  },
};

/** UI 处理约定（docs/17 §2.5）：只在这里判"能不能重试"，组件不重复判断。 */
export function isRetryable(e: unknown): boolean {
  return toApiError(e).retryable;
}
