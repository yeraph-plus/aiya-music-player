import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import {
  fail,
  ok,
  type AiyaApi,
  type AppSettings,
  type ApplyDecision,
  type BeetsImportEvent,
  type BeetsStatus,
  type Diagnostics,
  type HistoryStats,
  type IpcResult,
  type LibraryStats,
  type Lyrics,
  type PlayerCommand,
  type PlayerReport,
  type PlayerState,
  type QueueInsertMode,
  type QueueSnapshot,
  type RevealResult,
  type ScanProgress,
  type ScanResult,
  type SelectionIdsQuery,
  type ThemeMode,
  type ListDensity,
  type Track,
  type TrackQuery,
  type UiState,
} from '@aiya/contracts';
import { MockBackend } from './mock.js';
import { artworkUrl } from './artwork.js';

/**
 * `window.api` 的唯一适配层（docs/07 §2.1）。
 *
 * - Tauri 环境：全部走 invoke/event，命令名与 Rust 侧一一对应（snake_case）；
 * - 浏览器预览：路由到 compat/mock.ts，不写任何 `if` 进 UI 代码。
 *
 * 保留旧 `IpcResult<T>` 信封 —— Rust 命令直接返回同构 JSON，
 * UI 的 `result.ok` 分支零改动。事件订阅按旧约定返回同步 unsubscribe。
 */

const IS_TAURI = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

/* ---------------- Tauri 事件名（与 Rust 侧约定） ---------------- */

const EV_PLAYER_STATE = 'player:state';
const EV_SCAN_PROGRESS = 'scan:progress';

type InvokeFn = (cmd: string, args?: Record<string, unknown>) => Promise<unknown>;

function tauriInvoke(cmd: string, args?: Record<string, unknown>): Promise<unknown> {
  return invoke<unknown>(cmd, args);
}

function onTauriEvent<T>(event: string, cb: (payload: T) => void): () => void {
  let un: UnlistenFn | null = null;
  let closed = false;
  void listen<T>(event, (e) => cb(e.payload))
    .then((u) => {
      if (closed) u();
      else un = u;
    })
    .catch(() => {
      /* 事件通道不可用：订阅静默失败，UI 走降级态 */
    });
  return () => {
    closed = true;
    un?.();
    un = null;
  };
}

/* ---------------- 装配 ---------------- */

export function installApi(): void {
  const mock = IS_TAURI ? null : new MockBackend();

  const call: InvokeFn = mock
    ? (cmd, args) => mockCall(mock, cmd, args)
    : tauriInvoke;

  const onScanProgress = (cb: (progress: ScanProgress) => void): (() => void) =>
    mock ? mock.emitter.on(EV_SCAN_PROGRESS, cb as (p: unknown) => void) : onTauriEvent(EV_SCAN_PROGRESS, cb);

  const onState = (cb: (state: PlayerState) => void): (() => void) =>
    mock ? mock.emitter.on(EV_PLAYER_STATE, cb as (p: unknown) => void) : onTauriEvent(EV_PLAYER_STATE, cb);

  const onBeetsImport = (cb: (evt: BeetsImportEvent) => void): (() => void) =>
    mock ? mock.emitter.on('beets:import', cb as (p: unknown) => void) : onTauriEvent('beets:import', cb);

  const api: AiyaApi = {
    library: {
      list: (query?: TrackQuery) => call('library_list', { query }) as Promise<IpcResult<import('@aiya/contracts').TrackPage>>,
      tracks: (ids: string[]) => call('library_tracks', { ids }) as Promise<IpcResult<Track[]>>,
      stats: () => call('library_stats') as Promise<IpcResult<LibraryStats>>,
      scan: (options?: { path?: string }) => call('library_scan', { options }) as Promise<IpcResult<ScanResult>>,
      cancelScan: () => call('library_scan_cancel') as Promise<IpcResult<{ cancelled: boolean }>>,
      chooseRoot: () => call('library_choose_root') as Promise<IpcResult<{ path: string } | null>>,
      revealTrack: (trackId: string) => call('library_reveal_track', { trackId }) as Promise<IpcResult<RevealResult>>,
      idsForSelection: (request: SelectionIdsQuery) =>
        call('library_ids_for_selection', { request }) as Promise<IpcResult<string[]>>,
      removeTracks: (trackIds: string[]) => call('library_remove_tracks', { trackIds }) as Promise<IpcResult<{ removed: number }>>,
      artworkUrl,
      onScanProgress,
    },
    playlists: {
      list: () => call('playlist_list') as Promise<IpcResult<import('@aiya/contracts').PlaylistSummary[]>>,
      detail: (id: string) => call('playlist_detail', { id }) as Promise<IpcResult<import('@aiya/contracts').PlaylistSummary | null>>,
      create: (name: string, trackIds?: string[]) => call('playlist_create', { name, trackIds }) as Promise<IpcResult<import('@aiya/contracts').PlaylistSummary>>,
      rename: (id: string, name: string) => call('playlist_rename', { id, name }) as Promise<IpcResult<import('@aiya/contracts').PlaylistSummary>>,
      remove: (id: string) => call('playlist_delete', { id }) as Promise<IpcResult<{ deleted: boolean }>>,
      reorder: (from: number, to: number) => call('playlist_reorder', { from, to }) as Promise<IpcResult<import('@aiya/contracts').PlaylistSummary[]>>,
      addTracks: (id: string, trackIds: string[], mode?: 'append' | 'next') =>
        call('playlist_add_tracks', { id, trackIds, mode }) as Promise<IpcResult<import('@aiya/contracts').PlaylistAddResult>>,
      removeAt: (id: string, position: number) => call('playlist_remove_at', { id, position }) as Promise<IpcResult<import('@aiya/contracts').PlaylistSummary>>,
      move: (id: string, from: number, to: number) => call('playlist_move', { id, from, to }) as Promise<IpcResult<import('@aiya/contracts').PlaylistSummary>>,
      clear: (id: string) => call('playlist_clear', { id }) as Promise<IpcResult<import('@aiya/contracts').PlaylistSummary>>,
    },
    lyrics: {
      get: (trackId: string) => call('lyrics_get', { trackId }) as Promise<IpcResult<Lyrics>>,
    },
    history: {
      stats: () => call('history_stats') as Promise<IpcResult<HistoryStats>>,
      clear: () => call('history_clear') as Promise<IpcResult<{ cleared: boolean }>>,
    },
    player: {
      state: () => call('player_state') as Promise<IpcResult<PlayerState>>,
      queue: () => call('player_queue') as Promise<IpcResult<QueueSnapshot>>,
      playTracks: (trackIds: string[], startIndex?: number) =>
        call('player_play_tracks', { trackIds, startIndex }) as Promise<IpcResult<PlayerState>>,
      playQuery: (query: TrackQuery, startIndex?: number) =>
        call('player_play_query', { query, startIndex }) as Promise<IpcResult<PlayerState>>,
      toggle: () => call('player_toggle') as Promise<IpcResult<PlayerState>>,
      next: () => call('player_next') as Promise<IpcResult<PlayerState>>,
      prev: () => call('player_prev') as Promise<IpcResult<PlayerState>>,
      seek: (positionMs: number) => call('player_seek', { positionMs }) as Promise<IpcResult<PlayerState>>,
      setVolume: (volume: number) => call('player_set_volume', { volume }) as Promise<IpcResult<PlayerState>>,
      toggleMute: () => call('player_toggle_mute') as Promise<IpcResult<PlayerState>>,
      setRate: (rate: number) => call('player_set_rate', { rate }) as Promise<IpcResult<PlayerState>>,
      setRepeat: (repeat) => call('player_set_repeat', { repeat }) as Promise<IpcResult<PlayerState>>,
      setShuffle: (shuffle: boolean) => call('player_set_shuffle', { shuffle }) as Promise<IpcResult<PlayerState>>,
      enqueue: (trackIds: string[], options?: QueueInsertMode) =>
        call('player_enqueue', { trackIds, options }) as Promise<IpcResult<PlayerState>>,
      insertNext: (trackId: string) => call('player_insert_next', { trackId }) as Promise<IpcResult<PlayerState>>,
      removeAt: (index: number) => call('player_remove_at', { index }) as Promise<IpcResult<PlayerState>>,
      move: (from: number, to: number) => call('player_move', { from, to }) as Promise<IpcResult<PlayerState>>,
      clear: () => call('player_clear') as Promise<IpcResult<PlayerState>>,
      // 音频在 Rust sidecar 出声（ADR-0006）：渲染层出口不再消费指令
      onCommand: (_cb: (command: PlayerCommand) => void) => () => {
        /* 故意留空，见 docs/07 §2.1 */
      },
      report: (report: PlayerReport): void => {
        void call('player_report', { report });
      },
      onState,
    },
    app: {
      info: () => call('app_info') as Promise<IpcResult<{ version: string; electron: string; platform: string; dataDir: string }>>,
      initialLook: mock ? mock.initialLook : (window.__AIYA_LOOK__ ?? { theme: 'dark' as ThemeMode, density: 'comfortable' as ListDensity }),
      openDataDir: () => call('app_open_data_dir') as Promise<IpcResult<void>>,
      getUiState: () => call('app_get_ui_state') as Promise<IpcResult<UiState>>,
      setUiState: (state: UiState) => call('app_set_ui_state', { state }) as Promise<IpcResult<void>>,
      getSettings: () => call('app_get_settings') as Promise<IpcResult<AppSettings>>,
      setSettings: (settings: AppSettings) => call('app_set_settings', { settings }) as Promise<IpcResult<AppSettings>>,
      diagnostics: () => call('app_diagnostics') as Promise<IpcResult<Diagnostics>>,
      spectrumSet: (enabled: boolean) =>
        call('spectrum_set', { enabled }) as Promise<IpcResult<{ enabled: boolean }>>,
      beetsStatus: () => call('beets_status') as Promise<IpcResult<BeetsStatus>>,
      beetsPreview: (paths: string[]) =>
        call('beets_preview', { paths }) as Promise<IpcResult<{ started: boolean }>>,
      beetsApply: (decisions: ApplyDecision[]) =>
        call('beets_apply', { decisions }) as Promise<IpcResult<{ started: boolean }>>,
      beetsCancel: () => call('beets_cancel') as Promise<IpcResult<{ requested: boolean }>>,
      beetsSync: () => call('beets_sync') as Promise<IpcResult<{ started: boolean }>>,
      onBeetsImport,
    },
  };

  window.api = api;
}

/* ---------------- mock 路由 ---------------- */

function mockCall(mock: MockBackend, cmd: string, args: Record<string, unknown> = {}): Promise<unknown> {
  try {
    return Promise.resolve(ok(mockDispatch(mock, cmd, args)));
  } catch (err) {
    return Promise.resolve(fail('E_INTERNAL', err instanceof Error ? err.message : String(err)));
  }
}

function mockDispatch(mock: MockBackend, cmd: string, a: Record<string, unknown>): unknown {
  switch (cmd) {
    case 'library_list':
      return mock.libraryList(a.query as Parameters<MockBackend['libraryList']>[0]);
    case 'library_tracks':
      return mock.libraryTracks(a.ids as string[]);
    case 'library_stats':
      return mock.libraryStats();
    case 'library_scan':
      return mock.scan();
    case 'library_scan_cancel':
      return { cancelled: false };
    case 'library_choose_root':
      return null;
    case 'library_reveal_track':
      return mock.revealTrack(a.trackId as string);
    case 'library_ids_for_selection':
      return mock.idsForSelection(a.request as SelectionIdsQuery);
    case 'library_remove_tracks':
      return mock.removeTracks(a.trackIds as string[]);
    case 'playlist_list':
      return mock.playlistList();
    case 'playlist_detail':
      return mock.playlistDetail(a.id as string);
    case 'playlist_create':
      return mock.playlistCreate(a.name as string, a.trackIds as string[] | undefined);
    case 'playlist_rename':
      return mock.playlistRename(a.id as string, a.name as string);
    case 'playlist_delete':
      return mock.playlistRemove(a.id as string);
    case 'playlist_reorder':
      return mock.playlistReorder(a.from as number, a.to as number);
    case 'playlist_add_tracks':
      return mock.playlistAddTracks(a.id as string, a.trackIds as string[], a.mode as 'append' | 'next' | undefined);
    case 'playlist_remove_at':
      return mock.playlistRemoveAt(a.id as string, a.position as number);
    case 'playlist_move':
      return mock.playlistMove(a.id as string, a.from as number, a.to as number);
    case 'playlist_clear':
      return mock.playlistClear(a.id as string);
    case 'lyrics_get':
      return mock.lyricsGet(a.trackId as string);
    case 'history_stats':
      return mock.historyStats();
    case 'history_clear':
      return mock.historyClear();
    case 'player_state':
      return mock.playerStateNow();
    case 'player_queue':
      return mock.playerQueue();
    case 'player_play_tracks':
      return mock.playTracks(a.trackIds as string[], (a.startIndex as number) ?? 0);
    case 'player_play_query':
      return mock.playQuery(a.query as TrackQuery, (a.startIndex as number) ?? 0);
    case 'player_toggle':
      return mock.toggle();
    case 'player_next':
      return mock.next();
    case 'player_prev':
      return mock.prev();
    case 'player_seek':
      return mock.seek(a.positionMs as number);
    case 'player_set_volume':
      return mock.setVolume(a.volume as number);
    case 'player_toggle_mute':
      return mock.toggleMute();
    case 'player_set_rate':
      return mock.setRate(a.rate as number);
    case 'player_set_repeat':
      return mock.setRepeat(a.repeat as PlayerState['repeat']);
    case 'player_set_shuffle':
      return mock.setShuffle(a.shuffle as boolean);
    case 'player_enqueue':
      return mock.enqueue(a.trackIds as string[], a.options as QueueInsertMode | undefined);
    case 'player_insert_next':
      return mock.insertNext(a.trackId as string);
    case 'player_remove_at':
      return mock.removeAt(a.index as number);
    case 'player_move':
      return mock.move(a.from as number, a.to as number);
    case 'player_clear':
      return mock.clear();
    case 'player_report':
      mock.report(a.report as PlayerReport);
      return null;
    case 'app_info':
      return mock.appInfo();
    case 'app_open_data_dir':
      return undefined;
    case 'app_get_ui_state':
      return mock.getUiState();
    case 'app_set_ui_state':
      mock.setUiState(a.state as UiState);
      return undefined;
    case 'app_get_settings':
      return mock.getSettings();
    case 'app_set_settings':
      return mock.setSettings(a.settings as AppSettings);
    case 'app_diagnostics':
      return mock.diagnostics();
    case 'spectrum_set':
      return ok({ enabled: a.enabled as boolean });
    case 'beets_preview':
      return ok({ started: true });
    case 'beets_status':
      return ok({
        found: true,
        interpreter: 'mock-python',
        scriptPath: 'mock://beets_sidecar.py',
        protocolVersion: 1,
        beetsVersion: '2.14.1',
        pythonVersion: '3.14.0',
        libraryPath: 'C:/mock/beets/library.db',
        libraryExists: true,
        plugins: ['fetchart', 'lyrics'],
        configDir: 'C:/mock/beets',
        errorCode: null,
        errorMessage: null,
      });
    default:
      throw new Error(`mock 未实现的命令：${cmd}`);
  }
}
