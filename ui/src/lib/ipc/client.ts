/**
 * 新 IPC 客户端（docs/17）：**类型来自 Rust**，错误是结构化对象。
 *
 * 命令返回 `Result<T, ApiError>`，失败时宿主直接 **reject**，
 * 这里把 reject 值归一化成 `ApiError`（它不是 `Error` 实例，没有 stack）。
 *
 * 命令全量覆盖（条目数以本文件与方法为准，别在注释里写死 —— 这个数字在
 * 54/55 两个版本上腐烂过一次）。实际下发走 `transport()`（默认 Tauri invoke，
 * 浏览器预览时被 `lib/ipc/mock` 整体替换）—— 所以这个文件里没有任何
 * "是不是预览环境"的分支。
 *
 * 参数侧的约定（docs/17 §3.2）：`query` / `settings` / `options` / `decisions`
 * 这类复合对象在 Rust 侧收 `Json` 再手动解析，所以这里用结构类型而不是
 * 生成的 DTO —— 生成物只覆盖**返回值**。原因：参数反序列化失败发生在命令体
 * 之前，用强类型参数会让"参数不合法"变成 Tauri 自己的一句话 reject，
 * 绕开 `ApiError` 的分类。
 */
import type {
  ApiError,
  ApiErrorCode,
  AlbumPage,
  AppInfo,
  ArtistPage,
  BeetsFieldCatalog,
  BeetsMarketItem,
  BeetsPluginItem,
  BeetsPythonProbe,
  BeetsQueryPage,
  BeetsConfig,
  BeetsStatus,
  CancelRequested,
  ChosenRoot,
  Diagnostics,
  DroppedItem,
  EnrichmentGc,
  EnrichmentMatch,
  EnrichmentWorklist,
  EqSettings,
  HistoryCleared,
  HistoryStats,
  JobStarted,
  LibraryRoots,
  LibraryStats,
  Lyrics,
  OpenPathsReceipt,
  PlayerState,
  PlaylistAddResult, PlaylistRefresh,
  PlaylistDeleted,
  PlaylistExportResult,
  PlaylistImportResult,
  PlaylistSummary,
  QueueSnapshot,
  RemoveTracks,
  RefreshState,
  RepeatMode,
  RevealResult,
  RootsTrackCount,
  ScanCancel,
  ScanResult,
  SpectrumToggle,
  TrackDetail,
  TrackPage,
} from './generated';
import { transport } from './transport';
import type { SmartRule } from '../smart';

export type { ApiError, ApiErrorCode };

/**
 * 命令参数里的 TrackQuery 形状（Rust `TrackWindowQuery`，B1 起一直是动态 Value）。
 *
 * **这个类型是源头**：`lib/query.ts` 的 `TrackQuery` 是它的别名，那边的 zod
 * schema 负责补齐默认值与拒绝非法值。所以入参形状只有一处定义，改这里等于
 * 同时改了三处调用（list / playQuery / idsForSelection）。
 */
export type TrackQueryInput = {
  sort?: 'title' | 'added' | 'artist' | 'album';
  direction?: 'asc' | 'desc';
  search?: string | null;
  artistId?: string | null;
  albumId?: string | null;
  /** 入库时间范围（毫秒，含 from 不含 to）——筛选器模态框 */
  addedFromMs?: number | null;
  addedToMs?: number | null;
  /**
   * 筛选器临时条件（`[{field,op,value}]`）——与动态歌单同一批键
   * （smart.rs parse_conds 解析），只在本次查询生效、不落库。
   * 旧 lastPlayed* 字段已随「上次播放」筛选档移除（后端字段保留兼容）。
   */
  conds?: Array<{ field: string; op: string; value: string | number }> | null;
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

/**
 * 扫描选项（docs/29 批 A）：不传 `roots` 时后端按 settings `library.roots`
 * （数组，多目录并集单库）→ 旧 `library.root` → 系统 Music 目录取。
 */
export type ScanOptions = { roots?: string[] };

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

/**
 * beets 工具的两条安装轨道（docs/33 §6.2）。
 *
 * - `installer`：`.pyz` 安装器，用**系统** Python 建 venv。体积小，但要求机器上
 *   有 3.10–3.14 的 Python。
 * - `bundle`：免安装套件包，自带解释器（约 34MB 下载 / 97MB 落盘），解压即用。
 *
 * 播放器对两者做同一件事：下载 → 展开 → 记入口 → 跑一次 status 验证。
 * 它**不判断**哪条更合适——那是用户的机器状况决定的。
 */
export type BeetsInstallTrack = 'installer' | 'bundle';

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
    return await transport().invoke<T>(cmd, args);
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

  /**
   * 自动刷新管线（docs/33 §7）。
   *
   * 只有查询：触发是宿主自己的事（定时 30 s / 窗口聚焦 / 命令收尾），
   * 宿主不会替用户"点按钮"。手动立即刷新走 `beets.sync`。
   */
  refresh: {
    /** 间隔 + 是否在前台 + 最近一次结果（诊断区读） */
    state: () => call<RefreshState>('refresh_state'),
  },

  library: {
    list: (query: TrackQueryInput) => call<TrackPage>('library_list', { query }),
    /** 按 ID 批量取（含音频规格）；返回顺序与入参一致，缺失的跳过 */
    tracks: (ids: string[]) => call<TrackDetail[]>('library_tracks', { ids }),
    stats: () => call<LibraryStats>('library_stats'),
    /** 阻塞直到扫描结束（进度走 `scan:progress`）；并发调用得 `busy` */
    scan: (options?: ScanOptions) => call<ScanResult>('library_scan', { options }),
    scanCancel: () => call<ScanCancel>('library_scan_cancel'),
    /** 打开系统目录选择器；用户取消返回 null。选中时回填完整根列表 */
    chooseRoot: () => call<ChosenRoot | null>('library_choose_root'),
    /**
     * 打开系统目录选择器，**只回路径**（不写任何设置）。
     *
     * 与 `chooseRoot` 的区别是副作用：那条会把选中的目录追加进曲库目录，
     * 只适合"加一个曲库根"。选待整理目录 / 整理落点必须用这条——否则
     * 选完就把它变成了曲库根（copy 模式下原件被再扫一遍 = 重复曲目），
     * 而且紧接着的重叠校验会拒绝这次选择，用户看不懂为什么。
     */
    pickFolder: () => call<string | null>('pick_folder'),
    /** 扫描根列表（settings `library.roots`；textarea 数据源） */
    rootsGet: () => call<LibraryRoots>('library_roots_get'),
    /** 覆盖写扫描根列表（后端再归一化：trim/去空/去引号/去重） */
    rootsSet: (roots: string[]) => call<LibraryRoots>('library_roots_set', { roots }),
    /** 移除这些目录会波及多少曲目（path_lower 前缀统计，只读） */
    rootsCount: (paths: string[]) => call<RootsTrackCount>('library_roots_count', { paths }),
    /** 从曲库删除这些目录下的曲目（只删索引不动磁盘，外键级联清歌单/队列/历史） */
    rootsPrune: (paths: string[]) => call<RemoveTracks>('library_roots_prune', { paths }),
    /** 清空整个曲库记录（只删索引不动磁盘；歌单壳保留，条目/历史级联清空） */
    clear: () => call<RemoveTracks>('library_clear'),
    /** 在文件管理器中定位；`revealed=false` 时看 `reason`，不是错误 */
    revealTrack: (trackId: string) => call<RevealResult>('library_reveal_track', { trackId }),
    /** 批量选择 → ID（全选 5 万首也只回一数组，UI 不必持有整库） */
    idsForSelection: (request: SelectionIdsQuery) =>
      call<string[]>('library_ids_for_selection', { request }),
    /** 从曲库移除（只删索引，不动磁盘文件） */
    removeTracks: (trackIds: string[]) => call<RemoveTracks>('library_remove_tracks', { trackIds }),
    /**
     * 一批文件路径 → 曲目 id（`null` = 曲库里没有这个文件）。
     *
     * 谁在用：beets 查询页。它读的是**另一个库**，那边只有路径、没有本应用的
     * id（刻意不该有）。返回**同序等长**，好让界面按行标注"这首不在曲库"。
     * 未命中不会造行——另一个库可能收录了还没扫描进曲库的文件。
     */
    resolvePaths: (paths: string[]) =>
      call<(string | null)[]>('tracks_resolve_paths', { paths }),
    /**
     * 专辑/艺人聚合浏览（docs/24 §3.2 决策 2）。只含**实际有曲目**的条目，
     * 一次性返回前 500（后端 `FACET_LIMIT`），无游标；按名称排序。
     * 空白/缺省 `search` = 不过滤。
     */
    albums: (search?: string) => call<AlbumPage>('library_albums', { search }),
    artists: (search?: string) => call<ArtistPage>('library_artists', { search }),
    /** 曲库现有流派（去重，norm 序）；动态歌单"流派"条件的下拉数据源 */
    genres: () => call<string[]>('library_genres'),
  },

  playlist: {
    list: () => call<PlaylistSummary[]>('playlist_list'),
    /** 不存在返回 null（详情页空态），不是错误 */
    detail: (id: string) => call<PlaylistSummary | null>('playlist_detail', { id }),
    create: (name: string, trackIds?: string[], rule?: SmartRule) =>
      call<PlaylistSummary>('playlist_create', { name, trackIds, rule }),
    rename: (id: string, name: string) => call<PlaylistSummary>('playlist_rename', { id, name }),
    /** 更新动态歌单规则（手动歌单后端会明确报错） */
    setRule: (id: string, rule: SmartRule) => call<PlaylistSummary>('playlist_set_rule', { id, rule }),
    remove: (id: string) => call<PlaylistDeleted>('playlist_delete', { id }),
    /** 侧栏拖动排序；返回排好序的完整列表（省一次 roundtrip） */
    reorder: (from: number, to: number) => call<PlaylistSummary[]>('playlist_reorder', { from, to }),
    // 2026-10-30 语义：加歌一律首行插入（最后入库的在最前），无 append/next 分档
    addTracks: (id: string, trackIds: string[]) =>
      call<PlaylistAddResult>('playlist_add_tracks', { id, trackIds }),
    /** 刷新动态歌单：重跑规则只追加新命中；返回本次新增数与刷新后总数 */
    refresh: (id: string) => call<PlaylistRefresh>('playlist_refresh', { id }),
    removeAt: (id: string, position: number) =>
      call<PlaylistSummary>('playlist_remove_at', { id, position }),
    move: (id: string, from: number, to: number) =>
      call<PlaylistSummary>('playlist_move', { id, from, to }),
    clear: (id: string) => call<PlaylistSummary>('playlist_clear', { id }),
    /**
     * 导入 m3u/m3u8 播放列表。不传 `path` 时由宿主弹原生文件选择框；
     * `playlist = null` 有两种含义，都不是错误：用户取消（unmatched 为空），
     * 或一条路径都没匹配到曲库（unmatched 有内容，提示先扫描）。
     * 未匹配条目带行号和原因（`notFound`），UI 自己决定怎么展示。
     */
    import: (path?: string) => call<PlaylistImportResult>('playlist_import', { path }),
    /**
     * 导出歌单为 m3u/m3u8。缺省 format=m3u8、style=absolute、encoding=utf8
     * （encoding 目前只收 utf8，参数位留给将来 GBK）。
     */
    export: (
      id: string,
      path: string,
      format?: 'm3u' | 'm3u8',
      style?: 'absolute' | 'relative',
    ) => call<PlaylistExportResult>('playlist_export', { id, path, format, style }),
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
    /**
     * 拖入 / 选中一批文件：替换队列并播放第一项（docs/52 §5.3）。
     * 音频、`m3u8` 清单、老式 `m3u` 歌单都从这一个口进；不进曲库。
     */
    openPaths: (paths: string[]) => call<OpenPathsReceipt>('player_open_paths', { paths }),
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
    /**
     * EQ 设置（协议 v6）：持久化 + 即时下发。返回**生效值**（clamp 后）——
     * UI 以返回值为准，不要用自己发出去的值更新状态。
     */
    setEq: (eq: EqSettings) => call<EqSettings>('player_set_eq', { eq }),
    /** 当前 EQ 设置（面板打开时回读一次；没保存过 = 默认） */
    getEq: () => call<EqSettings>('player_get_eq'),
  },

  lyrics: {
    /** 没找到回 `found=false`（UI 显示"暂无"），不当错误 */
    get: (trackId: string) => call<Lyrics>('lyrics_get', { trackId }),
  },

  /**
   * 直播 / 拖放（docs/52）。
   *
   * `classify` 是**唯一分类器**的后端出口：UI 不自己写扩展名 if-else ——
   * 两边各写一份必然漂移（`is_http_url` 大小写不一致就是这么来的）。
   */
  live: {
    classify: (paths: string[]) => call<DroppedItem[]>('classify_paths', { paths }),
    /** 打开一个 HLS 地址（标题缺省由后端从 URL 推） */
    open: (url: string, title?: string) =>
      call<PlayerState>('live_open', { url, title: title ?? null }),
    /** 选一个本地 m3u8/m3u 文件；取消回 null（取消不是错误） */
    pick: () => call<string | null>('live_pick_playlist'),
  },

  history: {
    stats: () => call<HistoryStats>('history_stats'),
    clear: () => call<HistoryCleared>('history_clear'),
  },

  beets: {
    /** 探测最多 ~30s（逐个候选解释器）；设置页打开时调一次即可 */
    status: () => call<BeetsStatus>('beets_status'),
    /** 只读预览：进度走 `beets:import`；没装 beets 得 `beetsMissing`。
     *  已导入排除表（inbox 防重复）由宿主注入 options，UI 不用管 */
    preview: (paths: string[]) => call<JobStarted>('beets_preview', { paths }),
    /** resume（v2）：中断后续跑——重发同一批 decisions，sidecar 按断点跳过已完成的 */
    apply: (decisions: ApplyDecision[], resume?: boolean) =>
      call<JobStarted>('beets_apply', { decisions, resume: resume ?? false }),
    cancel: () => call<CancelRequested>('beets_cancel'),
    /** beets 配置快照：待整理目录 / 整理输出根 / 已导入记录数（docs/40 十三轮） */
    config: () => call<BeetsConfig>('beets_config_get'),
    /** 设置待整理目录（null = 清除）。与曲库目录重叠会被后端以 invalid_arg 拒绝 */
    setInbox: (path: string | null) => call<BeetsConfig>('beets_set_inbox', { path }),
    /** 勾选整理输出根（单选）：必须是曲库目录之一 */
    setTarget: (path: string) => call<BeetsConfig>('beets_set_target', { path }),
    /** 在资源管理器打开待整理目录（拖文件进去的入口） */
    openInbox: () => call<null>('beets_open_inbox'),
    /** 记录已成功导入的源路径（preview 排除表，FIFO 2000，幂等） */
    inboxImportedAdd: (paths: string[]) =>
      call<BeetsConfig>('beets_inbox_imported_add', { paths }),
    /**
     * 手动触发一次完整刷新（增量扫描 + beets 对齐）。
     *
     * 与自动定时跑的是**同一条管线**：结果走 `libraryRefreshed`
     * （无变化时手动也会发一条，好让界面有回应）。
     */
    /**
     * 装 beets 工具到应用数据目录（docs/33 §6.2 两条轨道）。
     *
     * 播放器只做「下载 → 展开 → 记入口 → 跑一次 status 验证」，成功判据是
     * probe 的 `found`，**不是**退出码。立刻返回「已启动」，进度走
     * `beets:install` 事件，最后一条是 `{type:"finished", status}`。
     *
     * `track` 缺省 = `installer`（体积小、优先试；机器没 Python 会明确报错，
     * 再由用户改选 `bundle`）。
     */
    install: (
      track: BeetsInstallTrack = 'installer',
      plugins: string[] = [],
    ) => call<JobStarted>('beets_install', { track, plugins }),

    /**
     * 内嵌插件表：**不需要装过组件**就能拿到的静态目录（docs/51 §4.2）。
     *
     * 与 `pluginCatalog()` 的分工：这条回答"有哪些插件、各自要装什么"；
     * 那条（要起 sidecar）回答"装上之后哪些真的 ready"。
     */
    pluginTable: () => call<BeetsPluginItem[]>('beets_plugin_table'),

    /** 探测本机 Python：`py -0p` 优先，再 PATH。找不到时 `reason` 带人话。 */
    pythonProbe: () => call<BeetsPythonProbe>('beets_python_probe'),

    /**
     * 卸载组件：只删安装目录（venv/套件/下载缓存），**不动**整理库与分析库。
     * 进度与 `install` 共用 `beets:install` 事件流（最后一条是 `finished`）。
     */
    uninstall: () => call<JobStarted>('beets_uninstall'),

    /*
     * ---- 批 D：曲库维护四件套 ----
     * 四条都是**异步**的：立刻返回「已启动」，进度/结果走 `beets:maintenance`。
     */
    /**
     * 建 beets 库基线：把自有库**在线**曲目按现状登记进 beets 库
     * （as-is 入库：不联网、不改标签、不移动文件）。
     *
     * `paths` 传空数组 = 宿主自己取全库路径（UI 永远这么传：上千条路径不必
     * 经 IPC，而且"哪些行可播"是 DB 的事实）。跑完自动对齐读模型。
     */
    adopt: (paths: string[] = []) => call<JobStarted>('beets_adopt', { paths }),
    /** 用文件标签刷新 beets 库。`pretend` = 只出 diff 预览，不写库。 */
    update: (pretend: boolean) => call<JobStarted>('beets_update', { pretend }),
    /** 按字段组查重（**只读**，应用内不删文件）。`keys` 空 = sidecar 默认 title+artist。 */
    duplicates: (keys: string[] = []) => call<JobStarted>('beets_duplicates', { keys }),
    /**
     * 按 beets 的 path_formats 整理文件。`pretend` = 只出「从哪到哪」清单。
     *
     * `directory` 是**校验值**（传 `beets.libraryDirectory`）：sidecar 拿它跟
     * config.yaml 的 directory 比对，不一致直接拒绝——库根只能有一处定义。
     */
    move: (directory: string, pretend: boolean) =>
      call<JobStarted>('beets_move', { directory, pretend }),
    /**
     * 在文件管理器中定位一个绝对路径（查重结果的「定位」）。
     * 与 `library.revealTrack` 的区别：查重结果来自 beets 库，可能还没进
     * 自有库，没有 trackId 可用。
     */
    reveal: (path: string) => call<RevealResult>('beets_reveal', { path }),
    /** 插件市场目录（白名单 + 三态 + 来源档）。一次性短进程查询，~1s。 */
    pluginCatalog: () => call<BeetsMarketItem[]>('beets_plugin_catalog'),
    /**
     * 安装白名单插件（仅 `pypi:<包名>` 档有意义；builtin 档随 beets 内置）。
     * 进度走 `beets:maintenance`（pluginProgress → pluginDone 终态），
     * 与维护四件套共用会话槽——装插件时其他维护任务会被拒。
     */
    pluginInstall: (name: string) => call<JobStarted>('beets_plugin_install', { name }),

    /*
     * ---- 查询页（docs/47）：读 **beets 库**，产物是「存为歌单」 ----
     * 与 `enrichment.query` 的分工：这里查的是用户自己的 beets 库（字段取决于
     * 他装了哪些插件），那边查的是索引增强的结论库（字段固定）。
     */
    /**
     * 查一页。`limit` 缺省由工具侧决定（500）。
     *
     * 三种用法，互斥：
     * - `{ raw: 'mood_happy:1' }` —— beets 自己的查询语法（逃生口）
     * - `{ rules: [{ field, op, value }] }` —— 结构化，工具侧求值（范围查询可靠）
     * - `{}` —— 全库
     *
     * `fields` 缺省是 `title/artist/album`；想显示 bpm 就自己加。
     */
    query: (filter: {
      raw?: string;
      rules?: { field: string; op: string; value?: unknown }[];
      fields?: string[];
      limit?: number;
    } = {}) =>
      call<BeetsQueryPage>('beets_query', {
        raw: filter.raw ?? null,
        rules: filter.rules ?? null,
        fields: filter.fields ?? null,
        limit: filter.limit ?? null,
      }),
    /** 字段清单（含插件写入的 flexible 键）+ 识别源三态。查询页的选择器数据源。 */
    fields: () => call<BeetsFieldCatalog>('beets_fields'),

    /**
     * 只读入库：把 **`root`**（缺省＝曲库根）登记进索引家（不做分析）。
     *
     * 异步：进度走 `beets:index`（`progress` → `indexDone`）。
     * 这是查询页的前置条件——索引库空着时任何筛选都是 0 命中。
     *
     * `root` 传落点子目录是给"整理完补索引"用的：整根重扫实测 82 s（96 批
     * 进程启动开销），一个落点目录 1 s。宿主在 apply 结束时会自己补一次，
     * UI 只在需要重扫某个特定目录时才传。
     */
    indexImport: (root?: string) =>
      call<JobStarted>('beets_index_import', root ? { root } : {}),
  },

  /**
   * 索引增强（docs/45）：beets 侧分析出的数值特征（bpm / 舞曲性 / 情绪）
   * 落在宿主自己的关联表里，这里只读它，**不触发任何分析**。
   *
   * 没装组件时 `available=false`：历史结论照样能查，但没有新的可算——
   * UI 按这一个布尔禁用「开始分析」，不必判断 `note`。
   */
  enrichment: {
    /** 曲库索引卡数据源：计数 + 清单前 20 条。`includeEmpty` = 连上次跑空的也重算 */
    worklist: (source?: string, includeEmpty?: boolean) =>
      call<EnrichmentWorklist>('enrichment_worklist', {
        source: source ?? null,
        includeEmpty: includeEmpty ?? false,
      }),
    /**
     * 起一次分析（异步）：立刻返回「已启动」，进度/结果走 `beets:enrich`。
     *
     * `force` 同时表达"连 empty 也重算"和 sidecar 侧的"清旧再补登"——
     * 少了后半段，beets 会把已在库的文件直接跳过，插件根本不跑。
     */
    start: (source?: string, force?: boolean) =>
      call<JobStarted>('beets_enrich_start', {
        source: source ?? null,
        force: force ?? false,
      }),
    /**
     * 按维度查（曲库页/智能歌单）。全部参数可省 = 不限制。
     *
     * 返回的 `trackId` 可直接喂给 `player.playTrack` / 歌单接口。
     */
    query: (filter: {
      source?: string;
      mood?: string;
      danceMin?: number;
      danceMax?: number;
      bpmMin?: number;
      bpmMax?: number;
      limit?: number;
    } = {}) => call<EnrichmentMatch[]>('enrichment_query', { query: filter }),
    /** 清理残留行（曲库里没有对应曲目且 N 天没更新过）。`keepDays` 缺省 30 */
    gc: (source?: string, keepDays?: number) =>
      call<EnrichmentGc>('enrichment_gc', {
        source: source ?? null,
        keepDays: keepDays ?? 30,
      }),
  },
};

/** UI 处理约定（docs/17 §2.5）：只在这里判"能不能重试"，组件不重复判断。 */
export function isRetryable(e: unknown): boolean {
  return toApiError(e).retryable;
}
