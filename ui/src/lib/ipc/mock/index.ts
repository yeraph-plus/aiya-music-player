/**
 * 浏览器预览的装配点：把 `MockBackend` 接成一个 `Transport`。
 *
 * 只在**没有 Tauri 运行时**加载（`main.ts` 里动态 import），所以：
 * - Tauri 包里这段代码是独立 chunk，永远不执行；
 * - UI 代码里没有 `if (preview)` 分支，`client.ts` / `events.ts` 完全不知道
 *   自己头顶上是真宿主还是一坨假数据。
 *
 * 派发表是**一对一**的：命令名与 Rust 侧 snake_case 完全一致（docs/17 §3），
 * 漏一条就在预览里表现为"这个按钮报 internal"——比默默返回 undefined 好排查。
 */
import type { ApplyDecision, DynamicJson, QueueInsertMode, ScanOptions, SelectionIdsQuery, TrackQueryInput } from '../client';
import type { AppSettings } from '../../settings';
import type { ApiError, RepeatMode } from '../generated';
import type { UiState } from '../schema';
import type { Transport } from '../transport';
import { setTransport } from '../transport';
import { MockBackend } from './backend';

/** 契约形状的失败：`toApiError` 会原样认出来，走正常的错误条渲染 */
function mockError(cmd: string): ApiError {
  return {
    code: 'unsupported',
    message: `预览模式未实现该命令：${cmd}`,
    detail: null,
    retryable: false,
  };
}

/**
 * 命令派发。参数名与 Rust 侧的命令签名一致（`trackId` / `trackIds` /
 * `positionMs`…），因为 `client.ts` 就是按那个名字传的。
 */
function dispatch(mock: MockBackend, cmd: string, a: Record<string, unknown>): unknown {
  switch (cmd) {
    /* ---------------- library ---------------- */
    case 'library_list':
      return mock.libraryList(a.query as TrackQueryInput | undefined);
    case 'library_tracks':
      return mock.libraryTracks(a.ids as string[]);
    case 'library_stats':
      return mock.libraryStats();
    case 'library_scan':
      void (a.options as ScanOptions | undefined);
      return mock.scan();
    case 'library_scan_cancel':
      return { cancelled: false };
    // 浏览器里没有系统目录选择器：返回 null = 用户取消（契约里的正常分支）
    case 'library_choose_root':
      return null;
    case 'library_reveal_track':
      return mock.revealTrack(a.trackId as string);
    case 'library_ids_for_selection':
      return mock.idsForSelection(a.request as SelectionIdsQuery);
    case 'library_remove_tracks':
      return mock.removeTracks(a.trackIds as string[]);
    case 'library_albums':
      return mock.libraryAlbums(a.search as string | undefined);
    case 'library_artists':
      return mock.libraryArtists(a.search as string | undefined);

    /* ---------------- playlist ---------------- */
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
      return mock.playlistAddTracks(
        a.id as string,
        a.trackIds as string[],
        a.mode as 'append' | 'next' | undefined,
      );
    case 'playlist_remove_at':
      return mock.playlistRemoveAt(a.id as string, a.position as number);
    case 'playlist_move':
      return mock.playlistMove(a.id as string, a.from as number, a.to as number);
    case 'playlist_clear':
      return mock.playlistClear(a.id as string);
    case 'playlist_import':
      return mock.playlistImport(a.path as string | undefined);
    case 'playlist_export':
      return mock.playlistExport(a.id as string, a.path as string);

    /* ---------------- player ---------------- */
    case 'player_state':
      return mock.playerStateNow();
    case 'player_queue':
      return mock.playerQueue();
    case 'player_play_tracks':
      return mock.playTracks(a.trackIds as string[], (a.startIndex as number | undefined) ?? 0);
    case 'player_play_query':
      return mock.playQuery(
        a.query as TrackQueryInput,
        (a.startIndex as number | undefined) ?? 0,
      );
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
      return mock.setRepeat(a.repeat as RepeatMode);
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

    /* ---------------- lyrics / history ---------------- */
    case 'lyrics_get':
      return mock.lyricsGet(a.trackId as string);
    case 'history_stats':
      return mock.historyStats();
    case 'history_clear':
      return mock.historyClear();

    /* ---------------- app ---------------- */
    case 'app_info':
      return mock.appInfo();
    case 'app_diagnostics':
      return mock.diagnostics();
    case 'app_get_settings':
      return mock.getSettings();
    case 'app_set_settings':
      // 客户端按 `DynamicJson` 传（设置项形状会演化），mock 存的是合并后的完整对象
      return mock.setSettings(a.settings as AppSettings);
    case 'app_get_ui_state':
      return mock.getUiState();
    case 'app_set_ui_state':
      mock.setUiState(a.state as UiState);
      return undefined;
    // 浏览器里没有"数据目录"可开：契约里这个命令也不报错（打不开就算成功）
    case 'app_open_data_dir':
      return undefined;
    case 'spectrum_set':
      return mock.setSpectrum(a.enabled as boolean);

    /* ---------------- beets ---------------- */
    case 'beets_status':
      return {
        found: false,
        interpreter: null,
        scriptPath: null,
        protocolVersion: null,
        beetsVersion: null,
        pythonVersion: null,
        libraryPath: null,
        libraryExists: null,
        plugins: null,
        configDir: null,
        errorCode: 'preview',
        errorMessage: '浏览器预览不含 beets sidecar（导入面板只看空态）',
      };
    case 'beets_preview':
    case 'beets_apply':
    case 'beets_sync':
      void (a.decisions as ApplyDecision[] | undefined);
      return { started: true };
    case 'beets_cancel':
      return { requested: false };

    default:
      throw mockError(cmd);
  }
}

/**
 * 安装预览后端。返回卸载函数（预览下没人调它，留给将来切环境用）。
 *
 * 事件侧：`listen` 直接挂 `Emitter.on`，subscribe 立刻 resolve —— 与真
 * `listen` 的异步语义一致，`events.ts` 的 sub 不用改。
 */
export function installMock(options: { failCommands?: string[] } = {}): () => void {
  const mock = new MockBackend();
  const failing = new Set(options.failCommands ?? []);

  const transport: Transport = {
    invoke: async <T,>(cmd: string, args?: Record<string, unknown>): Promise<T> => {
      if (failing.has(cmd)) {
        // 显式故障注入：用来手验错误条渲染（query param `?failCmd=library_list`）
        throw {
          code: 'io',
          message: '预览模式：该命令被故意设为失败',
          detail: `cmd=${cmd}`,
          retryable: true,
        } satisfies ApiError;
      }
      return dispatch(mock, cmd, args ?? {}) as T;
    },
    listen: <T,>(event: string, cb: (payload: T) => void): Promise<() => void> => {
      const off = mock.emitter.on(event, (payload) => cb(payload as T));
      return Promise.resolve(off);
    },
  };

  setTransport(transport);

  // 默认主题/密度：真宿主由 initialization_script 注入，预览没有，这里补上
  window.__AIYA_LOOK__ = mock.initialLook;

  return () => setTransport(null);
}
