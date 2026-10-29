import type { TrackRow } from './ipc/generated/TrackRow';
import { errorMessage } from './errors';

/**
 * 稀疏分页窗口。
 *
 * 5 万行的列表不能"加载全部再虚拟化"——那只是把渲染成本换成内存与查询成本。
 * 这里只按需加载可见区间覆盖的页，页面缓存有上限，超出后按先进先出丢弃。
 * 不做真正的 LRU：`get()` 在每次渲染里被调用几百次，刷新顺序只会造成抖动。
 *
 * 页面用「顺序号」而不是 offset 标识：offset 会随曲库变化漂移，
 * 但同一个查询顺序下，第 n 页始终是第 n 页（除非重排序，而那时会 reset）。
 */

export interface PageResult {
  items: TrackRow[];
  total: number;
}

export type FetchPage = (offset: number, limit: number) => Promise<PageResult>;

export interface TrackWindowOptions {
  fetchPage: FetchPage;
  pageSize?: number;
  /** 最多缓存多少页，默认 24 页 = 2400 行 ≈ 4MB */
  maxPages?: number;
}

export class TrackWindow {
  private readonly fetchPage: FetchPage;
  private readonly pageSize: number;
  private readonly maxPages: number;

  private pages = new Map<number, TrackRow[]>();
  private inflight = new Set<number>();
  private errors = new Map<number, string>();
  private totalValue = 0;
  private generation = 0;
  private listeners = new Set<() => void>();

  constructor(options: TrackWindowOptions) {
    this.fetchPage = options.fetchPage;
    this.pageSize = Math.max(1, options.pageSize ?? 100);
    this.maxPages = Math.max(2, options.maxPages ?? 24);
  }

  get total(): number {
    return this.totalValue;
  }

  get pageSizeValue(): number {
    return this.pageSize;
  }

  /** 换查询条件时调用：清缓存并把进行中的请求标记为过期 */
  reset(total: number): void {
    this.generation += 1;
    this.pages.clear();
    this.inflight.clear();
    this.errors.clear();
    this.totalValue = Math.max(0, total);
    this.notify();
  }

  /** 用它已经拿到的一页预热缓存，避免 reset 后立刻重复请求同一页 */
  primePage(pageIndex: number, items: TrackRow[]): void {
    if (items.length === 0) return;
    this.pages.set(pageIndex, items);
    this.notify();
  }

  setTotal(total: number): void {
    if (this.totalValue === total) return;
    this.totalValue = Math.max(0, total);
    this.notify();
  }

  get(index: number): TrackRow | undefined {
    if (index < 0 || index >= this.totalValue) return undefined;
    const page = Math.floor(index / this.pageSize);
    return this.pages.get(page)?.[index % this.pageSize];
  }

  isPending(index: number): boolean {
    if (index < 0 || index >= this.totalValue) return false;
    const page = Math.floor(index / this.pageSize);
    if (this.pages.has(page)) return false;
    return !this.errors.has(page);
  }

  errorAt(index: number): string | null {
    if (index < 0 || index >= this.totalValue) return null;
    const page = Math.floor(index / this.pageSize);
    return this.errors.get(page) ?? null;
  }

  /** 保证某个区间落在缓存里（越界自动裁剪） */
  ensureRange(start: number, end: number): void {
    if (this.totalValue === 0) return;
    const firstPage = Math.floor(Math.max(0, start) / this.pageSize);
    const lastPage = Math.floor(Math.min(this.totalValue - 1, Math.max(0, end - 1)) / this.pageSize);
    for (let page = firstPage; page <= lastPage; page += 1) this.requestPage(page);
  }

  ensure(index: number): void {
    this.ensureRange(index, index + 1);
  }

  subscribe(listener: () => void): () => void {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  }

  private requestPage(page: number): void {
    if (this.pages.has(page) || this.inflight.has(page)) return;
    if (page * this.pageSize >= this.totalValue) return;

    const generation = this.generation;
    this.inflight.add(page);

    void this.fetchPage(page * this.pageSize, this.pageSize)
      .then((result) => {
        if (generation !== this.generation) return;
        const items = result.items;
        this.pages.set(page, items);
        this.errors.delete(page);
        if (result.total !== this.totalValue) this.totalValue = Math.max(0, result.total);
        this.evict();
        this.notify();
      })
      .catch((err: unknown) => {
        if (generation !== this.generation) return;
        // errorMessage 归一化 ApiError 普通对象；String(err) 会得到 [object Object]
        this.errors.set(page, errorMessage(err));
        this.notify();
      })
      .finally(() => {
        if (generation === this.generation) this.inflight.delete(page);
      });
  }

  /** 先进先出：Map 的插入顺序就是加载顺序，超限时丢最早载入的那页 */
  private evict(): void {
    if (this.pages.size <= this.maxPages) return;
    for (const key of this.pages.keys()) {
      if (this.pages.size <= this.maxPages) break;
      if (!this.inflight.has(key)) this.pages.delete(key);
    }
  }

  private notify(): void {
    for (const listener of this.listeners) listener();
  }
}
