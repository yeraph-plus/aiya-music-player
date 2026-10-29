import type { BeetsMarketItem, BeetsStatus, Diagnostics } from '../lib/ipc';
import type { TrackSort } from '../lib/query';
import type { AppSettings } from '../lib/settings';
import { DEFAULT_SETTINGS } from '../lib/settings';
import { ipc, events, type BeetsInstallTrack, type DynamicJson } from '../lib/ipc';
import { errorMessage } from '../lib/errors';
import { devValidate, settingsSchema, uiStateSchema, type UiState } from '../lib/ipc/schema';
import {
  LIBRARY_VIEW,
  parseViewKey,
  type View,
} from '../lib/view';
import { readLook } from '../lib/look';
import { notify } from './notify.svelte';

/**
 * 查重结果里的单条曲目（docs/29 批 D）。
 *
 * 为什么手写而不进 `generated/`：它是 **sidecar 事件**的载荷，宿主只透传不
 * 解释（与 `beets:import` 同一定位）——把它塞进 ts-rs 等于让 Rust 编译期
 * 依赖 beets 的返回结构。字段与 `beets_sidecar.py::_item_brief` 一一对应。
 */
export interface BeetsDupItem {
  id: number;
  path: string;
  title: string;
  artist: string;
  album: string;
  /** 秒 */
  length: number;
  /** kbps */
  bitrate: number;
  format: string;
  size: number;
}

/**
 * 一级路由。**12 值**，每个值对应 `App.svelte` 里的一个分支（docs/24 §3.2）。
 *
 * 2026-10-26 B4-1 拆页：此前是 `home | nowPlaying | radio | settings` 四个值，
 * 六个列表视图全都寄居在 `home` 上，靠 `libraryDetail: boolean` + `view` 两个
 * 字段拼出"我现在到底在哪"——`App.svelte` 里那句
 * `{#if ui.page === 'home'}{#if ui.libraryDetail}<LibraryView/>{:else}<HomeView/>`
 * 就是这个双字段组合的化石。列表视图升级成一级值之后，`libraryDetail` 删掉，
 * 那句嵌套判断变成一行一个分支。
 *
 * 2026-10-26 B4-3b 再扩 4 值：`albums`/`artists` 是宫格浏览页（非列表页），
 * `album`/`artist` 是详情页（列表页）。**详情页的参数复用 `albumId`/`artistId`
 * 两个筛选字段**，不新增 `facetId`：详情页的曲目列表本来就是"曲库查询 +
 * albumId 过滤"（`list.query()` 已从这两个字段取筛选），复用意味着查询、
 * 返回栈快照、UiState 持久化三处**零改动**——新字段则三处都要跟着加。
 * 代价是"page=album 的参数"与"library 页的筛选 chip"共享一个存储位，但
 * 两者永远不会同时可见（不同路由值），语义不冲突。
 *
 * 2026-10-26 B4-3c 加 `search`：独立搜索结果页（此前搜索直接寄居曲库列表）。
 * 交给 LibraryView 渲染（曲目列表与曲库页同一查询路径），头部多专辑/艺人两个
 * 聚合段。`search` 是列表页（isListPage），因为它就是"library + search 过滤"。
 *
 * `playlist` / `album` / `artist` 是带参数的路由，参数见上。
 *
 * 2026-10-26 docs/28 减 2 值：`recent` / `mostPlayed` 单页退役（常听收进
 * 推荐页 top-15，最近播放被筛选器取代，侧栏席位让给专辑/艺人宫格页）。
 * 旧 UiState 里落着这两个值的，由 `restore()` 兜底换算成曲库。
 *
 * 2026-10-26 docs/29 批 A 加 `tools`：侧栏第七项「扫描入库」（扫描/beets
 * 任务 + 日志诊断从设置页拆出，成为独立路由页——任务是长时跑的，
 * 值得一个可以停留、可以刷新、可以分享位置的一级入口）。
 */
export type AppPage =
  | 'home'
  | 'library'
  | 'playlist'
  | 'albums'
  | 'artists'
  | 'album'
  | 'artist'
  | 'search'
  | 'nowPlaying'
  | 'settings'
  | 'tools'
  | 'radio';

/** 哪些路由是"列表页"（共用 `ListStore` 的查询与虚拟列表） */
export function isListPage(page: AppPage): boolean {
  return (
    page === 'library' ||
    page === 'playlist' ||
    page === 'album' ||
    page === 'artist' ||
    page === 'search'
  );
}

/**
 * 界面状态 store：视图、搜索排序、筛选、队列抽屉、设置、诊断。
 *
 * 边界（2026-10-26 收窄过一次）：只描述"界面在哪、长什么样"。
 * 歌单在 `library.playlists` + `playlists`（编排），歌词在 `stores/lyrics`，
 * 列表怎么刷在 `list`——这三样都不在这里。
 *
 * 这里只持有"是什么"，不持有"列表怎么刷"——reload/TrackWindow 仍在 App：
 * store 改完状态后由 App 触发一次 reload。
 *
 * 动态边界（docs/18 §4.3）：`app_get_settings` / `app_get_ui_state` 返回的是
 * **无 DTO 的动态 JSON**（老配置文件缺字段是常态），所以这两个走 zod：
 * 开发期 `devValidate` 报形状不符，生产期只做合并默认值。
 * 其余命令的返回值都由 Rust 生成类型，不再二次校验。
 */
class UiStore {
  page = $state<AppPage>('home');
  /** `page === 'playlist'` 时的参数。其余路由下无意义（保留旧值，返回时才清） */
  playlistId = $state<string | null>(null);
  queueOpen = $state(false);
  /** EQ 抽屉（播放条 Popover）会话态开合，与队列抽屉互斥（docs/25 §1） */
  eqOpen = $state(false);
  /**
   * 列表批量操作模式（docs/27）：开 = 显示勾选列，关 = 隐藏。
   * 会话态不持久化；Ctrl+点行会自动把它打开（用户不一定要找到开关），
   * 关闭时由调用方清空选择集。
   */
  batchMode = $state(false);

  /**
   * 当前列表视图（`list.query()` 用它算 `source`）。
   *
   * **是 getter，不是独立状态**：拆页之前 `page` / `libraryDetail` / `view`
   * 三个字段各自可写，能拼出"page=home、libraryDetail=true、view=playlist"
   * 这种自相矛盾的组合，而每个读它们的地方都只检查其中两个。现在
   * `page`（+ `playlistId`）是唯一真相，`view` 由它派生 —— 想改路由只能
   * 走 `enterList` / `setView` / `openPage`。
   *
   * 返回值**引用稳定**：同一个路由永远拿到同一个对象。否则 `$derived(ui.view)`
   * 这类写法每次求值都得到新对象，下游按引用做记忆的地方会白算一遍。
   */
  get view(): View {
    switch (this.page) {
      case 'library':
      case 'album':
      case 'artist':
      case 'search':
        // 专辑/艺人详情页的曲目列表 = 曲库查询 + albumId/artistId 筛选，
        // 搜索页 = 曲库查询 + search 过滤 —— 数据形状与曲库页完全相同
        // （见类头注释"参数复用筛选字段"）
        return LIBRARY_VIEW;
      case 'playlist': {
        const id = this.playlistId ?? '';
        if (this.playlistView?.id !== id) this.playlistView = { kind: 'playlist', id };
        return this.playlistView;
      }
      default:
        // 非列表页：查询没有意义，给一个稳定的兜底（不必是当前路由）
        return LIBRARY_VIEW;
    }
  }

  /** `playlist` 视图的缓存对象，保证 getter 引用稳定 */
  private playlistView: { kind: 'playlist'; id: string } | null = null;
  /**
   * 列表行高（px）。**不是设置项**，是 `--row-h` 的运行时镜像 ——
   * 虚拟列表要一个数字，而唯一出处是 CSS（见 `lib/look.ts: readRowHeight`）。
   * `App` 的外观 effect 每次切密度后重读。
   */
  rowHeight = $state(52);
  search = $state('');
  sort = $state<TrackSort>('title');
  direction = $state<'asc' | 'desc'>('asc');
  artistId = $state<string | null>(null);
  albumId = $state<string | null>(null);

  /**
   * 曲库页的 tab 索引（2026-10-28 用户拍板：最近播放/最近添加从推荐页
   * 迁来，成为曲库页「全部曲目」旁的 tab，不再是一级路由）。
   *
   * **会话态不持久化**（重启回「全部曲目」是可预期的复位）；
   * 只在 `page === 'library'` 时参与 `list.query()` 的 source 换算——
   * 搜索页/详情页同走 LIBRARY_VIEW 但不吃 tab，见 list store 的 query()。
   */
  libraryTab = $state<'all' | 'recent' | 'added'>('all');

  /**
   * 初始值不是纯 `DEFAULT_SETTINGS`：主题/密度先取 localStorage 镜像
   * （`lib/look.ts`）。否则 `main.ts` 已经把首帧设成浅色，这里却是深色，
   * `App` 的主题 effect 首次运行会把 `data-theme` 写回深色，等 bootstrap
   * 返回真值再改回来——实测就是 `light → dark → light` 三次跳变。
   *
   * 其余字段仍走默认值：它们不影响首帧观感，bootstrap 一回来就覆盖。
   */
  settings = $state<AppSettings>({ ...DEFAULT_SETTINGS, ...readLook() });
  diagnostics = $state<Diagnostics | null>(null);
  beets = $state<BeetsStatus | null>(null);

  /**
   * 列表滚动位置。**普通字段不是 `$state`**：只用于持久化与"重载后恢复"，
   * 没有任何视图按它渲染（滚动位置由 TrackList 自己的 DOM 状态决定）。
   */
  scrollTop = 0;

  /** UiState 防抖句柄（scheduleUiSave 内部用） */
  private readonly saveTimer = {
    handle: undefined as ReturnType<typeof setTimeout> | undefined,
    clear() {
      clearTimeout(this.handle);
    },
    arm(fn: () => void) {
      this.handle = setTimeout(fn, 600);
    },
  };

  /* ---------------- 视图状态持久化 ---------------- */

  /**
   * 防抖写 UiState。
   *
   * 定时器与 `scrollTop` 都归这里（批次 3 之前定时器挂在 App.svelte，调用方
   * 还得传一个 timer 对象进来 —— 三个调用点各写一遍 clear/arm，纯搬砖）。
   *
   * `scrollTop` 放在 ui 而不是列表 store：它本来就是 UiState 的字段，而
   * 列表 store 依赖 ui（不能反向 import），放这边 store 才不用为了一个整数
   * 在每层传 getter。列表滚动时调 `ui.scrollTop = n` 再 `scheduleUiSave()`。
   *
   * 600 ms 是"滚动停下就存"的经验值：再短会每滚一屏写一次库。
   */
  scheduleUiSave(): void {
    this.saveTimer.clear();
    this.saveTimer.arm(() => {
      const state: UiState = {
        sort: this.sort,
        direction: this.direction,
        scrollTop: Math.round(this.scrollTop),
        artistId: this.artistId,
        albumId: this.albumId,
        page: this.page,
        playlistId: this.playlistId,
      };
      // 视图状态丢失不影响可用性，失败只记日志（别打断滚动）
      void ipc.app.setUiState(state as DynamicJson).catch((e) => {
        console.warn('setUiState failed:', errorMessage(e));
      });
    });
  }

  /** 清理：组件卸载时取消未落盘的写入 */
  dispose(): void {
    this.saveTimer.clear();
  }

  /**
   * 还原 UiState。**要兼容拆页之前的旧文件**（docs/24 §3.2）。
   *
   * 旧形状是 `page: 'home'|'nowPlaying'|'radio'|'settings'` + `libraryDetail`
   * + `view: 'library'|'recent'|'mostPlayed'|'playlist:xxx'`。旧文件里
   * `page` 恒为 `home` 也可能表示六个列表视图之一，所以**不能**只看 `page`：
   * 先认新字段，缺了再按旧字段换算。换算错的表现是"重启后回到首页、
   * 用户以为丢了位置"，不报错，只会被当成玄学。
   */
  restore(data: UiState): void {
    if (data.sort) this.sort = data.sort;
    if (data.direction) this.direction = data.direction;
    this.artistId = data.artistId ?? null;
    this.albumId = data.albumId ?? null;

    // docs/28：recent/mostPlayed 单页退役，旧文件落在这两个值上时换算成曲库
    //（恢复成"最近播放"没有意义——那个页面已经不存在了）
    const RETIRED_PAGES: ReadonlySet<string> = new Set(['recent', 'mostPlayed']);
    const rawPage = data.page && RETIRED_PAGES.has(data.page) ? 'library' : data.page;

    if (rawPage && rawPage !== ('home' as string)) {
      // 新形状（或旧形状的一级页）
      this.page = rawPage as AppPage;
      this.playlistId = data.playlistId ?? null;
    } else if (data.libraryDetail) {
      // 旧形状：home + libraryDetail=true 其实是某个列表视图
      //（parseViewKey 对 retired 的 'recent'/'mostPlayed' 键同样回落曲库）
      const legacy = parseViewKey(data.view);
      this.applyView(legacy);
    } else {
      this.page = 'home';
      this.playlistId = null;
    }

    // 启动就落在设置页/扫描入库页时**不会**经过 openPage，诊断块会一直空着
    // （显示"正在读取…"直到用户手动点刷新）。这里补一次。
    if (this.page === 'settings' || this.page === 'tools') void this.refreshDiagnostics();
  }

  /* ---------------- 一级页面与队列抽屉 ---------------- */

  openPage(page: AppPage): void {
    this.page = page;
    if (page !== 'playlist') this.playlistId = null;
    this.queueOpen = false;
    this.eqOpen = false;
    this.scheduleUiSave();
    // 设置页与扫描入库页共用同一份诊断/beets 数据（docs/29 批 A 拆页）。
    //
    // **此前这里只写了 settings**：从侧栏点进「扫描入库」时诊断区永远停在
    // "正在读取…"、beets 块永远停在"正在检测…"，只有手动点一下刷新才出来
    // ——批 A 的 e2e 只断言了目录 textarea 与设置页分区，没看这两块内容，
    // 所以漏掉了。批 D 的「曲库维护」直接依赖 beets 状态（库根/是否可用），
    // 这个洞必须一起补上。
    if (page === 'settings' || page === 'tools') {
      this.diagnostics = null;
      void this.refreshDiagnostics();
    }
  }

  /**
   * 进入「列表视图」上下文：一级页回落首页、关掉队列抽屉、清搜索与筛选、
   * 换成目标视图。
   *
   * 抽出来之前这段在 list store 里抄了 4 份（openHome / openBuiltinView /
   * openPlaylist / setSearch），4×6 行同样的赋值。漏一行不会报错，只会留下
   * "切了视图筛选还在"这种状态残留 —— 症状是列表少一半数据，最难查的一类。
   *
   * 它**不负责落盘与重载**：调用方决定（搜索每敲一个字都落盘太吵）。
   */
  enterList(view: View): void {
    this.applyView(view);
    this.search = '';
    this.artistId = null;
    this.albumId = null;
    this.queueOpen = false;
    this.eqOpen = false;
  }

  /**
   * 回首页仪表盘。清理语义与 `enterList` 一致（首页不是列表页，但它同样是
   * "用户主动导航到别处"，旧搜索词不该跟着 —— 否则回去看到的是"首页 +
   * 一个还在生效的搜索框"）。
   */
  enterHome(): void {
    this.page = 'home';
    this.playlistId = null;
    this.search = '';
    this.artistId = null;
    this.albumId = null;
    this.queueOpen = false;
    this.eqOpen = false;
  }

  /**
   * 进入搜索结果页（B4-3c）。
   *
   * 与 enterList 的唯一差别：**不清 search**（search 是本页的主题），
   * 且目标是 search 页。旧筛选与歌单参数不该跟着搜索词走——在歌单里
   * 搜索会得到"全库搜索结果"，原来歌单的上下文到此为止。
   */
  enterSearch(): void {
    this.page = 'search';
    this.playlistId = null;
    this.artistId = null;
    this.albumId = null;
    this.queueOpen = false;
    this.eqOpen = false;
  }

  /**
   * 进入专辑/艺人详情页（B4-3b）。
   *
   * 参数复用 `albumId`/`artistId` 筛选字段（AppPage 注释说明了为什么不加
   * 独立字段），所以**不能**先调 `enterList` 再覆盖——enterList 会把两个
   * 筛选字段清空，顺序错了就是"详情页显示全库"。清理语义与 enterList
   * 一致（从别处点进来，旧搜索词与另一侧筛选不该跟着），只是目标值不同。
   *
   * 它不负责落盘与重载（同 enterList：调用方决定）。
   */
  enterFacet(kind: 'album' | 'artist', id: string): void {
    this.search = '';
    this.artistId = kind === 'artist' ? id : null;
    this.albumId = kind === 'album' ? id : null;
    this.page = kind;
    this.playlistId = null;
    this.queueOpen = false;
    this.eqOpen = false;
  }

  /**
   * 只换视图，**不动**搜索/筛选/队列抽屉。
   *
   * `enterList` 清三样是因为它代表"用户主动导航到某处"（从别的地方点进来，
   * 旧筛选不该跟着）。但有两类调用方不是这个语义：
   * - `playlists.refresh()` 发现当前歌单已被删掉 → 退回曲库，此时不该
   *   顺手把用户的搜索词也清掉；
   * - `playlists.play()` 先切视图再 playlist 整列表起播 → 同一个动作内部，
   *   清筛选会把"按当前筛选播放"变成"播放全部"。
   *
   * 拆页之前这两处直接写 `ui.view = ...`，绕过了 enterList 的清理 —— 行为
   * 看着对，但那是因为**清理逻辑当时不在同一个字段上**。现在 `view` 没有
   * setter，语义差异必须是两个方法，不能靠"有的地方绕过"。
   */
  setView(view: View): void {
    this.applyView(view);
  }

  private applyView(view: View): void {
    this.page = view.kind;
    this.playlistId = view.kind === 'playlist' ? view.id : null;
  }

  toggleQueue(): void {
    this.setQueue(!this.queueOpen);
  }

  /** 队列抽屉开合的**唯一入口**（B5a 起 PlayerBar 的 Popover 直接受控于它） */
  setQueue(open: boolean): void {
    this.queueOpen = open;
    // EQ 抽屉与队列同侧同层，互斥（docs/25 §1）：一个开另一个关
    if (open) this.eqOpen = false;
  }

  closeQueue(): void {
    this.queueOpen = false;
  }

  /** EQ 抽屉开合的唯一入口（docs/25 §1 T3）；开时关队列，规则对等 */
  setEqDrawer(open: boolean): void {
    this.eqOpen = open;
    if (open) this.queueOpen = false;
  }

  /* ---------------- 设置 ---------------- */

  /**
   * 必须 snapshot：settings 是 `$state`，里面的 `shortcuts` / `output` 是 Proxy。
   * Proxy 过不了结构化克隆，直接 invoke 会抛 "An object could not be cloned."，
   * 而主题/密度在本地已经生效 —— 看起来一切正常，设置却没落盘。
   *
   * 先用新值本地生效（主题切换要跟手），再以后端返回的为准：
   * 后端会补齐默认值/拒绝非法值，这一步是"以磁盘为准"的对账。
   *
   * 请求代次：快速连改两个设置会并发两条 setSettings，先发的可能后回来，
   * 把"以磁盘为准"的合并结果盖回旧值。只有最新一次发出的响应允许落盘；
   * 过期响应的失败也同样静默（新请求成功的话旧失败是噪音）。
   *
   * 失败**默认落到全局错误条**（不再收一个可选回调）：漏传回调的调用点会让
   * 写盘失败静默吞掉，症状是"设置看着生效、重启就回滚"——比不生效更难查。
   */
  private settingsGeneration = 0;

  applySettings(next: AppSettings): void {
    const generation = (this.settingsGeneration += 1);
    this.settings = next;
    void ipc.app
      .setSettings($state.snapshot(next) as DynamicJson)
      .then((raw) => {
        if (generation !== this.settingsGeneration) return;
        this.settings = mergeSettings(raw);
      })
      .catch((e) => {
        if (generation !== this.settingsGeneration) return;
        notify.showError(errorMessage(e));
      });
  }

  openDataDir(): void {
    void ipc.app.openDataDir().catch((e) => console.warn('openDataDir failed:', errorMessage(e)));
  }

  async refreshDiagnostics(): Promise<void> {
    try {
      this.diagnostics = await ipc.app.diagnostics();
    } catch (e) {
      console.warn('diagnostics failed:', errorMessage(e));
      this.diagnostics = null;
    }
    void this.refreshBeets();
  }

  /** beets 环境探测（低频：设置页打开/手动刷新时；最坏 ~30s，必须容错） */
  async refreshBeets(): Promise<void> {
    this.beets = null;
    try {
      this.beets = await ipc.beets.status();
    } catch (e) {
      console.warn('beets.status failed:', errorMessage(e));
    }
  }

  /**
   * beets 运行时安装状态（docs/29 批 C）。
   *
   * 放在 ui store 而不是组件里：进度来自事件流，而"组件不直接 listen"是
   * docs/17 §4 的硬约定（多个组件同时开时事件会被订阅多次）。
   */
  beetsInstall = $state<{ running: boolean; track: BeetsInstallTrack; stage: string; message: string; error: string | null }>({
    running: false,
    track: 'installer',
    stage: '',
    message: '',
    error: null,
  });

  /** 订阅安装进度。只在 App 启动时调一次。 */
  watchBeetsInstall(): () => void {
    return events.beetsInstall((e) => {
      const type = typeof e.type === 'string' ? e.type : '';
      const stage = typeof e.stage === 'string' ? e.stage : '';
      const message = typeof e.message === 'string' ? e.message : '';
      if (type === 'finished') {
        // 终态带 status：装没装成以 probe 为准，不以退出码为准
        this.beetsInstall = { running: false, track: this.beetsInstall.track, stage: 'done', message: '', error: null };
        const status = readBeetsStatus(e.status);
        if (status) this.beets = status;
        return;
      }
      if (type === 'error') {
        this.beetsInstall = {
          running: false,
          track: this.beetsInstall.track,
          stage,
          message,
          error: message || '安装失败',
        };
        return;
      }
      this.beetsInstall = { running: true, track: this.beetsInstall.track, stage, message, error: null };
    });
  }

  /**
   * 触发安装（docs/33 §6.2）。
   *
   * 两条轨道都只是「下载 → 展开 → 记入口 → 跑一次 status」；选哪条由用户的
   * 机器决定（没 Python 只能走 bundle），所以这里不做任何预判，把失败原样
   * 抛给按钮旁的文字。
   */
  async installBeets(track: BeetsInstallTrack = 'installer'): Promise<void> {
    this.beetsInstall = {
      running: true,
      track,
      stage: 'download',
      message: '正在准备…',
      error: null,
    };
    try {
      await ipc.beets.install(track);
    } catch (e) {
      this.beetsInstall = { running: false, track, stage: '', message: '', error: errorMessage(e) };
    }
  }

  /* ---------------- 批 D：曲库维护 ---------------- */

  /**
   * 维护任务状态。四条命令（adopt/update/duplicates/move）共用一份：
   * 同一时刻只可能有一个会话（宿主侧 reserve_session 互斥），分四份只会
   * 让组件写四遍几乎一样的渲染逻辑。
   *
   * `details` 每次开跑都清空——它们是"这次任务的结果"，不是历史。
   */
  beetsMaint = $state<{
    kind: '' | 'adopt' | 'update' | 'duplicates' | 'move' | 'plugin';
    running: boolean;
    stage: string;
    current: number;
    total: number;
    /** 一行汇总（各命令的形状不同，归一成给人看的句子） */
    summary: string;
    error: string | null;
    /** 是否停在预览（move/update 的 pretend 结果，等用户确认） */
    preview: boolean;
    /** move 的「从哪到哪」清单（含 conflict/same） */
    moves: { path: string; dest: string; status: string; reason: string }[];
    /** 查重分组 */
    dupGroups: { key: string[]; items: BeetsDupItem[] }[];
    /** update 的字段 diff 明细 */
    updates: { path: string; changes: Record<string, { from: string; to: string }> }[];
  }>({
    kind: '',
    running: false,
    stage: '',
    current: 0,
    total: 0,
    summary: '',
    error: null,
    preview: false,
    moves: [],
    dupGroups: [],
    updates: [],
  });

  /** 明细上限：与 sidecar 的 MAINT_MAX_DETAIL 对齐，多出来的不渲染 */
  private static readonly MAINT_DETAIL_CAP = 500;

  /**
   * 插件市场目录（docs/40 §3 白名单版）。`null` = 还没拉过。
   *
   * 更新有两条路：打开市场分区时 `loadBeetsMarket()` 拉一次；装完插件
   * 不再回查——pluginDone 终态自带复检报告，直接落进这里（少一次 ~1s
   * 的短进程开销）。
   */
  beetsMarket = $state<BeetsMarketItem[] | null>(null);
  beetsMarketError = $state<string | null>(null);

  /** 拉一次插件市场目录（市场分区展开时调用）。 */
  async loadBeetsMarket(): Promise<void> {
    try {
      this.beetsMarket = await ipc.beets.pluginCatalog();
      this.beetsMarketError = null;
    } catch (e) {
      this.beetsMarketError = errorMessage(e);
    }
  }

  /**
   * 安装白名单插件（仅 pypi 档）。会话槽与维护四件套**共用**——同一时刻
   * 只有一个 serve 会话，进度走同一条 `beets:maintenance` 流。
   */
  async installBeetsPlugin(name: string): Promise<void> {
    this.startMaint('plugin');
    try {
      await ipc.beets.pluginInstall(name);
    } catch (e) {
      this.beetsMaint = { ...this.beetsMaint, running: false, error: errorMessage(e) };
    }
  }

  /** 订阅维护事件。只在 App 启动时调一次。 */
  watchBeetsMaint(): () => void {
    return events.beetsMaintenance((e) => {
      const type = typeof e.type === 'string' ? e.type : '';
      const num = (k: string) => (typeof e[k] === 'number' ? (e[k] as number) : 0);
      if (type === 'progress') {
        this.beetsMaint = {
          ...this.beetsMaint,
          running: true,
          stage: typeof e.stage === 'string' ? e.stage : '',
          current: num('current'),
          total: num('total'),
        };
        return;
      }
      if (type === 'moveItem') {
        if (this.beetsMaint.moves.length >= UiStore.MAINT_DETAIL_CAP) return;
        this.beetsMaint.moves.push({
          path: typeof e.path === 'string' ? e.path : '',
          dest: typeof e.dest === 'string' ? e.dest : '',
          status: typeof e.status === 'string' ? e.status : '',
          reason: typeof e.reason === 'string' ? e.reason : '',
        });
        return;
      }
      if (type === 'duplicateGroup') {
        const items = Array.isArray(e.items) ? (e.items as BeetsDupItem[]) : [];
        this.beetsMaint.dupGroups.push({
          key: Array.isArray(e.key) ? (e.key as string[]) : [],
          items,
        });
        return;
      }
      if (type === 'adoptDone') {
        this.finishMaint(
          `登记 ${num('added')} 首（已存在跳过 ${num('skipped')} 首${
            num('failed') ? `，失败 ${num('failed')}` : ''
          }）`,
        );
        return;
      }
      if (type === 'updateDone') {
        // 明细也来自这里（sidecar 在终态事件里一次性给，不逐曲发）
        const details = Array.isArray(e.details) ? e.details : [];
        this.beetsMaint.updates = details as typeof this.beetsMaint.updates;
        const preview = e.pretend === true;
        this.finishMaint(
          `${preview ? '待更新' : '已更新'} ${num('changed')} 首（未变 ${num('unchanged')}${
            num('missing') ? `，缺失 ${num('missing')}` : ''
          }${num('failed') ? `，失败 ${num('failed')}` : ''}）`,
          preview,
        );
        return;
      }
      if (type === 'duplicatesDone') {
        this.finishMaint(
          `${num('groups')} 组重复，涉及 ${num('itemsInGroups')} 首（共 ${num('totalItems')} 首）`,
        );
        return;
      }
      if (type === 'moveDone') {
        const preview = e.pretend === true;
        this.finishMaint(
          preview
            ? `将移动 ${num('planned')} 首，冲突 ${num('conflict')} 首，原地 ${num('same')} 首`
            : `已移动 ${num('moved')} 首，冲突 ${num('conflict')} 首，原地 ${num('same')} 首`,
          preview,
        );
        return;
      }
      if (type === 'pluginProgress') {
        // 逐行 pip 输出，没有 current/total——stage 直接当"最新一行"显示
        this.beetsMaint = {
          ...this.beetsMaint,
          running: true,
          stage: typeof e.message === 'string' ? e.message : '',
        };
        return;
      }
      if (type === 'pluginDone') {
        // 终态自带复检报告（market_report），直接落目录，不再回查一次
        const name = typeof e.name === 'string' ? e.name : '插件';
        if (Array.isArray(e.report)) this.beetsMarket = e.report as BeetsMarketItem[];
        else void this.loadBeetsMarket();
        this.finishMaint(`${name} 安装完成（复检通过）`);
        return;
      }
      if (type === 'cancelled') {
        this.finishMaint('已取消（已完成的部分保留）');
        return;
      }
      if (type === 'sessionFailed') {
        const err = e.error;
        const msg =
          err && typeof err === 'object' && typeof (err as DynamicJson).beets_message === 'string'
            ? String((err as DynamicJson).beets_message)
            : '维护任务失败';
        this.beetsMaint = { ...this.beetsMaint, running: false, error: msg, summary: '' };
      }
    });
  }

  private finishMaint(summary: string, preview = false): void {
    this.beetsMaint = { ...this.beetsMaint, running: false, summary, preview, error: null };
  }

  private startMaint(kind: 'adopt' | 'update' | 'duplicates' | 'move' | 'plugin'): void {
    this.beetsMaint = {
      kind,
      running: true,
      stage: '',
      current: 0,
      total: 0,
      summary: '',
      error: null,
      preview: false,
      moves: [],
      dupGroups: [],
      updates: [],
    };
  }

  /** 建 beets 库基线（跑完宿主自动对账，读模型里的 beets_item_id 会被填上） */
  async adoptBeets(): Promise<void> {
    this.startMaint('adopt');
    try {
      await ipc.beets.adopt();
    } catch (e) {
      this.beetsMaint = { ...this.beetsMaint, running: false, error: errorMessage(e) };
    }
  }

  /** 刷新 beets 库标签。`pretend=true` 只出 diff 预览。 */
  async updateBeets(pretend: boolean): Promise<void> {
    this.startMaint('update');
    try {
      await ipc.beets.update(pretend);
    } catch (e) {
      this.beetsMaint = { ...this.beetsMaint, running: false, error: errorMessage(e) };
    }
  }

  /** 查重（只读）。keys 交给调用方给预设，空数组 = sidecar 默认 title+artist */
  async findDuplicates(keys: string[] = []): Promise<void> {
    this.startMaint('duplicates');
    try {
      await ipc.beets.duplicates(keys);
    } catch (e) {
      this.beetsMaint = { ...this.beetsMaint, running: false, error: errorMessage(e) };
    }
  }

  /**
   * 整理文件。`pretend=true` 只出清单（**默认路径**），确认后传 false 执行。
   * 库根取 `beets.libraryDirectory`：没配置就直接报错——那是唯一可信的基准。
   */
  async moveBeets(pretend: boolean): Promise<void> {
    const dir = this.beets?.libraryDirectory ?? '';
    if (pretend) this.startMaint('move');
    else this.beetsMaint = { ...this.beetsMaint, running: true, preview: false, error: null };
    try {
      await ipc.beets.move(dir, pretend);
    } catch (e) {
      this.beetsMaint = { ...this.beetsMaint, running: false, error: errorMessage(e) };
    }
  }

  /* ---------------- 启动引导 ---------------- */

  /** 首屏并发拉取：设置 / UiState。返回恢复出的 lastScrollTop。
   *
   * 歌单摘要不在这里：它在 `library.playlists`，由 `playlists.refresh()` 加载
   * 并在加载后兜底"当前歌单已不存在"（boot.ts 里紧跟着调）。
   */
  async bootstrap(): Promise<number> {
    let rawUi: DynamicJson;
    let rawSettings: DynamicJson;
    try {
      [rawUi, rawSettings] = await Promise.all([ipc.app.getUiState(), ipc.app.getSettings()]);
    } catch (e) {
      // 启动引导失败不致命：设置/视图恢复走默认值，界面仍可用
      console.warn('bootstrap failed:', errorMessage(e));
      return 0;
    }

    this.settings = mergeSettings(rawSettings);

    devValidate(uiStateSchema, rawUi, 'app.uiState');
    const restored = rawUi as UiState;
    this.restore(restored);
    return typeof restored.scrollTop === 'number' ? restored.scrollTop : 0;
  }
}

/**
 * 动态 JSON → `AppSettings`：以默认值为底合并。
 *
 * 为什么不直接当 `AppSettings` 用：老配置文件里没有 `output`（2026-10 才加），
 * 直接断言会让 `settings.output.mode` 在运行期是 undefined —— 类型说有、
 * 实际没有，是这类 bug 最难查的形态。
 */
function mergeSettings(raw: unknown): AppSettings {
  devValidate(settingsSchema, raw, 'app.settings');
  const partial = (typeof raw === 'object' && raw !== null ? raw : {}) as Partial<AppSettings>;
  return { ...DEFAULT_SETTINGS, ...partial } as AppSettings;
}

/**
 * 动态 JSON → `BeetsStatus`：逐字段 typeof 收口（与 `watchBeetsMaint` 同风格）。
 *
 * 终态载荷来自 beets 工具的 stdout，宿主只透传不解释——工具版本不齐时
 * 缺字段是常态。`found` 是判别字段：连它都不是 boolean，整包丢弃
 * （返回 null = 保留探测时的旧状态，别让坏载荷把设置页打成"未检测"）。
 */
function readBeetsStatus(raw: unknown): BeetsStatus | null {
  if (!raw || typeof raw !== 'object') return null;
  const o = raw as Record<string, unknown>;
  if (typeof o.found !== 'boolean') return null;
  const str = (v: unknown): string | null => (typeof v === 'string' ? v : null);
  const bool = (v: unknown): boolean | null => (typeof v === 'boolean' ? v : null);
  const num = (v: unknown): number | null => (typeof v === 'number' ? v : null);
  const strArr = (v: unknown): string[] | null =>
    Array.isArray(v) ? v.filter((x): x is string => typeof x === 'string') : null;
  return {
    found: o.found,
    interpreter: str(o.interpreter),
    scriptPath: str(o.scriptPath),
    entrySource: str(o.entrySource),
    protocolVersion: num(o.protocolVersion),
    beetsVersion: str(o.beetsVersion),
    pythonVersion: str(o.pythonVersion),
    pythonSupported: bool(o.pythonSupported),
    libraryPath: str(o.libraryPath),
    libraryExists: bool(o.libraryExists),
    libraryDirectory: str(o.libraryDirectory),
    plugins: strArr(o.plugins),
    pluginHealth: Array.isArray(o.pluginHealth) ? (o.pluginHealth as BeetsStatus['pluginHealth']) : null,
    pluginSummary: (o.pluginSummary ?? null) as BeetsStatus['pluginSummary'],
    configDir: str(o.configDir),
    dataHome: str(o.dataHome),
    errorCode: str(o.errorCode),
    errorMessage: str(o.errorMessage),
  };
}

export const ui = new UiStore();
