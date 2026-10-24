import type { AppSettings, BeetsStatus, Lyrics, PlaylistSummary, TrackSort } from '@aiya/contracts';
import type { Diagnostics } from '../lib/ipc/generated/Diagnostics';
import { DEFAULT_SETTINGS } from '@aiya/contracts';
import { ipc, type DynamicJson } from '../lib/ipc';
import { errorMessage } from '../lib/errors';
import { devValidate, settingsSchema, uiStateSchema, type UiState } from '../lib/ipc/schema';
import { LIBRARY_VIEW, parseViewKey, viewKey, type View } from '../lib/view';
import { LyricsLoader } from '../lib/lyrics-loader';

/**
 * 界面状态 store：视图、搜索排序、筛选、面板、设置、歌单列表、歌词、诊断。
 *
 * 这里只持有"是什么"，不持有"列表怎么刷"——reload/TrackWindow 仍在 App：
 * store 改完状态后由 App 触发一次 reload。
 *
 * 动态边界（docs/18 §4.3）：`app_get_settings` / `app_get_ui_state` 返回的是
 * **无 DTO 的动态 JSON**（老配置文件缺字段是常态），所以这两个走 zod：
 * 开发期 `devValidate` 报形状不符，生产期只做合并默认值。
 * 其余命令的返回值都由 Rust 生成类型，不再二次校验。
 */
class UiStore {
  view = $state<View>(LIBRARY_VIEW);
  search = $state('');
  sort = $state<TrackSort>('title');
  direction = $state<'asc' | 'desc'>('asc');
  artistId = $state<string | null>(null);
  albumId = $state<string | null>(null);
  sidePanel = $state<'queue' | 'lyrics' | 'spectrum' | 'import' | null>(null);

  settings = $state<AppSettings>({ ...DEFAULT_SETTINGS });
  playlists = $state<PlaylistSummary[]>([]);
  diagnostics = $state<Diagnostics | null>(null);
  settingsOpen = $state(false);
  beets = $state<BeetsStatus | null>(null);

  lyrics = $state<Lyrics | null>(null);
  lyricsLoading = $state(false);
  /**
   * 歌词加载的并发与幂等由 loader 负责：面板打开、换歌、点按钮三个入口
   * 都会调它，晚到的旧请求会被丢弃（详见 lib/lyrics-loader.ts）。
   */
  private readonly lyricsLoader = new LyricsLoader(
    (trackId) => ipc.lyrics.get(trackId),
    (state) => {
      this.lyrics = state.lyrics;
      this.lyricsLoading = state.loading;
    },
  );

  loadLyrics(trackId: string): void {
    this.lyricsLoader.load(trackId);
  }

  /* ---------------- 视图状态持久化 ---------------- */

  /** lastScrollTop 由列表持有；保存时通过 getter 注入 */
  scheduleUiSave(getScrollTop: () => number, timer: { clear(): void; arm(fn: () => void): void }): void {
    timer.clear();
    timer.arm(() => {
      const state: UiState = {
        sort: this.sort,
        direction: this.direction,
        scrollTop: Math.round(getScrollTop()),
        artistId: this.artistId,
        albumId: this.albumId,
        view: viewKey(this.view),
        sidePanel: this.sidePanel,
      };
      // 视图状态丢失不影响可用性，失败只记日志（别打断滚动）
      void ipc.app.setUiState(state as DynamicJson).catch((e) => {
        console.warn('setUiState failed:', errorMessage(e));
      });
    });
  }

  restore(data: UiState): void {
    if (data.sort) this.sort = data.sort;
    if (data.direction) this.direction = data.direction;
    this.artistId = data.artistId ?? null;
    this.albumId = data.albumId ?? null;
    this.sidePanel = data.sidePanel ?? null;
    this.view = parseViewKey(data.view);
  }

  /* ---------------- 设置 ---------------- */

  /**
   * 必须 snapshot：settings 是 `$state`，里面的 `shortcuts` / `output` 是 Proxy。
   * Proxy 过不了结构化克隆，直接 invoke 会抛 "An object could not be cloned."，
   * 而主题/密度在本地已经生效 —— 看起来一切正常，设置却没落盘。
   *
   * 先用新值本地生效（主题切换要跟手），再以后端返回的为准：
   * 后端会补齐默认值/拒绝非法值，这一步是"以磁盘为准"的对账。
   */
  applySettings(next: AppSettings, onError?: (message: string) => void): void {
    this.settings = next;
    void ipc.app
      .setSettings($state.snapshot(next) as DynamicJson)
      .then((raw) => {
        this.settings = mergeSettings(raw);
      })
      .catch((e) => onError?.(errorMessage(e)));
  }

  openDataDir(): void {
    void ipc.app.openDataDir().catch((e) => console.warn('openDataDir failed:', errorMessage(e)));
  }

  async openSettings(): Promise<void> {
    this.settingsOpen = true;
    this.diagnostics = null;
    await this.refreshDiagnostics();
  }

  async refreshDiagnostics(): Promise<void> {
    try {
      this.diagnostics = await ipc.app.diagnostics();
    } catch (e) {
      console.warn('diagnostics failed:', errorMessage(e));
      this.diagnostics = null;
    }
    void this.refreshBeets();
  }

  /** beets 环境探测（低频：设置页打开/手动刷新时；最坏 ~30s，必须容错） */
  async refreshBeets(): Promise<void> {
    this.beets = null;
    try {
      this.beets = await ipc.beets.status();
    } catch (e) {
      console.warn('beets.status failed:', errorMessage(e));
    }
  }

  /* ---------------- 歌单 ---------------- */

  async refreshPlaylists(): Promise<void> {
    try {
      this.playlists = await ipc.playlist.list();
    } catch (e) {
      console.warn('playlist.list failed:', errorMessage(e));
      return;
    }
    const view = this.view;
    if (view.kind === 'playlist' && !this.playlists.some((item) => item.id === view.id)) {
      this.view = LIBRARY_VIEW;
    }
  }

  playlistById(id: string): PlaylistSummary | null {
    return this.playlists.find((item) => item.id === id) ?? null;
  }

  /* ---------------- 歌单 CRUD（调用方负责随后的 refresh/reload） ----------------
   * 全部抛出：歌单操作是用户显式动作，失败必须让他看到。
   */

  async createPlaylist(name: string, trackIds?: string[]): Promise<string> {
    const created = await ipc.playlist.create(name, trackIds);
    return created.id;
  }

  async renamePlaylist(id: string, name: string): Promise<void> {
    await ipc.playlist.rename(id, name);
  }

  async removePlaylist(id: string): Promise<void> {
    await ipc.playlist.remove(id);
  }

  async clearPlaylist(id: string): Promise<void> {
    await ipc.playlist.clear(id);
  }

  /** 返回新增/跳过计数（`addTracks` 的去重结果，UI 要原样告诉用户） */
  async addTracks(id: string, trackIds: string[]): Promise<{ added: number; skipped: number }> {
    const result = await ipc.playlist.addTracks(id, trackIds);
    return { added: result.added, skipped: result.skipped };
  }

  async moveItem(id: string, from: number, to: number): Promise<void> {
    await ipc.playlist.move(id, from, to);
  }

  /** 侧栏歌单整体排序：后端回排好的完整列表，省一次 roundtrip */
  async reorderPlaylists(from: number, to: number): Promise<PlaylistSummary[]> {
    this.playlists = await ipc.playlist.reorder(from, to);
    return this.playlists;
  }

  async removeItemAt(id: string, position: number): Promise<void> {
    await ipc.playlist.removeAt(id, position);
  }

  /* ---------------- 启动引导 ---------------- */

  /** 首屏并发拉取：设置 / 歌单 / UiState。返回恢复出的 lastScrollTop */
  async bootstrap(): Promise<number> {
    let rawUi: DynamicJson;
    let rawSettings: DynamicJson;
    let playlists: PlaylistSummary[];
    try {
      [rawUi, rawSettings, playlists] = await Promise.all([
        ipc.app.getUiState(),
        ipc.app.getSettings(),
        ipc.playlist.list(),
      ]);
    } catch (e) {
      // 启动引导失败不致命：设置/歌单/视图恢复走默认值，界面仍可用
      console.warn('bootstrap failed:', errorMessage(e));
      return 0;
    }

    this.settings = mergeSettings(rawSettings);
    this.playlists = playlists;

    devValidate(uiStateSchema, rawUi, 'app.uiState');
    const restored = rawUi as UiState;
    this.restore(restored);
    const view = this.view;
    if (view.kind === 'playlist' && !this.playlists.some((item) => item.id === view.id)) {
      this.view = LIBRARY_VIEW;
    }
    return typeof restored.scrollTop === 'number' ? restored.scrollTop : 0;
  }
}

/**
 * 动态 JSON → `AppSettings`：以默认值为底合并。
 *
 * 为什么不直接当 `AppSettings` 用：老配置文件里没有 `output`（2026-10 才加），
 * 直接断言会让 `settings.output.mode` 在运行期是 undefined —— 类型说有、
 * 实际没有，是这类 bug 最难查的形态。
 */
function mergeSettings(raw: unknown): AppSettings {
  devValidate(settingsSchema, raw, 'app.settings');
  const partial = (typeof raw === 'object' && raw !== null ? raw : {}) as Partial<AppSettings>;
  return { ...DEFAULT_SETTINGS, ...partial } as AppSettings;
}

export const ui = new UiStore();
