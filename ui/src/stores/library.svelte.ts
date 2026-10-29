import type { TrackQueryInput } from '../lib/ipc/client';
import { events, ipc } from '../lib/ipc';
import type { LibraryStats } from '../lib/ipc/generated/LibraryStats';
import type { TrackPage } from '../lib/ipc/generated/TrackPage';
import type { TrackDetail } from '../lib/ipc/generated/TrackDetail';
import type { ScanResult } from '../lib/ipc/generated/ScanResult';
import type { PlaylistSummary } from '../lib/ipc/generated/PlaylistSummary';
import { errorMessage } from '../lib/errors';

/**
 * 曲库 store：统计、扫描进度、数据目录、分页取数。
 *
 * 失败约定：只有 `list` / `idsForSelection` 这类"要弹错误条"的方法**抛出**
 * （调用方 catch 后展示），其余（统计、进度、信息）失败只降级不抛 ——
 * 侧栏统计拉不到不该挡住整个界面。
 *
 * 扫描进度的事件形状随扫描器演化（`scan:progress` 是动态载荷），
 * 这里按结构读到啥用啥，字段缺失就当 0（`scanned ?? 0`）。
 */
class LibraryStore {
  stats = $state<LibraryStats | null>(null);
  scan = $state<ScanProgressView | null>(null);
  dataDir = $state<string | null>(null);

  /**
   * 歌单摘要（侧栏、首页、选择器共用这一份）。
   *
   * 为什么放在 library 而不是 playlists store：曲目变动后这份数据会过期，
   * 而 `list.removeTracks` 与 `scan.afterScan` 都要刷它 —— 它们**不能**
   * import playlists（playlists 反过来 import 它们做 reload/playAll，会成环，
   * 全仓此前是 0 环）。library 只依赖 ipc，是这张图里唯一夹在中间的位置。
   * playlists store 在此之上做"改完刷新 + 提示 + 重载"的编排。
   */
  playlists = $state<PlaylistSummary[]>([]);

  readonly scanning = $derived(this.scan?.phase === 'walking' || this.scan?.phase === 'parsing');

  async loadStats(): Promise<void> {
    try {
      this.stats = await ipc.library.stats();
    } catch (e) {
      console.warn('library.stats failed:', errorMessage(e));
    }
  }

  /** 重拉歌单摘要。失败降级为不动（侧栏旧数据比空列表好） */
  async loadPlaylists(): Promise<void> {
    try {
      this.playlists = await ipc.playlist.list();
    } catch (e) {
      console.warn('playlist.list failed:', errorMessage(e));
    }
  }

  /** 侧栏歌单整体排序：后端回排好的完整列表，省一次 roundtrip。失败抛出 */
  async reorderPlaylists(from: number, to: number): Promise<void> {
    this.playlists = await ipc.playlist.reorder(from, to);
  }

  /** 分页列表（TrackWindow 与 App.reload 共用这一个入口）。失败抛出，由调用方决定怎么显示 */
  async list(query: TrackQueryInput): Promise<TrackPage> {
    return ipc.library.list(query);
  }

  /**
   * 曲库流派列表（动态歌单"流派"条件的下拉数据源）。
   * 带会话级缓存 + 60s TTL：流派集合只在扫描/beets 同步后才会变，模态框
   * 每次打开都查一遍是浪费；扫描走 `invalidateGenres()` 精确失效，beets
   * 同步没有统一收尾钩子，用 TTL 兜底。
   */
  #genresCache: { items: string[]; at: number } | null = null;
  async genres(): Promise<string[]> {
    if (this.#genresCache && Date.now() - this.#genresCache.at < 60_000) {
      return this.#genresCache.items;
    }
    const items = await ipc.library.genres();
    this.#genresCache = { items, at: Date.now() };
    return items;
  }
  /** 扫描/同步完成后调：让下一次 genres() 重拉 */
  invalidateGenres(): void {
    this.#genresCache = null;
  }

  /** 按 ID 批量取（含音频规格）。失败降级为空数组：队列面板少一行说明比整块报错好 */
  async tracks(ids: string[]): Promise<TrackDetail[]> {
    try {
      return await ipc.library.tracks(ids);
    } catch (e) {
      console.warn('library.tracks failed:', errorMessage(e));
      return [];
    }
  }

  /** 在系统文件管理器中定位曲目；`revealed=false` 时看 `reason`（不是错误） */
  async revealTrack(trackId: string | null): Promise<{ revealed: boolean; reason?: string } | null> {
    if (!trackId) return null;
    try {
      const result = await ipc.library.revealTrack(trackId);
      return result.revealed ? { revealed: true } : { revealed: false, reason: result.reason ?? undefined };
    } catch (e) {
      return { revealed: false, reason: errorMessage(e) };
    }
  }

  /** 从曲库移除（只删索引，不动磁盘文件）。失败抛出 */
  async removeTracks(ids: string[]): Promise<number> {
    const result = await ipc.library.removeTracks(ids);
    return result.removed;
  }

  /** 打开系统目录选择器；用户取消返回 null。选中后回填根列表状态 */
  async chooseRoot(): Promise<string | null> {
    const chosen = await ipc.library.chooseRoot();
    if (chosen) this.roots = chosen.roots;
    return chosen?.path ?? null;
  }

  /* ---------------- 扫描根列表（docs/29 批 A：多目录并集单库） ---------------- */

  roots = $state<string[]>([]);

  /** 拉取扫描根列表（textarea 数据源）。失败降级为不动（旧值比空列表好） */
  async loadRoots(): Promise<void> {
    try {
      this.roots = (await ipc.library.rootsGet()).roots;
    } catch (e) {
      console.warn('library.rootsGet failed:', errorMessage(e));
    }
  }

  /** 覆盖写扫描根列表。后端会再归一化，这里以后端返回为准回填 */
  async saveRoots(roots: string[]): Promise<void> {
    this.roots = (await ipc.library.rootsSet(roots)).roots;
  }

  /** 移除目录的影响面（曲目数，UI 确认条用）。失败抛出 */
  async countForRemoval(paths: string[]): Promise<number> {
    return (await ipc.library.rootsCount(paths)).count;
  }

  /** 从曲库删除这些目录下的曲目（只删索引不动磁盘）。失败抛出 */
  async pruneRoots(paths: string[]): Promise<number> {
    return (await ipc.library.rootsPrune(paths)).removed;
  }

  /** 清空整个曲库记录（只删索引不动磁盘，歌单壳保留）。失败抛出 */
  async clearLibrary(): Promise<number> {
    return (await ipc.library.clear()).removed;
  }

  /** 订阅扫描进度；`done`/`cancelled` 都收尾（取消也可能已提交了批次，
   *  统计/歌单/列表同样需要重拉）。返回清理函数 */
  watchScan(onDone: () => void): () => void {
    return events.scanProgress((raw) => {
      const progress = readScanProgress(raw);
      this.scan = progress;
      if (progress.phase === 'done' || progress.phase === 'cancelled') onDone();
    });
  }

  /** 手动触发扫描。阻塞到结束（进度走事件）；失败时清掉本地进度并抛出 */
  async startScan(): Promise<ScanResult> {
    this.scan = {
      phase: 'walking',
      scanned: 0,
      parsed: 0,
      total: null,
      imported: 0,
      updated: 0,
      skipped: 0,
      failed: 0,
      missing: 0,
      currentPath: null,
      elapsedMs: 0,
    };
    try {
      return await ipc.library.scan();
    } catch (e) {
      this.scan = null;
      throw e;
    }
  }

  cancelScan(): void {
    void ipc.library.scanCancel().catch((e) => console.warn('scanCancel failed:', errorMessage(e)));
  }

  /** 应用信息（dataDir 用于侧栏展示） */
  async fetchInfo(): Promise<string | null> {
    try {
      const info = await ipc.app.info();
      return info.dataDir;
    } catch (e) {
      console.warn('app.info failed:', errorMessage(e));
      return null;
    }
  }

  /**
   * 选择集下标 → 曲目 ID（后端按同一查询换算，渲染层不拉整库）。失败抛出。
   * `indices: 'all'` = 整个查询结果的全量 ID 顺序表（「定位到正在播放」
   * 用它求当前曲目在视图里的下标；上限 ID_COLLECT_CAP=5 万）。
   */
  async idsForSelection(query: TrackQueryInput, indices: number[] | 'all'): Promise<string[]> {
    return ipc.library.idsForSelection({ query, indices });
  }

  /**
   * 清空播放历史。失败抛出（调用方要给用户看到"没清掉"）。
   *
   * 放在这里而不是单独建 `history` store：目前历史只有一个写操作，
   * 真正的历史视图（播放次数榜）在批次 3 才落地，届时再按 docs/18 §2
   * 拆出 `history.svelte.ts` 一并接 `history.stats` —— 现在拆是空壳。
   */
  async clearHistory(): Promise<void> {
    await ipc.history.clear();
  }
}

/** 扫描进度的视图形状（事件载荷是动态 JSON，这里定义读出来的结构） */
export interface ScanProgressView {
  phase: string;
  /** 已入库（含 unchanged / failed 记账）的文件数 —— 进度条按它走 */
  scanned: number;
  /** 解析已完成数（docs/29 批 B）。`parsed - scanned` = 排队等入库的积压 */
  parsed: number;
  total: number | null;
  imported: number;
  /** 重新读标签并写了行的数量（与 skipped 互斥：那批连读都没读） */
  updated: number;
  skipped: number;
  failed: number;
  /** 本轮被标为离线的行数（只有 done 那一刻有值，中途恒 0） */
  missing: number;
  currentPath: string | null;
  elapsedMs: number;
}

/** 动态载荷 → 视图形状：缺字段不报错，按 0/null 处理 */
function readScanProgress(raw: unknown): ScanProgressView {
  const o = (typeof raw === 'object' && raw !== null ? raw : {}) as Record<string, unknown>;
  const num = (key: string): number => (typeof o[key] === 'number' ? (o[key] as number) : 0);
  return {
    phase: typeof o.phase === 'string' ? o.phase : 'unknown',
    scanned: num('scanned'),
    // 后端新增 parsed 之前的事件没有这个字段 → 0，UI 会退回"只显示已入库"
    parsed: num('parsed'),
    total: typeof o.total === 'number' ? o.total : null,
    imported: num('imported'),
    updated: num('updated'),
    skipped: num('skipped'),
    failed: num('failed'),
    missing: num('missing'),
    currentPath: typeof o.currentPath === 'string' ? o.currentPath : null,
    elapsedMs: num('elapsedMs'),
  };
}

export const library = new LibraryStore();
