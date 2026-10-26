# docs/28 — 导航拆页与列表图标化（2026-10-26）

用户按第 6 批次驱动的 UI 迭代：专辑/艺人拆独立路由页、最近播放/常听单页退役、
曲库页 tab 撤除、列表工具条图标化、常听 top-15 算法收进推荐页。

## 1. 需求清单（用户原话归纳）

1. 专辑/艺人两个页面拆出来，各自独立路由，支持参数直达内页；列表自跳转失效
   （专辑页禁专辑列链接、艺人页禁歌手列链接）。
2. 菜单页面太多 → 取消「最近播放」「常听」两个单页。
3. 常听只放推荐页，最多 15 首；满额后新增一个自动退出一个
   （猜你喜欢留待 beets 标签能力，本批不做）。
4. 最近播放单页退役（筛选器已覆盖其场景）。
5. 曲库页第一层 tab 与分隔线撤除。
6. 工具条图标化：筛选器=funnel 系列纯图标；批量=纯图标；加入队列加
   list-plus；加入歌单加 heart-plus。

## 2. 为什么这样改

### 2.1 详情页拆分 = 抽共享接线，不是复制两个页面

`<TrackList>` 在 docs/27 后带 25 个 prop + 6 个 handler。拆详情页若原样复制，
是 3×250 行接线抄写，漏一个 prop 的症状是"某页批量按钮点了没反应"。
所以先抽 **`components/TrackListPanel.svelte`**（唯一一份接线 + 失败/启动
兜底空态），LibraryView / AlbumView / ArtistView 只声明差异：
`canSort` / `removeLabel` / `showFilter` / `onmove` / `disableArtistLink` /
`disableAlbumLink` / `empty` snippet。

### 2.2 自跳转失效的依据

在专辑详情页点"专辑"列链接，目标是当前页——`nav.go` 判等后不压栈但仍执行
（刷新语义），表现是白丢一次滚动位置。列链接与右键「查看专辑/艺人」同一条
规则：`TrackList` 新增 `disableArtistLink/disableAlbumLink`（渲染成纯文本），
`trackMenuItems` 新增 `self` 参数把对应菜单项禁掉。**对侧不失效**：专辑页
歌手列、艺人页专辑列照常可点——那才是真正能走的路。

### 2.3 单页退役、席位换人

- View（路由层）与 TrackSource（查询层）从此不再一一对应：`recent`/
  `mostPlayed` 两个**查询来源保留**（推荐页两段预览还走它们），只是不再是
  一级路由。`lib/view.ts` 的 View 类型、`AppPage`、侧栏、nav 全部收窄。
- 腾出的两个侧栏席位给**专辑/艺人宫格页**（曲库 tab 撤掉后宫格需要一级
  入口），导航总数不变（推荐/曲库/专辑/艺人/FM/设置）。宫格页与其详情页
  共用一个侧栏席位（高亮跟内容类别走，详情页不再零高亮）。
- **清空播放历史**的唯一入口随 recent 单页消失，改放推荐页「最近播放」段
  标题行（清空后显式重拉三段——stats.tracks 不变，原 effect 不会触发）。
- 旧 UiState 兜底：`page='recent'/'mostPlayed'` 或旧 `view` 键落到这两个值时
  `restore()` 换算成曲库。schema 枚举不收编退役值——devValidate 在开发期把
  "还在写退役值"的代码暴露出来。

### 2.4 常听 top-15 的"算法"

"满额新增一个退出一个"就是 **ORDER BY played_count DESC LIMIT 15** 的天然
语义，不需要额外记账。本批顺带把次键从 `t.id` 换成 **`recent_t0 DESC`**
（次数并列时"最近还在听"的留下）——推荐页第 15 名的争夺战里，半年没听的
那首先出去。mock 逐字对齐（用户惯例），补了并列次序的 Rust 单测。

### 2.5 tab 与分隔线

曲库页 tab 撤掉后 Toolbar 的 `border-bottom` 改为 `:has(.pt-tabs)` 才画：
tab 行是页面级分区值得一条线，chips 残行不值得——曲库页只剩时间筛选 chip
时不再拖一条孤立的分隔线。BrowseView 的 tab 条一并撤掉（侧栏就是这层导航，
页内再放一组互切是同一个功能的第二次出现）。

## 3. 变更清单

- 新增：`views/AlbumView.svelte`、`views/ArtistView.svelte`、
  `components/TrackListPanel.svelte`
- 路由收口：`stores/ui.svelte.ts`（AppPage 13→11 值）、`lib/view.ts`
  （View 4→2 值、LIB_TABS 删）、`stores/nav.svelte.ts`、
  `stores/list.svelte.ts`（openBuiltinView→openLibrary）、
  `stores/history.svelte.ts`、`lib/ipc/schema.ts`
- 链接失效：`components/TrackList.svelte`、`lib/menu.ts`（self 参数）、
  `components/OverlayHost.svelte`
- 壳与页：`shell/Sidebar.svelte`（专辑/艺人入席）、`views/HomeView.svelte`
  （常听 15 首 + 清历史入口）、`views/BrowseView.svelte`（撤 tab）、
  `views/LibraryView.svelte`（收窄 + 撤 tab）、`content/Toolbar.svelte`
  （分隔线跟 tab 走）
- 工具条：`components/ListToolbar.svelte`（Funnel/FunnelPlus/ListChecks/
  ListPlus/HeartPlus，筛选与批量改 IconButton）
- Rust：`crates/aiya-data/src/repo.rs`（mostPlayed 次键 → recent_t0）、
  mock `backend.ts` 对齐

## 4. 验证证据（2026-10-26 实测）

- `cargo test -p aiya-data`：**97 passed, 0 failed**（含新增的次数并列次序断言；
  首跑失败一次——q_library 缺省 limit=2 装不下 3 首，`q.limit = 10` 后过）
- `svelte-check --threshold error`：**0 errors, 0 warnings**
- `npm run build`：**✓ built in 5.10s**，JS 432.70 kB（gzip 129.06 kB，
  较上批 424 kB 增量来自 5 个新 lucide 图标与两个详情页视图）
- `npx playwright test`：**21 passed (23.0s)**——route.spec 重写 8 条
  （旧 UiState recent/mostPlayed → 曲库、专辑宫格往返、详情页 link-cell=1），
  batch.spec 批量开关改 `aria-pressed` 断言

## 5. 遗留与后期

- 真机走查（docs/23 §7 清单外新增）：专辑/艺人详情页参数直达、自跳转失效、
  推荐页常听 15 首、图标工具条 hover。
- 猜你喜欢：待 beets 标签能力落地后，基于 top-15 做关联推荐（用户明说本批不做）。
- workspace `cargo check`：上批因运行中的应用锁 sidecar exe 未跑完，本批未涉
  音频 crates，风险不变。

## 6. 修订（同日，真机走查反馈）

**问题 1：列表行内跳详情页，标题显示 uuid。**
根因：`AlbumView`/`ArtistView` 的标题回落链末位是 `ui.albumId`/`ui.artistId`
（路由参数本身），聚合缓存（`facets`）未命中时它直接渲染出来——真机 id 是
uuid v5 字符串（`ids::artist_id(name)`），mock 是 `ar-01` 这种短 id，所以只有
真机能看到。聚合缓存未命中的时机：页面刚挂载还在拉、或 `library_albums` 失败。

修法：回落链改为 **聚合缓存 → 列表首行的真名 → 通用文案**。列表首行
（`list.trackWindow.get(0)`，靠 `list.version` 计数驱动重算）与详情页下方
的曲目列表同一次查询同源，聚合缓存加载慢或失败都给得出真名；**任何路径都不
再把路由 id 渲染给用户**。副标题同链回落（`al?.artist ?? row?.artist`）。

**问题 2：专辑封面改为取首曲封面。**
回落链 **聚合缓存 artworkId → 首曲 artworkId**（扫描器给每条曲目都写了
`artwork_id`，专辑首曲封面即专辑封面）。

**问题 3：艺人不做图片获取逻辑。**
`artists.artwork_id` 扫描器从不填写，永远走占位没有意义（用户拍板）：
- 详情页头部：`Header` 新增 `icon` snippet 位（92px 圆形徽标，与 `Cover.lg`
  同尺寸同投影），艺人页放 `User` 图标；
- 艺人宫格卡片：不再传 `cover`，走 `Card` 既有 `icon` 位（48px 方块 `User`）。

验证：svelte-check **0/0**；build **✓ 4.90s**（JS 433.41 kB）；e2e
**22 passed**——新增 route 用例「曲库列表行内直达详情页，标题显示名称而非
路由 id」（歌手/专辑两条链接 + 艺人页 `.ch-icon`、专辑页 `.k-cover` 断言）。
