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
import type { AppSettings, ListDensity, ThemeMode } from '../../settings';
import { DEFAULT_SETTINGS } from '../../settings';
import type { TrackQuery, TrackQueryParsed } from '../../query';
import { parseTrackQuery } from '../../query';
import type { SelectionIdsQuery, QueueInsertMode } from '../client';
import type { UiState } from '../schema';
import type {
  Diagnostics,
  HistoryStats,
  LibraryStats,
  Lyrics,
  PlayerState,
  PlaylistAddResult,
  PlaylistSummary,
  QueueSnapshot,
  RevealResult,
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

function toRef(track: TrackRow): TrackRef {
  return {
    id: track.id,
    title: track.title,
    artist: track.artist,
    album: track.album,
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
        });
      }
    });
  });
  return tracks;
}

function buildPlaylists(tracks: TrackRow[]): PlaylistSummary[] {
  const now = Date.now();
  const names = ['华语偏爱', '深夜自习室', '运动节拍', '通勤路上'];
  return names.map((name, i) => {
    const slice = tracks.slice(i * 23, i * 23 + 12 + i * 5);
    return {
      id: `pl-${i + 1}`,
      name,
      description: null,
      trackCount: slice.length,
      durationMs: slice.reduce((sum, t) => sum + (t.durationMs ?? 0), 0),
      artworkId: null,
      createdAtMs: now - 86400_000 * (10 - i),
      updatedAtMs: now - 3600_000 * (i + 1),
      sortPosition: i,
    };
  });
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

  private tracks: TrackRow[] = buildLibrary();
  private playlists: PlaylistSummary[] = buildPlaylists(this.tracks);
  private settings: AppSettings = loadJson('aiya.settings', DEFAULT_SETTINGS);
  private uiState: UiState = loadJson('aiya.uiState', {} as UiState);
  private look: { theme: ThemeMode; density: ListDensity } = loadJson('aiya.look', {
    theme: 'dark' as ThemeMode,
    density: 'comfortable' as ListDensity,
  });

  private playerState: PlayerState = {
    queueLength: 0,
    queueVersion: 0,
    index: -1,
    cursor: -1,
    repeat: 'off',
    shuffle: false,
    status: 'idle',
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
  /** 频谱假帧定时器：只在 `spectrum_set(true)` 期间跑 */
  private spectrumTimer: ReturnType<typeof setInterval> | null = null;

  /* ---------------- library ---------------- */

  libraryList(rawQuery: TrackQuery | undefined): TrackPage {
    const q: TrackQueryParsed = parseTrackQuery(rawQuery ?? { mode: 'cursor' });
    let rows = this.filter(q);
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
    return {
      items,
      total,
      limit: q.limit,
      mode: q.mode,
      nextCursor,
      // 契约里 offset 是必填的 `number | null`（不是可选字段）
      offset: q.mode === 'offset' ? q.offset : null,
      tookMs: 1,
    };
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

  libraryStats(): LibraryStats {
    const albums = new Set(this.tracks.map((t) => t.albumId));
    const artists = new Set(this.tracks.map((t) => t.artistId));
    return {
      tracks: this.tracks.length,
      albums: albums.size,
      artists: artists.size,
      locations: this.tracks.length,
      missingLocations: 0,
      totalDurationMs: this.tracks.reduce((s, t) => s + (t.durationMs ?? 0), 0),
      dbBytes: 0,
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

  revealTrack(_trackId: string): RevealResult {
    return { revealed: false, reason: '浏览器预览模式不可用' };
  }

  async scan(): Promise<ScanResult> {
    const started = Date.now();
    const phases = ['walking', 'parsing', 'artwork', 'done'] as const;
    for (let i = 0; i < phases.length; i++) {
      this.emitter.emit('scan:progress', {
        phase: phases[i],
        scanned: Math.floor(((i + 1) / phases.length) * this.tracks.length),
        total: this.tracks.length,
        imported: i >= 1 ? this.tracks.length : 0,
        skipped: 0,
        failed: 0,
        currentPath: null,
        elapsedMs: Date.now() - started,
      });
      await new Promise((r) => setTimeout(r, 250));
    }
    return {
      scanned: this.tracks.length,
      imported: this.tracks.length,
      updated: 0,
      unchanged: 0,
      failed: 0,
      walkErrors: 0,
      cancelled: false,
      errors: [],
      elapsedMs: Date.now() - started,
    };
  }

  /* ---------------- playlists ---------------- */

  playlistList(): PlaylistSummary[] {
    return this.playlists;
  }

  playlistDetail(id: string): PlaylistSummary | null {
    return this.playlists.find((p) => p.id === id) ?? null;
  }

  playlistCreate(name: string, trackIds?: string[]): PlaylistSummary {
    const now = Date.now();
    const summary: PlaylistSummary = {
      id: `pl-${now}`,
      name,
      description: null,
      trackCount: trackIds?.length ?? 0,
      durationMs: 0,
      artworkId: null,
      createdAtMs: now,
      updatedAtMs: now,
      sortPosition: this.playlists.length,
    };
    this.playlists.push(summary);
    return summary;
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
    return { deleted: this.playlists.length < before };
  }

  playlistReorder(from: number, to: number): PlaylistSummary[] {
    if (from < 0 || from >= this.playlists.length) return this.playlists;
    const [moved] = this.playlists.splice(from, 1);
    if (moved) this.playlists.splice(Math.max(0, Math.min(to, this.playlists.length)), 0, moved);
    this.playlists.forEach((p, i) => (p.sortPosition = i));
    return this.playlists;
  }

  playlistAddTracks(id: string, trackIds: string[], mode?: 'append' | 'next'): PlaylistAddResult {
    const p = this.playlists.find((x) => x.id === id);
    if (!p) throw new Error('E_NOT_FOUND');
    const added = trackIds.length;
    p.trackCount += added;
    p.updatedAtMs = Date.now();
    void mode;
    return { added, skipped: 0, summary: p };
  }

  playlistRemoveAt(id: string, position: number): PlaylistSummary | null {
    const p = this.playlists.find((x) => x.id === id);
    if (!p) return null;
    p.trackCount = Math.max(0, p.trackCount - 1);
    p.updatedAtMs = Date.now();
    void position;
    return p;
  }

  playlistMove(id: string, from: number, to: number): PlaylistSummary | null {
    void from;
    void to;
    return this.playlistDetail(id);
  }

  playlistClear(id: string): PlaylistSummary | null {
    const p = this.playlists.find((x) => x.id === id);
    if (!p) return null;
    p.trackCount = 0;
    p.updatedAtMs = Date.now();
    return p;
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
    this.emitter.emit('player:state', this.playerState);
  }

  playerStateNow(): PlayerState {
    return this.playerState;
  }

  playerQueue(): QueueSnapshot {
    return {
      ids: this.queueIds,
      index: this.playerState.index,
      cursor: this.playerState.cursor,
      version: this.playerState.queueVersion,
      tookMs: 0,
    };
  }

  private playQueue(ids: string[], startIndex: number): PlayerState {
    this.queueIds = ids;
    const current = this.tracks.find((t) => t.id === ids[startIndex]) ?? null;
    this.playerState = {
      ...this.playerState,
      queueLength: ids.length,
      queueVersion: this.playerState.queueVersion + 1,
      index: ids.length ? startIndex : -1,
      cursor: ids.length ? startIndex : -1,
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
    return this.playQueue([...trackIds], startIndex);
  }

  playQuery(query: TrackQuery, startIndex = 0): PlayerState {
    const q = parseTrackQuery(query);
    const rows = this.sortForSource(this.filter(q), q);
    return this.playQueue(rows.map((t) => t.id), startIndex);
  }

  toggle(): PlayerState {
    if (this.playerState.status === 'playing') {
      this.playerState = { ...this.playerState, status: 'paused' };
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
    let next = this.playerState.index + delta;
    if (next < 0) next = 0;
    if (next >= this.queueIds.length) {
      next = this.playerState.repeat === 'all' ? 0 : this.queueIds.length - 1;
    }
    return this.playQueue(this.queueIds, next);
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
    if (options?.mode === 'next' && this.playerState.index >= 0) {
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
    this.playerState = {
      ...this.playerState,
      queueLength: this.queueIds.length,
      queueVersion: this.playerState.queueVersion + 1,
      index: index < this.playerState.index ? this.playerState.index - 1 : this.playerState.index,
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
    this.playerState = {
      ...this.playerState,
      queueLength: 0,
      queueVersion: this.playerState.queueVersion + 1,
      index: -1,
      cursor: -1,
      status: 'idle',
      current: null,
    };
    this.emitPlayer();
    return this.playerState;
  }

  /* ---------------- app ---------------- */

  get initialLook(): { theme: ThemeMode; density: ListDensity } {
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
    this.uiState = { ...this.uiState, ...state };
    saveJson('aiya.uiState', this.uiState);
  }

  getSettings(): AppSettings {
    return this.settings;
  }

  setSettings(settings: AppSettings): AppSettings {
    this.settings = settings;
    saveJson('aiya.settings', settings);
    const look = { theme: settings.theme, density: settings.density };
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
      listTimings: [],
      recentErrors: [],
      shortcutConflicts: [],
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
      return [...rows]
        .filter((t) => t.lastPlayedAtMs !== null)
        .sort((a, b) => (b.lastPlayedAtMs ?? 0) - (a.lastPlayedAtMs ?? 0) || a.id.localeCompare(b.id));
    }
    if (source.kind === 'mostPlayed') {
      return [...rows]
        .filter((t) => t.playCount > 0)
        .sort((a, b) => b.playCount - a.playCount || a.id.localeCompare(b.id));
    }
    // library / playlist：mock 不建 playlist_items 表，歌单视图就用同一批曲目
    return [...rows].sort(cmp);
  }
}
