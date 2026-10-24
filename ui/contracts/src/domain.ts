/**
 * 领域类型。
 *
 * 约束：这里不出现任何文件系统路径。渲染层拿到的是 ID 与展示字段，
 * 真实位置（locator）只存在于主进程。这样未来接 WebDAV 时契约不用改。
 */

export type Availability = 'online' | 'offline' | 'missing';

export type SourceKind = 'local' | 'webdav';

export interface LibrarySource {
  id: string;
  kind: SourceKind;
  enabled: boolean;
  /** 仅用于设置页展示，不含凭据 */
  displayPath: string | null;
}

export interface Track {
  id: string;
  title: string;
  artistId: string | null;
  artist: string | null;
  albumId: string | null;
  album: string | null;
  albumArtist: string | null;
  durationMs: number | null;
  trackNo: number | null;
  discNo: number | null;
  year: number | null;
  /** 入库时间，毫秒（DB 存秒，DTO 已换算） */
  addedAtMs: number;
  artworkId: string | null;
  /** 播放次数（track_stats）；从未播过为 0 */
  playCount: number;
  /** 最后一次播放时间；从未播过为 null */
  /** 最后播放时间，毫秒（DB 存秒，DTO 已换算） */
  lastPlayedAtMs: number | null;
  /**
   * ReplayGain 曲目增益（dB），来自标签；缺失为 null。
   * 只用于音量归一化的换算，不改变用户设置的音量本身。
   */
  gainDb: number | null;
  /**
   * 音频规格（来自 media_assets）。
   *
   * **列表查询不填**（undefined）—— 500 行 × 7 个字段的 JSON 白占 payload，
   * 而列表页目前不展示它。只有单曲查询（播放条的数据源）会填。
   * 老库未重扫前位深/无损为 null，徽章自动降级，不报错。
   */
  audio?: AudioFormat | null;
}

/** 音频文件的编码规格，扫描时从 media_assets 带出 */
export interface AudioFormat {
  codec: string | null;
  container: string | null;
  sampleRate: number | null;
  bitsPerSample: number | null;
  bitrate: number | null;
  channels: number | null;
  /**
   * 三态：true 无损 / false 有损 / null 未补算。
   *
   * 无损/有损的分流**只能看这个字段**：有损容器（AAC）也会报
   * `bitsPerSample = 16`（解码器输出位深），按位深判断会把 m4a 标成
   * "44.1 kHz / 16 bit"。
   */
  lossless: boolean | null;
}

export interface AlbumSummary {
  id: string;
  title: string;
  artist: string | null;
  year: number | null;
  trackCount: number;
  artworkId: string | null;
}

export interface ArtistSummary {
  id: string;
  name: string;
  trackCount: number;
  albumCount: number;
  artworkId: string | null;
}

export interface LibraryStats {
  tracks: number;
  albums: number;
  artists: number;
  locations: number;
  missingLocations: number;
  totalDurationMs: number;
  dbBytes: number;
  schemaVersion: number;
}

export interface ScanProgress {
  phase: 'walking' | 'parsing' | 'artwork' | 'done' | 'cancelled' | 'failed';
  scanned: number;
  total: number | null;
  imported: number;
  skipped: number;
  failed: number;
  currentPath: string | null;
  elapsedMs: number;
  message?: string;
}

export interface ScanResult {
  scanned: number;
  imported: number;
  updated: number;
  unchanged: number;
  failed: number;
  cancelled: boolean;
  errors: Array<{ path: string; message: string }>;
  elapsedMs: number;
}

/* ---------------- 播放 ---------------- */

export type RepeatMode = 'off' | 'all' | 'one';
export type PlaybackStatus = 'idle' | 'loading' | 'playing' | 'paused' | 'error';

/* ---------------- 播放列表 ---------------- */

export interface PlaylistSummary {
  id: string;
  name: string;
  description: string | null;
  trackCount: number;
  /** 曲目时长合计，用于显示 "1 小时 12 分" */
  durationMs: number;
  /** 自定义封面优先，否则取第一首曲目的封面 */
  artworkId: string | null;
  createdAt: number;
  updatedAt: number;
  sortPosition: number;
}

/* ---------------- 歌词 ---------------- */

export interface LyricLine {
  timeMs: number;
  text: string;
}

export interface Lyrics {
  found: boolean;
  /** 按时间升序；无时间轴的纯文本歌词 synced=false */
  lines: LyricLine[];
  /** LRC 的 [offset:] 校正值，已包含在 timeMs 里 */
  offsetMs: number;
  synced: boolean;
  encoding: string | null;
}

/* ---------------- 设置与诊断 ---------------- */

export type ThemeMode = 'dark' | 'light';
export type ListDensity = 'comfortable' | 'compact';

export const SHORTCUT_ACTIONS = [
  'playPause',
  'next',
  'prev',
  // 快退在前：设置页两列排布，读起来才是"快退 → 快进"
  'seekBackward',
  'seekForward',
  'volumeUp',
  'volumeDown',
  // 倍速紧挨音量：设置页两列排布读起来是"音量组 → 速率组 → 静音"
  'rateUp',
  'rateDown',
  'mute',
  'focusSearch',
  'toggleQueue',
  'toggleLyrics',
  'toggleShuffle',
  'cycleRepeat',
  'selectAll',
  'openSettings',
] as const;

export type ShortcutAction = (typeof SHORTCUT_ACTIONS)[number];

/**
 * 默认键位。改键只存覆盖项（AppSettings.shortcuts），
 * 这样后续调整默认值不会把用户自定义过的键位冲掉。
 */
export const DEFAULT_SHORTCUTS: Record<ShortcutAction, string> = {
  playPause: 'Space',
  next: 'Shift+ArrowRight',
  prev: 'Shift+ArrowLeft',
  seekForward: 'ArrowRight',
  seekBackward: 'ArrowLeft',
  volumeUp: 'Ctrl+ArrowUp',
  volumeDown: 'Ctrl+ArrowDown',
  rateUp: ']',
  rateDown: '[',
  mute: 'M',
  focusSearch: '/',
  toggleQueue: 'Ctrl+Shift+Q',
  toggleLyrics: 'Ctrl+Shift+L',
  toggleShuffle: 'S',
  cycleRepeat: 'R',
  selectAll: 'Ctrl+A',
  openSettings: 'Ctrl+,',
};

/** 覆盖项合并到默认键位；空字符串表示用户主动禁用该动作 */
export function resolveShortcutBindings(
  overrides: Partial<Record<ShortcutAction, string>> = {},
): Record<ShortcutAction, string> {
  const out = { ...DEFAULT_SHORTCUTS };
  for (const action of SHORTCUT_ACTIONS) {
    const value = overrides[action];
    if (typeof value === 'string') out[action] = value;
  }
  return out;
}

export const SHORTCUT_LABELS: Record<ShortcutAction, string> = {
  playPause: '播放 / 暂停',
  next: '下一首',
  prev: '上一首',
  seekForward: '快进 5 秒',
  seekBackward: '快退 5 秒',
  volumeUp: '音量 +5%',
  volumeDown: '音量 -5%',
  rateUp: '加速 0.1×',
  rateDown: '减速 0.1×',
  mute: '静音切换',
  focusSearch: '聚焦搜索框',
  toggleQueue: '播放队列面板',
  toggleLyrics: '歌词面板',
  toggleShuffle: '随机播放切换',
  cycleRepeat: '循环模式轮转',
  selectAll: '全选当前视图',
  openSettings: '打开设置',
};

export interface AppSettings {
  theme: ThemeMode;
  density: ListDensity;
  /** 从封面提取主色作为强调色；关闭则用固定主题色 */
  accentFromArtwork: boolean;
  /** 按 ReplayGain 调整音量（近似实现：换算到音量系数） */
  normalizeVolume: boolean;
  /**
   * 频谱可视化（ADR-037）。
   *
   * 开启时渲染层出口会惰性建音频图并返回 AnalyserNode 数据源；
   * 关闭时不建图，维持零额外对象的默认路径。
   */
  spectrum: boolean;
  /**
   * 频谱面板渲染引擎。
   *
   * compact：自研轻量条形（lib/spectrum.ts 纯函数，零依赖）；
   * pro：audioMotion-analyzer 专业渲染（径向/LED/镜像等，AGPL-3.0，见 docs/09）。
   */
  spectrumEngine: 'compact' | 'pro';
  /**
   * 系统媒体控制（Windows SMTC / 锁屏 / 蓝牙耳机）。
   *
   * 开启后由系统路由媒体键 —— 这也是为什么不再注册全局快捷键：
   * `globalShortcut` 一旦占用媒体键，Chromium 就不再向系统发布媒体会话，
   * 系统媒体面板会整个消失（实测）。
   */
  mediaKeys: boolean;
  /** 应用启动后自动播放上次的曲目 */
  autoplayOnStart: boolean;
  /** 记住上次播放进度；关闭则每次都从头开始 */
  rememberPosition: boolean;
  /** 播放时阻止系统休眠（允许熄屏） */
  preventSleep: boolean;
  /** 关闭窗口时最小化到托盘而不是退出 */
  closeToTray: boolean;
  shortcuts: Partial<Record<ShortcutAction, string>>;
}

export const DEFAULT_SETTINGS: AppSettings = {
  theme: 'dark',
  density: 'comfortable',
  accentFromArtwork: true,
  normalizeVolume: false,
  // 频谱默认关：不建音频图，保持最简出口路径
  spectrum: false,
  // 默认专业引擎：装饰观感优先；想要零依赖轻量路径可切回 compact
  spectrumEngine: 'pro',
  mediaKeys: true,
  // 启动就出声容易吓到人，默认关闭；想开的人在设置里开
  autoplayOnStart: false,
  // 进度记忆默认开：这是播放器的通行行为
  rememberPosition: true,
  // 默认开：否则整晚放歌会让电脑睡不着
  preventSleep: true,
  // 默认关：关窗即退出是既有行为，不制造意外
  closeToTray: false,
  shortcuts: {},
};

export interface Diagnostics {
  version: string;
  electron: string;
  platform: string;
  dataDir: string;
  schemaVersion: number;
  dbBytes: number;
  tracks: number;
  albums: number;
  artists: number;
  playlists: number;
  historyRows: number;
  totalDurationMs: number;
  /** 协议层的流计数：opened - closed 应始终为 0 */
  streams: { opened: number; closed: number; active: number };
  /** 最近若干次列表查询的耗时，用来发现索引退化 */
  listTimings: Array<{ at: number; rows: number; total: number; tookMs: number; source: string }>;
  recentErrors: Array<{ at: number; message: string }>;
  /** 快捷键冲突（同一组合绑定了多个动作） */
  shortcutConflicts: Array<{ binding: string; actions: ShortcutAction[] }>;
  /**
   * 系统媒体控制的开关状态。
   * "设置里开着"不等于"系统真收到了" —— 会话只在播放时出现，
   * 这条信息配合系统媒体面板一起看才有意义。
   */
  mediaKeys: { enabled: boolean; active: string[]; failed: string[] };
  /** 防休眠拦截器：active 为 true 表示当前不会进入系统睡眠 */
  power: { active: boolean; type: string | null };
  /** 托盘是否已创建；关闭"最小化到托盘"时不会有托盘图标 */
  tray: boolean;
  /**
   * 平台能力表（main/platform.ts 的运行时取值）。
   * 排查"为什么这个平台没有某功能"时只看这里，不用翻代码找分支。
   */
  platformCapabilities: {
    os: string;
    mediaIntegration: string;
    mediaKeysRoute: string;
    traySupported: boolean;
    systemLoopback: boolean;
  };
}

/**
 * beets 环境状态（sidecar status 的结构化结果；docs/08-beets.md B0）。
 * found=false 时播放功能不受影响，设置页只显示安装指引。
 */
export interface BeetsStatus {
  found: boolean;
  interpreter: string | null;
  scriptPath: string | null;
  protocolVersion: number | null;
  beetsVersion: string | null;
  pythonVersion: string | null;
  libraryPath: string | null;
  libraryExists: boolean | null;
  plugins: string[] | null;
  configDir: string | null;
  errorCode: string | null;
  errorMessage: string | null;
}

/* ---------------- beets 导入预览（docs/08 §3.2 事件协议） ---------------- */

/** 单曲级标签（当前与候选同形，UI 才能逐字段对齐） */
export interface TrackMeta {
  title: string | number | null;
  artist: string | null;
  album: string | null;
  albumartist: string | null;
  year: number | null;
  track: number | null;
  disc: number | null;
  genre: string | null;
  path?: string;
  length?: number;
}

export interface CandidatePreview {
  id: string;
  /** 0–1，1 - distance；低置信度 UI 不默认勾选 */
  confidence: number;
  fromScratch: boolean;
  extraItems: number;
  extraTracks: number;
  album: {
    id: string | null;
    album: string | null;
    artist: string | null;
    year: number | null;
    genre: string | null;
    dataSource: string | null;
    tracks: TrackMeta[];
  };
  /** item.path → 候选 TrackInfo（逐曲对齐） */
  mapping: Record<string, TrackMeta>;
}

export interface AlbumPreview {
  id: string;
  paths: string[];
  /** strong / medium / low / none；低置信度不默认勾选 */
  recommendation: 'strong' | 'medium' | 'low' | 'none';
  current: { album: string | null; artist: string | null; tracks: TrackMeta[] };
  candidates: CandidatePreview[];
}

/** `beets:import` 事件流（sidecar 逐行转发） */
export type BeetsImportEvent =
  | { type: 'progress'; session: string; stage: string; current: number; total: number; path: string | null }
  | { type: 'album'; session: string; album: AlbumPreview }
  | { type: 'albumError'; session: string; stage: string; path: string; beets_message: string }
  | { type: 'done'; session: string; albums: number }
  /* ---- B2：应用阶段 ---- */
  /** 单张专辑应用完成；failed 是逐曲失败明细（可重试），code=conflict 表示目标路径已存在 */
  | { type: 'applied'; session: string; albumId: string; written: number; moved: boolean; failed: Array<{ path: string; message: string; code?: string }> }
  | { type: 'applyDone'; session: string; applied: number; skipped: number; failed: number; moved: number }
  /* ---- B3：取消 ---- */
  /** 用户取消；session 收尾到最近的检查点为止（已完成的部分保留） */
  | { type: 'cancelled'; session: string; stage: string }
  /** sidecar 收到取消命令的回执 */
  | { type: 'cancelAck' }
  /* ---- 读模型对齐（beets 写权威 → 自有 DB 读模型，docs/08 §5 自闭环） ---- */
  /** beets 库 → 自有 DB 全量对齐汇总（apply 完成后自动触发，也可手动） */
  | { type: 'librarySynced'; matched: number; updated: number; imported: number; unchanged: number; orphaned: number }
  | { type: 'librarySyncFailed'; message: string }
  /** 正在扫描全库，本次对齐让位（可手动再触发） */
  | { type: 'libraryRefreshSkipped'; reason: string }
  | { type: 'sessionEnd' };

/**
 * B2 应用决策。candidate 数据由 UI 从预览结果**原样回传** ——
 * sidecar 不重跑网络匹配，保证"所见即所写"（这也是一次性进程下
 * apply 与 preview 无需共享 session 状态的原因）。
 */
export interface ApplyDecision {
  /** 对应 AlbumPreview.id */
  album: string;
  /**
   * apply：只写标签（docs/08 §4.1 默认，不移动文件）；
   * apply_move / apply_copy：写标签 + 移动/复制到 beets 库目录（显式选择），
   * 目标路径冲突的曲目不覆盖不改名，逐曲报 conflict 失败。
   */
  action: 'apply' | 'apply_move' | 'apply_copy' | 'skip';
  paths: string[];
  /** action=apply* 时必填 */
  candidate?: {
    album: CandidatePreview['album'];
    mapping: Record<string, TrackMeta>;
  } | null;
}

