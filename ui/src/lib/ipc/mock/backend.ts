/**
 * 浏览器预览的假后端（docs/07 §5 第 1–2 步）。
 *
 * 目的：不接 Rust 也能渲染完整 UI、验证样式/布局/交互回归。
 * 实现是有意的"假"：数据在内存里生成（固定 seed，每次刷新都一样），
 * 设置/视图状态进 localStorage。**不是**模拟器——它不假装能出声。
 *
 * 2026-10-25 批次 4：从 `compat/mock.ts` 搬来并适配新契约：
 * - `Track` 换成生成的 `TrackRow`（列表）与 `TrackDetail`（补 audio spec），
 *   玩家状态里的 `current` 用 `TrackRef`（不含 playCount 等列表字段）；
 * - 补齐 Rust 侧新增的必填字段（`availability` / `walkErrors` / `cursor` /
 *   `logDir` / `audioMode`）—— 少一个字段 TS 就红，这正是"生成类型当唯一来源"
 *   的好处。
 */
import type { AppSettings, ThemeMode } from '../../settings';
import { DEFAULT_SETTINGS } from '../../settings';
import type { TrackQuery, TrackQueryParsed } from '../../query';
import { parseTrackQuery } from '../../query';
import type { SelectionIdsQuery, QueueInsertMode } from '../client';
import type { UiState } from '../schema';

/**
 * 预览环境里"整理库文件"的假目录。
 *
 * 单独一个常量是因为它被两处共用：`beetsQuery` 造路径、`resolvePaths` 反解
 * 路径。两处各写一遍字符串时，改一处就静默变成"整页都未入库"。
 */
const MOCK_BEETS_DIR = 'D:/MUSIC_Player/preview';
import type {
  AlbumFacet,
  AlbumPage,
  ArtistFacet,
  ArtistPage,
  BeetsConfig,
  BeetsMarketItem,
  BeetsPluginItem,
  BeetsPythonProbe,
  BeetsFieldCatalog,
  BeetsQueryPage,
  Diagnostics,
  DroppedItem,
  EnrichmentMatch,
  EnrichmentWorklist,
  EqSettings,
  HistoryStats,
  LibraryRoots,
  LibraryStats,
  ListTiming,
  Lyrics,
  OpenPathsReceipt,
  PlayerState,
  PlaylistAddResult,
  PlaylistExportResult,
  PlaylistImportResult,
  PlaylistSummary,
  QueueSnapshot,
  RemoveTracks,
  RevealResult,
  RootsTrackCount,
  ScanCancel,
  ScanResult,
  TrackAudio,
  TrackDetail,
  TrackPage,
  TrackRef,
  TrackRow,
} from '../generated';

/* ---------------- 事件总线 ---------------- */

type Listener = (payload: unknown) => void;

/** 预览用内存事件总线：签名对齐 `transport().listen` 的底层（同步 unsubscribe） */
export class Emitter {
  private readonly listeners = new Map<string, Set<Listener>>();

  on(event: string, fn: Listener): () => void {
    let set = this.listeners.get(event);
    if (!set) {
      set = new Set();
      this.listeners.set(event, set);
    }
    set.add(fn);
    return () => {
      set.delete(fn);
    };
  }

  emit(event: string, payload: unknown): void {
    this.listeners.get(event)?.forEach((fn) => fn(payload));
  }
}

/* ---------------- 确定性假数据 ---------------- */

const ARTIST_POOL: Array<{ name: string; albums: string[] }> = [
  { name: '周深', albums: ['深的深', '大鱼'] },
  { name: '邓紫棋', albums: ['新的心跳', '摩天动物园'] },
  { name: '林俊杰', albums: ['伟大的渺小', '和自己对话'] },
  { name: '田馥甄', albums: ['日常', '渺小'] },
  { name: '陈奕迅', albums: ['C\'mon in~', '准备中'] },
  { name: '五月天', albums: ['自传', '后青春期的诗'] },
  { name: 'Taylor Swift', albums: ['1989', 'folklore'] },
  { name: 'YOASOBI', albums: ['THE BOOK'] },
  { name: 'ヨルシカ', albums: ['エルマ', '盗作'] },
  { name: '米津玄師', albums: ['STRAY SHEEP', 'BOOTLEG'] },
  { name: 'Aimer', albums: ['.mid dawn', 'walpurgis'] },
  { name: 'Coldplay', albums: ['Everyday Life', 'Music of the Spheres'] },
];

const TITLE_PARTS_A = ['夜行', '夏风', '纸飞机', '雨巷', '城南', '未寄', '候鸟', '灯塔', '薄荷', '长街', '微光', '绿洲', '潮汐', '枕边', '车站', '野火', '晨雾', '星图', '拾光', '晚风'];
const TITLE_PARTS_B = ['的信', '物语', '练习曲', '之后', '日记', '进行曲', '圆舞曲', '独白', '手记', '漫游', '物候', '絮语', '回声', '航路', '变奏', '慢板', '即景', '地图', '邮戳', '终点'];

function mulberry32(seed: number): () => number {
  let a = seed >>> 0;
  return () => {
    a |= 0;
    a = (a + 0x6d2b79f5) | 0;
    let t = Math.imul(a ^ (a >>> 15), 1 | a);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

/**
 * 假音频规格：按 id 序号取模，让预览页能同时看到**无损 / Hi-Res / 有损 /
 * 未知**四种徽章形态（批次 3 修好的 `lossless` 与 kbps 单位，正好靠它肉眼验）。
 */
function audioFor(id: string): TrackAudio {
  const n = Number.parseInt(id.slice(2), 10) || 0;
  switch (n % 4) {
    case 0:
      return { codec: 'FLAC', container: 'Flac', sampleRate: 44100, bitsPerSample: 16, bitrate: 900, channels: 2, lossless: true };
    case 1:
      return { codec: 'MPEG 1 Layer 3', container: 'Mpeg', sampleRate: 44100, bitsPerSample: 16, bitrate: 320, channels: 2, lossless: false };
    case 2:
      return { codec: 'FLAC', container: 'Flac', sampleRate: 96000, bitsPerSample: 24, bitrate: 2300, channels: 2, lossless: true };
    default:
      // 认不出（ALAC/AAC 共用 M4A 容器）：lossless = null，UI 显示"未知"
      return { codec: 'AAC', container: 'Mp4', sampleRate: 48000, bitsPerSample: 16, bitrate: 256, channels: 2, lossless: null };
  }
}

/**
 * 直播标题的缺省值：与 Rust 侧 `default_stream_title` 同一套口径。
 *
 * 末段像文件名（`index.m3u8`）就去掉扩展名当"台名"，否则退到主机名。
 * 抄一遍而不是让 UI 自己猜：真机下发的 `title` 已经是这个值，预览要是
 * 另算一套，同一串 URL 在两边会显示成不同的名字。
 */
function defaultStreamTitle(url: string): string {
  const stripped = url.split(/[?#]/)[0] ?? url;
  const tail = stripped.replace(/\/+$/, '').split('/').pop() ?? '';
  if (tail && tail.includes('.') && !tail.startsWith('http')) {
    const stem = tail.split('.')[0];
    if (stem) return stem;
  }
  const host = stripped.split('://')[1]?.split('/')[0];
  return host || url;
}

function toRef(track: TrackRow): TrackRef {
  return {
    id: track.id,
    title: track.title,
    artist: track.artist,
    album: track.album,
    artistId: track.artistId,
    albumId: track.albumId,
    durationMs: track.durationMs,
    artworkId: track.artworkId,
    gainDb: track.gainDb,
    audio: audioFor(track.id),
  };
}

function buildLibrary(): TrackRow[] {
  const rand = mulberry32(20261023);
  const tracks: TrackRow[] = [];
  const base = Date.UTC(2026, 8, 1) / 1000;
  // 流派池：筛选器的"流派"条件演练用（值域与曲库 genres 命令一致）
  const GENRE_POOL = ['流行', '民谣', '电子', '摇滚', '原声'];
  let n = 0;
  ARTIST_POOL.forEach((entry, ai) => {
    entry.albums.forEach((album, bi) => {
      const albumId = `al-${String(ai * 2 + bi + 1).padStart(2, '0')}`;
      const albumTracks = 8 + Math.floor(rand() * 6);
      for (let t = 1; t <= albumTracks; t++) {
        n += 1;
        const played = rand() < 0.35;
        tracks.push({
          id: `t-${String(n).padStart(4, '0')}`,
          title: `${TITLE_PARTS_A[Math.floor(rand() * TITLE_PARTS_A.length)]}${TITLE_PARTS_B[Math.floor(rand() * TITLE_PARTS_B.length)]}`,
          artistId: `ar-${String(ai + 1).padStart(2, '0')}`,
          artist: entry.name,
          albumId,
          album,
          albumArtist: entry.name,
          durationMs: 120000 + Math.floor(rand() * 200000),
          trackNo: t,
          discNo: 1,
          year: 2015 + Math.floor(rand() * 11),
          addedAtMs: Math.floor(base - rand() * 86400 * 180) * 1000,
          artworkId: null,
          playCount: played ? Math.floor(rand() * 60) : 0,
          lastPlayedAtMs: played ? Math.floor(base - rand() * 86400 * 60) : null,
          gainDb: null,
          // 假数据全部"在线"：预览里不需要模拟灰标，那要看真库的 missing 状态
          availability: 'online',
          genre: GENRE_POOL[Math.floor(rand() * GENRE_POOL.length)],
        });
      }
    });
  });
  return tracks;
}

/**
 * 歌单种子：摘要 + **内容**（playlistId → trackId[]）。
 *
 * 2026-10-25 之前只造摘要，歌单视图直接把全库按同一排序展示，拖动/移除只
 * 改计数不动内容 —— 版面能看，但"歌单语义"验不了。真实库是 playlist_items
 * 表，预览也建一份对应结构，代价只是一个 Map。
 */
function buildPlaylistSeed(tracks: TrackRow[]): {
  summaries: PlaylistSummary[];
  items: Map<string, string[]>;
} {
  const now = Date.now();
  const names = ['华语偏爱', '深夜自习室', '运动节拍', '通勤路上'];
  const items = new Map<string, string[]>();
  const summaries = names.map((name, i) => {
    const id = `pl-${i + 1}`;
    const slice = tracks.slice(i * 23, i * 23 + 12 + i * 5);
    items.set(
      id,
      slice.map((t) => t.id),
    );
    return {
      id,
      name,
      description: null,
      kind: 'manual',
      ruleJson: null,
      trackCount: slice.length,
      durationMs: slice.reduce((sum, t) => sum + (t.durationMs ?? 0), 0),
      artworkId: null,
      createdAtMs: now - 86400_000 * (10 - i),
      updatedAtMs: now - 3600_000 * (i + 1),
      sortPosition: i,
    };
  });
  return { summaries, items };
}

/* ---------------- localStorage 小工具 ---------------- */

function loadJson<T>(key: string, fallback: T): T {
  try {
    const raw = localStorage.getItem(key);
    return raw ? ({ ...fallback, ...(JSON.parse(raw) as object) } as T) : fallback;
  } catch {
    return fallback;
  }
}

function saveJson(key: string, value: unknown): void {
  try {
    localStorage.setItem(key, JSON.stringify(value));
  } catch {
    /* 隐私模式等场景：预览模式容忍 */
  }
}

/** mock 游标：base64url(JSON({ id }))；仅预览用，真实游标在 Rust 侧 */
function encodeCursorId(id: string): string {
  const bytes = new TextEncoder().encode(JSON.stringify({ id }));
  let binary = '';
  for (const b of bytes) binary += String.fromCharCode(b);
  return btoa(binary).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');
}

function decodeCursorId(cursor: string): string {
  const padded = cursor.replace(/-/g, '+').replace(/_/g, '/');
  const binary = atob(padded + '='.repeat((4 - (padded.length % 4)) % 4));
  const parsed = JSON.parse(new TextDecoder().decode(Uint8Array.from(binary, (c) => c.charCodeAt(0)))) as { id?: unknown };
  if (typeof parsed.id !== 'string') throw new Error('E_INVALID_CURSOR');
  return parsed.id;
}

/* ---------------- Mock 后端 ---------------- */

export class MockBackend {
  readonly emitter = new Emitter();

  /**
   * `beets_status` 报不报"已安装"。
   *
   * 默认 true：预览环境如果假装没装，维护四件套全是禁用态，事件流的渲染在
   * 浏览器里永远走不到（e2e 只能断言空态）。但"没装"这条路径本身也得能演练，
   * 所以留一个开关，由 `?beets=absent` 打开（见 main.ts）。
   */
  beetsFound = true;

  private tracks: TrackRow[] = buildLibrary();
  private readonly playlistSeed = buildPlaylistSeed(this.tracks);
  private playlists: PlaylistSummary[] = this.playlistSeed.summaries;
  /** 歌单内容（真实库对应 playlist_items 表） */
  private playlistItems: Map<string, string[]> = new Map(this.playlistSeed.items);
  private settings: AppSettings = loadJson('aiya.settings', DEFAULT_SETTINGS);
  private uiState: UiState = loadJson('aiya.uiState', {} as UiState);
  private look: { theme: ThemeMode } = loadJson('aiya.look', {
    theme: 'dark' as ThemeMode,
  });

  private playerState: PlayerState = {
    queueLength: 0,
    queueVersion: 0,
    index: null,
    cursor: null,
    repeat: 'off',
    shuffle: false,
    status: 'idle',
    source: 'file',
    positionMs: 0,
    durationMs: null,
    volume: 0.85,
    muted: false,
    rate: 1,
    error: null,
    current: null,
    token: 0,
    updatedAtMs: Date.now(),
  };
  private queueIds: string[] = [];
  /**
   * 队列里的临时项（`file:` / `stream:`）。真机由 `queue_snapshot()` 给出，
   * 预览里跟着 `playStream` 一起摆——队列面板靠它给这一类行画标题与来源标记。
   */
  private queueTransient: TrackRef[] = [];
  /** 频谱假帧定时器：只在 `spectrum_set(true)` 期间跑 */
  private spectrumTimer: ReturnType<typeof setInterval> | null = null;
  /**
   * 播放位置定时器：`playing` 期间按 ~10Hz 推进 positionMs（与真引擎的
   * 10Hz 上报同频），整秒变化才 emit `player:state`——暂停/空闲即停。
   */
  private stateTimer: ReturnType<typeof setInterval> | null = null;
  /**
   * 列表耗时环（诊断页）。真记录而不是塞假数据：预览里翻两页就能看到
   * 来源/行数/耗时列有值，验收"表格接上真值"这件事。
   * 最多 20 条，与宿主侧 `TIMING_LIMIT` 同口径。
   */
  private timings: ListTiming[] = [];

  /* ---------------- library ---------------- */

  libraryList(rawQuery: TrackQuery | undefined): TrackPage {
    const started = performance.now();
    const q: TrackQueryParsed = parseTrackQuery(rawQuery ?? { mode: 'cursor' });
    let rows = this.filter(q);
    const src = q.source;
    if (src.kind === 'playlist') {
      // 先按歌单内容过滤，再按用户拖出来的顺序铺底（sortForSource 会在其上覆盖排序）
      const ids = this.playlistItems.get(src.playlistId) ?? [];
      const order = new Map(ids.map((id, i) => [id, i]));
      rows = rows
        .filter((t) => order.has(t.id))
        .sort((a, b) => (order.get(a.id) ?? 0) - (order.get(b.id) ?? 0));
    }
    rows = this.sortForSource(rows, q);
    const total = rows.length;
    let start = 0;
    if (q.mode === 'offset') {
      start = q.offset;
    } else if (q.cursor) {
      // mock 游标：直接编码定位 id；语义与契约一致（只用于向后翻页）
      try {
        const decoded = decodeCursorId(q.cursor);
        const idx = rows.findIndex((t) => t.id === decoded);
        start = idx >= 0 ? idx + 1 : 0;
      } catch {
        start = 0;
      }
    }
    const items = rows.slice(start, start + q.limit);
    const nextCursor =
      q.mode === 'cursor' && start + q.limit < total && items.length > 0
        ? encodeCursorId(items[items.length - 1]!.id)
        : null;
    const tookMs = performance.now() - started;
    this.pushTiming({
      atMs: Date.now(),
      source: q.source.kind,
      rows: items.length,
      total,
      tookMs,
      // 预览没有 DB，SQL 耗时按六成编一个**成比例**的值（保持"排队成本"这一列
      // 有意义的形状），并在 UI 上无从区分——所以这段只在 mock chunk 里。
      dbTookMs: tookMs * 0.6,
      // 游标只在 cursor 模式的判别联合里存在（offset 模式没有这个字段）
      cursor: q.mode === 'cursor' ? (q.cursor ?? null) : null,
    });
    return {
      items,
      total,
      limit: q.limit,
      mode: q.mode,
      nextCursor,
      // 契约里 offset 是必填的 `number | null`（不是可选字段）
      offset: q.mode === 'offset' ? q.offset : null,
      tookMs: Math.max(1, Math.round(tookMs)),
    };
  }

  private pushTiming(timing: ListTiming): void {
    if (this.timings.length >= 20) this.timings.shift();
    this.timings.push(timing);
  }

  libraryTracks(ids: string[]): TrackDetail[] {
    const byId = new Map(this.tracks.map((t) => [t.id, t]));
    const out: TrackDetail[] = [];
    for (const id of ids) {
      const row = byId.get(id);
      if (row) out.push({ ...row, audio: audioFor(row.id) });
    }
    return out;
  }

  /**
   * 预览里 `library_stats` 报多少个"位置缺失"。
   *
   * 默认 0（假库的文件都在）。要给首页那条"位置缺失"告警留一条能走的路——
   * 它是全应用唯一还指向曲库维护的入口，2026-10-30 扫描入库页并入设置页之后
   * 这条路变成"设置 → 曲库"的一次性跳转，没有它就只能手验。`?missing=2` 打开。
   */
  missingLocations = 0;

  libraryStats(): LibraryStats {
    const albums = new Set(this.tracks.map((t) => t.albumId));
    const artists = new Set(this.tracks.map((t) => t.artistId));
    const durationMs = this.tracks.reduce((s, t) => s + (t.durationMs ?? 0), 0);
    return {
      tracks: this.tracks.length,
      albums: albums.size,
      artists: artists.size,
      locations: this.tracks.length,
      missingLocations: this.missingLocations,
      totalDurationMs: durationMs,
      // 假库不记文件大小（TrackDetail 也不带 sizeBytes）：按 320kbps 折算了
      // 一下——40 byte/ms。数值随曲库变化，截图里不会是一串 0，也不再需要
      // 为这一个字段编一个固定常量。
      totalBytes: Math.round(durationMs * 40),
      // 索引体积原本固定 0：预览里"索引大小 0 B"看着像库里没东西（真实库
      // 绝不可能是 0）。按每首约 1.6KB 估一个随曲库增长的数。
      dbBytes: Math.round(this.tracks.length * 1.6 * 1024),
      schemaVersion: 2,
    };
  }

  removeTracks(trackIds: string[]): { removed: number } {
    const gone = new Set(trackIds);
    const before = this.tracks.length;
    this.tracks = this.tracks.filter((t) => !gone.has(t.id));
    return { removed: before - this.tracks.length };
  }

  idsForSelection(request: SelectionIdsQuery): string[] {
    const q = parseTrackQuery({ ...request.query, mode: 'cursor' });
    const rows = this.sortForSource(this.filter(q), q);
    if (request.indices === 'all') return rows.map((t) => t.id);
    return request.indices
      .map((i) => rows[i]?.id)
      .filter((id): id is string => typeof id === 'string');
  }

  /**
   * 专辑/艺人聚合（真后端是 SQL GROUP BY，预览从内存曲目里聚合同一形状）。
   * 与真后端同口径：只含有曲目的条目、按名称排序、上限 500、响应带 total。
   */
  libraryAlbums(search?: string): AlbumPage {
    const needle = search?.trim().toLowerCase() || null;
    const map = new Map<string, AlbumFacet>();
    for (const t of this.tracks) {
      if (!t.albumId) continue;
      let f = map.get(t.albumId);
      if (!f) {
        f = {
          id: t.albumId,
          title: t.album ?? t.albumId,
          artistId: t.artistId,
          artist: t.albumArtist ?? t.artist,
          year: t.year,
          artworkId: t.artworkId,
          trackCount: 0,
          totalDurationMs: 0,
        };
        map.set(t.albumId, f);
      }
      f.trackCount += 1;
      f.totalDurationMs += t.durationMs ?? 0;
    }
    const all = [...map.values()]
      .filter((f) => !needle || f.title.toLowerCase().includes(needle))
      .sort((a, b) => a.title.localeCompare(b.title, 'zh') || a.id.localeCompare(b.id));
    return { items: all.slice(0, 500), total: all.length };
  }

  libraryArtists(search?: string): ArtistPage {
    const needle = search?.trim().toLowerCase() || null;
    const tracks = new Map<string, ArtistFacet>();
    const albums = new Map<string, Set<string>>();
    for (const t of this.tracks) {
      if (!t.artistId) continue;
      let f = tracks.get(t.artistId);
      if (!f) {
        f = {
          id: t.artistId,
          name: t.artist ?? t.artistId,
          artworkId: t.artworkId,
          trackCount: 0,
          albumCount: 0,
          totalDurationMs: 0,
        };
        tracks.set(t.artistId, f);
      }
      f.trackCount += 1;
      f.totalDurationMs += t.durationMs ?? 0;
      if (t.albumId) {
        let set = albums.get(t.artistId);
        if (!set) albums.set(t.artistId, (set = new Set()));
        set.add(t.albumId);
      }
    }
    const all = [...tracks.values()]
      .filter((f) => !needle || f.name.toLowerCase().includes(needle))
      .map((f) => ({ ...f, albumCount: albums.get(f.id)?.size ?? 0 }))
      .sort((a, b) => a.name.localeCompare(b.name, 'zh') || a.id.localeCompare(b.id));
    return { items: all.slice(0, 500), total: all.length };
  }

  revealTrack(_trackId: string): RevealResult {
    return { revealed: false, reason: '浏览器预览模式不可用' };
  }

  /**
   * beets 工具安装（docs/33 §6.2 两条轨道）的事件流演练。
   *
   * 形状按真后端逐字对齐：`progress`（stage + message）→ `error` 或 `done`
   * → `finished`（带 status）。这里刻意走**失败**分支：浏览器预览既没有系统
   * Python 也不会去下载 34MB 的套件包，装不成是事实，让 UI 的错误渲染也被
   * 执行到，比假装成功有用。两条轨道给不同的 stage 文案——真实运行时两者
   * 的步骤本来就不一样（A 建 venv / B 下载解压）。
   */
  async beetsInstall(
    track: 'installer' | 'bundle' = 'installer',
    plugins: string[] = [],
  ): Promise<{ started: boolean }> {
    const emit = (e: Record<string, unknown>) => this.emitter.emit('beets:install', e);
    if (track === 'bundle') {
      emit({ type: 'progress', stage: 'download', message: '下载套件包（预览模式模拟）' });
      await new Promise((r) => setTimeout(r, 250));
      emit({ type: 'progress', stage: 'extract', message: '解压到工具目录（预览模式模拟）' });
      await new Promise((r) => setTimeout(r, 250));
      emit({ type: 'error', stage: 'download', message: '浏览器预览不会真的下载套件包，这里是模拟失败' });
    } else {
      emit({
        type: 'progress',
        stage: 'download',
        message: `下载安装器（预览模式模拟；勾选 ${plugins.length} 个插件）`,
      });
      await new Promise((r) => setTimeout(r, 250));
      emit({ type: 'progress', stage: 'venv', message: '创建虚拟环境（预览模式模拟）' });
      await new Promise((r) => setTimeout(r, 250));
      emit({ type: 'error', stage: 'venv', message: '浏览器预览没有系统 Python，这里是模拟失败' });
    }
    emit({
      type: 'finished',
      status: {
        found: false,
        interpreter: null,
        scriptPath: null,
        entrySource: null,
        protocolVersion: null,
        beetsVersion: null,
        pythonVersion: null,
        pythonSupported: null,
        libraryPath: null,
        libraryExists: null,
        libraryDirectory: null,
        plugins: null,
        pluginHealth: null,
        pluginSummary: null,
        configDir: null,
        dataHome: null,
        errorCode: 'E_TOOL_NOT_FOUND',
        errorMessage: '浏览器预览不含 beets 工具',
      },
    });
    return { started: true };
  }

  /**
   * 应用导入决策的事件流演练。
   *
   * 必须发 `applied` + `applyDone`：会话 store 只在 `applyDone` 里收尾（归 idle、
   * 写汇总、记排除表）。不演这两条，整理页点完「应用」会永远停在"处理中"，
   * 按钮再也解不了锁——假数据自圆其说的最低要求。
   */
  async beetsApply(decisions: Array<{ album: string; action: string; paths?: string[] }>): Promise<{
    started: boolean;
  }> {
    const emit = (e: Record<string, unknown>) => this.emitter.emit('beets:import', e);
    const touched = decisions.filter((d) => d.action !== 'skip');
    const total = touched.length;
    let applied = 0;
    let moved = 0;

    for (let i = 0; i < total; i += 1) {
      const d = touched[i];
      emit({ type: 'progress', session: 'imp_01', stage: 'writing', current: i + 1, total, path: null });
      await new Promise((r) => setTimeout(r, 80));
      const willMove = d.action === 'apply_move' || d.action === 'apply_copy';
      if (willMove) moved += 1;
      applied += 1;
      emit({
        type: 'applied',
        session: 'imp_01',
        albumId: d.album,
        written: (d.paths ?? []).length || 1,
        moved: willMove,
        failed: [],
      });
    }

    emit({
      type: 'applyDone',
      session: 'imp_01',
      applied,
      skipped: decisions.length - touched.length,
      failed: 0,
      moved,
    });
    emit({ type: 'sessionEnd' });
    return { started: true };
  }

  /**
   * 导入预览的事件流演练（`beets:import`）。
   *
   * 为什么必须**发出专辑**而不是只发 done(0)：整理页的整块 UI（专辑卡、候选
   * 下拉、逐字段 diff、应用按钮）只在收到 album 事件后才会渲染。以前这条回
   * `albums: 0`，预览里那一页永远空白，改版式时只能靠想象——等于没有预览。
   *
   * 形状与 `beets-tool/src/aiya_beets/cmds/importer.py` 的 candidate_payload
   * 逐字对齐（album / mapping / evidence），字段少一个，真机就会和预览不一样。
   */
  async beetsPreview(): Promise<{ started: boolean }> {
    const emit = (e: Record<string, unknown>) => this.emitter.emit('beets:import', e);
    const dir = 'D:/MUSIC_Player/inbox';

    emit({ type: 'progress', session: 'imp_01', stage: 'scan', current: 0, total: 2, path: null });
    await new Promise((r) => setTimeout(r, 120));

    const albums = [
      {
        id: 'alb_000',
        paths: [`${dir}/Sample Album/01 - Intro.wav`, `${dir}/Sample Album/02 - Theme.wav`],
        recommendation: 'strong',
        current: {
          album: 'Sample Album',
          artist: 'Demo Artist',
          tracks: [
            { title: '01 - Intro', path: `${dir}/Sample Album/01 - Intro.wav`, artist: 'Demo Artist', album: 'Sample Album' },
            { title: '02 - Theme', path: `${dir}/Sample Album/02 - Theme.wav`, artist: 'Demo Artist', album: 'Sample Album' },
          ],
        },
        candidates: [
          {
            id: 'cand_000_00',
            confidence: 0.97,
            fromScratch: false,
            extraItems: 0,
            extraTracks: 0,
            evidence: 'recordingId',
            reasons: ['AcoustID 指纹命中 2/2 首'],
            album: {
              id: 'mbid-demo-1',
              album: 'Sample Album (Deluxe)',
              artist: 'Demo Artist',
              albumartist: 'Demo Artist',
              year: 2019,
              genre: 'Electronic',
              dataSource: 'MusicBrainz',
              tracks: [],
            },
            mapping: {
              [`${dir}/Sample Album/01 - Intro.wav`]: { title: 'Intro', artist: 'Demo Artist', album: 'Sample Album (Deluxe)' },
              [`${dir}/Sample Album/02 - Theme.wav`]: { title: 'Theme', artist: 'Demo Artist', album: 'Sample Album (Deluxe)' },
            },
          },
        ],
      },
      {
        id: 'alb_001',
        paths: [`${dir}/Loose Track.wav`],
        recommendation: 'low',
        current: { album: '', artist: '', tracks: [{ title: 'Loose Track', path: `${dir}/Loose Track.wav` }] },
        candidates: [
          {
            id: 'cand_001_00',
            confidence: 0.41,
            fromScratch: false,
            extraItems: 1,
            extraTracks: 1,
            evidence: 'similarity',
            reasons: ['仅标题相似，没有指纹或时长印证'],
            album: {
              id: 'mbid-demo-2',
              album: 'Unknown Collection',
              artist: 'Someone',
              year: null,
              genre: null,
              dataSource: 'MusicBrainz',
            },
            mapping: { [`${dir}/Loose Track.wav`]: { title: 'Loose Track', artist: 'Someone' } },
          },
        ],
      },
    ];

    for (const album of albums) {
      emit({ type: 'album', session: 'imp_01', album });
      await new Promise((r) => setTimeout(r, 100));
    }
    emit({ type: 'done', session: 'imp_01', albums: albums.length });
    return { started: true };
  }

  /**
   * 曲库维护事件流演练（docs/29 批 D）。
   *
   * 为什么预览里要**假装成功**：这四条命令的核心是"事件流怎么画"，而浏览器
   * 里没有 beets。真后端的事件形状写在 `sidecar/beets_sidecar.py`，这里
   * 逐字对齐（progress → 明细 → xxxDone），字段名一个都不能改——否则 e2e
   * 绿了、真机一片空白。
   */
  async beetsMaintenance(
    kind: 'adopt' | 'update' | 'duplicates' | 'move',
    pretend: boolean,
  ): Promise<{ started: boolean }> {
    const emit = (e: Record<string, unknown>) => this.emitter.emit('beets:maintenance', e);
    const stage = kind;
    const total = 3;
    // 展示用素材：`TrackRow` 没有 path（那是 DB 的行投影，路径不外传），
    // 这里按标题编一个像样的路径出来——事件里的 path 只是给人看的文本
    const sample = this.tracks.slice(0, total).map((t, i) => ({
      id: i + 1,
      path: `D:/MUSIC_Player/test-music/${t.artist ?? '未知艺人'}/${t.title}.mp3`,
      title: t.title ?? '',
      artist: t.artist ?? '',
      album: t.album ?? '',
      length: (t.durationMs ?? 0) / 1000,
      bitrate: 320,
      format: 'MP3',
      size: 1024,
    }));
    for (let i = 0; i <= total; i++) {
      emit({ type: 'progress', session: `${stage}_01`, stage, current: i, total, path: null });
      await new Promise((r) => setTimeout(r, 60));
    }
    if (kind === 'adopt') {
      emit({ type: 'adoptDone', session: 'adopt_01', total, added: total, skipped: 0, failed: 0, errors: [] });
    } else if (kind === 'update') {
      emit({
        type: 'updateDone',
        session: 'update_01',
        pretend,
        total,
        changed: sample.length,
        unchanged: 0,
        missing: 0,
        failed: 0,
        errors: [],
        details: sample.map((s) => ({ path: s.path, changes: { title: { from: `${s.title} (旧)`, to: s.title } } })),
        detailsTruncated: false,
      });
    } else if (kind === 'duplicates') {
      const pair = sample.slice(0, 2);
      if (pair.length === 2) {
        emit({ type: 'duplicateGroup', session: 'dup_01', key: [pair[0].title], count: 2, items: pair });
      }
      emit({
        type: 'duplicatesDone',
        session: 'dup_01',
        keys: ['title', 'artist'],
        totalItems: total,
        groups: 1,
        itemsInGroups: 2,
        truncated: false,
      });
    } else {
      for (const s of sample) {
        emit({
          type: 'moveItem',
          session: 'move_01',
          kind: pretend ? 'plan' : 'done',
          path: s.path,
          dest: s.path.replace(/[^/\\]+$/, `${s.artist} - ${s.title}.mp3`),
          status: 'move',
          reason: '',
        });
      }
      emit({
        type: 'moveDone',
        session: 'move_01',
        pretend,
        directory: 'D:/MUSIC_Player/test-music',
        total,
        planned: sample.length,
        moved: pretend ? 0 : sample.length,
        same: 0,
        conflict: 0,
        failed: 0,
        errors: [],
      });
    }
    return { started: true };
  }

  /**
   * 插件市场目录（docs/40 §3 白名单）。预览里放 3 项覆盖三种状态：
   * builtin ready / pypi ready / pypi 缺依赖——「安装」按钮只在第三种出现。
   * 内存态：装完翻绿，和真后端的复检语义一致。
   */
  private market: BeetsMarketItem[] = [
    { name: 'replaygain', note: '播放音量自动拉齐', source: 'builtin', packages: [], state: 'ready', enabled: false, missing: [] },
    { name: 'lyrics', note: '歌词抓取（LRC 字段可直供）', source: 'pypi:beautifulsoup4', packages: ['beautifulsoup4>=4.12,<5'], state: 'ready', enabled: false, missing: [] },
    // fetchart 只要 requests，而 requests 随 beets 装 —— packages 为空，
    // 所以 source 是 builtin 而不是 pypi（docs/51 §1 的口径）
    { name: 'fetchart', note: '封面抓取（本地缺图时补）', source: 'builtin', packages: [], state: 'missing_deps', enabled: false, missing: ['requests'] },
  ];

  /**
   * 索引增强（docs/45）：预览环境按曲目数派生出"算过一部分"的状态。
   *
   * 为什么不做成随机：卡片要显示的是"还剩多少要算"，随机数会让每次刷新
   * 数字都变，e2e 无法断言。假数据必须确定性——这是预览后端的硬要求。
   */
  async enrichmentWorklist(source?: string, includeEmpty = false): Promise<EnrichmentWorklist> {
    const total = this.tracks.length;
    const analyzed = Math.round(total * 0.6);
    const empty = Math.round(total * 0.1);
    const stale = Math.round(analyzed * 0.1);
    const missing = Math.max(0, total - analyzed - empty);
    return {
      source: source ?? 'xtractor',
      available: this.beetsFound,
      note: this.beetsFound
        ? null
        : '未找到 aiya-beets 组件：先安装组件，或检查设置里的组件路径',
      totalTracks: total,
      analyzed,
      empty,
      missing,
      stale,
      orphan: 0,
      work: missing + stale + (includeEmpty ? empty : 0),
      includeEmpty,
      // 真后端给的是绝对路径（卡片上显示"接下来要算哪几首"）；预览库里
      // TrackRow 不暴露 path，所以造一批同形状的假路径。
      preview: this.tracks.slice(0, 20).map((t) => `D:/MUSIC_Player/preview/${t.id}.flac`),
    };
  }

  async enrichmentQuery(
    filter: { mood?: string; bpmMin?: number; bpmMax?: number; limit?: number } = {},
  ): Promise<EnrichmentMatch[]> {
    const moods = [
      'mood_happy',
      'mood_electronic',
      'mood_relaxed',
      'mood_party',
      'mood_sad',
      'mood_acoustic',
      'mood_aggressive',
    ];
    const limit = filter.limit ?? 50;
    return this.tracks.slice(0, limit).map((t, i) => ({
      trackId: t.id,
      moodLabel: filter.mood ?? moods[i % moods.length],
      moodScore: Number((0.9 - (i % 5) * 0.1).toFixed(2)),
      bpm: 90 + ((i * 7) % 80),
      danceability: Number((0.3 + ((i * 0.13) % 0.7)).toFixed(3)),
      loudness: 0.62,
      analyzedAtMs: Date.now() - i * 60_000,
    }));
  }

  /**
   * 索引增强的演练事件流（`beets:enrich`）：逐条 enrichItem → enrichDone →
   * enrichPersisted。
   *
   * 三条终态事件的顺序是契约的一部分：`enrichDone` 说"工具跑了什么"，
   * `enrichPersisted` 说"库里真落了什么"。预览里也照这个顺序发，
   * 免得 UI 只在真后端上才处理正确。
   */
  async beetsEnrichStart(): Promise<{ started: boolean }> {
    const total = Math.min(5, this.tracks.length);
    const emit = (e: Record<string, unknown>) => this.emitter.emit('beets:enrich', e);
    void (async () => {
      emit({ type: 'progress', session: 'enr_01', stage: 'enrich', current: 0, total, path: null });
      for (let i = 0; i < total; i += 1) {
        await new Promise((r) => setTimeout(r, 200));
        emit({
          type: 'enrichItem',
          session: 'enr_01',
          path: `D:/MUSIC_Player/preview/${this.tracks[i].id}.flac`,
          source: 'probe',
          status: 'ok',
          fields: { bpm: 100 + i, danceability: 0.5 + i / 100 },
          message: null,
        });
      }
      emit({
        type: 'enrichDone',
        session: 'enr_01',
        total,
        succeeded: total,
        empty: 0,
        failed: 0,
        skipped: 0,
        sources: ['probe'],
      });
      emit({
        type: 'enrichPersisted',
        written: total,
        skipped: 0,
        notInLibrary: 0,
        deferred: 0,
        batches: 1,
      });
    })();
    return { started: true };
  }

  /**
   * 只读入库（`beets_index_import`）的演练事件流（`beets:index`）：
   * `progress`（按批）→ `indexDone`。
   *
   * 预览库里没有第二个数据库，所以"入库"只是把进度演一遍，不改变
   * `beetsQuery` 的返回（它本来就当索引库非空）。
   */
  async beetsIndexImport(): Promise<{ started: boolean }> {
    const total = this.tracks.length;
    const emit = (e: Record<string, unknown>) => this.emitter.emit('beets:index', e);
    void (async () => {
      const batches = 3;
      emit({ type: 'progress', session: 'idx_01', stage: 'import', current: 0, total, path: null });
      for (let b = 1; b <= batches; b += 1) {
        await new Promise((r) => setTimeout(r, 180));
        emit({
          type: 'progress',
          session: 'idx_01',
          stage: 'import',
          current: Math.round((total * b) / batches),
          total,
          path: null,
        });
      }
      emit({
        type: 'indexDone',
        session: 'idx_01',
        stage: 'import',
        files: total,
        items: total,
        roots: this.tracks.slice(0, 3).map((t) => `D:/MUSIC_Player/preview/${t.id}.flac`),
      });
    })();
    return { started: true };
  }

  /**
   * beets 查询页（docs/47/48）的预览数据。
   *
   * 查询条件被刻意**不实现**：预览库里没有 beets 的 SQL，假装支持 rules/raw
   * 会让前端在预览环境点出"有结果"而真机上 0 命中——那种假绿比不实现更贵。
   * （要验条件求值，去 `smoke:enrich`，那边跑的是真 sidecar。）
   *
   * 但两件事必须模拟得像：
   * 1. `path` 能反解出曲目 id —— 页面要拿它换 `trackId`，换不到就整页标灰；
   * 2. 末尾挂两条**只在 beets 库里存在**的条目（曲库没有对应文件），
   *    否则"未入库"那一行永远没有画面，只有真机能看见。
   */
  async beetsQuery(
    filter: { limit?: number; fields?: string[] } = {},
  ): Promise<BeetsQueryPage> {
    const limit = filter.limit ?? 50;
    const wanted = filter.fields ?? ['title', 'artist', 'album'];
    const detail: Record<string, (t: (typeof this.tracks)[number]) => unknown> = {
      title: (t) => t.title,
      artist: (t) => t.artist,
      album: (t) => t.album,
      genre: () => 'Jazz',
      year: () => 2004,
      bpm: (t) => 90 + (Number.parseInt(t.id.replace(/\D+/g, ''), 10) % 40),
    };
    const fieldsOf = (t: (typeof this.tracks)[number]) => {
      const out: Record<string, unknown> = {};
      for (const name of wanted) {
        const read = detail[name];
        if (read) out[name] = read(t);
      }
      return out;
    };

    const orphans = [
      { id: 9001, path: `${MOCK_BEETS_DIR}/beets-only-1.flac`, fields: { title: '只在整理库里的单曲', artist: '某艺人', album: '某专辑' } },
    ];
    // 曲目数可能已经吃满 limit：那样 orphans 会被 slice 掉，"未入库"那条分支
    // 在预览里就永远不出现（e2e 也就断言不到）。所以先给它们留出名额。
    const items = [
      ...this.tracks.slice(0, Math.max(0, limit - orphans.length)).map((t, i) => ({
        id: i + 1,
        path: `${MOCK_BEETS_DIR}/${t.id}.flac`,
        fields: fieldsOf(t),
      })),
      ...orphans,
    ].slice(0, Math.max(limit, 1));
    return {
      items,
      total: this.tracks.length + orphans.length,
      truncated: this.tracks.length + orphans.length > items.length,
    };
  }

  /**
   * 路径 → 曲目 id。预览库里的路径形状是 `…/preview/<trackId>.flac`，
   * 靠这个反解；反解不出（或曲目不存在）就是 `null` = 未入库。
   */
  async resolvePaths(paths: string[]): Promise<(string | null)[]> {
    const known = new Set(this.tracks.map((t) => t.id));
    return paths.map((p) => {
      const match = /([^/\\]+)\.flac$/.exec(p);
      const id = match?.[1];
      if (!id || !known.has(id) || !p.includes('/preview/')) return null;
      return id;
    });
  }

  async beetsFields(): Promise<BeetsFieldCatalog> {
    const builtin = ['title', 'artist', 'album', 'year', 'genre', 'bpm', 'length'];
    return {
      fields: builtin.map((name) => ({
        name,
        type: name === 'year' || name === 'bpm' ? 'integer' : 'string',
        source: 'builtin',
      })),
      sources: [
        { name: 'probe', state: 'ready' },
        { name: 'xtractor', state: 'unwired' },
      ],
    };
  }

  async beetsPluginCatalog(): Promise<BeetsMarketItem[]> {
    return this.market.map((m) => ({ ...m }));
  }

  /**
   * 内嵌插件表（docs/51 §1）：预览环境给一份精简版。
   *
   * 真宿主读的是编译期 `include_str!` 的 `plugins.json`；这里手写四条，
   * 刻意覆盖四种形态（要额外包 / 不需要包 / 需外部二进制 / 免配置包预置），
   * 好让安装页的勾选清单、包名标签、"另下 ffmpeg"标签都被渲染到。
   */
  async beetsPluginTable(): Promise<BeetsPluginItem[]> {
    return [
      {
        name: 'replaygain',
        title: '音量拉齐',
        note: '提供 gain_db，引擎链路直接消费',
        builtin: true,
        packages: [],
        exe: ['ffmpeg'],
        writes: true,
        network: false,
        bundle: false,
        default: true,
        indexDefault: false,
      },
      {
        name: 'lyrics',
        title: '歌词抓取',
        note: '抓取歌词，LRC 字段可直供',
        builtin: true,
        packages: ['beautifulsoup4>=4.12,<5'],
        exe: [],
        writes: false,
        network: true,
        bundle: true,
        default: true,
        indexDefault: false,
      },
      {
        name: 'chroma',
        title: '音频指纹',
        note: 'acoustid 指纹识别（需要 fpcalc，安装时自动下载）',
        builtin: true,
        packages: ['pyacoustid>=1.3,<2'],
        exe: ['fpcalc'],
        writes: false,
        network: true,
        bundle: false,
        default: false,
        indexDefault: false,
      },
      {
        name: 'xtractor',
        // 不再是"自备"：插件（beets-xtractor）与 Essentia 提取器都随免配置包发（docs/54）
        title: '音频特征',
        note: 'bpm / danceability / 响度（插件与 Essentia 提取器都随免配置包预置）',
        builtin: false,
        packages: ['beets-xtractor>=0.4.2,<0.5'],
        exe: ['streaming_extractor_music'],
        writes: true,
        network: false,
        bundle: true,
        default: false,
        indexDefault: true,
      },
    ];
  }

  /**
   * 本机 Python 探测：浏览器里没有这个概念，如实报"找不到"。
   *
   * 不做成假成功：安装页要根据这个结果决定按钮可不可点，
   * mock 说找得到而真机找不到，是最难查的那种"预览里好好的"。
   */
  async beetsPythonProbe(): Promise<BeetsPythonProbe> {
    return {
      found: false,
      path: null,
      version: null,
      supported: false,
      source: null,
      reason: '浏览器预览拿不到本机 Python；真应用会先试 py 启动器，再试 PATH 上的 python',
    };
  }

  /** 卸载组件：事件流形状与真后端一致（progress → done → finished）。 */
  async beetsUninstall(): Promise<{ started: boolean }> {
    const emit = (e: Record<string, unknown>) => this.emitter.emit('beets:install', e);
    emit({ type: 'progress', stage: 'uninstall', message: '将删除安装目录（预览模式模拟）' });
    await new Promise((r) => setTimeout(r, 300));
    emit({
      type: 'done',
      stage: 'uninstall',
      removed: true,
      files: 3765,
      bytes: 83518898,
      message: '组件已卸载（预览模式模拟）；整理库与分析库未改动',
    });
    emit({
      type: 'finished',
      status: {
        found: false,
        interpreter: null,
        scriptPath: null,
        entrySource: null,
        protocolVersion: null,
        beetsVersion: null,
        pythonVersion: null,
        pythonSupported: null,
        libraryPath: null,
        libraryExists: null,
        libraryDirectory: null,
        plugins: null,
        pluginHealth: null,
        pluginSummary: null,
        configDir: null,
        dataHome: null,
        errorCode: 'E_TOOL_NOT_FOUND',
        errorMessage: '浏览器预览不含 beets 工具',
      },
    });
    return { started: true };
  }

  async beetsPluginInstall(name: string): Promise<{ started: boolean }> {
    const emit = (e: Record<string, unknown>) => this.emitter.emit('beets:maintenance', e);
    const item = this.market.find((m) => m.name === name);
    if (!item) return { started: true };
    emit({ type: 'pluginProgress', session: 'plug_01', stage: 'pluginInstall', message: `安装 ${item.source.replace(/^pypi:/, '')}` });
    await new Promise((r) => setTimeout(r, 400));
    emit({ type: 'pluginProgress', session: 'plug_01', stage: 'pluginInstall', message: 'Running setup.py install for beautifulsoup4' });
    await new Promise((r) => setTimeout(r, 400));
    // 复检翻绿：pip 退出码 0 不等于可用，mock 也保持"装完必须复检"的口径
    item.state = 'ready';
    item.missing = [];
    emit({ type: 'pluginDone', session: 'plug_01', stage: 'pluginInstall', name, report: this.market.map((m) => ({ ...m })) });
    return { started: true };
  }

  /**
   * 预览环境的取消支持（八轮）：真后端走 scan_cancel 标志位，mock 用一个
   * 布尔等价物。每步之间轮询一次，置位后发 `cancelled` 终态事件并提前返回
   * ——与真后端"已完成批次保留、取消路径不跑对账"的语义对齐。
   */
  private scanRunning = false;
  private scanCancelRequested = false;

  scanCancel(): ScanCancel {
    if (!this.scanRunning) return { cancelled: false };
    this.scanCancelRequested = true;
    return { cancelled: true };
  }

  async scan(): Promise<ScanResult> {
    const started = Date.now();
    // 阶段顺序与真后端逐字对齐（docs/29 批 B 之后）：walking → parsing（解析与
    // 入库重叠）→ importing（解析全完、排队入库）→ done。
    // 旧版这里有一段 `artwork`：封面落盘并进解析 worker 之后它不再单独出现，
    // mock 留着会让"只在 mock 里才有"的状态白白多一个。
    //
    // `parsed > scanned` 是常态而不是 bug：解析比入库快，两者差值就是排队量，
    // 也正是 UI 要显示「解析 X · 已入库 Y」的理由。
    // 步长 400ms：给 e2e 的「取消任务」点击留出落在扫描中途的窗口。
    const total = this.tracks.length;
    const half = Math.floor(total / 2);
    const steps: { phase: string; scanned: number; parsed: number; imported: number }[] = [
      { phase: 'walking', scanned: 0, parsed: 0, imported: 0 },
      { phase: 'parsing', scanned: half, parsed: Math.floor(total * 0.75), imported: half },
      { phase: 'importing', scanned: total - 1, parsed: total, imported: total - 1 },
      { phase: 'done', scanned: total, parsed: total, imported: total },
    ];
    this.scanRunning = true;
    this.scanCancelRequested = false;
    try {
      for (const s of steps) {
        if (this.scanCancelRequested && s.phase !== 'done') {
          const cancelledAt = s.scanned;
          this.emitter.emit('scan:progress', {
            phase: 'cancelled',
            scanned: cancelledAt,
            parsed: s.parsed,
            total,
            imported: s.imported,
            updated: 0,
            skipped: 0,
            failed: 0,
            missing: 0,
            currentPath: null,
            elapsedMs: Date.now() - started,
          });
          return {
            scanned: cancelledAt,
            imported: s.imported,
            updated: 0,
            unchanged: 0,
            failed: 0,
            missing: 0,
            walkErrors: 0,
            cancelled: true,
            errors: [],
            elapsedMs: Date.now() - started,
          };
        }
        this.emitter.emit('scan:progress', {
          ...s,
          total,
          skipped: 0,
          failed: 0,
          currentPath: null,
          elapsedMs: Date.now() - started,
        });
        await new Promise((r) => setTimeout(r, 400));
      }
    } finally {
      this.scanRunning = false;
    }
    return {
      scanned: total,
      imported: total,
      updated: 0,
      unchanged: 0,
      failed: 0,
      missing: 0,
      walkErrors: 0,
      cancelled: false,
      errors: [],
      elapsedMs: Date.now() - started,
    };
  }

  /* ---------------- 扫描根列表（docs/29 批 A：多目录并集单库） ---------------- */

  private scanRoots: string[] = ['D:/MUSIC_Player/test-music'];

  rootsGet(): LibraryRoots {
    return { roots: [...this.scanRoots] };
  }

  /** 与真后端同语义：trim / 去空行 / 去引号 / 大小写不敏感去重 */
  rootsSet(roots: string[]): LibraryRoots {
    const cleaned: string[] = [];
    for (const item of roots) {
      const s = item
        .trim()
        .replace(/^["']+|["']+$/g, '')
        .trim();
      if (!s) continue;
      if (!cleaned.some((r) => r.toLowerCase() === s.toLowerCase())) cleaned.push(s);
    }
    this.scanRoots = cleaned;
    return { roots: [...cleaned] };
  }

  /** mock 库没有真实 path_lower 数据，按目录名出现次数给个示意值 */
  rootsCount(paths: string[]): RootsTrackCount {
    return { count: paths.length * 3 };
  }

  rootsPrune(paths: string[]): RemoveTracks {
    void paths;
    return { removed: 0 };
  }

  /* ---------------- beets 配置（docs/49 §4）：inbox / 落点 / 排除表 ----------------
   * 与真后端同语义的校验：
   * - inbox 与任何曲库目录**任一方向**重叠都拒（在里面 → copy 的原件被扫；
   *   是父目录 → 预览把整个曲库再导一遍）；
   * - 落点必须在曲库目录**之内**（可以是子目录，不必相等）。
   * mock 曲库根固定 D:/MUSIC_Player/test-music，e2e 用别的目录即可绕开校验。 */

  private beetsInbox: string | null = null;
  private beetsTarget: string | null = null;
  private inboxImported: string[] = [];

  /** Windows 路径比较归一（与 Rust canon_path 同语义） */
  private static normPath(p: string): string {
    return p.trim().replaceAll('/', '\\').replace(/\\+$/, '').toLowerCase();
  }

  /** 重叠：相等 / a 在 b 里 / b 在 a 里（与 Rust paths_overlap 同语义） */
  private static overlaps(a: string, b: string): boolean {
    const ca = MockBackend.normPath(a);
    const cb = MockBackend.normPath(b);
    if (!ca || !cb) return false;
    return ca === cb || ca.startsWith(`${cb}\\`) || cb.startsWith(`${ca}\\`);
  }

  /** 在内或相等（含分隔符边界，防 D:\Music2 冒充 D:\Music 的子目录） */
  private static within(a: string, b: string): boolean {
    const ca = MockBackend.normPath(a);
    const cb = MockBackend.normPath(b);
    if (!ca || !cb) return false;
    return ca === cb || ca.startsWith(`${cb}\\`);
  }

  beetsConfigGet(): BeetsConfig {
    const roots = this.scanRoots;
    const target = this.beetsTarget;
    const effective =
      target && roots.some((r) => MockBackend.within(target, r)) ? target : (roots[0] ?? null);
    return {
      inbox: this.beetsInbox,
      targetRoot: target,
      effectiveTarget: effective,
      importedCount: this.inboxImported.length,
    };
  }

  beetsSetInbox(path: string | null): BeetsConfig {
    const p = path?.trim();
    if (p) {
      if (this.scanRoots.some((r) => MockBackend.overlaps(p, r))) {
        throw new Error('待整理目录不能与曲库目录重叠（相同、在里面、或是它的上级目录都不行）');
      }
      this.beetsInbox = p;
    } else {
      this.beetsInbox = null;
    }
    this.inboxImported = [];
    return this.beetsConfigGet();
  }

  beetsSetTarget(path: string): BeetsConfig {
    const p = path.trim();
    if (!this.scanRoots.some((r) => MockBackend.within(p, r))) {
      throw new Error('整理落点必须在曲库目录之内（可以选曲库目录本身或它的子目录）');
    }
    this.beetsTarget = p;
    return this.beetsConfigGet();
  }

  beetsInboxImportedAdd(paths: string[]): BeetsConfig {
    for (const raw of paths) {
      const p = raw.trim();
      if (!p) continue;
      const pc = MockBackend.normPath(p);
      if (!this.inboxImported.some((e) => MockBackend.normPath(e) === pc)) {
        this.inboxImported.push(p);
      }
    }
    if (this.inboxImported.length > 2000) {
      this.inboxImported = this.inboxImported.slice(-2000);
    }
    return this.beetsConfigGet();
  }

  /**
   * 纯目录选择器（`pick_folder`）。浏览器里没有系统对话框，所以回假路径。
   *
   * **交替返回**两个目录：待整理目录需要一个曲库之外的，整理落点需要一个在
   * 曲库之内的——同一个路径不可能同时满足（这正是不该复用二者的原因）。
   * 交替让"选待整理目录"和"选落点"两条路在预览里都能各走完一次。
   */
  private pickSeq = 0;

  pickFolder(): string {
    // 两个干净路径（不拼 `..`）：展示用，也免得规范化差异让校验结论变得难猜
    const root = this.scanRoots[0] ?? 'D:/MUSIC_Player/test-music';
    this.pickSeq += 1;
    return this.pickSeq % 2 === 1 ? 'D:/MUSIC_Player/inbox' : `${root}/整理`;
  }

  /** 清空整个曲库记录（真后端级联清歌单条目/历史；mock 只清 tracks） */
  clearLibrary(): RemoveTracks {
    const removed = this.tracks.length;
    this.tracks = [];
    return { removed };
  }

  /* ---------------- playlists ---------------- */

  playlistList(): PlaylistSummary[] {
    return this.playlists;
  }

  playlistDetail(id: string): PlaylistSummary | null {
    return this.playlists.find((p) => p.id === id) ?? null;
  }

  playlistCreate(name: string, trackIds?: string[], rule?: Record<string, unknown>): PlaylistSummary {
    const now = Date.now();
    const isSmart = rule != null;
    const summary: PlaylistSummary = {
      id: `pl-${now}`,
      name,
      description: null,
      kind: isSmart ? 'smart' : 'manual',
      ruleJson: isSmart ? JSON.stringify(rule) : null,
      trackCount: isSmart ? 0 : (trackIds?.length ?? 0),
      durationMs: 0,
      artworkId: null,
      createdAtMs: now,
      updatedAtMs: now,
      sortPosition: this.playlists.length,
    };
    // 物化模型（2026-10-30）：动态歌单建单即按规则入库，与手动歌单同一张表
    this.playlistItems.set(summary.id, isSmart ? this.matchRule(rule) : [...(trackIds ?? [])]);
    this.playlists.push(summary);
    return this.syncPlaylist(summary.id) ?? summary;
  }

  /** 规则求值（mock 版）：字段/操作符与 smart.ts、后端 parse_conds 同一套键 */
  private matchRule(rule: Record<string, unknown> | undefined): string[] {
    if (!rule) return [];
    const conds = (rule['all'] as Array<Record<string, unknown>> | undefined) ?? [];
    const hits = this.tracks.filter((t) =>
      conds.every((c) => {
        const field = String(c['field']);
        const op = String(c['op']);
        const value = c['value'];
        switch (field) {
          case 'genre': {
            const v = String(value).toLowerCase();
            const g = (t.genre ?? '').toLowerCase();
            return op === 'eq' ? g === v : g.includes(v);
          }
          case 'artist':
            return (t.artist ?? '').toLowerCase().includes(String(value).toLowerCase());
          case 'album':
            return (t.album ?? '').toLowerCase().includes(String(value).toLowerCase());
          case 'year':
            return t.year != null && (op === 'gte' ? t.year >= Number(value) : t.year <= Number(value));
          case 'playCount':
            return op === 'gte' ? t.playCount >= Number(value) : t.playCount <= Number(value);
          default:
            return false;
        }
      }),
    );
    return hits.map((t) => t.id);
  }

  playlistSetRule(id: string, rule: Record<string, unknown>): PlaylistSummary | null {
    const p = this.playlists.find((x) => x.id === id);
    if (!p || p.kind !== 'smart') return p ?? null;
    p.ruleJson = JSON.stringify(rule);
    p.updatedAtMs = Date.now();
    // 替换重算：清空现有曲目、按新规则重新物化（与宿主同语义）
    this.playlistItems.set(id, this.matchRule(rule));
    return this.syncPlaylist(id);
  }

  /** 刷新动态歌单：重跑规则只追加新命中（块保持规则顺序，顶到最前） */
  playlistRefresh(id: string): { added: number; total: number } {
    const p = this.playlists.find((x) => x.id === id);
    if (!p || p.kind !== 'smart') throw new Error('E_NOT_FOUND');
    const rule = JSON.parse(p.ruleJson ?? '{}') as Record<string, unknown>;
    const current = this.playlistItems.get(id) ?? [];
    const known = new Set(current);
    const fresh = this.matchRule(rule).filter((tid) => !known.has(tid));
    this.playlistItems.set(id, [...fresh, ...current]);
    this.syncPlaylist(id);
    return { added: fresh.length, total: current.length + fresh.length };
  }

  /** 按内容表重算计数/时长（真实库是 DB 侧维护，预览手算） */
  private syncPlaylist(id: string): PlaylistSummary | null {
    const p = this.playlists.find((x) => x.id === id);
    if (!p) return null;
    const ids = this.playlistItems.get(id) ?? [];
    const byId = new Map(this.tracks.map((t) => [t.id, t]));
    p.trackCount = ids.length;
    p.durationMs = ids.reduce((sum, tid) => sum + (byId.get(tid)?.durationMs ?? 0), 0);
    p.updatedAtMs = Date.now();
    return p;
  }

  playlistRename(id: string, name: string): PlaylistSummary | null {
    const p = this.playlists.find((x) => x.id === id);
    if (!p) return null;
    p.name = name;
    p.updatedAtMs = Date.now();
    return p;
  }

  playlistRemove(id: string): { deleted: boolean } {
    const before = this.playlists.length;
    this.playlists = this.playlists.filter((p) => p.id !== id);
    const deleted = this.playlists.length < before;
    if (deleted) this.playlistItems.delete(id);
    return { deleted };
  }

  playlistReorder(from: number, to: number): PlaylistSummary[] {
    if (from < 0 || from >= this.playlists.length) return this.playlists;
    const [moved] = this.playlists.splice(from, 1);
    if (moved) this.playlists.splice(Math.max(0, Math.min(to, this.playlists.length)), 0, moved);
    this.playlists.forEach((p, i) => (p.sortPosition = i));
    return this.playlists;
  }

  playlistAddTracks(id: string, trackIds: string[]): PlaylistAddResult {
    const p = this.playlists.find((x) => x.id === id);
    if (!p) throw new Error('E_NOT_FOUND');
    const current = this.playlistItems.get(id) ?? [];
    const known = new Set(current);
    // 去重语义与宿主一致：已在歌单里的算 skipped；首行插入（块保持入参顺序）
    const fresh = trackIds.filter((tid) => !known.has(tid));
    this.playlistItems.set(id, [...fresh, ...current]);
    const summary = this.syncPlaylist(id) ?? p;
    return { added: fresh.length, skipped: trackIds.length - fresh.length, summary };
  }

  playlistRemoveAt(id: string, position: number): PlaylistSummary | null {
    const ids = this.playlistItems.get(id);
    if (!ids) return this.playlistDetail(id);
    if (position < 0 || position >= ids.length) return this.playlistDetail(id);
    ids.splice(position, 1);
    return this.syncPlaylist(id);
  }

  playlistMove(id: string, from: number, to: number): PlaylistSummary | null {
    const ids = this.playlistItems.get(id);
    if (!ids) return this.playlistDetail(id);
    if (from < 0 || from >= ids.length) return this.playlistDetail(id);
    const [moved] = ids.splice(from, 1);
    // to 是"移动后应处的下标"：删掉 from 之后长度少一，所以上限是 length
    if (moved !== undefined) ids.splice(Math.max(0, Math.min(to, ids.length)), 0, moved);
    return this.syncPlaylist(id);
  }

  playlistClear(id: string): PlaylistSummary | null {
    if (!this.playlistItems.has(id)) return this.playlistDetail(id);
    this.playlistItems.set(id, []);
    return this.syncPlaylist(id);
  }

  /* ---------------- m3u 导入导出（docs/22 G2） ---------------- */

  /**
   * 预览不吃真文件：不传 `path` 就按"用户取消"回（浏览器没有系统文件选择器，
   * 与 `library_choose_root` 同语义）；传了就造一份**假导入结果**——真落一个
   * 歌单、真带一条未匹配项，让 UI 的结果弹层（N 匹配 / M 未匹配）有东西可渲染。
   * 假数据仅此一处，字段语义与真宿主一致。
   */
  playlistImport(path?: string): PlaylistImportResult {
    if (!path) return { playlist: null, matched: 0, duplicates: 0, unmatched: [] };
    const name = path.split(/[\\/]/).pop()?.replace(/\.[^.]+$/, '') || '导入的歌单';
    const picked = this.tracks.slice(0, 3).map((t) => t.id);
    const summary = this.playlistCreate(name, picked);
    return {
      playlist: summary,
      matched: picked.length,
      duplicates: 0,
      unmatched: [{ line: 4, raw: 'D:/music/预览专用-未匹配.flac', reason: 'notFound' }],
    };
  }

  /**
   * 预览不写盘（浏览器沙箱里没有任意路径写权限），只回正确的形状让 UI 的
   * 成功提示/计数能验；`count` 取自当前歌单真实条数。
   */
  playlistExport(id: string, path: string): PlaylistExportResult {
    const count = this.playlistItems.get(id)?.length ?? 0;
    if (count === 0) throw new Error('歌单是空的，没有可导出的曲目');
    return { path, count };
  }

  /* ---------------- lyrics / history ---------------- */

  lyricsGet(_trackId: string): Lyrics {
    void _trackId;
    return { found: false, lines: [], offsetMs: 0, synced: false, encoding: null };
  }

  historyStats(): HistoryStats {
    const played = this.tracks.filter((t) => t.playCount > 0);
    return {
      totalPlays: played.reduce((s, t) => s + t.playCount, 0),
      totalMs: played.reduce((s, t) => s + (t.durationMs ?? 0) * t.playCount, 0),
      completed: played.length,
      skipped: 0,
      firstPlayAtMs: played.length
        ? Math.min(...played.map((t) => t.lastPlayedAtMs ?? Number.POSITIVE_INFINITY))
        : null,
      lastPlayAtMs: played.length ? Math.max(...played.map((t) => t.lastPlayedAtMs ?? 0)) : null,
      topTracks: [...played]
        .sort((a, b) => b.playCount - a.playCount)
        .slice(0, 10)
        .map((t) => ({ id: t.id, title: t.title, artist: t.artist, playCount: t.playCount })),
    };
  }

  historyClear(): { cleared: boolean } {
    this.tracks = this.tracks.map((t) => ({ ...t, playCount: 0, lastPlayedAtMs: null }));
    return { cleared: true };
  }

  /* ---------------- player ---------------- */

  private emitPlayer(): void {
    this.playerState = { ...this.playerState, updatedAtMs: Date.now() };
    this.syncStateTimer();
    this.emitter.emit('player:state', this.playerState);
  }

  /** 播放状态变了就同步位置定时器的启停（所有命令都经 emitPlayer 出口） */
  private syncStateTimer(): void {
    if (this.playerState.status === 'playing' && this.stateTimer === null) {
      this.stateTimer = setInterval(() => this.tickPosition(), 100);
    } else if (this.playerState.status !== 'playing' && this.stateTimer !== null) {
      clearInterval(this.stateTimer);
      this.stateTimer = null;
    }
  }

  /**
   * 播放中的位置推进：暂停/空闲时定时器已清，不会走到这。
   *
   * 到曲尾按 repeat/shuffle 语义推进，对齐 actor（发布前审查 P1-8）：此前
   * 是"当前曲目循环"，整专辑连播、随机切歌、队列播完停止这三条核心路径
   * 在 e2e（全跑 mock）里永远走不到。自动连播先落 playQueue（paused + 归零）
   * 再翻回 playing——同步块内连发两版状态，与真机"Eof → Load"的两拍一致。
   */
  private tickPosition(): void {
    const duration = this.playerState.durationMs;
    const position = this.playerState.positionMs + Math.round(100 * this.playerState.rate);
    if (duration == null || duration <= 0 || position < duration) {
      const secondBefore = Math.floor(this.playerState.positionMs / 1000);
      this.playerState = { ...this.playerState, positionMs: position, updatedAtMs: Date.now() };
      // 整秒才广播：10Hz 的内部推进不值得各层 rAF 都醒一次
      if (Math.floor(position / 1000) !== secondBefore) {
        this.emitter.emit('player:state', this.playerState);
      }
      return;
    }
    // ---- 曲尾（直播无 duration，到不了这）----
    if (this.playerState.repeat === 'one') {
      this.playerState = { ...this.playerState, positionMs: 0, updatedAtMs: Date.now() };
      return;
    }
    if (this.playerState.shuffle) {
      const n = this.queueIds.length;
      if (n <= 1) {
        this.playerState = { ...this.playerState, positionMs: 0, updatedAtMs: Date.now() };
        return;
      }
      // 随机挑一个非当前的下标：[0, n-1] 均匀采样后跳过当前
      let pick = Math.floor(Math.random() * (n - 1));
      const cur = this.playerState.index ?? 0;
      if (pick >= cur) pick += 1;
      this.playQueue(this.queueIds, pick);
      this.playerState = { ...this.playerState, status: 'playing' };
      this.emitPlayer();
      return;
    }
    const next = (this.playerState.index ?? 0) + 1;
    if (next < this.queueIds.length) {
      this.playQueue(this.queueIds, next);
      this.playerState = { ...this.playerState, status: 'playing' };
      this.emitPlayer();
    } else if (this.playerState.repeat === 'all') {
      this.playQueue(this.queueIds, 0);
      this.playerState = { ...this.playerState, status: 'playing' };
      this.emitPlayer();
    } else {
      // 队列播完（repeat=off）：停在最后一首结尾、转 paused
      this.playerState = { ...this.playerState, status: 'paused', positionMs: duration };
      this.emitPlayer();
    }
  }

  playerStateNow(): PlayerState {
    return this.playerState;
  }

  playerQueue(): QueueSnapshot {
    return {
      ids: this.queueIds,
      // QueueSnapshot 的 index/cursor 是 order 位置（number）；空队列时 ids
      // 本来就是空的，兜底 0 无观察者。
      index: this.playerState.index ?? 0,
      cursor: this.playerState.cursor ?? 0,
      // 临时项（直播/拖入的文件）不在曲库里，标题只能由快照自己带
      transient: this.queueTransient,
      version: this.playerState.queueVersion,
      tookMs: 0,
    };
  }

  private playQueue(ids: string[], startIndex: number): PlayerState {
    this.queueIds = ids;
    this.queueTransient = [];
    const current = this.tracks.find((t) => t.id === ids[startIndex]) ?? null;
    this.playerState = {
      ...this.playerState,
      queueLength: ids.length,
      queueVersion: this.playerState.queueVersion + 1,
      index: ids.length ? startIndex : null,
      cursor: ids.length ? startIndex : null,
      status: ids.length ? 'paused' : 'idle',
      current: current ? toRef(current) : null,
      durationMs: current?.durationMs ?? null,
      positionMs: 0,
      token: this.playerState.token + 1,
      error: null,
    };
    this.emitPlayer();
    return this.playerState;
  }

  playTracks(trackIds: string[], startIndex = 0): PlayerState {
    this.playQueue([...trackIds], startIndex);
    // 对齐 actor：QueueOp::ReplaceAll → instruct_load(true, 0)，点播即出声。
    // 此前 mock 停在 paused，与真机行为漂移（CDP 真窗口实测发现）。
    return this.setPlayingIf(true);
  }

  playQuery(query: TrackQuery, startIndex = 0): PlayerState {
    const q = parseTrackQuery(query);
    const rows = this.sortForSource(this.filter(q), q);
    this.playQueue(rows.map((t) => t.id), startIndex);
    return this.setPlayingIf(true);
  }

  /** playQueue 落在 paused；命令级起播语义由调用方决定（见 actor.rs 各分支） */
  private setPlayingIf(autoplay: boolean): PlayerState {
    if (autoplay && this.playerState.status === 'paused' && this.playerState.index != null) {
      this.playerState = { ...this.playerState, status: 'playing' };
      this.emitPlayer();
    }
    return this.playerState;
  }

  toggle(): PlayerState {
    if (this.playerState.status === 'playing') {
      // 直播：暂停 = 断开连接，位置归零（与 `actor.rs` 的 Stream 分支一致）
      const disconnecting = this.playerState.source === 'stream';
      this.playerState = {
        ...this.playerState,
        status: 'paused',
        positionMs: disconnecting ? 0 : this.playerState.positionMs,
      };
    } else if (this.playerState.status === 'paused') {
      this.playerState = { ...this.playerState, status: 'playing' };
    }
    this.emitPlayer();
    return this.playerState;
  }

  next(): PlayerState {
    return this.step(1);
  }

  prev(): PlayerState {
    return this.step(-1);
  }

  private step(delta: number): PlayerState {
    if (!this.queueIds.length) return this.playerState;
    // 对齐 actor 的 Next/Prev：暂停中切曲保持暂停，播放中切曲保持播放
    // （"暂停中点下一首应保持暂停（主流播放器行为）"——actor.rs 同注释）
    const wasPlaying = this.playerState.status === 'playing';
    // index 现在是 number | null（空队列=null）；走到这 queueIds 非空，
    // 但防御式兜底 0，别让 null 参与算术得 NaN。
    let next = (this.playerState.index ?? 0) + delta;
    if (next < 0) next = 0;
    if (next >= this.queueIds.length) {
      next = this.playerState.repeat === 'all' ? 0 : this.queueIds.length - 1;
    }
    this.playQueue(this.queueIds, next);
    return this.setPlayingIf(wasPlaying);
  }

  seek(positionMs: number): PlayerState {
    this.playerState = { ...this.playerState, positionMs: Math.max(0, positionMs) };
    this.emitPlayer();
    return this.playerState;
  }

  setVolume(volume: number): PlayerState {
    this.playerState = { ...this.playerState, volume: Math.min(1, Math.max(0, volume)) };
    this.emitPlayer();
    return this.playerState;
  }

  toggleMute(): PlayerState {
    this.playerState = { ...this.playerState, muted: !this.playerState.muted };
    this.emitPlayer();
    return this.playerState;
  }

  setRate(rate: number): PlayerState {
    this.playerState = { ...this.playerState, rate };
    this.emitPlayer();
    return this.playerState;
  }

  setRepeat(repeat: PlayerState['repeat']): PlayerState {
    this.playerState = { ...this.playerState, repeat };
    this.emitPlayer();
    return this.playerState;
  }

  setShuffle(shuffle: boolean): PlayerState {
    this.playerState = { ...this.playerState, shuffle };
    this.emitPlayer();
    return this.playerState;
  }

  enqueue(trackIds: string[], options?: QueueInsertMode): PlayerState {
    if (options?.mode === 'next' && this.playerState.index != null) {
      this.queueIds.splice(this.playerState.index + 1, 0, ...trackIds);
    } else {
      this.queueIds.push(...trackIds);
    }
    this.playerState = {
      ...this.playerState,
      queueLength: this.queueIds.length,
      queueVersion: this.playerState.queueVersion + 1,
    };
    this.emitPlayer();
    return this.playerState;
  }

  insertNext(trackId: string): PlayerState {
    return this.enqueue([trackId], { mode: 'next' });
  }

  removeAt(index: number): PlayerState {
    this.queueIds.splice(index, 1);
    const cur = this.playerState.index;
    this.playerState = {
      ...this.playerState,
      queueLength: this.queueIds.length,
      queueVersion: this.playerState.queueVersion + 1,
      // 删掉当前之前的行 → 当前下标左移一格；删后队列空则 null
      index:
        cur == null
          ? null
          : this.queueIds.length === 0
            ? null
            : index < cur
              ? Math.max(0, cur - 1)
              : Math.min(cur, this.queueIds.length - 1),
    };
    this.emitPlayer();
    return this.playerState;
  }

  move(from: number, to: number): PlayerState {
    const [moved] = this.queueIds.splice(from, 1);
    if (moved) this.queueIds.splice(to, 0, moved);
    this.playerState = { ...this.playerState, queueVersion: this.playerState.queueVersion + 1 };
    this.emitPlayer();
    return this.playerState;
  }

  clear(): PlayerState {
    this.queueIds = [];
    this.queueTransient = [];
    this.playerState = {
      ...this.playerState,
      queueLength: 0,
      queueVersion: this.playerState.queueVersion + 1,
      index: null,
      cursor: null,
      status: 'idle',
      current: null,
    };
    this.emitPlayer();
    return this.playerState;
  }

  /* ---------------- 直播 / 拖放（docs/52） ---------------- */

  /**
   * 拖入条目的分类（预览版）。
   *
   * 真机是宿主读文件头 + 扩展名（`src-tauri/src/drop.rs`，6 条单测钉着
   * "`.m3u` 按内容分流"这类判断）。预览里**读不到文件内容**，只能按扩展名
   * 猜——故意保持"猜"的粗糙度：这条路径在预览里要验的是 UI 的反应
   * （吐司文案、跳过清单、进不进队列），分类精度归 Rust 单测管。
   */
  classifyPaths(paths: string[]): DroppedItem[] {
    return paths.map((path) => {
      const label = path.split(/[\\/]/).pop() || path;
      const ext = label.includes('.') ? (label.split('.').pop() ?? '').toLowerCase() : '';

      // 没有扩展名 = 当文件夹（真机上 `is_dir()` 说了算）
      if (!ext) return { path, kind: 'directory' as const, note: '文件夹', label };

      if (ext === 'm3u8') return { path, kind: 'hlsPlaylist' as const, note: 'HLS 清单', label };
      if (ext === 'm3u') return { path, kind: 'playlist' as const, note: 'm3u 歌单', label };
      if (['mp3', 'flac', 'wav', 'ogg', 'oga', 'm4a', 'aac', 'aiff', 'aif'].includes(ext)) {
        return { path, kind: 'audio' as const, note: '', label };
      }
      // 认识但解不了的要报格式名，不能笼统说"不支持"
      const named: Record<string, string> = {
        wma: 'WMA 不支持',
        opus: 'Opus 不支持',
        ape: 'APE 不支持',
        wv: 'WavPack 不支持',
        dsf: 'DSD 不支持',
        dff: 'DSD 不支持',
      };
      return {
        path,
        kind: 'unsupported' as const,
        note: named[ext] ?? '不支持的格式',
        label,
      };
    });
  }

  /**
   * 拖入一批文件：替换队列并播第一项（docs/52 §5.3）。
   *
   * 预览里只认两种结局：**音频直接开播**、其余全进 `skipped`。`.m3u` 展开是
   * 宿主的读盘活，浏览器里没有那份文件，一律报跳过——比假装展开成功诚实。
   */
  openPaths(paths: string[]): OpenPathsReceipt {
    const items = this.classifyPaths(paths);
    const playable = items.filter((it) => it.kind === 'audio' || it.kind === 'hlsPlaylist');
    const skipped = items
      .filter((it) => !playable.includes(it))
      .map((it) => `${it.label}：${it.note || '无法识别'}`);

    const first = playable[0];
    if (first) {
      if (first.kind === 'hlsPlaylist') {
        this.playStream(`file:///${first.path.replace(/\\/g, '/')}`, first.label);
      } else {
        // 预览的曲库是内存假数据，拖进来的路径查不到行：借第一首曲目的元数据
        // 顶替，让"播放条出现时长/可拖"这条 UI 路径走得通
        const stand = this.tracks[0];
        if (stand) this.playTracks([stand.id], 0);
      }
    }
    return { opened: playable.length, skipped };
  }

  /** 打开一个 HLS 地址（直播页的地址框 / 拖入的 m3u8） */
  liveOpen(url: string, title?: string): PlayerState {
    const clean = url.trim();
    if (!clean) return this.playerState;
    const name = title?.trim() || defaultStreamTitle(clean);
    return this.playStream(clean, name);
  }

  /** 「打开文件…」：浏览器没有系统对话框，回一个假清单让 UI 路径走通 */
  livePickPlaylist(): string {
    return 'D:\\MUSIC_Player\\test-music\\live.m3u8';
  }

  /**
   * 直播流的播放状态。
   *
   * 三处必须和 Rust 侧对齐，否则预览会"看起来对、真机不对"：
   * - `durationMs = null`（直播没有总时长，播放条据三态切 LIVE 徽章）；
   * - `source = 'stream'`（唯一读这个字段的地方是播放条）；
   * - `current.id` 带 `stream:` 前缀，且 `audio = null`（临时项没有曲库元数据）。
   */
  private playStream(url: string, title: string): PlayerState {
    const ref: TrackRef = {
      id: `stream:${url}`,
      title,
      artist: null,
      album: null,
      artistId: null,
      albumId: null,
      durationMs: null,
      artworkId: null,
      gainDb: null,
      audio: null,
    };
    this.queueIds = [ref.id];
    this.queueTransient = [ref];
    this.playerState = {
      ...this.playerState,
      queueLength: 1,
      queueVersion: this.playerState.queueVersion + 1,
      index: 0,
      cursor: 0,
      status: 'playing',
      source: 'stream',
      current: ref,
      durationMs: null,
      positionMs: 0,
      token: this.playerState.token + 1,
      error: null,
    };
    this.emitPlayer();
    return this.playerState;
  }

  /* ---------------- app ---------------- */

  get initialLook(): { theme: ThemeMode } {
    return this.look;
  }

  appInfo(): { version: string; electron: string; platform: string; dataDir: string } {
    return {
      version: '0.1.0-preview',
      electron: 'n/a（Tauri 2 浏览器预览）',
      platform: navigator.platform || 'web',
      dataDir: '（浏览器预览：localStorage）',
    };
  }

  getUiState(): UiState {
    return this.uiState;
  }

  setUiState(state: UiState): void {
    // 整体覆盖，不合并 —— 与真后端（setting_set 整键覆盖）同语义。合并的话
    // 拆页前留下的旧字段（libraryDetail/view）会永远洗不掉（B4-1 e2e 实测）。
    this.uiState = state;
    saveJson('aiya.uiState', state);
  }

  getSettings(): AppSettings {
    return this.settings;
  }

  setSettings(settings: AppSettings): AppSettings {
    this.settings = settings;
    saveJson('aiya.settings', settings);
    const look = { theme: settings.theme };
    this.look = look;
    saveJson('aiya.look', look);
    return this.settings;
  }

  /**
   * 频谱假帧：正弦叠加，形状像个会跳的频谱但**不来自音频**（预览不出声）。
   * 30fps，48 带，0–255 —— 与 `spectrum:frame` 的契约一致。
   */
  setSpectrum(on: boolean): { enabled: boolean } {
    if (on && this.spectrumTimer === null) {
      let tick = 0;
      this.spectrumTimer = setInterval(() => {
        tick += 1;
        const bands = new Array<number>(48);
        for (let i = 0; i < 48; i += 1) {
          const shape = Math.exp(-i / 18);
          const wave = 0.5 + 0.5 * Math.sin(tick / 6 + i / 5);
          const jitter = 0.5 + 0.5 * Math.sin(tick / 2.3 + i);
          bands[i] = Math.round(255 * Math.min(1, shape * (0.35 + 0.65 * wave * jitter)));
        }
        this.emitter.emit('spectrum:frame', bands);
      }, 33);
    } else if (!on && this.spectrumTimer !== null) {
      clearInterval(this.spectrumTimer);
      this.spectrumTimer = null;
    }
    return { enabled: on };
  }

  /**
   * EQ（协议 v6）：与真后端同语义——clamp 后存 localStorage 并原样返回生效值。
   * 预览不出声，滤波本身无从演示；这里保住的是"发出去的值和回读的值一致"。
   */
  setEq(eq: EqSettings): EqSettings {
    const LIMIT = 12;
    const applied: EqSettings = {
      enabled: eq.enabled,
      bassDb: Math.max(-LIMIT, Math.min(LIMIT, eq.bassDb)),
      trebleDb: Math.max(-LIMIT, Math.min(LIMIT, eq.trebleDb)),
      pregain: Math.max(-LIMIT, Math.min(LIMIT, eq.pregain)),
      bands: eq.bands.map((b) => Math.max(-LIMIT, Math.min(LIMIT, b))),
    };
    saveJson('aiya.eq', applied);
    return applied;
  }

  getEq(): EqSettings {
    // 旧 localStorage 行可能没有 bassDb/trebleDb（docs/39 前的存量），兜底 0
    const v = loadJson('aiya.eq', {
      enabled: false,
      bands: [0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
      bassDb: 0,
      trebleDb: 0,
      pregain: 0,
    });
    return { ...v, bassDb: v.bassDb ?? 0, trebleDb: v.trebleDb ?? 0 };
  }

  diagnostics(): Diagnostics {
    const stats = this.libraryStats();
    return {
      version: '0.1.0-preview',
      electron: 'n/a（Tauri 2 浏览器预览）',
      platform: navigator.platform || 'web',
      dataDir: '（浏览器预览：localStorage）',
      logDir: null,
      logFile: null,
      schemaVersion: stats.schemaVersion,
      dbBytes: 0,
      tracks: stats.tracks,
      albums: stats.albums,
      artists: stats.artists,
      playlists: this.playlists.length,
      historyRows: 0,
      totalDurationMs: stats.totalDurationMs,
      streams: { opened: 0, closed: 0, active: 0 },
      listTimings: this.timings,
      recentErrors: [],
      mediaKeys: { enabled: false, active: [], failed: [] },
      power: { active: false, type: null },
      tray: false,
      audioMode: {
        requested: 'shared',
        mode: 'shared',
        clock: 'events',
        bufferMs: 200,
        format: 'n/a（预览无音频）',
        note: '浏览器预览不建音频流',
        desired: { mode: 'shared', bufferMs: 200, clock: 'auto', releaseOnPause: false },
      },
    };
  }

  /* ---------------- 查询实现 ---------------- */

  private filter(q: TrackQueryParsed): TrackRow[] {
    const needle = q.search?.trim().toLowerCase();
    return this.tracks.filter((t) => {
      if (needle) {
        const hay = `${t.title}\n${t.artist ?? ''}\n${t.album ?? ''}`.toLowerCase();
        if (!hay.includes(needle)) return false;
      }
      if (q.artistId && t.artistId !== q.artistId) return false;
      if (q.albumId && t.albumId !== q.albumId) return false;
      // 时间筛选（docs/27）：语义与 Rust repo.rs 逐字对齐——
      // from 含、to 开区间，毫秒比较
      if (q.addedFromMs !== undefined && t.addedAtMs < q.addedFromMs) return false;
      if (q.addedToMs !== undefined && t.addedAtMs >= q.addedToMs) return false;
      // 筛选器临时条件（与动态歌单同键，语义对齐 smart.rs build_clause）：
      // 文本走 norm 后 =/LIKE，数字直接比较；artist/album 用子串
      for (const c of q.conds ?? []) {
        const norm = (s: string) => s.trim().toLowerCase();
        if (c.field === 'genre' || c.field === 'artist' || c.field === 'album') {
          const hay =
            c.field === 'genre' ? (t.genre ?? '') : c.field === 'artist' ? (t.artist ?? '') : (t.album ?? '');
          const h = hay.toLowerCase();
          const v = norm(String(c.value));
          if (c.op === 'eq' && h !== v) return false;
          if (c.op === 'contains' && !h.includes(v)) return false;
        } else {
          const v = Number(c.value);
          const n = c.field === 'year' ? t.year : t.playCount;
          if (c.op === 'eq' && n !== v) return false;
          if (c.op === 'gte' && (n ?? Number.NaN) < v) return false;
          if (c.op === 'lte' && (n ?? Number.NaN) > v) return false;
        }
      }
      return true;
    });
  }

  private sortForSource(rows: TrackRow[], q: TrackQueryParsed): TrackRow[] {
    const dir = q.direction === 'desc' ? -1 : 1;
    /** 文本键取 localeCompare 的符号再乘方向，数值键直接相减乘方向；id 恒升序保稳定 */
    const cmp = (a: TrackRow, b: TrackRow): number => {
      const tie = a.id.localeCompare(b.id);
      switch (q.sort) {
        case 'added':
          return (a.addedAtMs - b.addedAtMs) * dir || tie;
        case 'artist':
          return Math.sign((a.artist ?? '').localeCompare(b.artist ?? '', 'zh')) * dir || tie;
        case 'album':
          return Math.sign((a.album ?? '').localeCompare(b.album ?? '', 'zh')) * dir || tie;
        case 'title':
        default:
          return Math.sign(a.title.localeCompare(b.title, 'zh')) * dir || tie;
      }
    };

    const source = q.source;
    if (source.kind === 'recent') {
      // 方向跟随查询（曲库 tab 二次点击翻转，与 repo.rs join_and_order 同口径）；
      // id 恒升序保稳定
      const d = q.direction === 'desc' ? -1 : 1;
      return [...rows]
        .filter((t) => t.lastPlayedAtMs !== null)
        .sort(
          (a, b) =>
            d * ((a.lastPlayedAtMs ?? 0) - (b.lastPlayedAtMs ?? 0)) || a.id.localeCompare(b.id),
        );
    }
    if (source.kind === 'mostPlayed') {
      // 与 Rust repo.rs join_and_order 逐字对齐：次数 desc → 最近播放 desc → id。
      // docs/28：推荐页 top-15 的第 15 名争夺战里，并列次数下"最近还在听"的留下
      return [...rows]
        .filter((t) => t.playCount > 0)
        .sort(
          (a, b) =>
            b.playCount - a.playCount ||
            (b.lastPlayedAtMs ?? 0) - (a.lastPlayedAtMs ?? 0) ||
            a.id.localeCompare(b.id),
        );
    }
    // library / playlist：mock 不建 playlist_items 表，歌单视图就用同一批曲目
    return [...rows].sort(cmp);
  }
}
