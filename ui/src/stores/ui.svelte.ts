import type {
  AppSettings,
  BeetsStatus,
  Diagnostics,
  Lyrics,
  PlaylistSummary,
  TrackSort,
  UiState,
} from '@aiya/contracts';
import { DEFAULT_SETTINGS } from '@aiya/contracts';
import { LIBRARY_VIEW, parseViewKey, viewKey, type View } from '../lib/view';
import { LyricsLoader } from '../lib/lyrics-loader';

/**
 * 界面状态 store：视图、搜索排序、筛选、面板、设置、歌单列表、歌词。
 *
 * 这里只持有"是什么"，不持有"怎么取数"——列表刷新（reload/TrackWindow）
 * 仍在 App：store 改完状态后由 App 触发一次 reload。
 * `window.api.app` / `window.api.playlists` / `window.api.lyrics` 的调用
 * 全部收敛在这里。
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
    (trackId) => window.api.lyrics.get(trackId),
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
      void window.api.app.setUiState(state);
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
   * 必须 snapshot：settings 是 $state，里面的 shortcuts 是 Proxy。
   * Proxy 过不了结构化克隆，直接 invoke 会抛 "An object could not be cloned."，
   * 而主题/密度在本地已经生效 —— 于是看起来一切正常，设置却没有落盘。
   */
  applySettings(next: AppSettings, onError?: (message: string) => void): void {
    this.settings = next;
    void window.api.app
      .setSettings($state.snapshot(next) as AppSettings)
      .then((result) => {
        if (result.ok) this.settings = result.data;
        else onError?.(result.error.message);
      })
      .catch((err: unknown) => {
        onError?.(err instanceof Error ? err.message : String(err));
      });
  }

  openDataDir(): void {
    void window.api.app.openDataDir();
  }

  async openSettings(): Promise<void> {
    this.settingsOpen = true;
    this.diagnostics = null;
    const result = await window.api.app.diagnostics();
    if (result.ok) this.diagnostics = result.data;
    void this.refreshBeets();
  }

  async refreshDiagnostics(): Promise<void> {
    const result = await window.api.app.diagnostics();
    if (result.ok) this.diagnostics = result.data;
    void this.refreshBeets();
  }

  /** beets 环境探测（低频：设置页打开/手动刷新时） */
  async refreshBeets(): Promise<void> {
    this.beets = null;
    const result = await window.api.app.beetsStatus();
    if (result.ok) this.beets = result.data;
  }

  /* ---------------- 歌单 ---------------- */

  async refreshPlaylists(): Promise<void> {
    const result = await window.api.playlists.list();
    if (!result.ok) return;
    this.playlists = result.data;
    const view = this.view;
    if (view.kind === 'playlist' && !this.playlists.some((item) => item.id === view.id)) {
      this.view = LIBRARY_VIEW;
    }
  }

  playlistById(id: string): PlaylistSummary | null {
    return this.playlists.find((item) => item.id === id) ?? null;
  }

  /* ---------------- 歌单 CRUD（调用方负责随后的 refresh/reload） ---------------- */

  async createPlaylist(name: string, trackIds?: string[]): Promise<{ ok: true; id: string } | { ok: false; message: string }> {
    const result = await window.api.playlists.create(name, trackIds);
    return result.ok
      ? { ok: true, id: result.data.id }
      : { ok: false, message: result.error.message };
  }

  async renamePlaylist(id: string, name: string): Promise<{ ok: boolean; message?: string }> {
    const result = await window.api.playlists.rename(id, name);
    return result.ok ? { ok: true } : { ok: false, message: result.error.message };
  }

  async removePlaylist(id: string): Promise<{ ok: boolean; message?: string }> {
    const result = await window.api.playlists.remove(id);
    return result.ok ? { ok: true } : { ok: false, message: result.error.message };
  }

  async clearPlaylist(id: string): Promise<{ ok: boolean; message?: string }> {
    const result = await window.api.playlists.clear(id);
    return result.ok ? { ok: true } : { ok: false, message: result.error.message };
  }

  async addTracks(
    id: string,
    trackIds: string[],
  ): Promise<{ ok: true; added: number; skipped: number } | { ok: false; message: string }> {
    const result = await window.api.playlists.addTracks(id, trackIds);
    return result.ok
      ? { ok: true, added: result.data.added, skipped: result.data.skipped }
      : { ok: false, message: result.error.message };
  }

  async moveItem(id: string, from: number, to: number): Promise<{ ok: boolean; message?: string }> {
    const result = await window.api.playlists.move(id, from, to);
    return result.ok ? { ok: true } : { ok: false, message: result.error.message };
  }

  /** 侧栏歌单整体排序 */
  async reorderPlaylists(from: number, to: number): Promise<PlaylistSummary[]> {
    const result = await window.api.playlists.reorder(from, to);
    if (result.ok) this.playlists = result.data;
    return this.playlists;
  }

  async removeItemAt(id: string, position: number): Promise<{ ok: boolean; message?: string }> {
    const result = await window.api.playlists.removeAt(id, position);
    return result.ok ? { ok: true } : { ok: false, message: result.error.message };
  }

  /* ---------------- 启动引导 ---------------- */

  /** 首屏并发拉取：设置/歌单/UiState。返回恢复出的 lastScrollTop */
  async bootstrap(): Promise<number> {
    const [ui, settingsResult, playlistsResult] = await Promise.all([
      window.api.app.getUiState(),
      window.api.app.getSettings(),
      window.api.playlists.list(),
    ]);
    if (settingsResult.ok) this.settings = settingsResult.data;
    if (playlistsResult.ok) this.playlists = playlistsResult.data;
    let scrollTop = 0;
    if (ui.ok) {
      this.restore(ui.data);
      scrollTop = ui.data.scrollTop ?? 0;
      const view = this.view;
      if (
        view.kind === 'playlist' &&
        !this.playlists.some((item) => item.id === view.id)
      ) {
        this.view = LIBRARY_VIEW;
      }
    }
    return scrollTop;
  }
}

export const ui = new UiStore();
