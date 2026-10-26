import type { TrackSort } from '../lib/query';
import { list } from './list.svelte';
import { isListPage, ui, type AppPage } from './ui.svelte';

/**
 * 导航 store：**路由动作的唯一入口** + 返回栈。
 *
 * 为什么要有它（docs/24 §3.1，2026-10-26 壳体收口）：
 *
 * 1. 宽 TitleBar 上有「返回上一页」，而路由状态曾经是 `ui.page` /
 *    `ui.libraryDetail` / `ui.view` **三个字段**拼出来的。按钮若自己去猜
 *    "回哪"，会出现「从设置回首页却变回歌单」这种反直觉跳转。把"去哪 +
 *    记到哪"收进一处，顶栏与侧栏读同一个栈，语义才是同一条。
 * 2. 之前四个入口（`list.openHome` / `openBuiltinView` / `openPlaylist` /
 *    `ui.openPage`）各自手写一遍"清搜索 + 清筛选 + 清选择"，四处抄同一段；
 *    调用方还得记住哪个入口带排序参数。侧栏、首页卡片、空态按钮、播放条
 *    **都**是调用方，一处漏了就变成"点了没反应"。现在它们只说"我要去哪"。
 *
 * **两步走（S1 只收动作，B4-1 收状态）**：S1 时路由表示法没动，`go()` 只是
 * 转调既有入口，视觉零变化；B4-1 拆页后 `ui.page` 扩到 8 值、`libraryDetail`
 * 删除，`view` 变成 `page` 的派生 —— 而这个类的**签名一处没改**，侧栏/首页
 * 卡片/空态按钮那些调用点一行都没动。先收**动作**、再收**状态**，正是这两步
 * 能分开做、且都不需要一次性大爆的原因。
 *
 * 依赖方向：nav → list → ui，单向。`list` **不** import nav（否则成环），
 * 所以"搜索引起的隐式跳转"由顶栏调 `nav.search()` 触发压栈，而不是让
 * `list.setSearch()` 回头调 nav。
 */

/**
 * 一帧路由快照：够 `restore()` 把界面还原到那一刻。
 *
 * B4-1 之后快照只剩**一个**路由字段（`page` + `playlistId` 参数）。之前要存
 * `page` / `libraryDetail` / `view` 三样，靠"三个都写对"才能还原 —— 少写
 * 一个就变成"返回了但落在错误的列表上"，而这种 bug 只在多点几次返回时出现。
 */
type RouteSnapshot = {
  page: AppPage;
  playlistId: string | null;
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
    // 判据是"当前已在曲库列表页"而不是"搜索词空不空"：后者在"清空再输入"
    // 时会再压一次，栈里出现两个一样的快照。
    if (value.trim() && ui.page !== 'library') this.push();
    list.setSearch(value);
  }

  clearSearch(): void {
    list.clearSearch();
  }

  /* ---------------- 内部 ---------------- */

  private snapshot(): RouteSnapshot {
    return {
      page: ui.page,
      playlistId: ui.playlistId,
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
        return ui.page === 'home';
      case 'builtin':
        return ui.page === target.view;
      case 'playlist':
        return ui.page === 'playlist' && ui.playlistId === target.id;
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
    ui.playlistId = s.playlistId;
    ui.search = s.search;
    ui.artistId = s.artistId;
    ui.albumId = s.albumId;
    ui.sort = s.sort;
    ui.direction = s.direction;
    ui.scrollTop = s.scrollTop;
    ui.closeQueue();

    // 只有落在列表页才重拉列表：正在播放/设置页看不到列表，
    // 白跑一次查询还会把错误条带出来
    if (isListPage(s.page)) void list.reload({ restore: true });
    if (s.page === 'settings') void ui.refreshDiagnostics();
    ui.scheduleUiSave();
  }
}

export const nav = new NavStore();
