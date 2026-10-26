# 24 — 前端收尾批：代码整理 · 功能全量 · 页面分配 · UI Kit · 分批计划

> 2026-10-26。本文是**动工前**的收口计划。前几批（docs/18 的四批、docs/23 的架构收口）是"把
> Electron 搬过来 + 补功能"，结果正确但**结构边建边长**：样式分层名不副实、两套图标、三份列表、
> 上帝 store。这一批的目标不是加功能，而是**让后面每加一个功能都只改一处**。
>
> 与既有文档的关系：
> - **docs/18**（前端重写计划）：四批已全部完成，本文续接。
> - **docs/19**（设计规范）：§2.2 三栏、§2.3 三档断点、§6.4 Tailwind 全量落地 —— 这三条**已作废**
>   （三栏在 docs/23 取消；Tailwind 实测零使用，本文 §1 P2 建议删除）。其余（色彩角色、
>   可访问性、反馈原则）仍然有效，本文 §4 直接继承。
> - **docs/23**（信息架构与表格多选）：三栏取消、首页表格化、bits-ui Checkbox 的结论本文不重开。
>
> 一句话结论：**先把"清场 + token + kit"做完，再动页面；播放区（含 EQ）独立成批，因为它要后端。**

### 修订 v4（2026-10-26 夜，B3b 拆三刀 + 两处记账更正）

1. **B3b 拆成三刀**：B3b-1 浮层四件（✅ 完成）/ B3b-2 壳体换皮（✅ 完成）/ B3b-3 Content 与设置页（待做）。
   理由是 B3b-1 会改**事件与焦点**，和"只换视觉"的壳体换皮混在一起时，出问题无法归因。
2. **§4.5 坑 4 重写**：原判断"`ContextMenu` 的锚点会随行回收丢失"**是错的** ——
   它用的是指针虚拟元素。真正原因是**坐标只能由它自己的 Trigger 捕获**，
   所以改用 `DropdownMenu` + 0×0 固定锚点。同时补坑 11–13（`data-highlighted`、
   `snippet` + `{...props}`、Dialog 的 Description）。
3. **B2 的体积记账更正**：kit 在 B2 之后**没有进生产包**（只被 dev-only 的 `KitView` 引用，被摇掉）。
   所以 B1 那 +4.4 kB 全在 token 轴与 `layout.css` 上；kit 是 B3b-1 才第一次进包（CSS +7.4 kB、JS +155 kB）。
4. **侧栏「专辑/艺人」入口推迟到 B4**：它要的是 group-by 查询，不是换皮。

### 修订 v3（2026-10-26 深夜，Content 层归位 + 组件引用规划）

1. **`PageHeader` / `PageToolbar` 改名**：→ `Header` / `Toolbar`，移到 `components/content/`。
   二者是 **Content 层的版式件**，不属于"页面布局层"（页面布局层只有 AppShell 的四区席位），
   所以**不带 `Page` 前缀** —— 前缀说的是"它属于页"，而实际归属是"它属于内容区"。
   文件 `components/content/{Header,Toolbar}.svelte`，类名同步 `.content-header` / `.content-toolbar`。
2. **新增 §4.5 框架组件引用规划**：22 件逐个定来源（bits-ui 原语 / 自研 / 包装），
   并列出用原语时必须绕开的 8 个坑。B2 的施工图。
3. **`components/` 目录开始分层**：`shell/`（壳体，B3b 迁）、`content/`（版式件）、`ui/`（原子件 kit）、
   其余浮层/面板暂留原地到 B3b/B6 —— **本批只动 Content 两个文件，不搞一次性大搬迁**
   （移动 + 改 import 的噪音会淹没真正的改动）。

### 修订 v2（2026-10-26 晚，壳体三条）

用户在评审本文 §3.1 后给了三条更明确的壳体要求，本文据此修订：

1. **宽 TitleBar**：应用名、返回上一页、搜索框、窗口控制**四件都在顶栏一行**，顶栏空白区整条可拖拽。
   —— 取代 §3.1 原方案（品牌 + 面包屑 + 窗口控制，搜索另起 TopBar）。
2. **不设 TopBar 这一级**：页内标题与页内 tab 工具条拆成**两个独立零件**（`PageHeader` / `PageToolbar`），
   **不在一行、不组合**，由页面自己按顺序调用。
   —— 取代 §3.1 原方案（TopBar 一行装"标题 + 搜索 + 视图级工具"）。
3. **Sidebar 与顶栏归到同一套导航**：侧栏的路由动作与顶栏的返回按钮必须读同一份历史，
   由 `stores/nav.svelte.ts` 单点收口。

连带调整：§3.1 重写、§3.2 加布局契约、§4.3 零件 20 → 22 件、**§5 新增 B3a 并提前到 B0 之前**、
新增 **§5.5 改动方式评估（就地改 vs 隔离重做）**。

原方案为什么错：把"搜索"放 TopBar，就把一个**全局**能力挂到了**页面级**容器上 —— 页面标题会
随滚动走、会因路由消失，而搜索框必须常驻。`TopBar` 这一级本质上是"全局 + 页内"两种东西的中转站，
它一存在，就必然出现"设置页的标题锚点条要不要进 TopBar"这类问题（现在正是：`SettingsPanel`
把标题和锚点 tab 塞在同一个 `header.page-heading` 里）。拆掉这一级，分类立刻干净。

---

## 0. 体检：现状数字

| 层 | 文件 | 行数 | 说明 |
|---|---:|---:|---|
| `components/` | 21 | 4 043 | 含 shell 级（AppShell/TitleBar/Sidebar/TopBar/PlayerBar/QueuePanel）、浮层（4 个）、面板（Lyrics/Spectrum/Import/Settings） |
| `views/` | 5 | 约 600 | home / library / nowPlaying / radio / settings |
| `stores/` | 12 | 1 724 | ui 一个就 315 行 |
| `lib/` | 33 | 3 194 | 不含 `ipc/generated` 与 `ipc/mock` |
| `styles/` | 7 | 2 472 | 全部是全局 CSS，零 scoped（例外：ImportPanel、SpectrumPanel 自带 `<style>`） |
| 后端命令 | — | 51 | `generate_handler!` 实测 51 条 |
| CSS 类名 | — | 207 | 命名靠习惯，无前缀无约定 |
| CSS 变量引用 | — | 293 | 其中 2 个引用了**不存在**的变量（有 fallback 所以没炸） |

依赖（`ui/package.json`）：`@lucide/svelte` `@tauri-apps/api` `bits-ui` `zod` + `tailwindcss`/`@tailwindcss/vite`（dev）+ `@playwright/test`（dev）。
`audiomotion-analyzer` 已于 2026-10-25 随 pro 频谱自研一起移除。

> **B3a 之后的同一组数字**（S1–S5 落地，2026-10-26 实测）：`components/` 仍是 **21** 个
> （−TopBar −ViewHeader +PageHeader +PageToolbar）、`stores/` 12 → **13**（新增
> `stores/nav.svelte.ts`，约 190 行）、`styles/` 2 472 → **2 205 行**（一部分迁进两个版式件
> 的 scoped `<style>`，全局部分净减 267 行）、CSS 类名 207 → **181**
> （`.titlebar*` `.topbar` `.search*` `.view-*` `.page-heading*` `.settings-jump`
> `.chip*` `.filters` 全部消失）。上表保留动工前的基线，方便对比。
>
> **B0 之后的同一组数字**（2026-10-26 实测）：`components/` 仍 **21**、`stores/` 13 → **14**
> （新增 `stores/lyrics.svelte.ts`）、`views/` 5、`styles/` 2 205 → **2 265 行**（tokens 只剩
> 74 行变量、base 补了元素默认，见 B0 表）、CSS 类名 181 → **185**、产物 CSS
> 41.10 → **35.28 kB**（−5.82 kB / −14.2%）、JS 237.28 → **239.41 kB**（+2.13 kB，19 个 lucide
> 图标定义换掉 169 行手写 path 的代价）、`test:e2e` **6/6**。

---

## 1. 问题清单（每条都有证据，不是印象）

| # | 问题 | 证据 | 影响 |
|---|---|---|---|
| 状态 | **B3a 关掉 P1 的一半**（`.titlebar*/topbar/view-*/page-heading*` 随组件删、缩进改由组件自带 scoped）；**B0 关掉 P1 的另一半、P2、P3、P4 的歌单/歌词部分、P7、P8、P9**（见 §5 B0 表）。P5/P6/P10/P11/P12 留给 B4/B5。下表是动工前的原始审计，保留原文以便对照。 |
| **P1** | 样式分层名不副实 | `tokens.css` 里装着 `box-sizing`/`body`/`button`/滚动条 reset；`base.css` 里装着 `.app` 网格、`.sidebar`、`.nav-item`；`pages.css` 里装着 `.titlebar`、`.queue-panel`。三份文件的头注释都写着别的职责 | 换皮/换布局时不知道该改哪个文件；新同事按注释找会找错 |
| **P2** | Tailwind 的 preflight 是白付的成本 | 全仓零 utility 使用（`grep 'class=".*\b(flex\|px-4\|text-sm)'` 只命中我们自己的 `diag-grid`/`grid-head`/`playlist-grid`）。产物 CSS 39 752 B，其中 **5 498 B（13.8%）是 tailwind v4 的 preflight + `@theme` + `@layer properties`**，一行都不生效 | 每屏白付 5.5 KB；更麻烦的是它和 `tokens.css` 各自 reset 一遍按钮/输入（双份 reset 是"改了一条不生效"的经典来源）。**决议：依赖保留，入口裁成只引 `theme` + `utilities`，不再引 preflight**（见 §4.1） |
| **P3** | 两套图标系统 | `Icon.svelte` 手写 29 条 SVG path（169 行），同时 9 个组件直接 `import { X } from '@lucide/svelte'`；strokeWidth 1.7 与 1.8 混用；ImportPanel 用文字 `✕` | 同一屏里三种视觉粗细；加一个图标要判断"写 path 还是装 lucide" |
| **P4** | `ui` store 是上帝对象 | 一个类里同时有：视图状态（view/page/search/sort/filter）、设置、歌单列表 + 8 个歌单 CRUD（`createPlaylist`…`removeItemAt`）、诊断、beets 状态、歌词 loader、UiState 防抖落盘。而 `stores/playlists.svelte.ts` 里**又有一份**歌单 CRUD，`ui.*` 那 8 个方法**唯一调用方就是它** —— 纯转发层 | 改歌单要动两个文件；`ui` 什么都 import，谁都能改它，等于没有边界 |
| **P5** | 三份列表实现 | `TrackList`（虚拟滚动 + 拖拽 + 勾选 + 搜索高亮）、`TrackTable`（非虚拟 + 勾选）、`QueuePanel`（虚拟滚动 + 拖拽）。`computeRange` + `ResizeObserver` + `restoreScrollTop` + `dropLine` 接线**抄了两份**；勾选语义（点=切、Shift=连、表头=三态）也抄了两份，连"`onclick` 不能挂 `Checkbox.Root`"的注释都抄了两份 | 修一个虚拟滚动 bug 要改两处；两份迟早不一致 |
| **P6** | BatchBar 两处渲染、两套动作 | `AppShell` 渲染一次（曲库详情用）、`HomeView` 再渲一次（首页区块用）；`batchPlay`/`batchEnqueue` 逻辑在 `list.svelte.ts` 与 `HomeView` 各写一遍 | 加一个批量动作要改三处 |
| **P7** | 死代码/死注释 | 未引用的 CSS：`.modal.settings`、`.tabs`、`.modal-body`、`.settings`（设置是弹窗时代的遗留）、`.stats`、`.home-skeleton`；未定义变量 `var(--warning)`（ImportPanel）、`var(--font-mono)`（overlays.css），都靠 fallback 兜着；`lib/artwork-blob.ts` 注释还写 `media://`（实际已是 `aiya-artwork`） | 读代码时无法判断"这规则还有用吗" |
| **P8** | 视图重置逻辑复制 4 份 | `openHome` / `openBuiltinView` / `openPlaylist` / `setSearch` 都手写"清 search + 清 artist/album + 设 page/libraryDetail" | 漏一处就是"搜索词串到歌单里"这类 bug |
| **P9** | E2E 全过期 | 刚跑 `npm run test:e2e`：**4 failed / 0 passed**。失败原因是选择器停留在重构前：`.main .list .row`（首页现在是仪表盘，行是 `.tt-row`）、`.nav-item` 文本「设置与诊断」（现在是「设置」）、`.tabs button`（设置是一整页，没有 tab） | 唯一的自动化 UI 回归网是坏的，等于没有 |
| **P10** | 缺页 | 没有专辑页、没有艺人页（只能在右键菜单里"按艺人筛选"，得到的是列表 + 一个筛选 chip）；搜索没有独立结果页（直接复用曲库列表）；`RadioView` 是纯占位（10 行"功能规划中"）却占着一级导航 | 侧栏 4 个入口里 1 个是空壳；曲库的"专辑/艺人"两条主浏览路径缺失 |
| **P11** | EQ 完全不存在 | 前后端各 0 处（`grep -i 'eq\|equalizer\|biquad'` 无命中）。音频链目前只有解码 → 变速 → ReplayGain 近似 → WASAPI | 用户预期里的"播放器 EQ"是**新功能**，要 Rust DSP + 协议升版 |
| **P12** | 选择集两套模型无抽象 | 主列表用视图下标（`stores/selection`），首页表格用曲目 ID（`HomeView` 局部状态）；`selectAll` 用 `list.total`，在首页（不走 list store）语义就是错的 | 批量能力每接一个新页面就要重想一遍 |

---

## 2. 功能全量清单（现状：能用 / 半成品 / 缺失）

标注 **[新]** = 本轮要做，**[修]** = 已有但要重做，**[留]** = 不动。

### A. 曲库与入库

| # | 功能 | 入口 | 实现 | 状态 |
|---|---|---|---|---|
| A1 | 选目录并扫描入库（阶段/计数/取消） | 设置·曲库工具 / 空态按钮 | `scan.pickRootAndScan` → `library_scan` | 能用 |
| A2 | 重新扫描（增量，保留播放记录） | 设置·曲库工具 | `scan.start` | 能用 |
| A3 | beets 预览（专辑卡片 + 候选 + 逐字段 diff） | 设置·导入 | `ImportPanel` + `beets_preview` | 能用（面板皮，要 [修] 结构） |
| A4 | beets 应用（只写标签 / 复制 / 移动 / 取消 / 结果摘要） | 同上 | `beets_apply`/`beets_cancel` | 能用 |
| A5 | beets 库 → 本地读模型全量对齐 | 同上 | `beets_sync` | 能用 |
| A6 | 曲库统计（曲目/专辑/艺人/缺失位置） | 侧栏底部 | `library.stats` | 能用，[修] 位置与信息层级 |
| A7 | 在资源管理器定位文件 | 右键菜单 | `library_reveal_track` | 能用 |
| A8 | 从曲库移除（仅索引） | 右键 / 批量条 | `library_remove_tracks` | 能用 |
| A9 | m3u 导入 / 导出 | 设置·导入 | `playlist_import`/`playlist_export` | 能用 |

### B. 列表与查找

| # | 功能 | 入口 | 状态 |
|---|---|---|---|
| B1 | 全部曲目（游标分页 + 虚拟滚动 + 滚动位置恢复） | 侧栏/首页 | 能用，[修] 抽公共虚拟化 |
| B2 | 排序（标题/艺人/专辑/添加 × 升降） | 表头 | 能用 |
| B3 | 全库搜索（防抖 120 ms，标题高亮） | 顶栏 | 能用，[修] 独立结果页 |
| B4 | 艺人/专辑筛选 | 右键菜单 | 半成品 → [新] 升级为专辑页/艺人页 |
| B5 | 最近播放 | 首页区块 / 查看全部 | 能用 |
| B6 | 常听（按播放次数） | 同上 | 能用 |
| B7 | 首页仪表盘（歌单宫格 + 三段表） | 侧栏 | 能用，[修] 用 kit 重排 |
| B8 | 空态/失败态/重试（区分"空库""无匹配""加载失败"） | 各列表 | 能用 |

### C. 选择与批量

| # | 功能 | 状态 |
|---|---|---|
| C1 | 行点击播放 / Ctrl·Shift 行选择 | 能用 |
| C2 | 勾选列：点=切换、Shift=连选、表头三态全选、上限 5000 | 能用，[修] 抽公共 |
| C3 | 批量条：播放 / 入队 / 加歌单 / 移出曲库 / 取消 | 能用，[修] 单点渲染 |
| C4 | 单曲右键菜单 9 项（播放/下一首播放/入队/加歌单/按艺人筛/按专辑筛/定位/移除） | 能用 |
| C5 | 键盘：↑↓ 移动、Enter 播放、Esc 清选择、Ctrl+A 全选 | 能用，[修] 语义统一 |

### D. 歌单

| # | 功能 | 状态 |
|---|---|---|
| D1 | 新建 / 重命名 / 删除 / 清空 | 能用，[修] CRUD 归位到 playlists store |
| D2 | 加入曲目（返回新增/跳过计数） | 能用 |
| D3 | 歌单内拖拽排序 + 侧栏整体排序 | 能用 |
| D4 | 播放全部 | 能用 |
| D5 | 歌单右键菜单 6 项（含上移/下移禁用规则） | 能用 |

### E. 播放

| # | 功能 | 状态 |
|---|---|---|
| E1 | 播放/暂停/上下曲/seek/音量/静音 | 能用 |
| E2 | 随机 + 循环三态 | 能用 |
| E3 | 倍速 0.5–2.0×（面板 + 双击复位） | 能用 |
| E4 | 队列（查看/跳播/移除/拖拽/清空/插入下一首/加入队列） | 能用 |
| E5 | 播放错误（错误条 + 跳到下一首） | 能用 |
| E6 | 启动恢复（队列 + 进度 + 上次曲目） | 能用 |
| E7 | 系统媒体键 / SMTC / 防休眠 | 能用 |
| E8 | 输出模式（共享/独占/独占必需）、缓冲、时钟源、暂停释放设备 | 能用（设置页） |
| E9 | 音量归一化（ReplayGain 近似） | 能用 |
| E10 | **均衡器 EQ（10 段 + 预设）** | **缺失** → [新]，需协议 v6 |

### F. 视觉与信息

| # | 功能 | 状态 |
|---|---|---|
| F1 | 歌词（LRC 解析、当前行、点击 seek、手动滚动不抢） | 能用 |
| F2 | 频谱（compact / pro 五形态） | 能用 |
| F3 | 封面（`aiya-artwork` 协议，96/160/256/320 分级） | 能用 |
| F4 | 封面主色提取 | 能用，[修] 加对比度约束 |
| F5 | 主题（暗/亮）+ 密度（舒适/紧凑） | 能用，[修] 变量化 |
| F6 | 格式徽章（无损/Hi-Res/规格） | 能用 |
| F7 | 标题栏（拖拽/最小化/最大化/关闭） | 能用，[修] 用 kit |
| F8 | 托盘 + 关窗隐藏 + 单实例 | 能用 |

### G. 设置与诊断

| # | 功能 | 状态 |
|---|---|---|
| G1 | 设置 6 组 22 项（曲库/外观/播放/系统/快捷键/诊断） | 能用，[修] 改分组导航 |
| G2 | 快捷键 17 个动作：录制、冲突检测、恢复默认、禁用 | 能用，[修] 表格式重排 |
| G3 | 诊断（版本/平台/Schema/曲库/输出模式/媒体键/电源/侧车/查询耗时/最近错误） | 能用 |
| G4 | 清空播放历史 | 能用 |

### H. 明确没有的（写出来，避免"以为有"）

专辑页、艺人页、独立搜索结果页、FM 电台后端、EQ、智能歌单/评级/收藏歌单、睡眠定时器、
迷你播放器/紧凑模式、播放列表自定义封面、命令面板（Ctrl+K）、多选拖拽跨视图。

---

## 3. 页面功能分配（四区）

### 3.1 应用壳体

骨架是**三行两列**，顶栏是唯一横贯全宽的一行：

```
┌──────────────────────────────────────────────────────────────────────┐
│ TitleBar [A AIYA] [←] [ 搜索本地曲库 ] ·········空白可拖········ [—][□][✕] │ 46px
├───────────┬──────────────────────────────────────────────────────────┤
│ Sidebar   │ Header         ← 页内标题零件（页面自己调用，随内容滚动）   │
│ 216px     │ Toolbar        ← 页内工具条零件（tab / 筛选 chip / 动作）   │
│           ├──────────────────────────────────────────────────────────┤
│ 一级导航  │ Content  唯一 dominant region（列表/宫格/表单）             │
│ 歌单分组  │                                                          │
│ 统计底部  │                                                          │
├───────────┴──────────────────────────────────────────────────────────┤
│ PlayerDock  封面/标题 · 传输/进度 · 音效·正在播放·队列·音量            │ 92px
│             （队列抽屉从它上沿升起，压住 Content，不推挤布局）          │
└──────────────────────────────────────────────────────────────────────┘
```

| 区 | 装什么 | 不装什么 |
|---|---|---|
| **TitleBar**（唯一全局条） | 应用名（品牌块）、**返回上一页**、**搜索框**（全局唯一）、窗口三键；**整条空白可拖拽，双击最大化** | 页面标题、页面级动作、播放控制 |
| **Sidebar** | 一级导航（首页/曲库/最近/常听/专辑/艺人）、歌单分组（新建、右键、排序）、底部统计 | 设置入口降级为导航最后一项；扫描进度 → 设置页 |
| **Header**（Content 层） | kicker + 标题 + 副标题 + meta（`12 首 · 43 分钟`）+ 右侧动作组；可选大封面（歌单/专辑） | tab、筛选 chip、搜索 |
| **Toolbar**（Content 层） | tab 组（曲库三视图 / 设置六区块）、筛选 chip、右侧次级动作 | 标题、播放全部（那是 Header 的主按钮） |
| **PlayerDock** | 当前曲信息、传输、进度、音效(EQ)、正在播放入口、队列按钮、音量 | 歌词正文与频谱画布（在「正在播放」页） |

**三条形态约束（本次修订的核心）：**

1. **顶栏四件一行**。搜索框宽度固定（约 360px），右侧留给窗口控制，中间是唯一的拖拽余量。
   `--titlebar-height` 由 36px 提到 **46px** —— 36px 塞不下 30px 高的输入框 + 上下呼吸；
   46 也是 Windows 自定义标题栏的常见值（系统标题栏 32 + 应用条 14）。
2. **Header 与 Toolbar 不在一行、不组合**。它们是两个零件，页面写两行标签，中间想插
   "扫描进度条"就插，不需要给"组合零件"开插槽。理由：这两个东西的**生命周期不同** —— 标题是静态的，
   工具条会因视图/权限/空态出现或消失（歌单页没有 tab，曲库页有；扫描时工具条下面还要多一条进度）。
   组合 = 提前把它们的出现条件绑死。
3. **不设 TopBar 这一级**。全局的（搜索/返回/窗口）在 TitleBar，页内的（标题/工具条）归页面。
   中间层一旦存在，就会持续吸收"不知道放哪"的东西，最后变成一个什么都装但谁都不归它的组件。

**顶栏拖拽的实现要点**（Tauri 2.12.5 `drag.js` 实测，三处必须写对）：

| 事实 | 后果 | 写法 |
|---|---|---|
| `data-tauri-drag-region="deep"` = 子树任意点击都可拖 | 只写一行就够，不用给每个子元素加 | 只挂在 `<header>` 上 |
| 可点元素（`A`/`BUTTON`/`INPUT`/`SELECT`/`TEXTAREA`/`LABEL`/`SUMMARY`、`role` 属于交互角色、**带 `tabindex` 且不为 `-1`**、`contenteditable`）**无属性时阻断拖拽** | 返回键、窗口三键、输入框天然不拖 ✅ | 不需要给按钮加例外 |
| `data-tauri-drag-region="false"` 显式阻断，且优先于 `deep` | **搜索框的胶囊容器**（`div`，不是可点元素）必须加，否则点输入框左右内边距会开始拖窗 | `<div class="tb-search" data-tauri-drag-region="false">` |

> 坑：`isClickableElement()` 把**任何非 `-1` 的 `tabindex`** 当成可点元素。给品牌块或空白区
> 加 `tabindex="0"` 会静默抹掉整条拖拽区。同理，`svelte-ignore` 里为了消 a11y 警告而加的
> `tabindex` 会顺手破坏拖拽 —— 这条要和 docs/23 里"包装 span 加 `tabindex="-1"`"的约定对齐。

**导航模型（Sidebar 与顶栏共用一套）：**

```
      Sidebar 一级导航 ─┐
      侧栏歌单项      ─┤
      首页卡片/查看全部 ─┼─→ nav.go(target) ─→ [压栈] ─→ list.openXxx / ui.openPage
      空态"去曲库看看" ─┤                      ↑
      播放条"正在播放" ─┤                  nav.history（快照栈，≤50）
      顶栏搜索框       ─┘                      ↓
                             TitleBar [←] ─→ nav.back() ─→ 逐字段还原快照
```

`NavTarget` 是判别联合（`dashboard` / `builtin` / `playlist` / `page`），**不是** URL 字符串：
桌面单窗口没有地址栏，引入字符串路由只会多一层解析与拼错的机会。
`nav.go()` 在目标是"当前所在"时不压栈但仍执行（= 同页再点一次是刷新，不往栈里塞自己）。

**本轮只收"动作"，不收"状态"**：`ui.page` / `ui.libraryDetail` / `ui.view` 三字段仍是真相，
`nav` 只是它们的唯一写入口。等 B4 拆页（删 `libraryDetail`、路由从 4 值扩到 8 值）时，
`nav.go()` 的签名不变、调用点一行都不用改 —— 先收动作、再收状态，是这两步能分开做的原因。

### 3.2 主体页面（路由表）

**布局契约**：所有列表页共用同一套"三段式"，但**三段是页面自己拼的**，不是某个 `ListPage` 组件的插槽：

```
<Header …>              ← 零件 1（静态）
<Toolbar …>             ← 零件 2（可缺席：歌单页无 tab、空库无 chip 时整条不渲染）
<列表 / 空态 / 表单>      ← 各自实现
```

`ListPage` 若要做，只做"给这三段排间距"的细壳（B4 再定），**不允许**把 Header/Toolbar
降级成它的内部 prop —— 那样又回到"组合组件"的老路。

`ui.page` 从 4 值扩展到 8 值。**每个页面自己调 Header / Toolbar**，不经过任何中间组件。

| 路由 | 页面 | 装什么 | 数据来源 | 备注 |
|---|---|---|---|---|
| `home` | 首页仪表盘 | 歌单宫格、最近播放 8 条、常听 8 条、最近添加 8 条（三段都可勾选批量） | `library_list` × 3 + playlist | 现有，重排 |
| `library` | 曲库（全部曲目） | 表头排序、勾选、右键、批量、滚动恢复 | `library_list`（cursor） | 现在寄生在 home+libraryDetail，**拆成独立页** |
| `recent` | 最近播放 | 列表（时长列 → 末次播放时间列） | `source: recent` | 同上拆出 |
| `mostPlayed` | 常听 | 列表（次数列） | `source: mostPlayed` | 同上拆出 |
| `playlist/:id` | 歌单详情 | 头部封面 + 播放全部 + 重命名/清空/删除、拖拽排序 | `source: playlist` | 同上拆出 |
| `album/:id` | **专辑详情 [新]** | 封面大头、年份/曲目数/总时长、曲目表 | 需新命令 `library_facets` + `library_list(albumId)` | 见 §6 决策 2（已定：做） |
| `artist/:id` | **艺人详情 [新]** | 头像占位、专辑宫格 + 全部曲目 | 同上 | 同上 |
| `search?q=` | **搜索结果 [新]** | 分段：曲目 / 专辑 / 艺人（先做曲目，另两段可空） | `library_list` + search | 现在直接复用曲库页 |
| `nowPlaying` | 正在播放 | 大封面 + 歌词（左）+ 频谱（右）+ EQ（可切换第三块） | lyrics/spectrum | 现有，重排 |
| `settings` | 设置 | 左分组导航 + 右内容：曲库工具（扫描/导入/beets）、外观、播放、系统、快捷键、诊断 | 各 store | 现有 6 区块改分组 |
| `radio` | FM 电台 | —— | —— | **从一级导航移出**，等后端；未实现前不占位（决策 4） |

拆页的收益：现在 `ui.page === 'home' && ui.libraryDetail` 这个双字段组合承担了"首页 vs 六个列表视图"的区分，
`App.svelte` 里是 `{#if ui.page === 'home'}{#if ui.libraryDetail}<LibraryView/>{:else}<HomeView/>{/if}`。
改成独立路由值后，`libraryDetail` 字段可以删，`openBuiltinView` 的三段式判断也一起消失。

**两个零件的真实调用点（改造前后对照）：**

```
首页 HomeView（整页滚动，零件在 .page-scroll 内，滚动时跟着走）
  <Header kicker="YOUR LIBRARY" title="首页" subtitle="回到常听的音乐…">
    actions: [浏览全部曲目 →]
  <Toolbar>                         ← 首页没有：三段预览各自带 section-heading
  <section> 歌单宫格 / 最近播放 / 常听 / 最近添加

曲库 LibraryView（固定头 + 独立滚动列表，零件不跟列表滚）
  <div class="page-pad">            ← 横向 22px 归布局层，零件自己不带
    <Header kicker="曲库" title="全部曲目" meta="5 000 首"> actions: [播放全部]
    <Toolbar tabs=[全部曲目|最近播放|常听] active="library" filters=[已按艺人筛选 ×]>
  <TrackList>                       ← 自己滚动、自己恢复位置

设置 SettingsView（**现役反例**）
  今天：<header class="page-heading"> <h1>设置</h1> + <nav class="settings-jump">18 个 chip 锚点</nav> </header>
        —— 标题与工具条挤在同一个 header 里，各自 padding 靠猜
  改造：<Header kicker="PREFERENCES" title="设置" />
        <Toolbar tabs=[音乐库|外观|播放|系统|快捷键|诊断] active={滚动位置反查} />
        —— 锚点 chip 从"标题右边的导航"变成"独立工具条上的 tab"，滚动联动逻辑不变
```

设置页是本轮最有说服力的证据：它**已经**把两种东西塞进一行，且因为塞在一起，`jumpTo()` 的
active 状态只能靠 `IntersectionObserver` 反查后写回一个 `.chip.on` —— 拆开之后，`Toolbar`
的 `active` 是它的正常职责，不再需要"标题区域兼职承载导航状态"。

### 3.3 播放条与播放相关浮层

| 控件 | 形态 | 内容 |
|---|---|---|
| 音效（EQ） | Popover（点击外关闭、Esc 关闭） | 10 段滑杆、预设下拉、复位、开关；非 1.0 增益时按钮带标记 |
| 倍速 | Popover | 现有滑块 + 复位（已实现，改用 kit 的 Popover/Slider） |
| 队列 | 抽屉（从 Dock 上沿升起，宽 380，`position:absolute`，不推挤） | 现有 QueuePanel 内容 + 头部总时长 |
| 正在播放 | 页面跳转 | 大封面 + 歌词 + 频谱 |
| 输出状态 | Popover（挂在格式徽章上） | 当前模式（共享/独占实际生效）、采样率/位深、缓冲、时钟 |
| 音量 | 滑杆常驻 | 静音键 + 滑杆；窄窗收进 Popover |

### 3.4 浮层

菜单（右键/下拉，支持分组与危险项）、Dialog（Alert/Prompt/Picker 三态共用一套壳）、Toast、InlineError、Tooltip。
复杂任务（beets 导入、扫描）**不进浮层**，进设置页的任务区。

---

## 4. UI Kit

### 4.1 决策：语义 CSS + Svelte 组件，不采用 Tailwind

三个选项：

| 方案 | 代价 | 判定 |
|---|---|---|
| 按 docs/19 §6.4 全量 Tailwind 化（utility + `@theme inline`） | 把 2 472 行手写 CSS 重写成 utility，13 k 行组件全要动；收益是"不再有 CSS 文件" | 否决：仓库已经跑通 token 换皮，重写风险 > 收益 |
| **Tailwind 依赖保留、入口裁剪**（只引 `theme` + `utilities`，不引 preflight） | 少了 preflight 约 4 KB 与"双份 reset"的坑；将来想写 utility 仍然能用 | **采用**（2026-10-26 拍板） |
| **Kit 组件自带 scoped `<style>`，全局 CSS 只留布局与主题** | 每个零件自包含、可覆盖靠 props 不靠后代选择器；已有先例（ImportPanel/SpectrumPanel） | **采用** |

落地规则：
- `styles/tokens.css` **只有变量**（含 reset 移出）。
- `styles/base.css`：元素默认样式（body/button/input/滚动条/选区/焦点环）——**唯一的 reset**。
- `styles/layout.css`：`.app` 网格、`.sidebar`/`.main`/`.player-dock` 的席位（只有布局，没有零件外观）。
- `components/ui/*.svelte`：零件，自带 `<style>`，只吃 CSS 变量，不写原始色值。
- 剩下的 `styles/{lists,player,pages,overlays}.css` 随批次**逐步删除**，内容迁进对应 feature 组件。
- **Tailwind 入口改造**：`app.css` 从 `@import 'tailwindcss'` 换成显式的 `theme` + `utilities` 两层，
  不引 `tailwindcss/preflight.css`。四个入口文件实测源码体积：`index.css` 29 786 B / `theme.css` 19 586 B /
  `preflight.css` 8 489 B / `utilities.css` 21 B —— 真正进产物的膨胀来自 preflight 与未用的 theme 变量，
  裁掉 preflight 后按产物字节验收（预期 ≥3 KB 下降）。

### 4.2 Token 三轴

```css
/* 色彩：语义角色，不出现 blue-500 这种原始色 */
--surface-1: 底（canvas）      --surface-2: 面板/侧栏      --surface-3: 浮层/菜单
--surface-hover / --surface-active / --surface-input
--line / --line-strong
--text-1 / --text-2 / --text-3（原 text / text-dim / text-faint）
--accent / --accent-fg / --accent-weak / --accent-text（主色可由封面派生，见 4.4）
--danger / --danger-weak / --warn / --warn-weak / --ok / --ok-weak
--thumb / --thumb-hover / --shadow / --backdrop / --glow-1 / --glow-2

/* 尺寸：密度轴只改这三个 */
--space-1..6: 4/6/8/12/16/22px
--radius-sm/md/lg/pill: 6/8/10/999px
--control-h: 28px（紧凑）/ 32px（舒适）
--row-h:     42px / 52px
--titlebar-h / --sidebar-w / --player-h

/* 字体 */
--font-ui / --font-num（tabular） / --fs-xs..xl: 10.5/11.5/12.5/13/15/22px
--dur-fast/base: 120/180ms  --ease: cubic-bezier(.2,.7,.3,1)
```

现有 293 处 `var(--bg)`/`var(--text-dim)` 等引用做**机械重命名**（脚本 + 人工核对），一次改完不共存两套名字。

### 4.3 零件清单（20 原子件 + 2 版式件）与迁移来源

| # | 零件 | 取代现有 | 变体/要点 |
|---|---|---|---|
| 1 | `Button` | `.btn` `.btn.primary` `.btn.danger` `.apply-btn` | variant: primary/secondary/ghost/danger；size: sm/md；leading 图标；loading；disabled |
| 2 | `IconButton` | `.icon-btn` `.icon-btn.on` `.icon-btn.tiny` `.icon-btn.play` | 必须传 `label`（生成 aria-label + title）；`pressed`；size sm/md/lg |
| 3 | `Chip` | `.chip` `.filter` `.mode-chip` | 可选/可关；`on` 用 accent-weak |
| 4 | `Checkbox` | `.tt-box`（现包 bits-ui） | 三态；**唯一**勾选入口，内含"点击处理不能挂 Root"的约定 |
| 5 | `Switch` | `.switch` | 必带可见标签 |
| 6 | `SegmentedControl` | `.segmented` | 2–4 短选项；方向键 |
| 7 | `Slider` | `.rate-slider` `.volume-slider` `.timeline input` | `onInput`（预览）/`onCommit`（提交）分离 |
| 8 | `Progress` | `.progress` | 确定/不确定 |
| 9 | `Field` | `.field-label` `.text-input` | label + hint + error |
| 10 | `Card` | `.playlist-card` `.library-card` | 封面/图标 + 标题 + 副标题；可点 |
| 11 | `Cover` | `.cover` `.cover.xs/.sm/.lg` `.view-cover` | 尺寸档位 + placeholder |
| 12 | `EmptyState` | `.empty` `.playlist-empty` `.queue-empty` `.lyrics-empty` `.tt-empty` | icon/title/hint/action 四段 |
| 13 | `Skeleton` | `.skeleton` `.shimmer` `.tt-skeleton` | 行/块两种 |
| 14 | `Toast` / `InlineError` | `.toast` `.error` `.notice` `.import-error` | 沿用 notify store；error 分可重试/阻断 |
| 15 | `Menu` | `.context-menu` `.menu-item` `.batch-menu` | 分组、分隔、危险项、碰撞翻转、键盘 |
| 16 | `Dialog` | `.modal` `.confirm-message` `.modal-actions` | Alert/Prompt/Picker 三态共用壳；焦点陷阱 + 还原 |
| 17 | `Tooltip` | 现在全靠 `title=` | 统一延迟与位置；不承载唯一信息 |
| 18 | `Popover` | `.rate-box` `.rate-pop` | 挂触发点、外点关闭、Esc 关闭、还原焦点 |
| 19 | ~~`DataTable`~~ → **`CheckCell`** | ~~`.list` `.row` `.grid-head` `.cell` `.tt*`~~ 勾选格：`.k-check` `.k-box` | 受控 Checkbox + 包装层点击 + Space/Enter 键盘防护的唯一实现（§5 B4-2 决策修订：整表合并不做） |
| 20 | `StatBlock` | 侧栏统计、诊断 grid | 数字 + 标签，tabular 数字 |

**版式件（2 件，v2 新增）** —— 与上面 20 个原子件的规则**不同**：

| # | 零件 | 取代现有 | 约束 |
|---|---|---|---|
| 21 | `Header`（Content 层） | `.page-heading` `.page-heading h1/p` `.view-head` `.view-meta` `.view-kicker` `.view-cover` `.view-actions` | 形态由 `cover` 三态决定（缺省=纯文字 / `null`=占位 / 字符串=大封面）。**不带横向 padding**（归调用方的 `.page-pad`），所以能在 `.page-scroll` 内也能在固定头位置。`meta` 是字符串不是 snippet —— snippet 里写的 scoped class 不生效（渲染在父作用域） |
| 22 | `Toolbar`（Content 层） | `.settings-jump` `.chip.on` `.filters` `.filter` | `tabs` + `filters` + `start`/`end` snippet；**全部为空时整条不渲染**（不占高度）。`active` 由页面给（设置页仍是滚动反查，但反查结果只是这个 prop 的值） |

> **版式件的铁律：只允许页面直接调用，不允许被别的零件内部调用。**
> 一旦 `Header` 被 `ListPage` 之类包起来，就会出现"这个页面想改标题右边的东西，
> 得先给中间组件加 prop"——那正是 `TopBar` 死掉的原因。原子件可以互相嵌套（Button 里放 Icon），
> 版式件不行。

演示页：`?page=kit`（仅 dev/mock 下注册）—— 一屏看全 20 件的全状态（normal/hover/active/disabled/loading/dark/light/compact），
外加 Header / Toolbar 的三种形态各一例。
它是 kit 的自测页，也是后续改 token 时的目检页。

### 4.4 主题适配

- **暗/亮**：`html[data-theme]` 切 token，组件零分支（现在已如此，继续保持）。
- **密度**：`html[data-density]` 只改 `--row-h`/`--control-h`/`--space-*` 的档位；列表虚拟化读同一个 `--row-h`（现在 `rowHeight={density==='compact'?42:52}` 是 JS 硬编码，要和 CSS 同源 —— 改成从 token 读或单一常量导出）。
- **主色**：`--accent` 默认来自主题；开启"封面主色"后由 `accentForArtwork` 派生，但**必须过对比度约束**：派生色与 `--surface-1` 的对比度 < 3:1 就回落到主题色（现在无约束，浅色主题下深色封面会得到看不见的 accent）。
- **禁用规则**：组件内禁止 `rgba(255,122,122,...)` 这类硬编码（`.error` 现在就是这么写的，浅色主题下底色仍是深红调），一律 `color-mix(in srgb, var(--danger) 10%, transparent)`。

### 4.5 框架组件引用规划（bits-ui 2.20.2）

> 2026-10-26 晚追加。起因：`bits-ui` 从 docs/23 起就在依赖里，但**只用了 `Checkbox` 一个原语**，
> 「引了却不用」和「全量重写」一样浪费。本节把 22 个零件逐个过一遍：哪些该用原语、哪些必须自研、
> 哪些明确不用，以及用的时候必须绕开的坑。**这是 B2 的施工图，不是选型讨论。**

#### 一、判断线（一条，不是四条）

> **行为难写对 → 用原语；视觉难统一 → 自研；两者都难 → 原语包壳。**

"行为难写对"的判据很具体：**焦点陷阱、Esc/外点关闭、Portal 定位与碰撞翻转、ARIA 状态机、
键盘导航、指针捕获**。这几样每一条都能写出来，但写对需要覆盖的组合数远超它的体积。
反过来，`Button` 的"primary/ghost/danger + loading + 图标"没有任何一条属于上面 —— 它只是样式，
用 `bits-ui/button` 反而要写 adapter（`Button.Root` 是 builder 模式，不产出样式按钮）。

#### 二、逐件映射

| # | 零件 | 来源 | 内核 / 子件 | 我们要包的东西 | 批次 |
|---|---|---|---|---|---|
| 1 | `Button` | **自研** | — | `variant: primary/secondary/ghost/danger` × `size: sm/md` × `loading` / `disabled` / `icon` snippet | B2 |
| 2 | `IconButton` | **自研** | — | `label` 必填（同时产出 `aria-label` + `title`）、`pressed`、`size: sm/md/lg` | B2 |
| 3 | `Chip` | **自研** | — | `on`（accent-weak）、可关（右侧 ×，`onclear`） | B2 |
| 4 | `Checkbox` | **bits-ui** `Checkbox.Root` | 三态 `checked`/`indeterminate` | 受控唯一的勾选入口；点击处理**挂包装层** | B2（已有用法，抽件） |
| 5 | `Switch` | **bits-ui** `Switch.Root` + `Thumb` | `bind:checked` | 可见标签由 `Field` 提供；`label` 必填 | B2 |
| 6 | `SegmentedControl` | **bits-ui** `RadioGroup` + 自研外观（原定 `ToggleGroup`，B3b-3 实测换掉，见 §4.5 坑 8） | `value` 受控、方向键、单选不可置空 | 2–5 个短选项；`onchange(value)` | B2 建 / B3b-3 换核 |
| 7 | `Slider` | **bits-ui** `Slider.Root` + `Range` + `Thumb` | `onValueChange`（预览）/ `onValueCommit`（提交） | 进度 / 音量 / 倍速三处共用；`format` 提示；`step` | B2 |
| 8 | `Progress` | **bits-ui** `Progress.Root` | `value` / `max` | 确定态（扫描）；不确定态用自研条纹 | B2 |
| 9 | `Field` | **自研**（可选 `bits-ui/label`） | — | label + hint + error + 单行 input；`bind:value` | B2 |
| 10 | `Card` | **自研** | — | 封面/图标 + 标题 + 副标题；`href` 语义用 button | B2 |
| 11 | `Cover` | **自研** | — | `xs/sm/md/lg` + `src` 空占位（♪）；`radius` 档位 | B2 |
| 12 | `Skeleton` | **自研** | — | `line` / `block` / `row` | B2 |
| 13 | `EmptyState` | **自研** | — | icon / title / hint / action 四段 | B2 |
| 14 | `StatBlock` | **自研** | — | 数字（tabular）+ 标签；`size` | B2 |
| 15 | `Menu` | **bits-ui** `ContextMenu`（`Root`/`Trigger`/`Content`/`Item`/`Separator`） | 定位、碰撞翻转、键盘、Esc | 数据驱动：`MenuItem[]` → 子件；危险项 `variant="danger"` | B3b |
| 16 | `Dialog` | **bits-ui** `Dialog`（确认类用 `AlertDialog`） | 焦点陷阱、Esc、还原焦点、`aria-modal` | `title`/`description`/`actions` snippet；`danger` 变体 | B3b |
| 17 | `Popover` | **bits-ui** `Popover` | 定位、外点关闭、还原焦点 | 触发点 + `content` snippet；`width` | B3b |
| 18 | `Tooltip` | **bits-ui** `Tooltip` + 顶层 `Provider` | hover 延迟、定位 | 统一 `delayDuration=400`；**不承载唯一信息** | B3b |
| 19 | ~~`DataTable`~~ → **`CheckCell`** | **自研** | — | 受控 + 包装层点击 + 键盘防护一处实现（B4-2 决策修订，整表合并取消）；依赖 `bits-ui/Checkbox` | ✅ B4-2c |
| 20 | `Toast` / `InlineError` | **自研** | — | 沿用 `notify` store；错误分「可重试 / 阻断」 | B6 |
| 21 | **`Header`**（Content 层） | **自研** | — | `kicker`/`title`/`subtitle`/`meta`/`cover` 三态/`actions` snippet | ✅ 已改名 |
| 22 | **`Toolbar`**（Content 层） | **自研** | — | `tabs` / `active` / `ontab` / `filters` / `end` snippet；全空不渲染 | ✅ 已改名 |

合计：**bits-ui 9 件**（B2 已落地 5：Checkbox / Switch / SegmentedControl / Slider / Progress；B3b 落地 4：Menu / Dialog / Popover / Tooltip），**自研 13 件**。

#### 三、明确不用，以及为什么

| bits-ui 组件 | 为什么不用 |
|---|---|
| `Button` / `Toggle` | `Button.Root` 是「带 builder 的裸元素」，产出的是 `<button>` + `data-state`，样式全要自己写；我们的按钮 90% 是变体矩阵，用它等于多写一层 adapter |
| `Tabs` | 曲库三视图是**改查询后重载列表**，不是面板切换；设置页六个锚点是**滚动跳转**。两处都不是 `tablist/tabpanel` 语义（docs/24 §3.1 已定 `nav` + `aria-current`），硬套会给读屏软件一个不存在的 tabpanel |
| `ScrollArea` | 列表的 `scrollTop` 由我们自己算（虚拟窗口 + 稀疏分页 + 拖拽自动滚动）；它插一层 viewport 后所有 `scrollTop` 读写点都要改，回归面覆盖整个列表 |
| `Select` / `Combobox` | 现在的选择器是 `PlaylistPicker`（列表 + 新建入口），不是"输入框 + 候选"；没有可搜索下拉的真实场景 |
| `Toolbar`（role=toolbar） | 我们的 `Toolbar` 是版式件（tab + 筛选 chip），不是应用工具栏；两者同名不同物，**不要因为名字像就套** |
| `Separator` | 分隔线一条 `border-top` 就够，不需要一个组件管 `orientation` |
| `Avatar` / `Meter` / `RatingGroup` / `PinInput` / `Calendar` / `DatePicker` / `Command` / `Menubar` / `NavigationMenu` / `Accordion` / `Collapsible` / `LinkPreview` / `AspectRatio` | 无对应场景。列表里没有日期筛选、没有命令面板、没有多级菜单栏、设置区块直接展开 |

#### 四、用原语时必须绕开的坑（每条都来自实测或源码）

1. **`Checkbox` 不能同时受控 + 挂 `onCheckedChange`**（docs/23 实测）。它的 click 处理器会先翻内部 state，
   我们再翻一次受控值 → 视觉与模型反相。约定：**勾选态只由 `checked` prop 决定，点击处理挂包装层**。
   包装层要 `tabindex="-1"`（可编程聚焦但不进 Tab 序列；真正的键盘入口是内层 `button`）。
   —— 与顶栏拖拽无关（顶栏里没有选框），但**同理**：任何给元素补 `tabindex` 的地方都要想一下。
2. **`Switch` 同样不要双绑**：`checked` 受控 + `onCheckedChange` 只用于触发写设置，不要在回调里再改本地副本。
3. **`Slider` 的两个回调分工不同**：`onValueChange` 每次指针移动都来（音量实时生效可以），
   `onValueCommit` 只在松手/键盘结束时来。**进度条必须用 commit** —— 拖动中每帧 seek 会让
   Rust 侧反复 flush，实测有咔哒声；音量可以用 change（用户期待拖的时候就听到）。
4. **右键菜单用 `DropdownMenu` + 0×0 锚点，不要用 `ContextMenu`**（B3b-1 实测，原判断错了一半）。
   原文写「`ContextMenu` 的定位基准是行节点，行被回收就会丢锚点」——**这条是错的**：
   `ContextMenu.Root` 用的是**指针虚拟元素**（`menu.svelte.js` 里 `watch(#point)` 造一个
   `getBoundingClientRect()` 返回点击坐标的对象），行怎么回收都影响不到它。

   真正的问题在另一头：**那个坐标只能由它自己的 `Trigger.oncontextmenu` 捕获**
   （`#handleOpen(e)` 读 `clientX/clientY`，没有对外暴露 `point` 入参）。我们的模型是
   「**事件在行上收，菜单在宿主里画**」—— 要用 `ContextMenu` 就得把 Trigger 挂到每一行，
   而虚拟列表的行节点本来就不该长菜单（会跟回收、拖拽选择、双击播放打架）。

   所以改用 `DropdownMenu`：它的定位基准是**触发元素的 rect**，于是放一个
   `position:fixed; width:0; height:0` 的锚点摆在点击坐标上，菜单就长在那里；
   碰撞翻转、外点关闭、Esc、键盘导航、`data-highlighted` 全由它负责。
   代价：锚点是个隐藏元素，菜单关闭时随组件卸载 —— 所以 **Esc 后的焦点还给 BODY**
   （触发它的行元素已经不在了）。可接受：菜单是瞬态浮层，不像对话框那样需要还原焦点。
5. **`Dialog` 必须有 `Title`**，否则运行时 a11y 警告；`Description` 可省但要显式 `aria-describedby={undefined}`。
6. **bits-ui 渲染出来的 DOM 不在你的 scoped 作用域里**（B2 实测，此前"哈希会带上"的判断是错的）。
   `<Switch.Root class="k-switch">` 里的 `class` 是普通属性，**不会带 scoped 哈希**；写在父组件
   `<style>` 里的 `.k-switch { }` 会被 svelte-check 报 `Unused CSS selector`，运行时完全不生效 ——
   症状是"控件出现了，但一点样式都没有"，比报错更难查。三条规矩：
   - 包装件的样式选择器一律写 `:global(.k-xxx)`，类名带 `k-` 前缀且全局唯一（本批有 10 个：
     `.k-check` / `.k-switch(-thumb)` / `.k-seg(-item)` / `.k-slider(-range/-thumb)` / `.k-progress(-fill)`）；
   - **`class:` 指令不能用在组件上**（编译期 `component_invalid_directive`），要拼字符串：
     `class={`k-progress${size === 'md' ? ' md' : ''}`}`；`style:` 同样，用 `style={...}`；
   - 想要真正 scoped，只能用 bits-ui 的 `child` snippet 自己渲染元素 —— 本批不用（会让 14 件里 5 件写法各不相同）。
7. **`Tooltip.Provider` 只放一次**（`App.svelte` 顶层），否则每个 Tooltip 自带 700ms 延迟且互不协调。
8. **分段控件用 `RadioGroup`，别用 `ToggleGroup type="single"`**（B3b-3 实测，原判断**是错的**）。
   原文写「包壳里把 `undefined` 吃掉就行」—— 实测不行：`ToggleGroup` 在已选中项上按 `Space`
   会**真的取消选中**（两项都 `data-state="off"`），而回调被吃掉后设置里还是旧值 →
   **画面说没选、状态说选了**。吃掉 undefined 只挡住了写状态，挡不住它的内部视觉态。
   `RadioGroup` 语义同构（单选永不为空），方向键同时移焦点 + 换选中。
   另外 `orientation="horizontal"` 必须显式给（默认 vertical，左右键不生效），
   `data-state` 是 `checked`/`unchecked` 不是 `on`/`off`，CSS 选择器要跟着改。
9. **`Slider.Thumb` 必须给 `index`**：单拇指也要 `index={0}`，它是必填项不是可选项（TS 会直接报缺属性）。
10. **不要给包装组件加泛型**：`<script generics="T extends string">` 配 `options` 数组字面量时，
    推导结果是 `string` 而不是联合类型（数组先被推成 `string[]`），最后还是要在调用点断言。
    直接把 `value` 声明成 `string`、断言写在调用处更诚实（`SegmentedControl` 最终就是这么定的）。
11. **菜单项高亮只认 `data-highlighted`**，不能只写 `:hover`。bits-ui 的键盘导航（↑↓/Home/End/首字母）
    移动时只改这个属性、不移动鼠标 —— 只写 `:hover` 的话，用户按方向键会看到"选中项没变"。
12. **`snippet` + `{...props}` 是接原语触发器的唯一姿势**：`Popover` / `Tooltip` / `DropdownMenu` 的
    `Trigger` 都提供 `child={({ props }) => ...}`，把 props 展开到**调用方自己的元素**上，
    组件样式才留在调用方。类型只能写 `Record<string, any>`（bits-ui 没导出合并后的公共类型，
    写成 `unknown` 会让 `{...props}` 在 svelte-check 里报"不能展开到该元素"）。
13. **`Dialog` 的标题必填、说明可选**：没有 `Description` 时不要留一个空的 `Dialog.Description`，
    读屏会把空节点当成描述。`Dialog.svelte` 里是"给了才渲染"。

#### 五、图标与样式的引用规则

- **kit 内部直接 `import { X } from '@lucide/svelte'`**，图形通过 snippet/prop 由调用方给。
  `components/Icon.svelte` 是「语义名 → 组件」的旧适配层，留给**业务视图**用；kit 不再叠一层映射
  （否则一个图标两处注册，加图标要改两个地方）。
- **kit 不写全局类**，外观全在自身 `<style>`，只吃 token 变量。唯一的全局样式例外是
  `.icon-solid`（scoped 够不到子组件渲染的 `<svg>`）。
- **kit 不引 `styles/*.css`**：B1 之后全局 CSS 只剩 `tokens/base/layout`，其余逐批删。

#### 六、批次分配与理由

- **B2（本批）**：14 件（1–14）。共同点：**替换时只影响视觉，不动交互路径**，可以边替边看。
  `Header` / `Toolbar` 已在 B3a 落地，本轮改名为 Content 层归位。
- **B3b**：15–18（Menu / Dialog / Popover / Tooltip）。共同点：**会改事件与焦点**（右键菜单从手算坐标
  改到 bits-ui、对话框从自有 backdrop 改到 Portal），必须和壳体换皮一起验，否则两套浮层同时存在。
- **B4**：19 `DataTable`。**B6**：20 `Toast/InlineError`。

---

## 5. 分批计划

每批独立可发布、可回滚、验收标准明确。

**v2 顺序调整**：`B3a（壳体骨架）→ B0（清场）→ B1（token）→ B2（kit）→ B3b（壳体换皮）→ B4 → B5 → B6`。

调整理由（原顺序是 B0 打头）：

- B0 的两项（"删死样式"与"E2E 修绿"）都会被 B3a 重新洗一遍 —— B3a 会删掉 `.topbar`/`.search`/
  `.view-*`/`.page-heading`/`.titlebar*` 一整批样式，也会再改一次 DOM 选择器（E2E 正是因此坏的）。
  先 B3a 再 B0，清场只做一次。
- 原原则"kit 先于页面，否则页面重写两遍"**仍然成立**，但它约束的是**视觉**：B3a 只改"谁调用谁、
  在哪一层"，标签名与 class 都不换；B2 之后 B3b 才把 `.btn` 换成 `<Button>`。结构先定、换皮后做，
  换皮阶段只改标签名，不会二次重写结构。
- 代价：B3a 是"有视觉变更的批次"排在"无视觉变更的清场批次"前面。可以接受 —— 唯一的视觉变更就是
  顶栏高度 36→46 与搜索框搬家，用户在评审时已经明确要求了。

### B3a 壳体骨架（v2 新增 · 提前）★

| 项 | 动作 |
|---|---|
| 宽 TitleBar | 重写 `TitleBar.svelte`：应用名 + 返回 + 搜索 + 窗口控制四件一行；`data-tauri-drag-region="deep"` + 搜索容器 `"false"`；`--titlebar-height` 36 → **46**；`focus-search` 注册点从 AppShell 上移到 TitleBar 内部 |
| 两个版式零件 | 新建 `PageHeader.svelte` / `PageToolbar.svelte`（自带 scoped `<style>`，只吃 CSS 变量，**不带横向 padding**） |
| 删 TopBar 这一级 | 删 `components/TopBar.svelte`；搜索并入 TitleBar；AppShell 不再有 `.main > TopBar` |
| 删 ViewHeader | 删 `components/ViewHeader.svelte`，三分支逻辑并入 `LibraryView`（歌单=大封面 / 统计=文字 / 曲库=文字） |
| 落位三页 | `HomeView`（PageHeader）/ `LibraryView`（PageHeader + PageToolbar 三 tab + 筛选 chip）/ `SettingsPanel`（标题与锚点 chip 拆成两件） |
| 导航收口 | 新建 `stores/nav.svelte.ts`（`go`/`back`/`search` + 50 深快照栈）；`Sidebar` 改**零 prop**（自己读 store、自己调 `nav.go`）；AppShell 的 `navigate()` 删除 |
| 调用点迁移 | `HomeView` ×5、`LibraryView` ×1、`playlists.svelte` ×1、`keyboard.ts` ×2、PlayerBar 回调 ×1 → 全走 `nav.go` |
| E2E | 4 条过期用例改 `data-testid` 契约，随 DOM 改动一起修绿（原本全红） |

验收：`npm run check` 0/0、`build:ui` 通过、`probe:ui` 无异常、`test:e2e` **4/4 绿**、
真窗口实测 6 条：①顶栏四件位置正确 ②拖空白移动窗口 & 双击最大化 ③点输入框不拖窗 ④
首页→曲库→返回回首页 ⑤搜索后返回还原搜索前状态 ⑥设置页标题与 tab 分两行。

#### ✅ 已完成（2026-10-26，5 个提交）

| 步 | commit | 内容 |
|---|---|---|
| S1 | `976fff9` | nav store + 10 处调用点迁移（视觉零变化，CSS 体积未变可证） |
| S2 | `320bcfb` | 宽 TitleBar 吸收搜索与返回、删 TopBar |
| S3 | `53907cd` | PageHeader + 三页落位、删 ViewHeader |
| S4 | `be9f711` | PageToolbar + 曲库三 tab / 设置六区块 |
| S5 | `4af54aa` | E2E 重写 5/5 绿 + B0 死代码清理 |

**实测数据**

| 项 | 结果 |
|---|---|
| `npm run check` | 0 errors / 0 warnings |
| `npm run build` | CSS 40.76 → **41.10 kB**、JS 235.86 → **237.28 kB**（44 行 scoped 样式 + 两个零件的逻辑；删掉的全局样式 170 行抵消了一部分） |
| `npm run test:e2e` | **5 passed**（原本 4 failed / 0 passed） |
| `probe --nav` | 10 步全绿，无 console error / 无 4xx / 无未捕获异常 |
| 顶栏几何（1280 宽） | 品牌 12..111 / 返回 119..147 / 搜索 155..515（360px）/ **拖拽余量 523..1140（617px）** / 窗控 1148..1280（3×44） |
| 顶栏属性 | `barH=46`、`data-tauri-drag-region=deep`、搜索容器 `false`、四件齐全 |
| 返回栈 | disabled → enabled → disabled 随栈切换；三次返回都落到正确页面；同页再点不压栈 |

**没验的（需要真窗口手动操作，自动化判不了）**

- 拖空白移动窗口、双击最大化 —— 探针只能读属性，拖拽是原生行为
- 点输入框左右内边距不拖窗 —— 同上（`drag="false"` 已就位，但没手拖验证）
- 搜索后按返回是否还原搜索前状态 —— 曲库为空时搜索无结果，等有数据再验

### B0 清场（无视觉变更）✅ 已完成（2026-10-26）

| 项 | 动作 | 结果 |
|---|---|---|
| Tailwind | 入口换成 `theme` + `utilities` 两层，不引 preflight；依赖与 vite 插件保留 | CSS 41 102 → **35 278 B**。**关键发现**：光去 preflight 只降 3.9 KB —— 剩下的 `@layer properties` + 25 条 utility 是 Tailwind 从**我们自己 CSS 的值**里误判出来的（`position: absolute` / `display: flex` / `visibility: visible` 被当成类名）。加 `source(none)` 关掉源码扫描后归零 |
| 死代码 | 删 `.modal.settings`/`.tabs`/`.modal-body`/`.stats`/`.home-skeleton`/`body.is-dragging`；`--warning`→`--warn`；补 `--font-mono` | 已在 S5 落地 |
| store 归位 | 8 个歌单命令 + `playlists` 字段 + `refreshPlaylists` + `playlistById` 全部离开 `ui`；`ui.lyrics*` → `stores/lyrics.svelte.ts` | 那 8 个命令的**唯一调用方就是 playlists store**，所以直接内联成 `ipc.playlist.*` 调用，公开 API 从 10 个降到 0 个。歌单摘要落到 `library.playlists`（理由见下） |
| 图标 | `Icon.svelte` 改成"语义名 → lucide 组件"的薄映射，169 行 → 41 行 | 29 条手写 path → 19 行映射（实际用到 19 个，含 2 个动态传入的 `repeat`/`volume`）；strokeWidth 统一 **1.8**，大号展示图标自己传更细的值 |
| 去重 | 视图重置 4 处 → `ui.enterList(view, {detail})` | 4×6 行同样的赋值收敛成一个方法 |
| 元素默认 | **reset 从 `tokens.css` 挪回 `base.css`**，并补上 preflight 原本代劳的几条 | tokens 从 133 行降到 **74 行纯变量**；补的 5 条各有出处（`ul/ol` 去掉 disc、`svg` 变 block、`h1..h6` 归零、`a` 继承色、`table` 折叠边框） |

**一条不能省的推论**：preflight 不是"白付的钱"，它同时**替我们做了 5 件小事**。真窗口实测
`ImportPanel` 的 `.panel-head h2` 没有任何自有样式，全靠 preflight 归零 —— 直接删不补，那个
面板头会变成浏览器默认 `1.5em` 加粗 + 上下 margin。所以删 preflight 和补元素默认是**同一个动作**。

**歌单摘要为什么落在 `library` 而不是 `playlists`**：曲目变动后这份数据会过期，而
`list.removeTracks` 与 `scan.afterScan` 都要刷它。它们不能 import `playlists`（`playlists` 反过来
import 它们做 reload/playAll）→ 会成环，而全仓在此之前实测是 **0 环**。`library` 只依赖 ipc，
是这张图里唯一夹在中间的位置。保护：把环检测写成了脚本，改完复核仍是 0。

验收（实测）：`npm run check` 0/0、`build:ui` 通过且 CSS **35.28 kB**（目标 ≤ 36 kB）、
`test:e2e` **6/6** 绿（新增 1 条歌单链路）、`probe:ui -- --nav` 10 步无异常。
真窗口另测：元素默认 `h2` 13px/400/0 margin、`ul` 无圆点、`svg` block、骨架 46/216/1064 未变、
播放条 4 个 `.icon-solid` 的 `fill` 取到 `currentColor`、27 个 svg 无一零尺寸。

### B1 Token 与样式分层 ✅ 已完成（2026-10-26）

**做了什么**

1. `tokens.css` 重写成**只有变量**（145 行，含注释）：三条轴齐备
   （色彩 `--surface-1..4/--surface-head/--surface-input/--surface-hover/--surface-active/--surface-cover`、
   `--line/--line-strong`、`--text-1..3`、`--accent/--accent-weak/--accent-text/--accent-fg`、`--danger/--warn/--ok`、
   `--play-bg/--play-fg`、`--close-hover/--close-fg`；尺寸 `--space-1..6`、`--radius-sm/md/lg/pill`、
   `--control-h/--row-h/--titlebar-h/--sidebar-w/--player-h`；字体与动效 `--font-ui/--font-mono/--fs-xs..xl/--dur-*/--ease`）。
2. **299 处 `var(--x)` 机械重命名**（脚本跑 + 逐文件核对，0 残留）：`--bg*` → `--surface-*`、`--border*` → `--line*`、
   `--text*` → `--text-1/2/3`、`--accent-soft` → `--accent-weak`、`--*-height/width` → `--*-h/w`。
3. 新建 `styles/layout.css`（75 行）：`.app` 网格 + `.sidebar/.main/.page-content/.page-scroll/.page-pad/.player-dock` 的席位
   从 `base.css` / `pages.css` 集中过来。`base.css` 只剩元素默认 + 待迁移的侧栏零件样式。
4. 硬编码色清零：`.error`/`.notice.danger` 的 `rgba(255,122,122,x)` → `color-mix(in srgb, var(--danger) x%, transparent)`；
   `ImportPanel` 的 `var(--danger, #c33)` 去掉 fallback；开关拇指 `#fff` → `--accent-fg`；关闭键 `#e81123` → `--close-hover`。
5. 密度**同源**：`html[data-density]` 只改 `--row-h/--control-h/--space-5/6`；`lib/look.ts` 新增 `readRowHeight()`
   读 CSS 值，`ui.rowHeight` 是它的运行时镜像，`LibraryView` 不再写 `density === 'compact' ? 42 : 52`。

**实测**

| 门槛 | 结果 |
|---|---|
| `npm run check` | 0 error / 0 warning |
| `grep -E "\-\-(bg|border|text|accent-soft|fg-play|player-h...)([^-a-z]|$)"` | 0 命中 |
| `grep "rgba(255, *122"` | 0 命中 |
| 密度分档 | `data-density=compact` 时 `--row-h` 读到 `42px`（CSS 单点定义） |
| CSS 体积 | 35 278 → **39 690 B**（+4.4 kB 全部来自 token 轴重写与 `layout.css`，**不含 kit** —— 见下方更正）。B1 本身**不是**下降批 —— 它把散落的字面量换成了变量，压缩率反而略降；真正的下降在 B3b/B4 删旧全局 CSS 时发生 |
| JS | 239.41 → **239.72 kB** |

> **更正（B3b-1 实测，2026-10-26）**：上面把 +3.2 kB 记在 kit 头上是错的。B2 结束时 kit 只被 `KitView` 引用，而 `KitView` 是 `import.meta.env.DEV` 门控的 dev-only 页 —— 生产构建里整棵子树（含 14 个组件的 CSS/JS）被摇掉，`.k-btn` / `.k-field-box` / `.k-cover` 在 dist 里**一条都没有**。所以「kit +3.2 kB」当时是错的，实际是 0；这批把 kit 接进真实路径（Dialog/Field/Cover/Button）后才第一次进包。
>
> 已知遗留：`base.css` 尾部的侧栏零件样式（`.nav-item/.side-*/.sidebar-footer`）与
> `.sidebar` 自身的 border/background 还没进组件 —— 按 §5.5 的"结构就地改、视觉隔离试"，
> 它们随 B3b 换皮一起搬，避免同一批既改结构又改皮。

### B2 UI Kit 第一批（14 件）+ 演示页 ✅ 已完成（2026-10-26）

`components/ui/` 14 件，全部**自带 scoped 样式、只吃 token、不写全局类**（bits-ui 包装件用 `:global(.k-*)`，原因见 §4.5 坑 6）：

| 文件 | 来源 | 备注 |
|---|---|---|
| `Button.svelte` | 自研 | 4 variant × 2 size + `icon` snippet + `loading`（conic-gradient 转圈） |
| `IconButton.svelte` | 自研 | `label` 必填；`pressed`（aria-pressed）与 `active`（纯视觉）分开 |
| `Chip.svelte` | 自研 | 外层 span（可关 chip 内套 × 按钮，button 套 button 非法） |
| `Checkbox.svelte` | bits-ui `Checkbox.Root` | 不暴露 `onCheckedChange`（双翻反相，docs/23 实测） |
| `Switch.svelte` | bits-ui `Switch.Root`+`Thumb` | 受控 `checked` + `onCheckedChange` |
| `SegmentedControl.svelte` | bits-ui `RadioGroup` | `orientation="horizontal"`；`data-state` 是 checked/unchecked（原用 ToggleGroup，B3b-3 换核） |
| `Slider.svelte` | bits-ui `Slider` | `oninput`（预览）/ `oncommit`（进度 seek）分离 |
| `Progress.svelte` | bits-ui `Progress.Root` | `value=null` → 不确定态条纹 |
| `Field.svelte` | 自研 | `$props.id()` 生成 id，error/hint 共用一个描述位 |
| `Card.svelte` | 自研 | 恒 `<button>`（卡片只作可点入口） |
| `Cover.svelte` | 自研 | 四档 22/34/48/92 |
| `Skeleton.svelte` | 自研 | line / block / row，`prefers-reduced-motion` 停动画 |
| `EmptyState.svelte` | 自研 | icon/title/hint/action 四段 |
| `StatBlock.svelte` | 自研 | tabular 数字，tone 三态 |

**演示页** `views/KitView.svelte`（`?page=kit`，仅 `import.meta.env.DEV`）：
三态 Header + Toolbar + 14 件全状态。**不注册进 `AppPage`**（否则要处理持久化与侧栏入口）。

**实测**

| 门槛 | 结果 |
|---|---|
| `npm run check` | 0 error / 0 warning（含演示页） |
| 演示页 DOM 计数 | btn 9 / icon-btn 6 / chip 4 / check 4 / switch 2 / seg-item 4 / slider 2 / progressbar 3 / field 4 / card 3 / cover 6 / skeleton 10 / empty 1 / stat 3 / toolbar 1 / header 3 |
| 状态断言（CDP） | check `unchecked/checked/indeterminate`；switch `checked/unchecked`；seg `on/off`；slider range 151/238（64%）、41/238（17%）；progress fill 260/420（62%）+ 不确定态 126/420（30%）无 `aria-valuenow` |
| 运行时 | Console error 0 / 未捕获异常 0 / 4xx 0 |
| 亮色 | 截图目检无白字白底；kit 全部吃 token，切 `data-theme=light` 零分支 |
| CSS | kit 14 件 + 演示页合计 +3.2 kB（39.69 kB 总量，gzip 8.44 kB） |
| E2E | 6/6（`Header` 改名后选择器同步改为 `.main h1`） |

> **未做**（有意）：把 kit 替换进现有视图。按计划 B2 只建零件与演示页，
> 设置页/首页/列表页的 `.btn`/`.icon-btn` 替换放在 B3b（换皮批）一起做 ——
> 替换时视觉会变，和壳体换皮放同一批才能一次目检完。

### B3b 壳体换皮与播放条（原 B3）→ 已拆三刀，见本节末尾「B3b 壳体换皮（分三刀）」

> 下面这段是最初的一刀式描述，保留原文以便对照；实际执行拆成 B3b-1/-2/-3，
> 因为浮层那一刀会改事件与焦点，混在换皮里出问题分不清是谁的锅。

B3a 定好的结构**不再动**，这一批只换零件实现：
TitleBar（返回键与窗控换 `IconButton`、搜索框换 `Field`）/ Sidebar / PlayerDock 用 kit 重写（含 Popover、Tooltip）；
Header / Toolbar 内部改用 `Button`/`Chip`/`Cover`，对外的 props 一行不改。
侧栏加"专辑/艺人"入口与分组（**实测后推迟到 B4**：需要 group-by 查询，不是换皮）；电台入口移出；统计区改用 `StatBlock`。
播放条：EQ/倍速/输出状态三个 Popover 的**壳**（EQ 内容在 B5）。

验收：1280×800 / 1440×900 / 960×700 三档目检；`grep 'class="btn'` 在 shell 组件里降到 0；
B3a 的 6 条真窗口实测重跑一遍全过；Dock 在窄窗下的收纳规则。

### B4 主体页面收敛

#### B4-1 路由拆页 + 点击语义 ✅ 已完成（2026-10-26）

先拆两件互相纠缠的事：路由（决定"我在哪"）与点击（决定"点行是什么"）。

##### 路由拆页：`page` 4 值 → 8 值，`view` 变 getter

| 项 | 改动 |
|---|---|
| `AppPage` | `home\|nowPlaying\|radio\|settings` → **8 值**（+`library`/`recent`/`mostPlayed`/`playlist`），`isListPage()` 辅助 |
| `ui.svelte.ts` | 删 `libraryDetail` 与 `view` 可写字段；**`view` 改为 getter**（由 `page`+`playlistId` 派生，单真相）。新增 `enterList` / `setView` / `enterHome` 三入口，语义差异写在注释里 |
| `view.ts` | `LIBRARY_VIEW` / `RECENT_VIEW` / `MOST_PLAYED_VIEW` 单例常量 + `builtinView(kind)` —— getter 返回值**引用必须稳定**，否则下游按引用记忆的地方白算 |
| `nav.svelte.ts` | 快照从三字段（page/libraryDetail/view）减到两字段；`isCurrent` 从三字段组合退化为相等比较 |
| `schema.ts` | `UiState.page` 扩成 8 值 + `playlistId`；旧 `view`/`libraryDetail` 保留**只读**（迁移用，不再写回） |
| `App.svelte` | `{#if libraryDetail}` 化石删除；四个列表页共用 `LibraryView`（本来就是同一套列表，只是数据源不同） |
| 迁移 | `restore()` 兼容旧文件：`page !== 'home'` 走新形状；否则按 `libraryDetail`+`view` 换算。**换算错的表现是"重启回首页"，不报错**，所以 e2e 拿真实 localStorage 种子回放 |

**e2e 探针升级常驻 `route.spec.ts`（5 条）**：旧形状三种回放（歌单/最近播放/首页）+ 新形状往返落盘 + 八值路由逐个可达且**侧栏高亮恰好一个**。途中顺带修一个 mock 契约偏差：

> `mock/backend.ts` 的 `setUiState` 原来是 `{...旧, ...新}` **合并写**，真后端是 `setting_set` 整键**覆盖**
> —— 合并的话拆页前的 `libraryDetail`/`view` 在 mock 里永远洗不掉（e2e 实测抓到）。已改整体覆盖。

##### 点击语义：单击选中 / 双击播放 / hover 播放键（§6 决策 1 落地）

| 组件 | 新行为 |
|---|---|
| `TrackList` | 裸点=选中（清别的）、Ctrl=切换、Shift=连选（全部走 `selection.handleSelect`，修饰键分流在 store 内部）；**双击=播放**；Enter/Space=播放；行 hover 时序号换播放键（正在播放的行保留跳动条不放） |
| `TrackTable` | 同语义；紧凑表是预览性质，**不设 hover 播放键** |
| `QueuePanel` | 不动（队列单击即播是惯例） |
| 勾选框 | 语义不变（点=累加切换、Shift=连选），**但键盘修了一个真 bug**（见下） |

**⚠️ 真 bug：勾选框键盘操作与选择集分叉（既有问题，本批修）**

实测：焦点在 `Checkbox.Root` 上按 Space，`data-state` 翻成 `checked`，但 **bits-ui 只翻内部 state、不派发 click**
→ 包装层的 `oncheck` 没跑 → 选择集没动 → 下一次渲染被受控 prop 拉回，用户看到"勾上了又自己掉"。
更早的版本里行级 `keydown` 还会 `preventDefault + onplay`，等于"按空格勾选框却切了歌"。

修法：`Checkbox.Root` 上拦 `Space/Enter` → `preventDefault + stopPropagation + oncheck(index, event)`。
`KeyboardEvent` 同样有 `shiftKey`/`ctrlKey`，Shift 连选不丢。四处（TrackList 行内+表头、TrackTable 行内+表头）同修；
`handleCheck`/`toggle`/`toggleAll` 签名放宽为 `MouseEvent | KeyboardEvent`。

**实测（vite preview 产物 + Playwright，含临时探针验后删除）**

| 门槛 | 结果 |
|---|---|
| `npm run check` | 0 / 0 |
| `check:deps` | 0 环（94 模块 / **301** 条内部依赖） |
| e2e | **11/11**（smoke 6 + route 5；smoke 两处 `firstRow.click()` → `dblclick()`） |
| 单击/多选 | 裸点=单选且**不播放**（footer 仍"未在播放"、批量条出现）；Ctrl 追加两行同 selected；再裸点第三行清掉前两行 |
| 双击/hover | hover 出播放键且序号隐藏；点播放键起播（footer 出歌名时长）；双击换歌（薄荷变奏→薄荷的信） |
| 勾选框 | 鼠标点=累加（勾第二行第一行不掉）；键盘 Space 勾选后 `class` 与 `data-state` **同步**为 checked，且不触发播放、当前曲不变 |
| 首页紧凑表 | 单击 selected → footer 未播放；双击起播 |
| 运行时 | console error 0 / pageerror 0 |

| 产物 | B3b-3 后 | B4-1 后 |
|---|---|---|
| CSS | 46 844 B | 47 254 B（hover 播放键 + 单例视图注释无 CSS 变化，+410 B 为 `.play-btn` 与迁移样式） |
| JS 合计 | 520 856 B | **526 982 B**（index 407 718 + settings 95 265 + mock 19 031；+6.1 kB 为路由 getter/迁移/交互逻辑） |

**剩余（归 B4-3）**：搜索结果页、专辑/艺人页（配 `library_facets`）。
ListPage 与 DataTable 的取舍见 §5 B4-2 末尾的决策修订。

#### B4-3 聚合浏览与搜索结果页 ✅ 已完成（2026-10-26，三刀：后端 / 浏览+详情 / 搜索页）

| 刀 | commit | 内容 |
|---|---|---|
| **B4-3a** | 后端 | `aiya-data` 新增 `facets.rs`：`list_albums`/`list_artists`（GROUP BY 聚合：id/名称/封面/曲目数/总时长/年份/专辑数），只含**实际有曲目**的条目（JOIN tracks，空专辑/无主艺人不出现——点进去是空列表没有意义，与 `library_stats` 的表行数口径允许不一致）。按 `*_norm` 排序、LIKE 按字面转义、上限 500（`FACET_LIMIT`，聚合毫秒级，不需要游标）。**宿主拆两条命令** `library_albums` / `library_artists`，不是草案的一条 `library_facets(kind)`：仓库派发表是命令一一对应的（mock/契约测试按名字对账），一条命令收 kind 再返回带 tag 的 enum，等于把"参数校验+判别联合"两层复杂度压到两端调用点上。ts-rs +2 类型（48），mock 从内存曲目聚合同一形状。**审查补丁（2026-10-26）**：响应从裸数组改为 `AlbumPage`/`ArtistPage`（`items` + `total`）——截断不静默，`total > items.length` 时宫格页显示"共 N 个，仅显示前 500"提示；total 用"全量取回后 Rust 侧截断"实现，一条查询出两个数字，不用复制 WHERE 跑第二条 COUNT |
| **B4-3b** | 浏览+详情 | `AppPage` 8 → 12 值：`albums`/`artists`（宫格浏览页，非列表页）+ `album`/`artist`（详情页，列表页）。**详情页参数复用 `albumId`/`artistId` 筛选字段**，不加 `facetId`：详情页的曲目列表本来就是"曲库查询 + albumId 过滤"（`list.query()` 已从这两个字段取筛选），复用 = 查询/返回栈快照/UiState 持久化三处**零改动**；新字段则三处都要跟着加。`ui.enterFacet()` 与 `enterList` 同清理语义但不能先调它（enterList 会清掉要设的参数）。`stores/facets.svelte.ts`（聚合数据 + 详情页头部查单条）；`views/BrowseView.svelte`（两模式一组件，页内过滤 200ms 防抖 + untrack 防击键触发 effect 直拉）；LibraryView 详情页头部（封面/年份/曲目数/总时长），tab 与筛选 chip 只在内置视图显示（详情页的"筛选"就是页面本身，chip × 会清掉路由参数）。右键菜单「按此艺人/专辑筛选」→「查看艺人/专辑」直达详情页（docs/24 §2 B4 的"半成品→[新]升级"落地）。侧栏一级导航 6 → 8 项（+专辑/艺人） |
| **B4-3c** | 搜索页 | `AppPage` 12 → 13 值（+`search`）：顶栏搜索落**独立结果页**，不再寄居曲库列表（docs/24 §2 B3"独立结果页"落地）。曲目列表与曲库页**同一条查询路径**（isListPage 含 search、view getter → LIBRARY_VIEW），20 个 props 的 TrackList 接线零复制——搜索页就是 LibraryView 的又一个头部分支 + 曲目列表上方两个聚合卡片段（专辑/艺人各前 8 张，独立 200ms 防抖，`facetQuery` 锁查询词防旧段闪现）。空词显示"输入关键词"提示而不是全库列表 |

**两个实现要点**：

1. **聚合排序与 limit**：按名称排（`title_norm`/`name_norm`），limit 500。宫格页一次全画，
   超过 500 的曲库用页内搜索收窄——不为浏览页做游标分页。
2. **搜索页的卡片段与曲目查询防抖独立**：曲目 120ms（list store）、聚合 200ms（组件内）。
   合成一个防抖要么让曲目等聚合（200ms 才开始查），要么聚合跟着 120ms 每击键刷 IPC ——
   两边节奏本来就不同，分开写才是对的。

**实测（B4-3 三刀合计）**：

| 门槛 | 结果 |
|---|---|
| `cargo test --workspace` | 全绿（aiya-data 93 条含 facets 7：聚合/空专辑跳过/搜索/通配字面/空白过滤） |
| `ipc_contract` | 5/5（新命令双向对账；先注册后补 UI 时它确实红了，正是这层测试的用途） |
| `npm run check` | 0 / 0 |
| `check:deps` | 0 环（100 模块 / 330 条内部依赖） |
| `gen:types:check` | 48 类型一致 |
| e2e | **16/16**（smoke 6 + route 8 + batch 2；route +3：宫格→卡片→详情→返回、艺人详情行含艺人名、搜索三段+返回词不丢） |
| 产物 | CSS 47.2 → **50.9 kB**、JS 527 → **539 kB**（+browse 视图 +侧栏两入口 +搜索页两段） |

#### B4-2 抽公共三刀 ✅ 已完成（2026-10-26）

原计划"ListPage + DataTable + 抽公共"一刀，实测拆三刀（`22c1f09` / `0fc377b` / `a9dc666`）：

| 刀 | 内容 |
|---|---|
| **B4-2a** | `lib/create-virtual-window.svelte.ts`（scroller/视口测量/区间，TrackList+QueuePanel 各抄过一遍的 30 行水管收敛）；`lib/row-selection.ts`（Mods + 意图分流 + ID 版 `applyIdSelection`，与下标版共享同一套分流规则） |
| **B4-2b** | `stores/batch.svelte.ts`：批量条全应用**只剩 AppShell 一个渲染点**；批量动作（play/enqueue/addTo/remove/clear）**一份实现**，替代 `list.batchPlay`/`batchEnqueue`/`playlists.addSelection` + HomeView 本地四函数的两份拷贝。首页三段的选择状态从 HomeView 挪进 store（`selectedIn`/`setSection`） |
| **B4-2c** | kit 第 15 件 `CheckCell`：包装层点击 + 受控 Checkbox + **Space/Enter 键盘防护的唯一实现** —— B4-1 修的分叉防护此前复制在四个调用点，最容易漂移。类名统一 `.check`/`.tt-check`/`.tt-box` → `.k-check`/`.k-box` |

**实测要点**：
- `createVirtualWindow` 不提供 `onScroll` 回调选项 —— prop 内联箭头每次渲染都是新实例，
  创建时捕获拿到的是首次实例（行为对但来源隐蔽）；组件在自己的 `onscroll` 里先调
  `vw.handleScroll()` 再取 `vw.scrollTop`，依赖显式
- `scroller`/`scrollTop` 用显式 getter/setter 暴露，DOM 节点不进 `$state` 代理
- TrackTable 的 Shift 无锚点回退与主列表不同（回退成 toggle，不是焦点行）——
  因为紧凑表没有键盘焦点概念，差别写在 `applyIdSelection` 注释里
- 批量条 `active`/`count` 只认**当前页**的来源：跨页导航另一来源自然失活，与旧行为一致

**验收（修订）**：

| 原条目 | 结果 |
|---|---|
| 批量动作只有一处实现 | ✅ `stores/batch.svelte.ts` |
| TrackList+TrackTable 行数下降 ≥40% | **修订为"共享实现去重"**：TrackList 348→322、TrackTable 223→187（−14%），降幅来自水管/选择语义/勾选格四处抽取；**整表合并不做**（理由见下） |
| 6 个列表页视觉同构 | ✅ 结构性达成：四个列表路由共用 `LibraryView`（B4-1 拆页），首页三段共用 `TrackTable` |
| 双击播放与单击选中互不串 | ✅ e2e/batch.spec.ts + smoke 双击用例 |

**DataTable 整表合并：不做（决策）**。`TrackTable` 头注释记录的设计理由仍然成立：
两边共用的是**交互模型**而不是组件——TrackList 绑死 `TrackWindow`（游标分页+虚拟滚动+
拖拽排序+行高密度），TrackTable 是"一次查 8 条"的快照表。硬合并得到的是
`virtual?`/`rows?` 双数据源 + 双选择键型 + 条件模板的"两个组件假装一个"。
真正要防漂移的是**语义**（点行/勾选/键盘/批量），已由四处共享实现覆盖：
虚拟化水管（B4-2a）、选择分流（B4-2a）、批量动作（B4-2b）、勾选格（B4-2c）。

**ListPage 细壳：不做（决策）**。B4-1 拆页后四个列表路由共用 `LibraryView`，
它就是那个"细壳"；再加一层只有转发。§3.2 的布局契约（页面自己拼三段）维持原样。

### B5 播放区与 EQ（含后端子批）✅ 已完成（2026-10-26，两个提交）

**B5b 后端**（先做，协议 v5 → v6）：
- `aiya-audio/src/eq.rs`：RBJ cookbook 直 I 型 biquad ×10（低架 / 8 峰值 / 高架），**无外部依赖**。
  增益 ±12 dB、pregain ±12 dB，入口与引擎各 clamp 一次（持久化的就是生效值）。
- **全 0 dB 或关闭 = 硬旁路**：逐样本直通。旁路证明（docs/24 验收）不靠浮点巧合——
  RBJ 公式在 A=1 时数学上退化为恒等，但浮点等式依赖"逐项相等"，硬旁路是零风险承诺。
- 插入点：`resample_take` 内 **插值后 → EQ → 音量斜坡**（提升增益不顶爆输出电平）；
  频谱 tap 改吃**滤波后**单声道（频谱反映耳朵听到的）。
- **系数热切不重建延迟线**：切预设只重算系数，延迟线保留（重建会在切换点引入直流瞬态）。
  20 次连切预设的测试断言输出有界且无 NaN（自动化的"无爆音"代理指标）。
- 超 Nyquist 频段跳过（32 kHz 下 16 kHz 段公式出 NaN，clamp 救不了）。
- `SetEq(EqParams{bands,pregain,enabled})` + `EngineEvent::EqApplied{token,params}`，
  **EqApplied 带 clamp 后生效值**，UI 以它为准。`aiya-audio-host` 已显式重建（协议改动铁律）。
- `player_set_eq`（Json 手动解析 + clamp）+ `player_get_eq`（面板回读）。
  **持久化独立键 `eq`，不进 `app_settings`**：EQ 滑杆是 300ms 一次的高频连调，
  设置页是低频整体写，分键天然互不覆盖（diff 两边字段的方案已否——复杂度换不来正确性）。
- sidecar 崩溃恢复序列在 `SetOutputConfig` 之后**无条件回放 `SetEq`**（设置项，与装载无关）；
  `LoadExit` 携带 eq 记账（与 rate/cfg 同一套账法），play_null 只记账不滤波。

**B5a UI**：
- `stores/eq.svelte.ts`：回读一次 + 300ms 防抖下发，**以返回的生效值回写**
  （拖到 +20 被 clamp 成 +12 时画面跟着回弹）；7 预设 + 命中检测。
- `EqPanel.svelte`：kit Switch（开关）+ kit Menu（预设下拉，点击坐标锚点，命中打 ✓）
  + 10 段竖向滑杆 + 前置增益。kit `Slider` 加 `orientation="vertical"`（bits-ui 原生支持）。
- 正在播放页三块可切（kit SegmentedControl：歌词/频谱/均衡器），一次只渲染一块、独占整行；
  选中值存页面本地——纯 UI 偏好不进设置 schema。
- **队列抽屉换 kit Popover**：触发键 = 播放条队列键，aria-expanded / Esc / 外点关闭 /
  焦点还原全归 bits-ui；删 QueuePanel 的 `svelte:window` 两件套与
  `data-queue-trigger` 自防（双触发问题由 Popover 的受控模型天然消除）。
  触发键用裸 button + scoped 样式——IconButton 是固定签名组件，接不住 bits-ui 的
  props 展开（rate-chip 已有同款先例）。kit `Popover` 加 `contentClass` 附加类。
  几何从 `.player-dock` 绝对定位迁到 `.queue-float`（portal 后旧锚点失效）。
  LyricsPanel / SpectrumPanel 的关闭键同步换 kit IconButton——全局 `.icon-btn` 清零
  （player.css 里只剩定义，随 B6 删）。

实测：aiya-audio 61 绿（eq 5 条：旁路位级一致 / +6 dB 频响≈2× / 20 次连切有界 /
超 Nyquist 跳过 / clamp）、workspace 全绿、ipc_contract 5/5、gen:types 49 类型一致、
check 0/0、check:deps 0 环（102 模块）、e2e **18/18**（+nowplaying 两条常驻：三块可切、
抽屉 Esc+焦点还原）；CSS 52.6 kB / JS 423 kB。

### B6 设置与浮层 + 收尾

设置页改"左分组导航 + 右内容"，22 项全部用 kit；Dialog/Menu 全量替换（删 `ContextMenu.svelte`/`ConfirmDialog.svelte`/
`PromptDialog.svelte`/`PlaylistPicker.svelte` 四个独立实现，收敛到 `ui/Dialog`+`ui/Menu`）；
E2E 补到 8–10 条（覆盖：拆页路由、批量、设置分组、kit 演示页、队列抽屉、EQ）；
可访问性与键盘巡检表（docs/19 §5 逐条打勾）；更新 docs/19 的作废标记与 docs/23 的补充。

验收：`npm run verify`（test + check + build + gen:types:check）全绿；`test:e2e` 10/10（B0 后基线 6/6）；真窗口 12 步走查表。

#### B6 实测（2026-10-26 深夜，三刀：B6a 零件+删壳 / B6b 设置左栏 / B6c 巡检+EQ 用例）

**B6a**：kit 第 16/17 件 `InlineError`/`Toast` 自研（零件表 20，错误分「可重试/阻断」——区分不是颜色
而是**有没有动作按钮**）；AppShell 两处 `.error`、OverlayHost `.toast`、HomeView loadError 全换零件，
全局 `.error`/`.toast` 样式删除。`ConfirmDialog`/`PromptDialog`/`PlaylistPicker` 三个壳物理删除
（B3b-1 已收敛语义，本批删文件），五个对话框分支以 `ui/Dialog` 组合内联进 OverlayHost——
输入框文本收拢为一个 `promptText` 状态、打开/切换时按语义初始化（原壳里的 `untrack` 初始化花招
换成 effect，Svelte 5 没有「`$state` 解构出 setter」这回事，第一版内联试过这个不存在的语法，被 check 拦下）。

**B6b**：设置页 Toolbar tab → 左分组导航（148px sticky 列 + scroll-spy，≤720px 退回顶部横条）。
**所有区块仍常驻挂载**——扫描/beets 任务进度不能因切组卸载，这是单页布局的硬约束；
`aria-current` 与视觉高亮同源，选中指示用左侧 2px accent 边线（不只靠明暗，docs/19 §5）。

**B6c 可访问性巡检表（docs/19 §5 逐条，代码证据为准）**：

| 条目 | 状态 | 证据 |
|---|---|---|
| 状态不只靠红/绿 | ✅ | InlineError 靠"有无动作按钮"分类；rail 选中用边线+字重 |
| 全键盘可达、浮层关闭还原焦点 | ✅ | bits-ui 五原语；e2e 断言 Esc 后 `trigger` focused（队列/倍速/对话框三处） |
| 焦点环统一不永久移除 | ✅ | base.css `:focus-visible` 全局规则，无 `outline:none` 裸写 |
| 装饰图标 aria-hidden、图标按钮有名称 | ✅ | Icon.svelte 默认 `aria-hidden`；IconButton `label` 必填 |
| toast polite / 阻断错误才 assertive | ✅ | B6a 落地：Toast `polite`，InlineError `assertive`（唯一挂点） |
| `user-select:none` 不扩散 | ✅ **巡检修出真缺口** | 此前零例外——歌词/路径/诊断全选不中；修法：`.selectable` 工具类 + 歌词纯文本/错误详情/诊断值/beets 提示放开（同步歌词行是 button，让位给 seek 手势） |
| 快捷键录制屏蔽全局键 | ✅ | docs/23 §6 实测（录制中空格记键不起播）；capture store + keyboard.ts 让路 |
| `prefers-reduced-motion` | ✅ | base.css 全局 + Toast/骨架等组件级 |

**E2E**：B0 基线 6 条 → **19 条**（smoke 7 + route 8 + batch 2 + nowplaying 2，超计划 8–10 条；
B6 补设置左栏 scroll-spy 改写与 EQ 预设交互两条）。`check` 0/0。

#### B3b 壳体换皮（分三刀）

原计划一刀做完，实测后拆成三条 —— 理由见每条末尾的"为什么单独提交"。

#### B3b-1 浮层四件 ✅ 已完成（2026-10-26）

| 零件 | 内核 | 取代了什么 | 白捡的能力 |
|---|---|---|---|
| `ui/Menu.svelte` | `DropdownMenu` + 0×0 锚点 | `components/ContextMenu.svelte`（已删） | 键盘导航（↑↓/Home/End/首字母）、碰撞翻转、焦点进菜单 |
| `ui/Dialog.svelte` | `Dialog` + `FocusScope` | 三个对话框各写一遍的 backdrop + 手写焦点环 | 焦点陷阱 + 还原、Esc、滚动锁、`aria-modal` |
| `ui/Popover.svelte` | `Popover` | PlayerBar 里 `box.contains` + `window` Esc | 碰撞翻转、`aria-expanded`、Esc 后焦点回触发点 |
| `ui/Tooltip.svelte` | `Tooltip` + 顶层 `Provider` | TitleBar 四个 `title` 属性 | 统一 400ms 延迟、相邻共享跳过延迟、主题化外观 |

调用点迁移：`OverlayHost`（两个菜单）、`ConfirmDialog` / `PromptDialog` / `PlaylistPicker`（三个对话框，共 −96 行手写逻辑）、
`PlayerBar` 倍速面板（手写开关 → `Popover` + `Slider`）、`TitleBar`（返回键 + 三窗控 → `Tooltip`）。

**实测（CDP，dev 预览 + mock 传输）**

| 断言 | 结果 |
|---|---|
| 菜单落点 | 右键 (346, 228) → 菜单 rect `left 346 / top 230`（`sideOffset=2`），190×254，8 项、2 项禁用 |
| 菜单键盘 | 打开后焦点在菜单内容（`activeTag=DIV`）；按 ↓ → `[data-highlighted]` = 播放 |
| 菜单关闭 | Esc → `.k-menu` 消失；焦点回 BODY（锚点已卸载，见 §4.5 坑 4） |
| 对话框 | `aria-modal=true`、`aria-labelledby` 有值；焦点落 `input.k-field-input`（PromptDialog 的 `autofocus`） |
| 对话框关闭 | Esc → 关闭且焦点**还原到侧栏「新建歌单」按钮** |
| Field 关联 | `label[for]` === `input#id`（`k-field-c32` / `k-field-c32`）—— 顺手修了 B2 遗留：`<input {id}>` 用的是未填的 prop，label 的 `for` 一直指向不存在的 id |
| 倍速面板 | chip (726,716) → 面板 (642,615)，`bottom ≤ chip.top`，`aria-expanded=true`，内含 `.k-slider`（`aria-valuenow=1`） |
| 面板关闭 | Esc → 消失且焦点回 `.rate-chip` |
| Tooltip | hover 最大化键 → 400ms 后 `.k-tip` 文本「最大化」，路径为"顶栏无空间 → 翻到下方" |
| 运行时 | console error 0 / 未捕获异常 0 |

**体积**（这一条要单独说，因为它推翻了我上一批的记账）

| 产物 | B2 之后 | B3b-1 之后 | 差 |
|---|---|---|---|
| CSS | 39 692 B（gz 8.44 k） | 47 134 B（gz 9.46 k） | +7.4 kB |
| JS | 354 012 B | 508 696 B（gz 152 k） | +155 kB |

两笔都是有原因的、不是泄漏：
- **CSS +7.4 kB**：`.k-btn / .k-field-box / .k-cover / .k-icon-btn` 等 88 条 scoped 规则**第一次进生产包**。B2 时 kit 只被 dev-only 的 `KitView` 引用，整棵子树被摇掉（dist 里 `.k-btn` 一条都没有），所以"kit +3.2 kB"当时记错了，见 B1 节的更正。
- **JS +155 kB**：bits-ui 的浮层栈（Dialog/Portal/FocusScope/DismissibleLayer/ScrollLock/Popover/DropdownMenu + floating-ui）第一次进包。桌面应用从本地加载，这笔是**一次性的、无网络成本**；换来的是焦点陷阱、碰撞翻转、ARIA 状态机这些手写容易错的东西。

> **为什么单独提交**：这一刀会改**事件与焦点**（菜单从手算坐标改到浮层、对话框从自有 backdrop 改到 Portal）。
> 和壳体换皮放一起，出问题时无法判断是"换皮错了"还是"浮层错了"。

#### B3b-2 壳体换皮 ✅ 已完成（2026-10-26）

`components/shell/` 归位：`TitleBar` / `Sidebar` / `PlayerBar`（`git mv`，import 深度改一层）。

| 件 | 改了什么 |
|---|---|
| `TitleBar` | 返回键 → `IconButton`（**不套 Tooltip**：IconButton 的 label 已产出原生 title，两者并存会弹两次）；窗控三个仍是裸 button + `Tooltip`（要 44px 通栏悬停，不能是圆角方） |
| `Sidebar` | **零 prop**（自己读 store、自己调 `nav.go`）；一级导航 6 项：首页 / **曲库** / **最近播放** / **常听** / 正在播放 / 设置；**电台移出**；歌单封面 → `Cover xs`；+ 号 → `IconButton`；底部统计 → 三个 `StatBlock` 并排 |
| `PlayerBar` | 两个 `<input type=range>` → `Slider`（进度 `oncommit`、音量 `oninput`）；八个图标键 → `IconButton`（圆形，`solid` 只给播放键）；封面 → `Cover md` |

**没换的两处，理由写进代码注释**：
- **搜索框不换 `Field`**：`data-tauri-drag-region="false"` 必须挂在胶囊那层 div 上，而 `Field` 的内层盒子
  由它自己渲染、kit 不透传任意 DOM 属性。为此给 kit 开"透传属性"的口子，等于放弃封装。
- **倍速 chip 不换 `Chip`**：它必须是 `Popover.Trigger` 的宿主（要展开 bits-ui 给的 props），
  `Chip` 不接 rest props。形状/字号本来也只有这一处用。

**实测**

| 门槛 | 结果 |
|---|---|
| `npm run check` | 0 error / 0 warning |
| `check:deps` | 0 环（94 模块 / 289 条依赖） |
| e2e | **6/6**。两处选择器随之更新：返回键改 `getByRole('button', {name:'返回上一页'})`（换 IconButton 后不再自带 `.tb-back` 类），队列键改 `name: /播放队列/`（label 带首数） |
| 侧栏（1280×800，产物预览） | 6 项导航、`首页` 高亮、4 个歌单（封面实测 **22px**）、统计 `233/23/12`、`grid-template-columns` 三列各 ≈59.7px、歌单区 `scrollHeight ≤ clientHeight` |
| 侧栏（960×700） | 无横向溢出、歌单名省略号、统计仍三列 |
| 播放条 | 高 **92px**、9 个 button；`aria-label` 齐全（随机播放/上一首/播放/下一首/顺序播放/正在播放/播放队列（N 首）/静音）；两个滑块 `aria-valuenow`：进度 `0/236347`、音量 `0.85/1`；封面 48px；音量槽 92px；时间 0:00 / 3:56 |
| 窄窗 960 | 播放条 `scrollWidth === clientWidth`（无溢出），进度槽 457px |
| 顶栏几何（1280） | 品牌 12..111 / 返回 **119..143**（原 119..147，IconButton sm 是 24）/ 搜索 **151..511**（原 155..515）/ 窗控 1148..1280 |
| 亮色 | 目检：导航文字、统计数字、播放键（深底浅图标）、两条滑块轨道、搜索框边框均可见 |
| 运行时 | console error 0 / pageerror 0 |

| 产物 | B3b-1 后 | B3b-2 后 | 差 |
|---|---|---|---|
| CSS | 47 134 B（gz 9.46 k） | 47 665 B（gz 9.53 k） | +0.5 kB（播放条 200 行全局样式搬成 scoped，注水后又瘦回去） |
| 全局 CSS 行数 | `player.css` 482 | **282**（−200：播放条段全部搬进组件；`base.css` 又少了 130 行侧栏段） |

> **还留在全局的**：`.icon-btn`（QueuePanel 在用；SettingsPanel / ImportPanel 已在 B3b-3 换掉）、
> `.cover`（TrackList / QueuePanel 在用）、`player.css` 里的队列/歌词/频谱面板段（B5 重写）。
> `layout.css` 保留 `.player-dock` 的 `z-index` —— 那是"播放条压在内容之上、队列抽屉从它沿口升起"的层级约定，
> 属四区关系，不是播放条自己的皮。

**专辑 / 艺人入口不在本刀**：两者需要"按专辑/艺人聚合"的查询（现在只有 `artistId`/`albumId` 单值筛选
`TrackQuery`，没有 group-by 命令）。做入口等于做 B4 拆页的数据层，不在换皮批里夹带。

#### B3b-3 Content 内部与设置页 ✅ 已完成（2026-10-26）

三处：`Header`/`Toolbar` 内部件、五个视图/工具条里的 `.btn`、设置页三个成套控件。
对外 props 一个没动，调用方零改动。

| 文件 | 改了什么 |
|---|---|
| `content/Header.svelte` | 92px 封面 div → `Cover lg`；删 `.ch-cover` 两条规则 |
| `content/Toolbar.svelte` | 筛选 chip 的 span + × → `Chip`（`onclear` 走它自己的 ×）；删 `.pt-chip` / `.pt-chip-x` / `:hover`（567 B） |
| `LibraryView` | 10 处 `.btn` → `Button`（含 `primary` / `danger` / `snippet icon`） |
| `HomeView` / `NowPlayingView` / `LibraryTools` | 各 1–2 处 → `Button` |
| `BatchBar` | 四个 `.btn` → `Button size="sm"`；`.icon-btn` → `IconButton` |
| `ImportPanel` | 5 处 `.btn` + 1 处 `.icon-btn`；`.apply-btn`（`align-self`+小字号）→ `.apply-row` 包裹 + `Button sm` |
| `SettingsPanel` | **9 个 `.switch` → `Switch`**、**6 个 `.segmented` → `SegmentedControl`**、4 处 `.btn` → `Button`（清空历史用 `danger`） |
| `styles/overlays.css` | 删 `.segmented` / `.switch` 全套（**−928 B**） |
| `styles/shell.css` | 删 `.btn` 全套（**−560 B**） |

**给 `SegmentedControl` 加了 `icon` 插槽**：主题项的月/日图标不能丢。图标不进 `options`
（那是纯数据，塞 Snippet 进去每次渲染都新建片段），而是 `Snippet<[string]>` 按 `value` 回查。

##### ⚠️ 关键发现：分段控件的内核必须是 `RadioGroup`，不是 `ToggleGroup`

原实现用 `ToggleGroup type="single"`，注释还写着"方向键做对了"。实测推翻：

| 操作 | 期望 | 实测（ToggleGroup） |
|---|---|---|
| `ArrowRight` | 换选中 | 只移焦点，`data-state` 不变（`on`/`off` 都不动） |
| `Space`（在已选中项上） | 保持选中 | **两项全变 `off`** —— 而设置里还是旧值 |

`Space` 那条是真 bug：ToggleGroup 的 `onValueChange` 在取消选中时回 `undefined`，
原代码"吃掉 undefined 不写回"，于是**画面说没选、状态说选了**，两者分叉。
自研 CSS 只认 `[data-state='on']`，两项都不亮，用户看不到当前位置。

换 `bits-ui/RadioGroup` 后（语义同构：单选永不为空）实测：

| 操作 | RadioGroup 结果 |
|---|---|
| `ArrowRight` / `ArrowLeft` | 焦点**和**选中一起移动（`checked` 从 标准 → 紧凑 → 标准） |
| `Space`（在已选中项上） | 保持 `checked`，不再取消 |
| 点另一项 | `data-state` 翻转 + `onchange` 写回设置 |

三条硬约束记在组件注释里：
1. `orientation="horizontal"` **必须显式给**，默认 `vertical` 时左右键不生效；
2. `data-state` 是 `checked`/`unchecked`（**不是** ToggleGroup 的 `on`/`off`），CSS 选择器跟着改；
3. `RadioGroup.Item` 的 `disabled` 是 `boolean | null | undefined`，根上的 `disabled` 不会下传。

**实测（1280×800，`vite preview` 产物 + Playwright）**

| 门槛 | 结果 |
|---|---|
| `npm run check` | 0 / 0 |
| `check:deps` | 0 环（94 模块 / **302** 条内部依赖，+13） |
| e2e | **6/6**。一处选择器更新：`button.switch[aria-label="频谱可视化"]` → `getByRole('switch', {name})` |
| 开关 | **9 个**，全部 `role="switch"`、38×22px（与旧 CSS 逐像素一致）、`aria-checked` 与 `data-state` 一致；点「频谱可视化」`unchecked → checked` 且写回设置 |
| 分段控件 | **6 组** `role="radiogroup"`：主题 / 列表密度 / 输出模式 / 缓冲 / 时钟 / 频谱风格；每组恰好一个 `checked` |
| 主题段 | `svg` 数量 **2**（月/日图标保住了）；点「浅色」后 `html[data-theme]` = `light` 且 `data-state` 同步翻转 |
| 几何 | `.k-seg` 86×32、项 39×26（`md`）；主题段 122 宽；行高 62；控件右边缘与行右边缘差 **0**、垂直居中偏移 **0.5px**（取整） |
| 诊断页按钮 | 三个 `Button sm` 高 **28px** |
| 运行时 | console error 0 / pageerror 0 |

| 产物 | B3b-2 后 | B3b-3 后 | 差 |
|---|---|---|---|
| CSS | 47 665 B | **46 844 B**（gz 9 479） | **−821 B**（删掉 1 488 B 旧规则，换来的 `:global` 注水 667 B） |
| JS 合计 | 510 129 B | **520 856 B**（index 406 560 + settings 95 265 + mock 19 031） | +10.7 kB |

> **+10.7 kB 的来源要说清**：`RadioGroup` + `Switch` 是**第一次进生产包**。B2 之后这两个零件
> 只被 `KitView` 引用，而 `KitView` 由 `import.meta.env.DEV` 门控、整棵被摇掉 —— 所以 B2 那批
> "kit 让包变大"的记账是错的（已在 v4 更正）。真正的体积代价在**开始用**的时刻付，不在**写组件**的时刻付。

**还留在全局的**：`.icon-btn`（`LyricsPanel` / `QueuePanel` / `SpectrumPanel` 在用，归 B5）、
`.cover`（`TrackList` / `QueuePanel` 在用，归 B4/B5）、`.chip` 的其他变体、`player.css` 面板段。
`settings-*.js` 独立 chunk 说明设置页已被路由懒加载分开（B4 拆页前就存在）。


## 5.5 改动方式评估：就地改，还是隔离重做？（v2 新增）

本节回答一个决策问题：**基于现状就地改，还是另起一棵组件树隔离重做**。先给结论，再给依据。

### 结论

**结构就地改（A），视觉隔离试（B'）。** 三条要求里没有一条是"实验新视觉"，它们全是**同一棵组件树里的
职责重新分配**；而真正需要"先看效果再定"的部分（PageHeader / PageToolbar 的**长相**）用 `?page=kit`
演示页隔离，不进主界面。理由逐条如下。

### 一、量化：三条要求到底动多少

| 涉及 | 文件 | 现在 | 改后 | 性质 |
|---|---|---|---|---|
| 顶栏 | `TitleBar.svelte` | 69 | ~160 | 吸收搜索 + 返回（**重写**） |
| | `TopBar.svelte` | 36 | 0 | **删除** |
| | `styles/pages.css` `.titlebar*` 段 | 81 | 0 | 迁进组件 scoped |
| | `styles/shell.css` `.topbar` `.search*` 段 | ~50 | 0 | 同上 |
| 版式零件 | `PageHeader.svelte` / `PageToolbar.svelte` | — | ~135 + ~120 | **新建** |
| | `ViewHeader.svelte` | 88 | 0 | **删除**（逻辑并入 LibraryView） |
| | `styles/shell.css` `.view-*` `.filters` 段 | ~83 | 0 | 同上 |
| | `styles/pages.css` `.page-heading` 段 | ~35 | 0 | 同上 |
| 导航 | `stores/nav.svelte.ts` | — | ~200 | **新建** |
| | `Sidebar.svelte` | 95 | ~95 | props → store（行数不变，**接口简化**） |
| | `AppShell.svelte` | 133 | ~110 | 删 `navigate` / TopBar / sidebar props |
| 落位 | `HomeView` / `LibraryView` / `SettingsPanel` | 247 / 141 / 568 | +30 / +50 / −20 | 头部替换 |
| 调用点 | `keyboard.ts` ×2、`playlists.svelte.ts` ×1、PlayerBar 回调 ×1 | — | — | `list.openXxx` → `nav.go` |
| 测试 | `e2e/smoke.spec.ts` | 全红 | 4/4 绿 | 选择器改 testid |

合计：**新建 ~450 行、删除 ~290 行、净变化 ~+160 行，触及 15 个文件**。没有一行是"新功能"，
全部是把已有代码从 A 处搬到 B 处 + 加一个 200 行的导航 store。

### 二、为什么"隔离重做"在 Svelte 里不便宜

隔离（新建 `components/shell2/` + 开关切换）在 React 里靠"两个 root + 两套 CSS Module"很自然，
这里有三处硬障碍：

| 障碍 | 具体 | 绕过它的代价 |
|---|---|---|
| **全局类名抢同一个根** | `.app` 的三行网格、`.sidebar`、`.main` 是 `base.css` 里的**全局选择器**，两套壳都要挂在 `document.body` 的同一个 `#app` 上 | 加前缀（`.app2`）→ 整套布局 CSS 复制一份；或 iframe → 拖拽区、窗口控制、CDP 探针全部要重做 |
| **样式会互相命中** | `.page-heading` / `.view-head` / `.btn` 这些类名新旧壳共用，旧的全局规则会命中新壳的 DOM | 给新壳单独一份 entry CSS → 维护两份 token 引用，改 token 要改两处 |
| **状态是共享的** | `ui.page` / `libraryDetail` / `uiState` 持久化 / `boot.ts` 启动恢复 都绑在这三个字段上，两套壳必然共用同一份状态 | 要真隔离就得连状态一起隔离 = 重写整个前端 |

第三条是决定性的：**隔离的边界在状态层，不在视图层**。视图能隔离、状态不能，那就只是"两个视图抢一份
状态"，切换开关时必然要同步状态，等于多一层转译。

### 三、"隔离"什么时候才对

| 场景 | 该不该隔离 | 本项目对应 |
|---|---|---|
| 换一套**设计语言**（新配色/新密度/新圆角体系） | **该**，但用演示页隔离，不用第二棵组件树 | `?page=kit`（B2 已规划）—— 一屏看全 22 件全状态，改 token 在这里目检 |
| 换**技术栈**（引入组件库替换手写 CSS） | 该，需要 `ui-next/` 并行 + 路由开关 | 已否决（§4.1：语义 CSS + 自建 kit） |
| **职责重分配**（谁调用谁、在哪一层） | **不该** | 本轮三条 |
| 不确定能不能做成的**技术验证** | 该，spike 分支 | 无（三条都是确定性改造） |

### 四、就地改怎么控风险：拆成 5 个"每步都能跑"的小步

隔离的最大卖点是"中途不会坏"。这个卖点在原地改里可以用**更细的步**拿到，而且不用维护双份代码：

| 步 | 内容 | 完成时能否运行 | 能否单独回退 |
|---|---|---|---|
| **S1** | `nav.svelte.ts` + 全部调用点改 `nav.go`（`go()` 内部转调现有入口） | ✅ 视觉零变化 | ✅ 删一个文件 + 还原 10 处调用 |
| **S2** | 宽 TitleBar 吸收搜索，删 `TopBar`，`--titlebar-height` 46 | ✅ 顶栏可用 | ✅ 还原 3 文件 |
| **S3** | `PageHeader` 零件 + 三页逐个迁（每迁一页都可运行） | ✅ | ✅ 单页回退 |
| **S4** | `PageToolbar` 零件 + 曲库三 tab / 设置六区块 | ✅ | ✅ |
| **S5** | 删死样式 + E2E 改 testid 修绿 | ✅ | ✅ |

每步之间 commit 一次，`npm run check` 与 `probe:ui` 通过才进下一步。中途任何一步出问题，回退粒度是
"一步"而不是"一整批"——这比隔离分支更细，因为隔离分支最后仍要一次性合并。

**为什么不能更细到"文件级隔离"**：S2 里 TitleBar 吸收搜索的同时 `TopBar` 就必须消失（否则两个搜索框）。
这是**一个不可分的原子动作**，但它的影响面只有 3 个文件 + 2 段样式，风险可控。

---

## 6. 决策（2026-10-26 已拍板）

| # | 决策 | 结论 | 影响面 |
|---|---|---|---|
| 1 | 列表点击语义 | **单击选中、双击播放、Enter 播放**（桌面惯例） | B4。要同步改：行 `onclick` → 选中、`ondblclick` → 播放、`lib/list-keys.ts` 的 Enter 语义、`TrackTable` 与 `QueuePanel` 保守起见只改主列表与首页表格；右键菜单里"播放"仍是显式入口。**风险**：用户已习惯单击播；缓解 = 首行 hover 出播放按钮 + 双击有 150ms 判定延迟，感知上不明显 |
| 2 | 专辑页 / 艺人页 | **做**，新增后端聚合命令 `library_facets`（专辑/艺人：id、名称、封面、曲目数、总时长、年份）；复用 `library_list` 的 `albumId`/`artistId` 取曲目 | B4 末尾 + 一个后端子步（SQLite `GROUP BY`，与现有一致用游标或一次性返回前 N + 搜索）。路由 `album/:id`、`artist/:id`；侧栏加两个入口。**B4-3 实现记录**：命令拆成 `library_albums`/`library_artists` 两条（理由见 B4-3 章节）；详情页参数复用筛选字段（不加 facetId）；一次性返回前 500，不做游标；审查补丁后响应带 `total`，截断在宫格页有提示（见 B4-3a） |
| 3 | EQ 范围 | **含后端一起做**：10 段 biquad + 预设 + 协议 5→6 + 旁路一致性测试 | B5a（UI）+ B5b（Rust DSP）。B5b 必须先跑 `cargo build -p aiya-audio-host`（协议升版铁律） |
| 4 | FM 电台入口 | **移出一级导航**，等后端 | B3。`RadioView` 暂时保留文件但不注册路由 |
| 5 | Tailwind | **依赖保留、入口裁剪**（只引 `theme` + `utilities`，不引 preflight，不写 utility） | B0。产物 CSS 下降 ≥3 KB，消除双份 reset；vite 插件与两个 devDependency 留着 |

### v2 追加的三条（本轮拍板）

| # | 决策 | 结论 | 影响面 |
|---|---|---|---|
| 6 | 顶栏形态 | **宽 TitleBar**：应用名 + 返回 + 搜索 + 窗口控制在**一行**；空白区整条可拖；高度 36 → **46px** | B3a。搜索从"页面级容器"升到"全局容器"，必须处理 `data-tauri-drag-region` 的两个反向陷阱（可点元素阻断、非可点容器默认可拖） |
| 7 | 页内标题 / 工具条 | **两个独立零件，不在一行、不组合**，页面自己调用；**删掉 TopBar 这一级** | B3a。`Header`/`Toolbar` 属"版式件"，禁止被其它零件内部调用（§4.3 铁律） |
| 8 | 导航归属 | **Sidebar 与顶栏共用 `stores/nav.svelte.ts`**：`go`/`back`/`search` 三个动作 + 50 深快照栈；Sidebar 改零 prop | B3a。本轮只收"动作"（`go()` 转调既有入口），**不收"状态"**；B4 拆页时 `nav.go()` 签名不变、调用点不用改 |
| 9 | 改动方式 | **结构就地改、视觉隔离试**（详见 §5.5） | B3a 分 S1–S5 五个可运行小步；PageHeader/PageToolbar 的长相先在 `?page=kit`（B2）定稿 |

---

## 7. 验证门槛（每批都要过）

| 门槛 | 命令 | 通过标准 |
|---|---|---|
| 类型 | `npm run check` | 0 error / 0 warning |
| 构建 | `npm run build:ui` | 通过；JS ≤ 240 KB、CSS 逐批下降 |
| 契约 | `npm run gen:types:check` | 无漂移 |
| 依赖图 | `npm run check:deps` | **0 环**（B0 起固化：`tools/dep-graph.mjs`。store 的分层是隐性契约，环在 ESM 里不一定立刻炸，只会让初始化顺序变得不可推理） |
| 运行时 | `npm run probe:ui -- --play` | 无 console error / 无 4xx / 无未捕获异常 |
| E2E | `npm run test:e2e` | **B3a 起 5/5**（原本 0/4）、**B0 起 6/6**，B6 起 10/10 |
| 视觉 | 真窗口 3 档 × 明暗 2 套 | 12 张目检图，记进本文档附录 |
| 壳体（v2） | 真窗口手测 6 条 | ①顶栏四件位置正确 ②拖空白移动窗口、双击最大化 ③点搜索框内边距不拖窗 ④首页→歌单→返回回首页 ⑤搜索后返回还原搜索前状态 ⑥设置页标题与 tab 分两行 |
| 版式件边界（v2/v3） | `grep -rn "content/Header\|content/Toolbar" components views` | 引用方**只能是 `views/` 与 `SettingsPanel`**；被 `ListPage`/`Card` 这类零件内部调用就是被组合化了，打回 |

后端子批（B5b）另加：`cargo test --workspace` 全绿 + `cargo build -p aiya-audio-host`（协议升版必跑）。

---

## 8. 明确不做

- 不做 SSR/路由库/URL 状态（桌面单窗口）。v2 的 `nav` 是**对象路由**（`NavTarget` 判别联合）而不是
  URL 字符串 —— 没有地址栏、没有分享链接、没有前进键，引入字符串路由只多一层解析与拼错的机会
- 不做 TanStack table/virtual、不做第二套 headless、不引入 DnD 库（docs/19 §6.3 结论不变）
- 不做主题编辑器（三套预设够用）
- 不做移动端适配（只保证 960px 以上不破）
- 不重写 Rust 播放内核（EQ 是**加法**，不动现有链）
