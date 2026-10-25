import { parseTrackQuery, type TrackQuery, type TrackSort } from '../lib/query';
import { errorMessage } from '../lib/errors';
import { TrackWindow } from '../lib/track-window';
import { sourceOf } from '../lib/view';
import { library } from './library.svelte';
import { notify } from './notify.svelte';
import { player } from './player.svelte';
import { selection } from './selection.svelte';
import { ui } from './ui.svelte';

/** 单页条数。游标分页的页大小，改它要一并看 `maxPages` 的内存账 */
export const PAGE_SIZE = 100;

/** 搜索防抖：输入停下 120 ms 才查库（短到感觉即时，长到不打满 IPC） */
const SEARCH_DEBOUNCE_MS = 120;

/**
 * 曲目列表 store：查询构造 + 窗口分页 + 重载 + 「播放当前列表」。
 *
 * 为什么从 App.svelte 搬出来（批次 3）：
 * 1. `LibraryView`（渲染）、`lib/keyboard.ts`（selectAll 要 total）、
 *    `playlists` store（加入歌单后要 reload）**三方都要**这套状态。
 *    留在 App 就得把 8 个东西逐层传给组件，或者绕回去问 App —— 而 App
 *    的目标是变得没有业务逻辑。
 * 2. `query()` 是 `library.list` / `player.playQuery` / `idsForSelection`
 *    三条命令的**同一个入参**。分开存 = 迟早有一处忘了带 `source`，
 *    症状是"播放的不是当前列表里的歌"，且只在特定视图下复现。
 *
 * 这个 store 只读别的 store（ui/library/player/selection），没有反向依赖。
 */
class ListStore {
  /**
   * 分页窗口。它是普通类（不是 `$state`），改动靠 `version` 通知 Svelte。
   * 把 5 万首的 `Track[]` 塞进响应式代理会让每次读写都走 Proxy，没必要。
   */
  readonly trackWindow = new TrackWindow({
    pageSize: PAGE_SIZE,
    maxPages: 24,
    fetchPage: async (offset, limit) => {
      // parseTrackQuery：offset 模式的查询带 offset 覆盖，直接展开会被
      // 判别联合的 cursor 分支拒绝。
      // 失败直接抛：TrackWindow 自己会把列表切到「重试」态。
      const page = await library.list(parseTrackQuery({ ...this.query(), offset, limit }));
      return { items: page.items, total: page.total };
    },
  });

  total = $state(0);
  /** 每次 TrackWindow 变化 +1，用来驱动 TrackList 重渲染 */
  version = $state(0);
  /** 首屏数据到过没有（决定显示「正在启动…」还是空态） */
  booted = $state(false);
  /** 首屏加载中（视图切换时用，与扫描进度无关） */
  loading = $state(false);
  /** 下一次渲染后要滚到的位置；`null` = 不恢复（停在顶部） */
  restoreScrollTop = $state<number | null>(null);

  /** reload 的请求代次，用来丢弃过期响应 */
  private generation = 0;
  /** 搜索防抖句柄 */
  private searchTimer: ReturnType<typeof setTimeout> | undefined;

  constructor() {
    this.trackWindow.subscribe(() => {
      this.version += 1;
      this.total = this.trackWindow.total;
    });
  }

  /**
   * 当前查询。**三条命令共用这一个**（list / playQuery / idsForSelection）。
   *
   * `mode: 'offset'` + `offset: 0`：首屏只要第一页，后续由 TrackWindow
   * 按需拉；`source` 决定曲库/最近/常听/歌单，排序与筛选直接搬 ui store。
   */
  query(): TrackQuery {
    return {
      mode: 'offset',
      offset: 0,
      limit: PAGE_SIZE,
      sort: ui.sort,
      direction: ui.direction,
      source: sourceOf(ui.view),
      ...(ui.search.trim() ? { search: ui.search.trim() } : {}),
      ...(ui.artistId ? { artistId: ui.artistId } : {}),
      ...(ui.albumId ? { albumId: ui.albumId } : {}),
    };
  }

  /**
   * 重新拉首屏。
   *
   * 请求代次：快速点两个播放列表时，两次 `library.list` 会并发。先发的
   * 可能后回来，把列表覆盖成上一个视图的内容 —— 而标题已经是新视图了。
   * 只有最新一次的结果允许落到 store 上。
   */
  async reload(options: { restore?: boolean } = {}): Promise<void> {
    const generation = (this.generation += 1);
    this.loading = true;
    notify.clearError();
    try {
      const page = await library.list(this.query());
      if (generation !== this.generation) return;
      this.trackWindow.reset(page.total);
      this.trackWindow.primePage(0, page.items);
      this.total = page.total;
      selection.clear();
      // restore=true：重新加载后回到原来的位置（"加歌/移动"这类原地操作）
      this.restoreScrollTop = options.restore ? ui.scrollTop : 0;
    } catch (err) {
      if (generation !== this.generation) return;
      notify.showError(errorMessage(err));
    } finally {
      if (generation === this.generation) this.loading = false;
      this.booted = true;
    }
  }

  /** 播放列表里第 index 项（列表来自后端，UI 不自己拼队列） */
  async playIndex(index: number): Promise<void> {
    notify.clearError();
    selection.clear();
    try {
      await player.playQuery(this.query(), index);
    } catch (err) {
      notify.showError(errorMessage(err));
    }
  }

  async playAll(): Promise<void> {
    if (this.total === 0) return;
    selection.clear();
    try {
      await player.playQuery(this.query(), 0);
    } catch (err) {
      notify.showError(errorMessage(err));
    }
  }

  /* ---------------- 搜索 / 排序 / 筛选（改查询 = 重载） ---------------- */

  /**
   * 搜索输入。**防抖在 store 里**：TitleBar 的搜索框每敲一个字符都调一次
   * （经 `nav.search()`），但只有停顿 120 ms 才发查询 —— 否则 5 首/字的连打会排队等 IPC。
   */
  setSearch(value: string): void {
    // 搜索始终针对全库。第一次输入时离开仪表盘/歌单详情，避免把关键词
    // 错套到当前歌单；后续每个字符只更新查询，不再重置视图。
    if (value.trim() && (!ui.libraryDetail || ui.page !== 'home' || !ui.search.trim())) {
      ui.page = 'home';
      ui.libraryDetail = true;
      ui.view = { kind: 'library' };
      ui.artistId = null;
      ui.albumId = null;
    }
    ui.search = value;
    clearTimeout(this.searchTimer);
    this.searchTimer = setTimeout(() => void this.reload(), SEARCH_DEBOUNCE_MS);
  }

  clearSearch(): void {
    clearTimeout(this.searchTimer);
    ui.search = '';
    void this.reload();
  }

  /** 点同一个字段 = 反向排序；换字段 = 升序重来 */
  setSort(field: TrackSort): void {
    if (ui.sort === field) {
      ui.direction = ui.direction === 'asc' ? 'desc' : 'asc';
    } else {
      ui.sort = field;
      ui.direction = 'asc';
    }
    void this.reload();
  }

  /** 清艺人/专辑筛选（列表上方的「清除」按钮） */
  clearFilter(): void {
    ui.artistId = null;
    ui.albumId = null;
    void this.reload();
  }

  /* ---------------- 批量操作（选择集 → 命令） ---------------- */

  /** 选中项加入队列末尾 */
  async batchEnqueue(): Promise<void> {
    const ids = await this.selectionIds();
    if (ids.length === 0) return;
    try {
      await player.enqueue(ids);
      notify.showToast(`已加入队列 ${ids.length} 首`);
      selection.clear();
    } catch (err) {
      notify.showError(errorMessage(err));
    }
  }

  /** 选中项替换当前队列并从第一首起播 */
  async batchPlay(): Promise<void> {
    const ids = await this.selectionIds();
    if (ids.length === 0) return;
    try {
      await player.playTracks(ids, 0);
      selection.clear();
    } catch (err) {
      notify.showError(errorMessage(err));
    }
  }

  /**
   * 从曲库移除（**只删索引，不动磁盘文件**，扫描会把它重新导回来）。
   *
   * 为什么要 reload 两次之外的东西：侧栏计数（stats）与歌单里的曲目数都会变
   */
  async removeTracks(ids: string[]): Promise<void> {
    if (ids.length === 0) return;
    let removed: number;
    try {
      removed = await library.removeTracks(ids);
    } catch (err) {
      notify.showError(errorMessage(err));
      return;
    }
    notify.showToast(`已移出曲库 ${removed} 首`);
    selection.clear();
    await library.loadStats();
    await ui.refreshPlaylists();
    await this.reload({ restore: true });
  }

  dispose(): void {
    clearTimeout(this.searchTimer);
  }

  /**
   * 选中集下标 → 曲目 ID（后端按同一查询换算，渲染层不拉整库）。
   * 失败时写错误条并返回空数组：调用方一律当"没选中"处理。
   */
  async selectionIds(): Promise<string[]> {
    if (selection.size === 0) return [];
    try {
      return await library.idsForSelection(this.query(), selection.sortedIndices());
    } catch (err) {
      notify.showError(errorMessage(err));
      return [];
    }
  }

  /* ---------------- 视图切换（切视图 = 换查询 + 重载 + 存状态） ---------------- */

  /**
   * 切到内置视图。**同一个视图再点一次 = 重新拉**（用户手滑点错了想刷新，
   * 不该毫无反应）。
   */
  openHome(): void {
    clearTimeout(this.searchTimer);
    ui.search = '';
    ui.artistId = null;
    ui.albumId = null;
    ui.view = { kind: 'library' };
    ui.libraryDetail = false;
    selection.clear();
    ui.openPage('home');
  }

  /**
   * 切到内置视图。
   *
   * `sort` 用于"最近添加 → 查看全部"这类**带排序语义的入口**：以前它只能
   * 打开「全部曲目」，排序用的是上一次留下的字段，用户看到的并不是最近
   * 添加的顺序。同一个视图再点一次仍然重新拉（手滑点错了想刷新）。
   */
  openBuiltinView(
    kind: 'library' | 'recent' | 'mostPlayed',
    sort?: { field: TrackSort; direction: 'asc' | 'desc' },
  ): void {
    clearTimeout(this.searchTimer);
    ui.page = 'home';
    ui.libraryDetail = true;
    ui.search = '';
    ui.artistId = null;
    ui.albumId = null;
    if (sort) {
      ui.sort = sort.field;
      ui.direction = sort.direction;
    }
    if (ui.view.kind !== kind) ui.view = { kind };
    void this.reload();
    ui.scheduleUiSave();
  }

  openPlaylist(id: string): void {
    clearTimeout(this.searchTimer);
    ui.page = 'home';
    ui.libraryDetail = true;
    ui.search = '';
    ui.artistId = null;
    ui.albumId = null;
    ui.view = { kind: 'playlist', id };
    void this.reload();
    ui.scheduleUiSave();
  }

  /**
   * 滚动位置：由 TrackList 的 onscroll 写，位置本身存在 ui store
   * （它是 UiState 的字段），这里只负责排一次防抖落盘。
   */
  setScroll(scrollTop: number): void {
    ui.scrollTop = scrollTop;
    ui.scheduleUiSave();
  }
}

export const list = new ListStore();
