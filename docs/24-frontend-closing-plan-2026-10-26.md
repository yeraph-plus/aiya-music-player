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

---

## 1. 问题清单（每条都有证据，不是印象）

| # | 问题 | 证据 | 影响 |
|---|---|---|---|
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
│ Sidebar   │ PageHeader     ← 页内标题零件（页面自己调用，随内容滚动）   │
│ 216px     │ PageToolbar    ← 页内工具条零件（tab / 筛选 chip / 动作）   │
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
| **PageHeader**（页内零件） | kicker + 标题 + 副标题 + meta（`12 首 · 43 分钟`）+ 右侧动作组；可选大封面（歌单/专辑） | tab、筛选 chip、搜索 |
| **PageToolbar**（页内零件） | tab 组（曲库三视图 / 设置六区块）、筛选 chip、右侧次级动作 | 标题、播放全部（那是 PageHeader 的主按钮） |
| **PlayerDock** | 当前曲信息、传输、进度、音效(EQ)、正在播放入口、队列按钮、音量 | 歌词正文与频谱画布（在「正在播放」页） |

**三条形态约束（本次修订的核心）：**

1. **顶栏四件一行**。搜索框宽度固定（约 360px），右侧留给窗口控制，中间是唯一的拖拽余量。
   `--titlebar-height` 由 36px 提到 **46px** —— 36px 塞不下 30px 高的输入框 + 上下呼吸；
   46 也是 Windows 自定义标题栏的常见值（系统标题栏 32 + 应用条 14）。
2. **PageHeader 与 PageToolbar 不在一行、不组合**。它们是两个零件，页面写两行标签，中间想插
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
<PageHeader …>          ← 零件 1（静态）
<PageToolbar …>         ← 零件 2（可缺席：歌单页无 tab、空库无 chip 时整条不渲染）
<列表 / 空态 / 表单>      ← 各自实现
```

`ListPage` 若要做，只做"给这三段排间距"的细壳（B4 再定），**不允许**把 PageHeader/PageToolbar
降级成它的内部 prop —— 那样又回到"组合组件"的老路。

`ui.page` 从 4 值扩展到 8 值。**每个页面自己调 PageHeader / PageToolbar**，不经过任何中间组件。

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
  <PageHeader kicker="YOUR LIBRARY" title="首页" subtitle="回到常听的音乐…">
    actions: [浏览全部曲目 →]
  <PageToolbar>                     ← 首页没有：三段预览各自带 section-heading
  <section> 歌单宫格 / 最近播放 / 常听 / 最近添加

曲库 LibraryView（固定头 + 独立滚动列表，零件不跟列表滚）
  <div class="page-pad">            ← 横向 22px 归布局层，零件自己不带
    <PageHeader kicker="曲库" title="全部曲目" meta="5 000 首"> actions: [播放全部]
    <PageToolbar tabs=[全部曲目|最近播放|常听] active="library" filters=[已按艺人筛选 ×]>
  <TrackList>                       ← 自己滚动、自己恢复位置

设置 SettingsView（**现役反例**）
  今天：<header class="page-heading"> <h1>设置</h1> + <nav class="settings-jump">18 个 chip 锚点</nav> </header>
        —— 标题与工具条挤在同一个 header 里，各自 padding 靠猜
  改造：<PageHeader kicker="PREFERENCES" title="设置" />
        <PageToolbar tabs=[音乐库|外观|播放|系统|快捷键|诊断] active={滚动位置反查} />
        —— 锚点 chip 从"标题右边的导航"变成"独立工具条上的 tab"，滚动联动逻辑不变
```

设置页是本轮最有说服力的证据：它**已经**把两种东西塞进一行，且因为塞在一起，`jumpTo()` 的
active 状态只能靠 `IntersectionObserver` 反查后写回一个 `.chip.on` —— 拆开之后，`PageToolbar`
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
| 19 | `DataTable` | `.list` `.row` `.grid-head` `.cell` `.tt*` | 表头可排序 + 勾选列 + 虚拟化 hook（见 §5 B4） |
| 20 | `StatBlock` | 侧栏统计、诊断 grid | 数字 + 标签，tabular 数字 |

**版式件（2 件，v2 新增）** —— 与上面 20 个原子件的规则**不同**：

| # | 零件 | 取代现有 | 约束 |
|---|---|---|---|
| 21 | `PageHeader` | `.page-heading` `.page-heading h1/p` `.view-head` `.view-meta` `.view-kicker` `.view-cover` `.view-actions` | 形态由 `cover` 三态决定（缺省=纯文字 / `null`=占位 / 字符串=大封面）。**不带横向 padding**（归调用方的 `.page-pad`），所以能在 `.page-scroll` 内也能在固定头位置。`meta` 是字符串不是 snippet —— snippet 里写的 scoped class 不生效（渲染在父作用域） |
| 22 | `PageToolbar` | `.settings-jump` `.chip.on` `.filters` `.filter` | `tabs` + `filters` + `start`/`end` snippet；**全部为空时整条不渲染**（不占高度）。`active` 由页面给（设置页仍是滚动反查，但反查结果只是这个 prop 的值） |

> **版式件的铁律：只允许页面直接调用，不允许被别的零件内部调用。**
> 一旦 `PageHeader` 被 `ListPage` 之类包起来，就会出现"这个页面想改标题右边的东西，
> 得先给中间组件加 prop"——那正是 `TopBar` 死掉的原因。原子件可以互相嵌套（Button 里放 Icon），
> 版式件不行。

演示页：`?page=kit`（仅 dev/mock 下注册）—— 一屏看全 20 件的全状态（normal/hover/active/disabled/loading/dark/light/compact），
外加 PageHeader / PageToolbar 的三种形态各一例。
它是 kit 的自测页，也是后续改 token 时的目检页。

### 4.4 主题适配

- **暗/亮**：`html[data-theme]` 切 token，组件零分支（现在已如此，继续保持）。
- **密度**：`html[data-density]` 只改 `--row-h`/`--control-h`/`--space-*` 的档位；列表虚拟化读同一个 `--row-h`（现在 `rowHeight={density==='compact'?42:52}` 是 JS 硬编码，要和 CSS 同源 —— 改成从 token 读或单一常量导出）。
- **主色**：`--accent` 默认来自主题；开启"封面主色"后由 `accentForArtwork` 派生，但**必须过对比度约束**：派生色与 `--surface-1` 的对比度 < 3:1 就回落到主题色（现在无约束，浅色主题下深色封面会得到看不见的 accent）。
- **禁用规则**：组件内禁止 `rgba(255,122,122,...)` 这类硬编码（`.error` 现在就是这么写的，浅色主题下底色仍是深红调），一律 `color-mix(in srgb, var(--danger) 10%, transparent)`。

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

### B0 清场（无视觉变更）

| 项 | 动作 |
|---|---|
| Tailwind | `app.css` 入口换成 `theme` + `utilities` 两层，不引 preflight（依赖与 vite 插件保留）；产物 CSS 应下降 ≥3 KB |
| 死代码 | 删 `.modal.settings`/`.tabs`/`.modal-body`/`.settings`/`.stats`/`.home-skeleton`（`.topbar`/`.view-*`/`.page-heading`/`.titlebar*` 已在 B3a 随组件删）；补 `--warning`→`--warn`、`--font-mono`→新增 token |
| store 归位 | `ui` 的 8 个歌单 CRUD + `playlists` 字段 → 迁进 `stores/playlists.svelte.ts`；`ui.lyrics*` → 新 `stores/lyrics.svelte.ts` |
| 图标 | `Icon.svelte` 改成"语义名 → lucide 组件"的薄映射（保留 API，内部不再存 path）；统一 strokeWidth |
| 去重 | 视图重置逻辑：**B3a 之后剩下的 3 处**（`openHome`/`openBuiltinView`/`openPlaylist` 各自清搜索+筛选）抽一处，或直接由 nav 统一 |

验收：`npm run check` 0/0、`build:ui` 通过且 CSS ≤ 32 KB（B3a 后基线重测）、`npm run test:e2e` 4/4 绿、`probe:ui` 无异常。

### B1 Token 与样式分层

token 三轴重命名（293 处引用机械替换）、reset 从 `tokens.css` 挪到 `base.css`、`.app`/`.sidebar`/`.main` 挪到 `layout.css`、
`.error`/`.notice` 等硬编码色改 token、密度变量化（JS 与 CSS 同源）。

验收：`grep 'rgba(255, *122'` 无命中；明暗 × 舒适紧凑 4 张目检图；`--row-h` 改动只动一处。

### B2 UI Kit 第一批（12 件）+ 演示页

Button、IconButton、Chip、Checkbox、Switch、SegmentedControl、Slider、Progress、Field、Card、Cover、Skeleton、
EmptyState、StatBlock（可拆两小步）。配 `?page=kit` 演示页。
已在用的视图（设置页、首页、列表页）**先只替换 icon-btn / btn 两类**，验证 kit 与既有全局 CSS 能共存。

验收：演示页 0 a11y warn；`grep -c 'class="btn'` 在 views 里降到 0；剩余件数进 B3/finish。

### B3b 壳体换皮与播放条（原 B3）

B3a 定好的结构**不再动**，这一批只换零件实现：
TitleBar（返回键与窗控换 `IconButton`、搜索框换 `Field`）/ Sidebar / PlayerDock 用 kit 重写（含 Popover、Tooltip）；
PageHeader / PageToolbar 内部改用 `Button`/`Chip`/`Cover`，对外的 props 一行不改。
侧栏加"专辑/艺人"入口与分组；电台入口移出；统计区改用 `StatBlock`。
播放条：EQ/倍速/输出状态三个 Popover 的**壳**（EQ 内容在 B5）。

验收：1280×800 / 1440×900 / 960×700 三档目检；`grep 'class="btn'` 在 shell 组件里降到 0；
B3a 的 6 条真窗口实测重跑一遍全过；Dock 在窄窗下的收纳规则。

### B4 主体页面收敛

- **点击语义切换**：单击选中、双击播放、Enter 播放（§6 决策 1）
- 路由拆页（`library`/`recent`/`mostPlayed`/`playlist` 独立，删 `libraryDetail`）
- `ListPage` 骨架（PageHeader/Toolbar/StatusBar）+ `DataTable`
- 虚拟化抽 `lib/create-virtual-window.svelte.ts`（TrackList/QueuePanel 共用）
- 勾选语义抽 `lib/row-selection.ts`（下标版 / ID 版两个实现，接口一致）
- BatchBar 单点渲染（AppShell），删 HomeView 那份
- 搜索结果页；**专辑页 / 艺人页**（配 `library_facets` 后端小步，§6 决策 2）

验收：`components/TrackList.svelte` + `TrackTable.svelte` 行数下降 ≥40%；批量动作只有一处实现；
6 个列表页视觉同构（同一骨架截图对比）；双击播放与单击选中互不串（各点 5 次无一次误触）。

### B5 播放区与 EQ（含后端子批）

**B5a UI**：QueueDrawer 改用 kit 重写；「正在播放」页三块可切（歌词/频谱/EQ）。
**B5b 后端**（独立可并行）：`aiya-audio` 加 10 段 biquad（峰值/低架/高架）+ 预设表 + 增益限制；
协议 `PROTOCOL_VERSION` 5 → 6（`SetEq{bands,pregain,enabled}` + `EngineEvent::EqApplied`）；`cargo build -p aiya-audio-host` 必跑；
`player_set_eq` 命令 + ts-rs 生成；smoke：EQ 全 0dB 时输出与关 EQ 逐样本一致（旁路证明）。

验收：旁路一致性测试；EQ 调整时无爆音（20 次连切预设无 click）；设置持久化。

### B6 设置与浮层 + 收尾

设置页改"左分组导航 + 右内容"，22 项全部用 kit；Dialog/Menu 全量替换（删 `ContextMenu.svelte`/`ConfirmDialog.svelte`/
`PromptDialog.svelte`/`PlaylistPicker.svelte` 四个独立实现，收敛到 `ui/Dialog`+`ui/Menu`）；
E2E 补到 8–10 条（覆盖：拆页路由、批量、设置分组、kit 演示页、队列抽屉、EQ）；
可访问性与键盘巡检表（docs/19 §5 逐条打勾）；更新 docs/19 的作废标记与 docs/23 的补充。

验收：`npm run verify`（test + check + build + gen:types:check）全绿；`test:e2e` 10/10；真窗口 12 步走查表。

---

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
| 2 | 专辑页 / 艺人页 | **做**，新增后端聚合命令 `library_facets`（专辑/艺人：id、名称、封面、曲目数、总时长、年份）；复用 `library_list` 的 `albumId`/`artistId` 取曲目 | B4 末尾 + 一个后端子步（SQLite `GROUP BY`，与现有一致用游标或一次性返回前 N + 搜索）。路由 `album/:id`、`artist/:id`；侧栏加两个入口 |
| 3 | EQ 范围 | **含后端一起做**：10 段 biquad + 预设 + 协议 5→6 + 旁路一致性测试 | B5a（UI）+ B5b（Rust DSP）。B5b 必须先跑 `cargo build -p aiya-audio-host`（协议升版铁律） |
| 4 | FM 电台入口 | **移出一级导航**，等后端 | B3。`RadioView` 暂时保留文件但不注册路由 |
| 5 | Tailwind | **依赖保留、入口裁剪**（只引 `theme` + `utilities`，不引 preflight，不写 utility） | B0。产物 CSS 下降 ≥3 KB，消除双份 reset；vite 插件与两个 devDependency 留着 |

### v2 追加的三条（本轮拍板）

| # | 决策 | 结论 | 影响面 |
|---|---|---|---|
| 6 | 顶栏形态 | **宽 TitleBar**：应用名 + 返回 + 搜索 + 窗口控制在**一行**；空白区整条可拖；高度 36 → **46px** | B3a。搜索从"页面级容器"升到"全局容器"，必须处理 `data-tauri-drag-region` 的两个反向陷阱（可点元素阻断、非可点容器默认可拖） |
| 7 | 页内标题 / 工具条 | **两个独立零件，不在一行、不组合**，页面自己调用；**删掉 TopBar 这一级** | B3a。`PageHeader`/`PageToolbar` 属"版式件"，禁止被其它零件内部调用（§4.3 铁律） |
| 8 | 导航归属 | **Sidebar 与顶栏共用 `stores/nav.svelte.ts`**：`go`/`back`/`search` 三个动作 + 50 深快照栈；Sidebar 改零 prop | B3a。本轮只收"动作"（`go()` 转调既有入口），**不收"状态"**；B4 拆页时 `nav.go()` 签名不变、调用点不用改 |
| 9 | 改动方式 | **结构就地改、视觉隔离试**（详见 §5.5） | B3a 分 S1–S5 五个可运行小步；PageHeader/PageToolbar 的长相先在 `?page=kit`（B2）定稿 |

---

## 7. 验证门槛（每批都要过）

| 门槛 | 命令 | 通过标准 |
|---|---|---|
| 类型 | `npm run check` | 0 error / 0 warning |
| 构建 | `npm run build:ui` | 通过；JS ≤ 240 KB、CSS 逐批下降 |
| 契约 | `npm run gen:types:check` | 无漂移 |
| 运行时 | `npm run probe:ui -- --play` | 无 console error / 无 4xx / 无未捕获异常 |
| E2E | `npm run test:e2e` | **B3a 起 5/5**（原本 0/4），B6 起 10/10 |
| 视觉 | 真窗口 3 档 × 明暗 2 套 | 12 张目检图，记进本文档附录 |
| 壳体（v2） | 真窗口手测 6 条 | ①顶栏四件位置正确 ②拖空白移动窗口、双击最大化 ③点搜索框内边距不拖窗 ④首页→歌单→返回回首页 ⑤搜索后返回还原搜索前状态 ⑥设置页标题与 tab 分两行 |
| 版式件边界（v2） | `grep -rn "PageHeader\|PageToolbar" components views` | 引用方**只能是 `views/`**；出现在 `components/` 里就是被组合化了，打回 |

后端子批（B5b）另加：`cargo test --workspace` 全绿 + `cargo build -p aiya-audio-host`（协议升版必跑）。

---

## 8. 明确不做

- 不做 SSR/路由库/URL 状态（桌面单窗口）。v2 的 `nav` 是**对象路由**（`NavTarget` 判别联合）而不是
  URL 字符串 —— 没有地址栏、没有分享链接、没有前进键，引入字符串路由只多一层解析与拼错的机会
- 不做 TanStack table/virtual、不做第二套 headless、不引入 DnD 库（docs/19 §6.3 结论不变）
- 不做主题编辑器（三套预设够用）
- 不做移动端适配（只保证 960px 以上不破）
- 不重写 Rust 播放内核（EQ 是**加法**，不动现有链）
