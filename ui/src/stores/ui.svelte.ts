import type { BeetsStatus, Diagnostics, Lyrics, PlaylistSummary } from '../lib/ipc';
import type { TrackSort } from '../lib/query';
import type { AppSettings } from '../lib/settings';
import { DEFAULT_SETTINGS } from '../lib/settings';
import { ipc, type DynamicJson } from '../lib/ipc';
import { errorMessage } from '../lib/errors';
import { devValidate, settingsSchema, uiStateSchema, type UiState } from '../lib/ipc/schema';
import { LIBRARY_VIEW, parseViewKey, viewKey, type View } from '../lib/view';
import { LyricsLoader } from '../lib/lyrics-loader';
import { readLook } from '../lib/look';
import { notify } from './notify.svelte';

export type AppPage = 'home' | 'nowPlaying' | 'radio' | 'settings';

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
  page = $state<AppPage>('home');
  /** 首页内部是仪表盘还是完整曲目列表；与四个一级页面分开持久化 */
  libraryDetail = $state(false);
  queueOpen = $state(false);
  search = $state('');
  sort = $state<TrackSort>('title');
  direction = $state<'asc' | 'desc'>('asc');
  artistId = $state<string | null>(null);
  albumId = $state<string | null>(null);

  /**
   * 初始值不是纯 `DEFAULT_SETTINGS`：主题/密度先取 localStorage 镜像
   * （`lib/look.ts`）。否则 `main.ts` 已经把首帧设成浅色，这里却是深色，
   * `App` 的主题 effect 首次运行会把 `data-theme` 写回深色，等 bootstrap
   * 返回真值再改回来——实测就是 `light → dark → light` 三次跳变。
   *
   * 其余字段仍走默认值：它们不影响首帧观感，bootstrap 一回来就覆盖。
   */
  settings = $state<AppSettings>({ ...DEFAULT_SETTINGS, ...readLook() });
  playlists = $state<PlaylistSummary[]>([]);
  diagnostics = $state<Diagnostics | null>(null);
  beets = $state<BeetsStatus | null>(null);

  /**
   * 列表滚动位置。**普通字段不是 `$state`**：只用于持久化与"重载后恢复"，
   * 没有任何视图按它渲染（滚动位置由 TrackList 自己的 DOM 状态决定）。
   */
  scrollTop = 0;

  /** UiState 防抖句柄（scheduleUiSave 内部用） */
  private readonly saveTimer = {
    handle: undefined as ReturnType<typeof setTimeout> | undefined,
    clear() {
      clearTimeout(this.handle);
    },
    arm(fn: () => void) {
      this.handle = setTimeout(fn, 600);
    },
  };

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

  /**
   * 防抖写 UiState。
   *
   * 定时器与 `scrollTop` 都归这里（批次 3 之前定时器挂在 App.svelte，调用方
   * 还得传一个 timer 对象进来 —— 三个调用点各写一遍 clear/arm，纯搬砖）。
   *
   * `scrollTop` 放在 ui 而不是列表 store：它本来就是 UiState 的字段，而
   * 列表 store 依赖 ui（不能反向 import），放这边 store 才不用为了一个整数
   * 在每层传 getter。列表滚动时调 `ui.scrollTop = n` 再 `scheduleUiSave()`。
   *
   * 600 ms 是"滚动停下就存"的经验值：再短会每滚一屏写一次库。
   */
  scheduleUiSave(): void {
    this.saveTimer.clear();
    this.saveTimer.arm(() => {
      const state: UiState = {
        sort: this.sort,
        direction: this.direction,
        scrollTop: Math.round(this.scrollTop),
        artistId: this.artistId,
        albumId: this.albumId,
        view: viewKey(this.view),
        page: this.page,
        libraryDetail: this.libraryDetail,
      };
      // 视图状态丢失不影响可用性，失败只记日志（别打断滚动）
      void ipc.app.setUiState(state as DynamicJson).catch((e) => {
        console.warn('setUiState failed:', errorMessage(e));
      });
    });
  }

  /** 清理：组件卸载时取消未落盘的写入 */
  dispose(): void {
    this.saveTimer.clear();
  }

  restore(data: UiState): void {
    if (data.sort) this.sort = data.sort;
    if (data.direction) this.direction = data.direction;
    this.artistId = data.artistId ?? null;
    this.albumId = data.albumId ?? null;
    this.page = data.page ?? 'home';
    this.libraryDetail = data.libraryDetail ?? false;
    this.view = parseViewKey(data.view);
    // 启动就落在设置页时**不会**经过 openPage，诊断块会一直空着
    // （显示"正在读取…"直到用户手动点刷新）。这里补一次。
    if (this.page === 'settings') void this.refreshDiagnostics();
  }

  /* ---------------- 一级页面与队列抽屉 ---------------- */

  openPage(page: AppPage): void {
    this.page = page;
    this.queueOpen = false;
    this.scheduleUiSave();
    if (page === 'settings') {
      this.diagnostics = null;
      void this.refreshDiagnostics();
    }
  }

  toggleQueue(): void {
    this.queueOpen = !this.queueOpen;
  }

  closeQueue(): void {
    this.queueOpen = false;
  }

  /* ---------------- 设置 ---------------- */

  /**
   * 必须 snapshot：settings 是 `$state`，里面的 `shortcuts` / `output` 是 Proxy。
   * Proxy 过不了结构化克隆，直接 invoke 会抛 "An object could not be cloned."，
   * 而主题/密度在本地已经生效 —— 看起来一切正常，设置却没落盘。
   *
   * 先用新值本地生效（主题切换要跟手），再以后端返回的为准：
   * 后端会补齐默认值/拒绝非法值，这一步是"以磁盘为准"的对账。
   *
   * 失败**默认落到全局错误条**（不再收一个可选回调）：漏传回调的调用点会让
   * 写盘失败静默吞掉，症状是"设置看着生效、重启就回滚"——比不生效更难查。
   */
  applySettings(next: AppSettings): void {
    this.settings = next;
    void ipc.app
      .setSettings($state.snapshot(next) as DynamicJson)
      .then((raw) => {
        this.settings = mergeSettings(raw);
      })
      .catch((e) => notify.showError(errorMessage(e)));
  }

  openDataDir(): void {
    void ipc.app.openDataDir().catch((e) => console.warn('openDataDir failed:', errorMessage(e)));
  }

  openSettings(): void {
    this.openPage('settings');
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
