import type { LibraryStats, ScanProgress, TrackPage, TrackQuery } from '@aiya/contracts';

/**
 * 曲库 store：统计、扫描进度、数据目录。
 *
 * 扫描进度的订阅收敛在这里（window.api.library.onScanProgress）；
 * 扫描完成后的联动（统计/歌单/列表刷新）通过 `onScanDone` 回调
 * 交回 App——store 不持有列表刷新逻辑。
 */
class LibraryStore {
  stats = $state<LibraryStats | null>(null);
  scan = $state<ScanProgress | null>(null);
  dataDir = $state<string | null>(null);

  readonly scanning = $derived(this.scan?.phase === 'walking' || this.scan?.phase === 'parsing');

  async loadStats(): Promise<void> {
    const result = await window.api.library.stats();
    if (result.ok) this.stats = result.data;
  }

  /** 分页列表（TrackWindow 与 App.reload 共用这一个入口） */
  async list(query: TrackQuery): Promise<{ ok: true; page: TrackPage } | { ok: false; message: string }> {
    const result = await window.api.library.list(query);
    return result.ok
      ? { ok: true, page: result.data }
      : { ok: false, message: result.error.message };
  }

  /** 在系统文件管理器中定位曲目；未定位出时给出原因 */
  async revealTrack(trackId: string | null): Promise<{ revealed: boolean; reason?: string } | null> {
    if (!trackId) return null;
    const result = await window.api.library.revealTrack(trackId);
    return result.ok ? result.data : null;
  }

  /** 从曲库移除（只删索引，不动磁盘文件） */
  async removeTracks(ids: string[]): Promise<{ ok: true; removed: number } | { ok: false; message: string }> {
    const result = await window.api.library.removeTracks(ids);
    return result.ok
      ? { ok: true, removed: result.data.removed }
      : { ok: false, message: result.error.message };
  }

  /** 打开系统目录选择器；用户取消返回 null */
  async chooseRoot(): Promise<{ ok: true; path: string | null } | { ok: false; message: string }> {
    const result = await window.api.library.chooseRoot();
    return result.ok
      ? { ok: true, path: result.data?.path ?? null }
      : { ok: false, message: result.error.message };
  }

  /** 按 ID 批量取曲目（队列面板补齐展示字段用） */
  async tracks(ids: string[]): Promise<import('@aiya/contracts').Track[]> {
    const result = await window.api.library.tracks(ids);
    return result.ok ? result.data : [];
  }

  /** 订阅扫描进度；返回清理函数 */
  watchScan(onDone: () => void): () => void {
    return window.api.library.onScanProgress((progress) => {
      this.scan = progress;
      if (progress.phase === 'done') onDone();
    });
  }

  /** 手动触发扫描；失败时清掉本地进度并把错误交回调用方 */
  async startScan(): Promise<{ ok: boolean; message?: string }> {
    this.scan = {
      phase: 'walking',
      scanned: 0,
      total: null,
      imported: 0,
      skipped: 0,
      failed: 0,
      currentPath: null,
      elapsedMs: 0,
    };
    const result = await window.api.library.scan();
    if (!result.ok) {
      this.scan = null;
      return { ok: false, message: result.error.message };
    }
    return { ok: true };
  }

  cancelScan(): void {
    void window.api.library.cancelScan();
  }

  /** 应用信息（dataDir 用于侧栏展示） */
  async fetchInfo(): Promise<string | null> {
    const result = await window.api.app.info();
    return result.ok ? result.data.dataDir : null;
  }

  /** 选择集下标 → 曲目 ID（主进程按同一查询换算，渲染层不拉整库） */
  async idsForSelection(query: TrackQuery, indices: number[]): Promise<string[] | null> {
    const result = await window.api.library.idsForSelection({ query, indices });
    return result.ok ? result.data : null;
  }

  async clearHistory(): Promise<{ ok: boolean; message?: string }> {
    const result = await window.api.history.clear();
    return result.ok ? { ok: true } : { ok: false, message: result.error.message };
  }
}

export const library = new LibraryStore();
