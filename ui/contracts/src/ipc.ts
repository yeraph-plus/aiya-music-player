import type {
  AppSettings,
  ApplyDecision,
  BeetsImportEvent,
  BeetsStatus,
  Diagnostics,
  LibraryStats,
  ListDensity,
  Lyrics,
  PlaylistSummary,
  RepeatMode,
  ScanProgress,
  ScanResult,
  ThemeMode,
  Track,
} from './domain.js';
import type { TrackQuery, TrackSort } from './query.js';
import type {
  PlayerCommand,
  PlayerReport,
  PlayerState,
  QueueSnapshot,
  RevealResult,
} from './player.js';

/* ---------------- IPC 通道 ---------------- */

export const CH = {
  libraryList: 'library.list',
  libraryTracks: 'library.tracks',
  libraryStats: 'library.stats',
  libraryScan: 'library.scan',
  libraryScanCancel: 'library.scanCancel',
  libraryScanProgress: 'library.scanProgress',
  libraryChooseRoot: 'library.chooseRoot',
  libraryRevealTrack: 'library.revealTrack',
  playerState: 'player.state',
  playerCommand: 'player.command',
  playerChanged: 'player.changed',
  playerReport: 'player.report',
  playerPlayTracks: 'player.playTracks',
  playerPlayQuery: 'player.playQuery',
  playerQueue: 'player.queue',
  playerToggle: 'player.toggle',
  playerNext: 'player.next',
  playerPrev: 'player.prev',
  playerSeek: 'player.seek',
  playerVolume: 'player.volume',
  playerMute: 'player.mute',
  playerRate: 'player.rate',
  playerRepeat: 'player.repeat',
  playerShuffle: 'player.shuffle',
  playerEnqueue: 'player.enqueue',
  playerInsertNext: 'player.insertNext',
  playerRemoveAt: 'player.removeAt',
  playerMove: 'player.move',
  playerClear: 'player.clear',
  playlistsList: 'playlists.list',
  playlistsDetail: 'playlists.detail',
  playlistsCreate: 'playlists.create',
  playlistsRename: 'playlists.rename',
  playlistsDelete: 'playlists.delete',
  playlistsReorder: 'playlists.reorder',
  playlistsAddTracks: 'playlists.addTracks',
  playlistsRemoveAt: 'playlists.removeAt',
  playlistsMove: 'playlists.move',
  playlistsClear: 'playlists.clear',
  lyricsGet: 'lyrics.get',
  libraryIdsForSelection: 'library.idsForSelection',
  libraryRemoveTracks: 'library.removeTracks',
  historyStats: 'history.stats',
  historyClear: 'history.clear',
  appInfo: 'app.info',
  appOpenDataDir: 'app.openDataDir',
  appUiState: 'app.uiState',
  appSetUiState: 'app.setUiState',
  appSettings: 'app.settings',
  appSetSettings: 'app.setSettings',
  appDiagnostics: 'app.diagnostics',
} as const;

/* ---------------- 结果信封 ---------------- */

export type IpcErrorCode =
  | 'E_INVALID_ARG'
  | 'E_INVALID_CURSOR'
  | 'E_NOT_FOUND'
  | 'E_DB'
  | 'E_IO'
  | 'E_CANCELLED'
  | 'E_BUSY'
  | 'E_INTERNAL';

export interface IpcError {
  code: IpcErrorCode;
  message: string;
  detail?: string;
}

export type IpcResult<T> = { ok: true; data: T } | { ok: false; error: IpcError };

export const ok = <T>(data: T): IpcResult<T> => ({ ok: true, data });
export const fail = (code: IpcErrorCode, message: string, detail?: string): IpcResult<never> => ({
  ok: false,
  error: detail === undefined ? { code, message } : { code, message, detail },
});

/* ---------------- 分页 ---------------- */

export interface TrackPage {
  items: Track[];
  /** 满足当前筛选条件的总行数；搜索时也返回真实命中数 */
  total: number;
  limit: number;
  mode: 'cursor' | 'offset';
  nextCursor: string | null;
  offset?: number;
  tookMs: number;
}

/* ---------------- 渲染层可见的 API ---------------- */

export interface QueueInsertMode {
  mode: 'append' | 'next';
}

/** 渲染层自己的视图状态：排序、筛选、滚动位置。存进 app_state，换设备/清缓存都不丢 */
export interface UiState {
  sort?: TrackSort;
  direction?: 'asc' | 'desc';
  scrollTop?: number;
  /** 上次筛选的艺人/专辑，用于重启后回到同一视图 */
  artistId?: string | null;
  albumId?: string | null;
  /** 上次停留的视图：'library' | 'recent' | 'mostPlayed' | 'playlist:<id>' */
  view?: string;
  /** 上次打开的右侧面板 */
  sidePanel?: 'queue' | 'lyrics' | 'spectrum' | 'import' | null;
}

export interface PlaylistAddResult {
  added: number;
  skipped: number;
  summary: PlaylistSummary;
}

/** 批量选择：把列表下标交给主进程换算成 ID，避免渲染层为了全选把 5 万行拉下来 */
export interface SelectionIdsQuery {
  query: TrackQuery;
  /** 'all' 表示当前视图全部；否则为 0 基下标（上限 5000） */
  indices: number[] | 'all';
}

export interface PlaylistApi {
  list(): Promise<IpcResult<PlaylistSummary[]>>;
  detail(id: string): Promise<IpcResult<PlaylistSummary | null>>;
  create(name: string, trackIds?: string[]): Promise<IpcResult<PlaylistSummary>>;
  rename(id: string, name: string): Promise<IpcResult<PlaylistSummary>>;
  remove(id: string): Promise<IpcResult<{ deleted: boolean }>>;
  /** 播放列表自身排序（侧栏拖动） */
  reorder(from: number, to: number): Promise<IpcResult<PlaylistSummary[]>>;
  addTracks(
    id: string,
    trackIds: string[],
    mode?: 'append' | 'next',
  ): Promise<IpcResult<PlaylistAddResult>>;
  removeAt(id: string, position: number): Promise<IpcResult<PlaylistSummary>>;
  move(id: string, from: number, to: number): Promise<IpcResult<PlaylistSummary>>;
  clear(id: string): Promise<IpcResult<PlaylistSummary>>;
}

export interface HistoryStats {
  totalPlays: number;
  totalMs: number;
  completed: number;
  skipped: number;
  firstPlayAt: number | null;
  lastPlayAt: number | null;
  topTracks: Array<{ id: string; title: string; artist: string | null; playCount: number }>;
}

export interface PlayerApi {
  state(): Promise<IpcResult<PlayerState>>;
  /** 队列按版本号单独拉；5 万首的 ID 数组不该挤进每秒一次的状态广播 */
  queue(): Promise<IpcResult<QueueSnapshot>>;
  /** 用一批曲目替换队列，并从 startIndex 开始播 */
  playTracks(trackIds: string[], startIndex?: number): Promise<IpcResult<PlayerState>>;
  /**
   * 用"当前视图的查询"建队列并从 startIndex 开始播。
   * 渲染层只持有窗口内的几百行，整库的播放顺序必须由主进程按同一份排序生成。
   */
  playQuery(query: TrackQuery, startIndex?: number): Promise<IpcResult<PlayerState>>;
  toggle(): Promise<IpcResult<PlayerState>>;
  next(): Promise<IpcResult<PlayerState>>;
  prev(): Promise<IpcResult<PlayerState>>;
  seek(positionMs: number): Promise<IpcResult<PlayerState>>;
  setVolume(volume: number): Promise<IpcResult<PlayerState>>;
  toggleMute(): Promise<IpcResult<PlayerState>>;
  /** 0.5–2.0，超出上下限返回 E_INVALID_ARG（不静默夹取） */
  setRate(rate: number): Promise<IpcResult<PlayerState>>;
  /** 传 'cycle' 按 顺序 → 列表循环 → 单曲循环 轮转 */
  setRepeat(repeat: RepeatMode | 'cycle'): Promise<IpcResult<PlayerState>>;
  setShuffle(shuffle: boolean): Promise<IpcResult<PlayerState>>;
  enqueue(trackIds: string[], options?: QueueInsertMode): Promise<IpcResult<PlayerState>>;
  insertNext(trackId: string): Promise<IpcResult<PlayerState>>;
  removeAt(index: number): Promise<IpcResult<PlayerState>>;
  move(from: number, to: number): Promise<IpcResult<PlayerState>>;
  clear(): Promise<IpcResult<PlayerState>>;
  /** 渲染层回报播放事件，无返回值 */
  report(report: PlayerReport): void;
  onState(callback: (state: PlayerState) => void): () => void;
  onCommand(callback: (command: PlayerCommand) => void): () => void;
}

export interface AiyaApi {
  library: {
    list(query?: TrackQuery): Promise<IpcResult<TrackPage>>;
    /** 按 ID 批量取（队列面板用）；返回顺序与入参一致，缺失的会被跳过 */
    tracks(ids: string[]): Promise<IpcResult<Track[]>>;
    stats(): Promise<IpcResult<LibraryStats>>;
    scan(options?: { path?: string }): Promise<IpcResult<ScanResult>>;
    cancelScan(): Promise<IpcResult<{ cancelled: boolean }>>;
    /** 打开系统目录选择器；用户取消返回 null */
    chooseRoot(): Promise<IpcResult<{ path: string } | null>>;
    /** 在系统文件管理器中定位曲目文件 */
    revealTrack(trackId: string): Promise<IpcResult<RevealResult>>;
    /** 批量选择 → ID。全选 5 万首只回一次 ID 数组，渲染层不必持有整库 */
    idsForSelection(request: SelectionIdsQuery): Promise<IpcResult<string[]>>;
    /** 从曲库移除（只删索引，不动磁盘文件） */
    removeTracks(trackIds: string[]): Promise<IpcResult<{ removed: number }>>;
    /** 同步生成，不走 IPC —— 只是字符串拼接 */
    artworkUrl(artworkId: string | null, size?: number): string | null;
    onScanProgress(callback: (progress: ScanProgress) => void): () => void;
  };
  playlists: PlaylistApi;
  lyrics: {
    get(trackId: string): Promise<IpcResult<Lyrics>>;
  };
  history: {
    stats(): Promise<IpcResult<HistoryStats>>;
    clear(): Promise<IpcResult<{ cleared: boolean }>>;
  };
  player: PlayerApi;
  app: {
    info(): Promise<IpcResult<{ version: string; electron: string; platform: string; dataDir: string }>>;
    /**
     * 首屏外观。主进程启动时把已保存的主题/密度通过 additionalArguments 传给
     * preload，渲染层在 mount 之前就能设好 data-theme —— 否则浅色主题会先
     * 画一帧深色再切回来，看起来就是"启动闪一下"。
     */
    initialLook: { theme: ThemeMode; density: ListDensity };
    openDataDir(): Promise<IpcResult<void>>;
    getUiState(): Promise<IpcResult<UiState>>;
    setUiState(state: UiState): Promise<IpcResult<void>>;
    getSettings(): Promise<IpcResult<AppSettings>>;
    setSettings(settings: AppSettings): Promise<IpcResult<AppSettings>>;
    diagnostics(): Promise<IpcResult<Diagnostics>>;
    /** 频谱 tap 订阅开关（面板打开/关闭时调用；关闭时引擎内零开销） */
    spectrumSet(enabled: boolean): Promise<IpcResult<{ enabled: boolean }>>;
    /** beets 环境探测（低频；设置页打开时调用） */
    beetsStatus(): Promise<IpcResult<BeetsStatus>>;
    /** 启动一次只读导入预览；事件流走 onBeetsImport */
    beetsPreview(paths: string[]): Promise<IpcResult<{ started: boolean }>>;
    /** B2：应用导入决策（写标签 + 入 beets 库 + 自动对齐读模型） */
    beetsApply(decisions: ApplyDecision[]): Promise<IpcResult<{ started: boolean }>>;
    /** B3：取消当前导入（preview/apply）；结果走 onBeetsImport 的 cancelled */
    beetsCancel(): Promise<IpcResult<{ requested: boolean }>>;
    /** 手动触发一次 beets → 自有读模型全量对齐；汇总走 onBeetsImport 的 librarySynced */
    beetsSync(): Promise<IpcResult<{ started: boolean }>>;
    /** beets 导入预览事件流（progress/album/albumError/done/sessionEnd） */
    onBeetsImport(callback: (evt: BeetsImportEvent) => void): () => void;
  };
}

export const ARTWORK_SIZES = [96, 256, 512] as const;
export type ArtworkSize = (typeof ARTWORK_SIZES)[number];

export function artworkUrl(artworkId: string | null, size: number = 96): string | null {
  if (!artworkId) return null;
  const safe = ARTWORK_SIZES.includes(size as ArtworkSize) ? size : 96;
  // Tauri 2 自定义协议在 Windows 上经 `http://<scheme>.localhost` 暴露。
  // 后端注册 aiya-artwork scheme 并带 ACAO:* —— artwork-blob 的 fetch 与
  // canvas 取色依赖它。v1 只发 Windows；跨平台时按 UA/平台切换前缀。
  return `http://aiya-artwork.localhost/artwork/${encodeURIComponent(artworkId)}?size=${safe}`;
}

export function trackMediaUrl(trackId: string): string {
  // 音频不再进 WebView（ADR-0001 边界 + ADR-0006 原生引擎）：
  // 出声在 Rust sidecar。保留为硬失败，防止遗漏的调用方悄悄建出第二条音频路径。
  throw new Error(`trackMediaUrl(${trackId}): 音频不走 WebView 协议（ADR-0006）`);
}
