import type { BeetsStatus, Diagnostics } from '../lib/ipc';
import type { BeetsPluginHealth, BeetsPluginSummary } from '../lib/ipc';
import type { TrackSort } from '../lib/query';
import type { AppSettings } from '../lib/settings';
import { DEFAULT_SETTINGS } from '../lib/settings';
import { ipc, events, type BeetsPluginItem, type DynamicJson } from '../lib/ipc';
import { errorMessage } from '../lib/errors';
import { devValidate, uiStateSchema, type UiState } from '../lib/ipc/schema';
// 合并/迁移逻辑是纯函数（可单独验证），store 只负责在引导与写回时调它
import { mergeSettings } from '../lib/settings-migrate';
import {
  LIBRARY_VIEW,
  parseViewKey,
  type View,
} from '../lib/view';
import { readLook } from '../lib/look';
import { t } from '../lib/i18n.svelte';
import { notify } from './notify.svelte';

/**
 * 一级路由：**12 个值**，每个值对应 `App.svelte` 的渲染分支（
 * `albums` / `artists` 共用一个宫格分支）。
 *
 * 列表页为 `library` / `playlist` / `album` / `artist` / `search`，共用
 * `ListStore` 的查询与虚拟列表（见 `isListPage`）；`albums` / `artists` 是宫格
 * 浏览页，`album` / `artist` 是详情页，`nowPlaying` / `settings` / `beets` /
 * `live` 各自独立成页。
 *
 * `playlist` 的参数是 `playlistId`；`album` / `artist` 的参数复用
 * `albumId` / `artistId` 两个筛选字段，不设独立 `facetId`：详情页的曲目列表
 * 即"曲库查询 + albumId 过滤"，`list.query()` 从这两个字段取筛选，因此查询、
 * 返回栈快照、UiState 持久化三处无需按路由分支。代价是页面参数与曲库筛选 chip
 * 共享同一存储位；两者不会同时可见（不同路由值），语义不冲突。
 *
 * 已退役的路由名由 `restore()` 换算，来源见该函数的 `RETIRED_PAGES`：
 * `recent` / `mostPlayed`、`tools`、`radio`
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
  | 'beets'
  | 'live';

/**
 * 设置页的分组 id。
 *
 * 定义在 store 而不是 `SettingsPanel` 里：`nav.settingsAt()` 要能指定"滚到
 * 哪一组"（曲库告警 → 曲库组），而导航层不该 import 一个组件。`SettingsPanel`
 * 直接拿这个类型当自己的 `Section`。
 */
export type SettingsSection =
  | 'appearance'
  | 'playback'
  | 'library'
  | 'system'
  | 'shortcuts'
  | 'about';

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
 * 职责范围限于"界面在哪、长什么样"。歌单归 `library.playlists` + `playlists`
 * （编排），歌词归 `stores/lyrics`，列表刷新归 `list`。
 *
 * 本 store 只持有状态，不执行刷新：reload 与 TrackWindow 在 App，状态变更后由
 * App 触发一次 reload。
 *
 * 校验边界：`app_get_settings` / `app_get_ui_state` 返回无 DTO 的
 * 动态 JSON（旧配置缺字段是常态），经 zod 处理——开发期 `devValidate` 报形状不符，
 * 生产期只合并默认值。其余命令的返回值由 Rust 生成类型，不二次校验。
 */
class UiStore {
  page = $state<AppPage>('home');
  /** `page === 'playlist'` 时的参数。其余路由下无意义（保留旧值，返回时才清） */
  playlistId = $state<string | null>(null);
  queueOpen = $state(false);
  /** EQ 抽屉（播放条 Popover）会话态开合，与队列抽屉互斥（docs/25 §1） */
  eqOpen = $state(false);
  /**
   * 列表批量操作模式：开 = 显示勾选列，关 = 隐藏。
   * 会话态不持久化；Ctrl+点击行会自动开启（`lib/row-selection.ts`），
   * 关闭时由调用方清空选择集。
   */
  batchMode = $state(false);

  /**
   * 一次性跳转请求：设置页的哪个分组该滚入视野（`nav.settingsAt()` 写入，
   * `SettingsPanel` 消费后立即清空）。
   *
   * 设置页是**单页平铺 + tab 跳转**，分组不是路由：曲库页的「位置缺失」告警
   * 需要把视口送到「曲库」组，只切路由会落在「外观」组顶部。
   *
   * 用一次性请求而不是"当前分组"状态：分组高亮只跟点击走（见 `SettingsPanel`
   * 的说明），改成持久状态会让滚动与高亮相互干扰。
   */
  settingsSection = $state<SettingsSection | null>(null);

  /**
   * 当前列表视图（`list.query()` 用它算 `source`）。
   *
   * **是 getter，不是独立状态**：`page`（+ `playlistId`）是唯一真相，`view` 由它
   * 派生，改路由只能走 `enterList` / `setView` / `openPage`。独立字段可与 `page`
   * 拼出自相矛盾的组合（如 `page=home` 而 `view=playlist`），而读取方只检查其中
   * 一部分。
   *
   * 返回值**引用稳定**：playlist 对象由 `setRoute` 在写入时缓存（见下），
   * getter 只读不写——getter 里做记忆化会让每次读取都可能改私有字段，
   * `$derived` 求值路径上出现写副作用是隐患。
   */
  get view(): View {
    switch (this.page) {
      case 'library':
      case 'album':
      case 'artist':
      case 'search':
        // 专辑/艺人详情页的曲目列表 = 曲库查询 + albumId/artistId 筛选，搜索页 = 曲库查询 + search 过滤 ——
        // 数据形状与曲库页完全相同 （见类头注释"参数复用筛选字段"）
        return LIBRARY_VIEW;
      case 'playlist':
        // page/playlistId 只经 setRoute 写入，缓存恒同步；?? 右侧仅是
        // 防御性兜底（正常到不了），容忍它产生一个一次性新对象
        return this.playlistView ?? { kind: 'playlist', id: this.playlistId ?? '' };
      default:
        // 非列表页：查询没有意义，给一个稳定的兜底（不必是当前路由）
        return LIBRARY_VIEW;
    }
  }

  /** `playlist` 视图的缓存对象，保证 getter 引用稳定；唯一写入点是 `setRoute` */
  private playlistView: { kind: 'playlist'; id: string } | null = null;

  /**
   * 路由的**唯一写入点**：page、playlistId 与 playlist 视图缓存三者在同一处
   * 变更。缓存若靠读取方（getter）惰性补写，读取就成了写副作用；改由写入方
   * 维护后，三者天然一致，getter 保持纯读。
   */
  private setRoute(page: AppPage, playlistId: string | null): void {
    this.page = page;
    this.playlistId = playlistId;
    this.playlistView = playlistId === null ? null : { kind: 'playlist', id: playlistId };
  }
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
   * 曲库页的 tab 索引：全部曲目 / 最近播放 / 最近添加。
   *
   * 会话态，不持久化：重启回到「全部曲目」。仅在 `page === 'library'` 时参与
   * `list.query()` 的 source 换算；搜索页与详情页同走 `LIBRARY_VIEW`，不使用 tab。
   */
  libraryTab = $state<'all' | 'recent' | 'added'>('all');

  /**
   * Beets 页的子页：安装 / 分析 / 整理。未安装时后两个 tab 置灰
   *
   * 跨导航保留：整理页是长流程（选目录 → 联网匹配 → 逐张专辑挑候选），中途转到
   * 曲库再返回时保持原 tab。
   *
   * 默认 `index`：已安装占多数。未安装时由壳在探测完成后翻到 `install`，见
   * `beetsTabTouched`。
   */
  beetsTab = $state<'install' | 'index' | 'organize'>('index');
  /**
   * 用户是否手动切过整理库页的 tab。只有"探测到未安装则翻到安装页"这条规则读它：
   * 手动选择之后自动翻页让位。
   */
  beetsTabTouched = false;

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

  /* 视图状态持久化 */

  /**
   * 防抖写 UiState。
   *
   * 定时器与 `scrollTop` 都归这里：`scrollTop` 是 UiState 的字段，而列表 store
   * 依赖 ui（不能反向 import），放在本 store 才不需要逐层传 getter。列表滚动时
   * 调 `ui.scrollTop = n` 再 `scheduleUiSave()`。
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
      // 视图状态丢失不影响可用性，失败只记日志
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
   * 还原 UiState，兼容拆页之前的旧形状。
   *
   * 旧形状：`page: 'home'|'nowPlaying'|'radio'|'settings'` + `libraryDetail` +
   * `view: 'library'|'recent'|'mostPlayed'|'playlist:xxx'`。旧文件中 `page` 为
   * `home` 时也可能表示六个列表视图之一，因此先认新字段，缺失时按旧字段换算；
   * 只看 `page` 会把列表视图还原成首页。
   */
  restore(data: UiState): void {
    if (data.sort) this.sort = data.sort;
    if (data.direction) this.direction = data.direction;
    this.artistId = data.artistId ?? null;
    this.albumId = data.albumId ?? null;

    // 退役路由换算表。未写入此表会导致强制转型把不存在的路由写进 `ui.page`：
    // 侧栏零高亮、整页落到 `App.svelte` 的 `{:else}` 分支，该脏值还会写回磁盘。
    // recent / mostPlayed：单页退役，语义并入曲库 tab
    // tools：扫描入库页并入设置页「曲库」组
    // radio：改名 live，页面语义整体更换
    const RETIRED_PAGES: ReadonlyMap<string, AppPage> = new Map([
      ['recent', 'library'],
      ['mostPlayed', 'library'],
      ['tools', 'settings'],
      ['radio', 'live'],
    ]);
    const rawPage = data.page ? (RETIRED_PAGES.get(data.page) ?? data.page) : data.page;

    if (rawPage && rawPage !== ('home' as string)) {
      // 新形状（或旧形状的一级页）
      this.setRoute(rawPage as AppPage, data.playlistId ?? null);
    } else if (data.libraryDetail) {
      // 旧形状：home + libraryDetail=true 表示某个列表视图
      //（parseViewKey 对 retired 的 'recent'/'mostPlayed' 键同样回落曲库）
      const legacy = parseViewKey(data.view);
      this.applyView(legacy);
    } else {
      this.setRoute('home', null);
    }

    // 启动即落在设置页时不经过 openPage，诊断块会保持空值（显示"正在读取…"
    // 直到手动刷新）。这里补一次读取。
    if (this.page === 'settings') void this.refreshDiagnostics();
  }

  /* 一级页面与队列抽屉 */

  openPage(page: AppPage): void {
    // 与旧实现同语义：openPage('playlist') 保留现有 playlistId（playlist
    // 的进入走 openPlaylist/enterList，这里只兜住直呼的情况）
    this.setRoute(page, page === 'playlist' ? this.playlistId : null);
    this.queueOpen = false;
    this.eqOpen = false;
    this.scheduleUiSave();
    // 进设置页要重拉诊断与 beets 状态：设置页的「关于」与「整理库」提示都读它。
    // 先置空再拉取，避免上一份陈旧诊断以"已加载"呈现。
    if (page === 'settings') {
      this.diagnostics = null;
      void this.refreshDiagnostics();
    }
  }

  /**
   * 进入列表视图上下文：切到目标视图、关闭队列与 EQ 抽屉、清空搜索与筛选。
   *
   * 四个调用点（`openHome` / `openBuiltinView` / `openPlaylist` / `setSearch`）共用
   * 这段赋值，避免逐处复制时遗漏——遗漏项表现为切换视图后筛选残留、列表数据不全。
   *
   * 不负责落盘与重载：由调用方决定（搜索每次输入都落盘过于频繁）。
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
   * 回首页仪表盘。清理语义与 `enterList` 一致：首页不是列表页，但同属用户主动
   * 导航，旧搜索词不保留。
   */
  enterHome(): void {
    this.setRoute('home', null);
    this.search = '';
    this.artistId = null;
    this.albumId = null;
    this.queueOpen = false;
    this.eqOpen = false;
  }

  /**
   * 进入搜索结果页。
   *
   * 与 `enterList` 的差别：不清 `search`（本页以搜索为主题），目标页为 `search`。
   * 旧筛选与歌单参数不随搜索词带入：在歌单内搜索得到全库结果，歌单上下文至此结束。
   */
  enterSearch(): void {
    this.setRoute('search', null);
    this.artistId = null;
    this.albumId = null;
    this.queueOpen = false;
    this.eqOpen = false;
  }

  /**
   * 进入专辑 / 艺人详情页。
   *
   * 参数复用 `albumId` / `artistId` 筛选字段（见 `AppPage`）。不可先调 `enterList`
   * 再覆盖：`enterList` 清空两个筛选字段，顺序错误会使详情页显示全库。清理语义与
   * `enterList` 一致，仅目标值不同。
   *
   * 不负责落盘与重载。
   */
  enterFacet(kind: 'album' | 'artist', id: string): void {
    this.search = '';
    this.artistId = kind === 'artist' ? id : null;
    this.albumId = kind === 'album' ? id : null;
    this.setRoute(kind, null);
    this.queueOpen = false;
    this.eqOpen = false;
  }

  /**
   * 只换视图，不清搜索 / 筛选 / 队列抽屉。
   *
   * `enterList` 清三样的前提是"用户主动导航到某处"。两个调用方不满足该前提：
   * - `playlists.refresh()` 发现当前歌单已删除，需退回曲库，此时搜索词应保留；
   * - `playlists.play()` 先切视图再按整列表起播，清筛选会把"按当前筛选播放"变成
   *   播放全部。
   *
   * `view` 无 setter，两类语义由两个方法区分。
   */
  setView(view: View): void {
    this.applyView(view);
  }

  private applyView(view: View): void {
    this.setRoute(view.kind, view.kind === 'playlist' ? view.id : null);
  }

  toggleQueue(): void {
    this.setQueue(!this.queueOpen);
  }

  /** 队列抽屉开合的**唯一入口**（`PlayerBar` 的 Popover 直接受控于它） */
  setQueue(open: boolean): void {
    this.queueOpen = open;
    // EQ 抽屉与队列同侧同层，互斥：一个开另一个关
    if (open) this.eqOpen = false;
  }

  closeQueue(): void {
    this.queueOpen = false;
  }

  /** EQ 抽屉开合的唯一入口（docs/25 §1）；开时关队列，规则对等 */
  setEqDrawer(open: boolean): void {
    this.eqOpen = open;
    if (open) this.queueOpen = false;
  }

  /* 设置 */

  /**
   * 必须 snapshot：settings 是 `$state`，其中的 `shortcuts` / `output` 是 Proxy。
   * Proxy 无法结构化克隆，直接 invoke 会抛 "An object could not be cloned."；
   * 此时主题/密度已在本地生效，表现为设置项看似生效但未写盘。
   *
   * 先用新值本地生效（主题切换需要即时响应），再以后端返回的为准：后端会补齐
   * 默认值、拒绝非法值，这一步是"以磁盘为准"的对账。
   *
   * 请求代次：快速连改两个设置会并发两条 setSettings，先发的可能后返回，把
   * "以磁盘为准"的合并结果盖回旧值。只有最新一次发出的响应允许落盘；过期响应
   * 的失败同样静默（新请求成功时旧失败无意义）。
   *
   * 失败**落到全局错误条**：写盘失败若被静默，外观是"设置看着生效、重启就回滚"。
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

  /** 出站链接（关于段）：URL 走宿主白名单，此处不校验 */
  openUrl(url: string): void {
    void ipc.app.openUrl(url).catch((e) => console.warn('openUrl failed:', errorMessage(e)));
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

  /**
   * beets 环境探测（低频：设置页打开/手动刷新时；最坏 ~30s，必须容错）。
   *
   * 请求代次：探测期间再次打开设置页/手动刷新会并发第二条，慢的旧响应
   * 晚到会把状态区盖回上一次的结果（"已检测"变回"未检测"）。只有最新
   * 一次的响应允许落到 store 上（与 applySettings 同一模式）。
   */
  private beetsGeneration = 0;

  async refreshBeets(): Promise<void> {
    const generation = (this.beetsGeneration += 1);
    this.beets = null;
    try {
      const status = await ipc.beets.status();
      if (generation !== this.beetsGeneration) return;
      this.beets = status;
    } catch (e) {
      // 失败保持 null（显示"未检测"），不打断设置页；日志留痕
      console.warn('beets.status failed:', errorMessage(e));
    }
  }

  /**
   * beets 运行时安装状态。
   *
   * 放在 ui store 而不是组件里：进度来自事件流，而"组件不直接 listen"是
   * 的硬约定（多个组件同时开时事件会被订阅多次）。
   */
  beetsInstall = $state<{ running: boolean; stage: string; message: string; error: string | null }>({
    running: false,
    stage: '',
    message: '',
    error: null,
  });

  /**
   * 内嵌插件表：`[]` = 还没加载过。
   *
   * 内嵌表在二进制里，"还没装"的时候也能给用户看清单，离线也可用；
   * 装没装、缺不缺依赖的三态由 `beets_status` 的 pluginHealth 另行给出。
   */
  beetsPlugins = $state<BeetsPluginItem[]>([]);
  /**
   * 卸载进度。与安装分开一个状态：两者终态事件不同
   * （install 以 `finished` 收尾，uninstall 以 `done` 收尾），共用一个状态会在
   * 「安装中」与「卸载中」之间互相覆盖。
   */
  beetsUninstall = $state<{ running: boolean; message: string; error: string | null }>({
    running: false,
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
        this.beetsInstall = { running: false, stage: 'done', message: '', error: null };
        this.beetsUninstall = { ...this.beetsUninstall, running: false };
        const status = readBeetsStatus(e.status);
        if (status) this.beets = status;
        return;
      }
      if (stage === 'uninstall') {
        // 卸载与安装共用同一条事件流，只靠 stage 区分。
        // `done` 是卸载的终态：带 removed / files / bytes / message。
        if (type === 'error') {
          this.beetsUninstall = { running: false, message: '', error: message || t('beets.uninstall.failed') };
          return;
        }
        if (type === 'done') {
          this.beetsUninstall = { running: false, message, error: null };
          return;
        }
        this.beetsUninstall = { running: true, message, error: null };
        return;
      }
      if (type === 'error') {
        this.beetsInstall = {
          running: false,
          stage,
          message,
          error: message || t('beets.install.failed'),
        };
        return;
      }
      this.beetsInstall = { running: true, stage, message, error: null };
    });
  }

  /**
   * 安装/更新便携包（单轨）。
   *
   * `plugins` 是预置之外的启用清单：装完写进 settings，工具侧渲染 config 照它
   * 启用。失败原因原样透传到按钮旁的文字（本机环境差异无法预判）。
   */
  async installBeets(plugins: string[] = []): Promise<void> {
    this.beetsInstall = {
      running: true,
      stage: 'download',
      message: t('beets.install.preparing'),
      error: null,
    };
    try {
      await ipc.beets.install(plugins);
    } catch (e) {
      this.beetsInstall = { running: false, stage: '', message: '', error: errorMessage(e) };
    }
  }

  /**
   * 换插件（操作桥）：spawn 包内解释器跑 `install --plugins <新> --previous <旧>`。
   * pip 装/卸、外部工具补齐、配置同步全在脚本侧；进度与安装同通道。
   */
  async setBeetsPlugins(plugins: string[] = []): Promise<void> {
    this.beetsInstall = {
      running: true,
      stage: 'pip',
      message: t('beets.install.preparing'),
      error: null,
    };
    try {
      await ipc.beets.pluginsSet(plugins);
    } catch (e) {
      this.beetsInstall = { running: false, stage: '', message: '', error: errorMessage(e) };
    }
  }

  /**
   * 加载内嵌插件表（幂等：已经拿到就不重复取）。
   *
   * 只在安装页进入时调；失败不弹错——最坏情况是勾选清单为空，基础组件仍可安装。
   */
  async loadBeetsPlugins(): Promise<void> {
    if (this.beetsPlugins.length > 0) return;
    try {
      this.beetsPlugins = await ipc.beets.pluginTable();
    } catch (e) {
      console.warn('beets.pluginTable failed:', errorMessage(e));
    }
  }

  /**
   * 卸载组件：只删安装目录，整理库与分析库不动。
   *
   * 进度走同一条 `beets:install` 事件流（`stage: uninstall`），终态后宿主会
   * 补一条 `finished` 带新 status，所以这里不需要再手动刷一次。
   */
  async uninstallBeets(): Promise<void> {
    this.beetsUninstall = { running: true, message: t('beets.uninstallRunning'), error: null };
    try {
      await ipc.beets.uninstall();
    } catch (e) {
      this.beetsUninstall = { running: false, message: '', error: errorMessage(e) };
    }
  }

  /* 启动引导 */

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
 * 动态 JSON → `BeetsStatus`：逐字段 typeof 收口（与 `watchBeetsInstall` 同一方式）。
 *
 * 终态载荷来自 beets 工具的 stdout，宿主只透传不解释——工具版本不齐时缺字段是
 * 常态。`found` 是判别字段：它不是 boolean 时整包丢弃（返回 null = 保留探测期的
 * 旧状态，避免坏载荷使设置页显示为未检测）。
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
    libraryPath: str(o.libraryPath),
    libraryExists: bool(o.libraryExists),
    libraryDirectory: str(o.libraryDirectory),
    plugins: strArr(o.plugins),
    pluginHealth: readPluginHealth(o.pluginHealth),
    pluginSummary: readPluginSummary(o.pluginSummary),
    configDir: str(o.configDir),
    dataHome: str(o.dataHome),
    // v2：schemaMatched 是宿主算好的 boolean（缺字段 = 老工具，按不匹配处理）；
    // 库锁三态与持有者原样透传
    schemaMatched: o.schemaMatched === true,
    libraryLock: str(o.libraryLock),
    libraryLockHolder: str(o.libraryLockHolder),
    errorCode: str(o.errorCode),
    errorMessage: str(o.errorMessage),
  };
}

/**
 * 动态 JSON → `BeetsPluginHealth[]`：逐条逐字段 typeof 收口。
 *
 * 列表里混进一条坏项就整条跳过，而不是让 `name: undefined` 混进卡片——
 * 渲染层拿到的是"每条都完整"的数组，不必再防缺字段。
 */
function readPluginHealth(raw: unknown): BeetsPluginHealth[] | null {
  if (!Array.isArray(raw)) return null;
  return raw.flatMap((item): BeetsPluginHealth[] => {
    if (!item || typeof item !== 'object') return [];
    const h = item as Record<string, unknown>;
    if (
      typeof h.name !== 'string' ||
      typeof h.state !== 'string' ||
      typeof h.enabled !== 'boolean'
    ) {
      return [];
    }
    return [
      {
        name: h.name,
        state: h.state,
        enabled: h.enabled,
        missing: Array.isArray(h.missing)
          ? h.missing.filter((m): m is string => typeof m === 'string')
          : [],
        note: typeof h.note === 'string' ? h.note : '',
      },
    ];
  });
}

/** 动态 JSON → `BeetsPluginSummary`：三个计数字段齐才收，缺一按没有处理 */
function readPluginSummary(raw: unknown): BeetsPluginSummary | null {
  if (!raw || typeof raw !== 'object') return null;
  const s = raw as Record<string, unknown>;
  if (
    typeof s.ready !== 'number' ||
    typeof s.missingDeps !== 'number' ||
    typeof s.unavailable !== 'number'
  ) {
    return null;
  }
  return { ready: s.ready, missingDeps: s.missingDeps, unavailable: s.unavailable };
}

export const ui = new UiStore();
