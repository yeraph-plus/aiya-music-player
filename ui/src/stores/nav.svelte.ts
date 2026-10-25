import type { TrackSort } from '../lib/query';
import { parseViewKey, viewKey } from '../lib/view';
import { list } from './list.svelte';
import { ui, type AppPage } from './ui.svelte';

/**
 * 导航 store：**路由动作的唯一入口** + 返回栈。
 *
 * 为什么要有它（docs/24 §3.1，2026-10-26 壳体收口）：
 *
 * 1. 宽 TitleBar 上有「返回上一页」，而路由状态是 `ui.page` / `ui.libraryDetail`
 *    / `ui.view` **三个字段**拼出来的。按钮若自己去猜"回哪"，会出现
 *    「从设置回首页却变回歌单」这种反直觉跳转。把"去哪 + 记到哪"收进一处，
 *    顶栏与侧栏读同一个栈，语义才是同一条。
 * 2. 之前四个入口（`list.openHome` / `openBuiltinView` / `openPlaylist` /
 *    `ui.openPage`）各自手写一遍"清搜索 + 清筛选 + 清选择"，四处抄同一段；
 *    调用方还得记住哪个入口带排序参数。侧栏、首页卡片、空态按钮、播放条
 *    **都**是调用方，一处漏了就变成"点了没反应"。现在它们只说"我要去哪"。
 *
 * **本轮范围克制（S1）**：这里不改路由的表示法，`ui.page` / `libraryDetail` /
 * `view` 仍然是真相，`go()` 只是转调既有入口 —— 所以这一步**视觉零变化**。
 * 等 B4 拆页（删 `libraryDetail`、路由从 4 值扩到 8 值）时，这个类的签名
 * 不用变，调用点一行都不用动。先收**动作**，再收**状态**，是这两步能分开做的原因。
 *
 * 依赖方向：nav → list → ui，单向。`list` **不** import nav（否则成环），
 * 所以"搜索引起的隐式跳转"由顶栏调 `nav.search()` 触发压栈，而不是让
 * `list.setSearch()` 回头调 nav。
 */

/** 一帧路由快照：够 `restore()` 把界面还原到那一刻 */
type RouteSnapshot = {
  page: AppPage;
  libraryDetail: boolean;
  /** 存字符串（UiState 的形态），恢复时 parseViewKey；避免快照持有 View 对象引用 */
  view: string;
  search: string;
  artistId: string | null;
  albumId: string | null;
  sort: TrackSort;
  direction: 'asc' | 'desc';
  scrollTop: number;
};

export type NavTarget =
  /** 首页仪表盘（歌单宫格 + 三段预览） */
  | { kind: 'dashboard' }
  /** 曲库三个内置视图，共用一套列表 */
  | {
      kind: 'builtin';
      view: 'library' | 'recent' | 'mostPlayed';
      /** 「最近添加 → 查看全部」这类带排序语义的入口 */
      sort?: { field: TrackSort; direction: 'asc' | 'desc' };
    }
  | { kind: 'playlist'; id: string }
  /** 一级整页：正在播放 / 电台 / 设置 */
  | { kind: 'page'; page: AppPage };

/** 栈深上限。50 步足够覆盖"点着点着迷路"，再深也没有用户会连按 50 次返回 */
const MAX_DEPTH = 50;

class NavStore {
  /**
   * 返回栈。**只读给 UI**（`canGoBack`），写入只在 `push` / `back` 里。
   * 用 `$state` 是为了让顶栏返回键的 disabled 跟着变。
   */
  history = $state<RouteSnapshot[]>([]);

  get canGoBack(): boolean {
    return this.history.length > 0;
  }

  /**
   * 去某个地方。
   *
   * 目标与当前路由等价时**不压栈但仍执行**：侧栏连点两次「全部曲目」应该是
   * "刷新这一页"（用户手滑点错了想要新数据），而不是往栈里塞一个自己。
   */
  go(target: NavTarget): void {
    if (!this.isCurrent(target)) this.push();
    this.apply(target);
  }

  /** 返回上一页。栈空时无操作（按钮该是 disabled 的） */
  back(): void {
    const snapshot = this.history.pop();
    if (!snapshot) return;
    this.restore(snapshot);
  }

  /**
   * 搜索框输入。
   *
   * 搜索**是**一次路由跳转（第一次输入会把仪表盘/歌单换成曲库详情列表），
   * 所以它要压栈；但每敲一个字符都调这里，不能每个字符压一次。
   * 判据用「当前在不在曲库详情」而不是「搜索词空不空」：后者在
   * "清空再输入"时会再压一次，栈里出现两个一样的快照。
   */
  search(value: string): void {
    const inLibrary = ui.page === 'home' && ui.libraryDetail && ui.view.kind === 'library';
    if (value.trim() && !inLibrary) this.push();
    list.setSearch(value);
  }

  clearSearch(): void {
    list.clearSearch();
  }

  /* ---------------- 内部 ---------------- */

  private snapshot(): RouteSnapshot {
    return {
      page: ui.page,
      libraryDetail: ui.libraryDetail,
      view: viewKey(ui.view),
      search: ui.search,
      artistId: ui.artistId,
      albumId: ui.albumId,
      sort: ui.sort,
      direction: ui.direction,
      scrollTop: ui.scrollTop,
    };
  }

  private push(): void {
    this.history.push(this.snapshot());
    if (this.history.length > MAX_DEPTH) this.history.shift();
  }

  /** 目标是否就是当前所在（用于"同页再点一次 = 刷新，不压栈"） */
  private isCurrent(target: NavTarget): boolean {
    switch (target.kind) {
      case 'dashboard':
        return ui.page === 'home' && !ui.libraryDetail;
      case 'builtin':
        return ui.page === 'home' && ui.libraryDetail && ui.view.kind === target.view;
      case 'playlist':
        return (
          ui.page === 'home' &&
          ui.libraryDetail &&
          ui.view.kind === 'playlist' &&
          ui.view.id === target.id
        );
      case 'page':
        return ui.page === target.page;
    }
  }

  private apply(target: NavTarget): void {
    switch (target.kind) {
      case 'dashboard':
        list.openHome();
        break;
      case 'builtin':
        list.openBuiltinView(target.view, target.sort);
        break;
      case 'playlist':
        list.openPlaylist(target.id);
        break;
      case 'page':
        ui.openPage(target.page);
        break;
    }
  }

  /**
   * 还原一帧快照。
   *
   * 字段逐个赋值而不是"调某个入口"：入口会清搜索/清筛选，正好把快照的意义
   * 抹掉（返回歌单页却丢了当时的筛选条件，是这类实现最常见的坏味道）。
   */
  private restore(s: RouteSnapshot): void {
    ui.page = s.page;
    ui.libraryDetail = s.libraryDetail;
    ui.view = parseViewKey(s.view);
    ui.search = s.search;
    ui.artistId = s.artistId;
    ui.albumId = s.albumId;
    ui.sort = s.sort;
    ui.direction = s.direction;
    ui.scrollTop = s.scrollTop;
    ui.closeQueue();

    // 只有落在曲库（page === 'home'）才重拉列表：正在播放/设置页看不到列表，
    // 白跑一次查询还会把错误条带出来
    if (s.page === 'home') void list.reload({ restore: true });
    if (s.page === 'settings') void ui.refreshDiagnostics();
    ui.scheduleUiSave();
  }
}

export const nav = new NavStore();
