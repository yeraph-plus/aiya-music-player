# 23 — 信息架构收口与表格多选（2026-10-26）

> 本批只做前端：**取消三栏布局的残余**、**首页曲目区块表格化 + 多选批量管理**、
> **主列表加勾选列与「添加」排序列**，以及上一轮审查剩下的 13 条。
> 后端与契约（54 命令 typed IPC）零改动。

## 0. 结论先放

1. **右侧情境面板整体取消**，不做"三栏可停靠"。骨架固定为：标题栏 / 侧栏 / 主区 / 播放条；
   播放队列是挂在播放条上方的抽屉；歌词与频谱进「正在播放」一级页；导入 / 扫描 / beets 进设置单页。
2. **表格多选不引第三方表格库**。用 `bits-ui` 的 `Checkbox` 原语（已是依赖、此前未使用）
   + 既有的 `stores/selection`（下标）与首页按 ID 的选择集。理由见 §2。
3. **首页「最近播放 / 常听 / 最近添加」三段都变成真表格**（表头 + 勾选列 + 三态全选），
   选中即出批量条：播放 / 加入队列 / 加入播放列表 / 移出曲库。
4. **主列表补「添加」列与排序入口**。`sort: 'added'` 后端一直支持，但表头没有按钮 —— 用户点不到。

## 1. 取消三栏布局（对 docs/19 的偏差）

docs/19 §1.1 写过「保留信息架构骨架」，§2 还给了三档宽度下的右栏策略。**本批作废那些条款**，
准确地说：作废的是"右侧常驻面板 + 三档右栏宽度"，保留的是导航与列表骨架。

| docs/19 原文 | 现在 |
|---|---|
| 应用壳 = 侧栏 / 主区 / **右侧情境面板** / 播放条 | 侧栏 / 主区 / 播放条；无右侧常驻面板 |
| 右栏同时只开一个（队列 / 歌词 / 频谱 / 导入） | 队列 = 播放条上方的**抽屉**；歌词 + 频谱 = 「正在播放」页内两栏；导入/扫描/beets = 设置页 |
| ≥1280 三栏停靠、1100–1279 右栏降 304px、<1100 右栏转覆盖层 | 只有两栏；主区宽度由侧栏（216px）与窗口宽决定 |

**为什么**：四种"情境"里只有队列需要随时可见（它是播放的延伸动作），
歌词/频谱是整屏看的，导入/beets 是分步任务（docs/19 §4.3 自己也写了"不能压缩成 320px 右侧栏里的长表单"）。
三栏在默认 1280 宽下主区只剩约 700px：专辑列被压到不可读，而队列抽屉一开一关不推动列表 ——
把「随时可见」降级成「一键可见」，换来主列表多 300px。这不是审美判断，是宽度账。

**队列抽屉**落在 `.player-dock` 内绝对定位（`pages.css` §播放队列抽屉），
`bottom: calc(100% + 8px)`、最大宽 `100vw - 24px`，不参与网格，所以打开/关闭不会让列表重排或丢滚动位置。

## 2. 表格多选：调研与结论

### 2.1 候选

| 方案 | 它给什么 | 为什么不用 / 为什么用 |
|---|---|---|
| `@tanstack/svelte-table`（v9 有 Svelte 5 适配） | headless 表模型：排序/过滤/分页/行选择，`rowSelection` 以**行 id 字典**存储，`getIsAllRowsSelected` / `toggleAllPageRowsSelected` | **行模型要求数据在内存**。本项目 5 万行走游标分页，任何时刻只有窗口内的约 100 行 + 已缓存页在内存；全选由后端按下标换算（ADR-020）。接进来要么把整库拉进前端，要么所有能力都被 `manual*` 关掉 —— 只剩一个 checkbox 状态机，约 15KB 换不掉几行代码 |
| `svelte-headless-table` | 同类的插件式表模型 | 维护活跃度低于 TanStack，且同样有"数据在内存"的前提；没有额外收益 |
| shadcn-svelte DataTable 组合（TanStack + bits-ui Checkbox） | 现成的行选择列范式 | 落地形态就是"TanStack 状态 + bits-ui Checkbox 渲染"，前者不适合本项目，后者才是真正有用的部分 |
| **bits-ui `Checkbox`**（已有依赖 `2.20.2`，此前装了但零引用） | 无障碍三态选框（`checked` / `indeterminate` / `data-state`），键盘与焦点行为齐全 | **采用**。它只解决"勾选框长什么样、怎么响键盘"，选择集仍由我们自己的 store 管 —— 与虚拟化、服务端分页、`idsForSelection` 全都不冲突 |
| 自研 `<span role="checkbox">` | 零依赖 | 键盘/焦点/`aria-checked` 要自己写对，而 bits-ui 已经在依赖里 |

### 2.2 结论

**不引表格库，用 bits-ui Checkbox 做原语，选择集分两套（各自贴合数据形状）：**

- **主列表（`TrackList`）**：选择集仍是**视图下标**（`stores/selection`），
  批量操作时由 `library.idsForSelection(query, indices)` 在后端换算 —— 全选 5 万首不需要把 5 万首拉到前端。这是 ADR-020 的既定设计，不动。
- **首页三段（`TrackTable`）**：数据是一次查 8 条的**快照**，下标在区块之间没有意义，
  所以选择集直接是**曲目 ID**，批量操作不需要后端换算。

两套共用同一个 `BatchBar`（它只收回调与数量，不认下标），交互语义也刻意对齐：
**点选框 = 切换（累加）**，`Shift` = 连选，表头 = 三态全选。
注意这与「点行 + Ctrl」不同：点行是播放，Ctrl 点行是"只选这一行"。
勾选框的常规预期是逐条累加，混用会导致"勾第二首时第一首自己掉了"（`selection.handleCheck` 与 `handleSelect` 因此分开）。

### 2.3 一个实现细节：勾选框不能靠 `stopPropagation` 独占点击

`bits-ui` 的 `Checkbox.Root` 渲染成 `<button role="checkbox">`，自带点击处理器。
我们还需要 `event.shiftKey`（连选），而 `onCheckedChange` 只回一个 boolean，
所以勾选框外面套一层 `<span onclick={...}>` 自己接管，`checked` 只作为**受控 prop** 传入
（不 bind、不传 `onCheckedChange`）—— 模型改了它就跟着改，不会出现"视觉上一个勾、
模型里没选中"的错位。

**拦"点勾选框顺手起播"不能只靠 `stopPropagation`**：Svelte 5 把 `click` 委托到根节点
统一派发，同一批委托 handler 之间的 `stopPropagation` 不保证先后。所以行自己还要认一次来源：

```svelte
onclick={(event) => {
  if (event.target.closest('.check')) return;   // 点的是勾选框，不是这一行
  onplay(index);
}}
```

这条**是实测踩出来的**：第一版只在勾选框的包装元素上 `stopPropagation`，
结果点勾选框会把那一行播起来。现在的写法不依赖监听器阶段，也不依赖谁先注册。

包装层上的 `onclick` 会让 svelte-check 报两条 a11y（非交互元素带点击 / 交互 role 需
tabindex）。处理方式是**显式豁免而不是改结构**：`tabindex="-1"`（可编程聚焦、不进 Tab 序列）
+ 一条 `svelte-ignore a11y_click_events_have_key_events`，理由写在注释里 ——
真正的键盘入口是内层 `button`，包装层只是扩大命中区。

反过来**不要把 `onclick` 挂到 `Checkbox.Root` 上**：它自带的处理器和我们的受控 prop
会各翻一次，实测结果是"勾了的行显示未勾"（`data-state` 与模型相反）。

## 3. 首页：从"整行按钮"到真表格

`views/HomeView.svelte` 的三段预览原来是 `<button class="home-track">`（整行可点、四列 grid）。
行里要放勾选框就不能再用 `<button>` 包（button 内嵌可交互元素是非法的），
所以抽成 `components/TrackTable.svelte`：

- 表头：选框 / 封面 / 标题 / 艺人 / 专辑 / 第四列（时长 · 次数 · 入库日期）
- 行：`role="row"`，点行播放，点选框勾选，右键菜单与主列表同源（`overlay.openTrackMenu`）
- 第四列按区块切换：最近播放=时长、常听=次数、**最近添加=入库日期**（`formatDay`，完整时间在 `title` 里）

**「最近添加」现在真的按添加时间排序**：`loadSection({kind:'library'}, true)` 带
`sort: 'added' / direction: 'desc'`；「查看全部」也带上同一个排序
（`list.openBuiltinView('library', { field: 'added', direction: 'desc' })`）。
以前「查看全部」只是打开「全部曲目」，用的是上一次留下的排序字段 —— 用户点"最近添加"的"查看全部"，
看到的可能按标题排的列表。

**三段选择互斥**：批量条只有一个，在 A 段勾选会清掉 B 段。选中集非空就渲染 `BatchBar`（fixed 定位，不挤布局）。

**重载时机**：首页不在 `onMount` 里自己查，而是盯 `library.stats.tracks`。
`boot` 之后 stats 才就位，提前查会在"曲库还没准备好"的时刻白跑三条查询；
盯着它同时也免费拿到"扫描完成 / 批量移出曲库后自动刷新"。

## 4. 主列表：勾选列 + 「添加」列

`TrackList` 的网格从 6 列变 8 列（有"次数"时 9 列）：

```
选框 22 | 序号 34 | 封面 42 | 标题 2.2fr | 艺人 1.4fr | 专辑 1.4fr | 添加 78 | [次数 48] | 时长 56
```

- 表头选框三态（全选上限 = `SELECTION_CAP` = 5000，与后端 `MAX_BATCH` 对齐）。
  不按这条对齐的话，5 万首的视图点全选后表头会一直显示"半选"。
- 「添加」列既是**信息**（入库日期）也是**入口**（`sort: 'added'` 的按钮）。
- 空态文案分叉：`notify.error` 存在时显示「列表加载失败 + 重试」，
  不再说「曲库还是空的」—— 查询失败时 `total` 也是 0，而那句话会把用户骗去设置页重新扫描。

## 5. 本批修掉的审查项

| # | 问题 | 修法 |
|---|---|---|
| 1 | 设置页录制快捷键时，按空格既录进绑定**又触发了播放** | 录制态进 `stores/capture`，`lib/keyboard.ts` 开头看这个标志直接返回。根因：两个 `onkeydown` 都挂在同一个 `window` 上，组件里 `stopPropagation` 拦不住同节点的另一个 listener |
| 2 | 错误条渲染两遍（AppShell + LibraryView 逐字重复） | 删掉 LibraryView 里那两段，全局只留 AppShell 一份 |
| 3 | 侧栏底部删了 `<br />` 但 `.library-stats` / `.warn` 无样式、`.sidebar-footer` 不是 flex | 改 `flex-direction: column`，补 `.library-stats` 与 `<strong>` 的字号 |
| 4 | 启动就落在设置页时诊断区永远空着 | `ui.restore()` 里 `page === 'settings'` 时调 `refreshDiagnostics()` |
| 5 | 「添加」排序点了没反应（无入口） | 见 §4 |
| 6 | 首页「最近添加 → 查看全部」不带 `sort: 'added'` | `openBuiltinView` 支持传排序，见 §3 |
| 7 | 查询失败时空态说「曲库还是空的」 | 见 §4 |
| 8 | `applySettings` 少传错误回调就静默吞错 | 去掉可选回调，失败**一律**落全局错误条 |
| 9 | 标题栏品牌区点不动（拖拽区域只覆盖父元素自身） | `data-tauri-drag-region="deep"`（tauri 2.12 `drag.js` 的三种取值：`deep` 子树 / 裸属性仅自身 / `false` 禁用）+ 去掉子元素上的重复属性 |
| 10 | 没有最大化按钮 | 换成 最小化 / 最大化⇄还原 / 关闭；`isMaximized()` 初值 + `onResized` 跟随系统改（拖边缘吸附、Win+↑ 都算） |
| 11 | 左右边距 18px 与 22px 混用 | 统一 22px；列表 10px + 行内 12px = 22px 对齐 |
| 12 | `ui.settingsOpen` 是 `page` 的手写镜像 | 删字段，直接用 `ui.page === 'settings'` |
| 13 | 死代码：`.app.side-open`、`--queue-width`、`.settings-entry`、`.brand` | 全部删除；骨架的三行网格**只在 base.css 定义一次**（pages.css 不再覆盖行号） |
| 14 | 设置页「诊断」的两个表格按 `atMs + 内容` 做 key，两条同毫秒记录会让整个页面抛 `each_key_duplicate` 并白屏 | 改非 keyed each。**这是实测撞到的**：连续两次列表查询落在同一毫秒时页面直接崩，之后所有交互都像"点了没反应"（Svelte 的渲染 effect 已经断了） |

## 6. 验证

命令行：

- `npm run check`（svelte-check）：**0 errors / 0 warnings**
- `npm run build:ui`：通过，`dist/assets/index-*.js` 233.67 kB（gzip 78.60 kB）——
  bits-ui 从"装了没用"变成真实引用，体积增量包含在其中
- `npm run dev`：vite 1420 就绪、`aiya.exe` 启动、`player state restored tracks=3`

真实窗口（CDP 探针 + 逐步交互，`WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9222`）：

| 项 | 实测 |
|---|---|
| 骨架 | `.app` 2 列 / 3 行 = `216px 1064px` × `36px 672px 92px`，子节点 `titlebar / sidebar / main / player-dock`（无右侧面板） |
| 标题栏 | `data-tauri-drag-region="deep"` 只在 header 上；三个按钮 `最小化 / 最大化 / 关闭`；点最大化 1280×800 → 3440×1392 且按钮变「还原」，还原回 1280×800 |
| 首页表格 | 3 段 × 3 行 = 9 行、12 个勾选框；表头 5 列，最近添加末列为「入库 2026-10-25」 |
| 首页多选 | 点一行 → `已选 1 首`、表头 `aria-checked="mixed"`；再点一行 → `已选 2 首`（累加）；表头 → `已选 3 首` + `checked`；再点表头 → 清空 |
| 勾选不起播 | 点第 1 行（高音测试 880Hz）勾选框：该行选中、批量条 `已选 1 首`、播放条仍是 `中音测试 440Hz`（未切歌） |
| 最近添加 → 查看全部 | 落到列表，表头 `添加 ↓` 高亮，日期列 2026-10-25 |
| 主列表勾选 | 逐行 `已选 1 首 → 2 首 → 3 首`，表头同步 `indeterminate → checked`，再点清空 |
| 添加排序 | 点「添加」表头：`↓ → ↑ → ↓` |
| 队列抽屉 | 打开后 `.queue-panel` `position: absolute`、宽 360px；`.app` 仍是 `216px 1064px`，主区宽 1064px 未变（不推动布局） |
| 设置单页 | 6 个区块全部常驻；诊断块 14 项 + 最近查询 8 行，取到真实值（版本 0.1.0 / 平台 windows / Schema v2 / 曲库 3 首） |
| 快捷键录制守卫 | 暂停态下：不录制时空格能起播；点开录制（显示「按下新键…」）后按空格，绑定记为「空格」且**播放未启动**，录制正常结束 |
| console / 异常 | 全流程 0 error / 0 warning / 0 未捕获异常 |

踩到的时序坑（写在这里省下次的时间）：WebView2 窗口被遮挡时 DOM 更新会比预期慢，
探针每一步之后要等 **≥600ms** 再读；读早了会看到"上一帧"的 DOM，
本次排查前半程的"勾选没反应"一半是这个原因，另一半是上面第 14 条的真崩溃。

## 7. 未完成 / 后续

- 快捷键设置页的逐项核对（录制态已修，但 16 个动作的实际触发未逐条点过）
- 主题与密度的逐项目视核对（紧凑档 42px 行高在 9 列下是否够）
- 队列抽屉在 960px 最小宽下的表现（宽度 360px，`max-width: calc(100vw - 24px)` 兜底）

> 2026-10-26 深夜补充：本文写完后的 B4/B5/B6 三批（docs/24）又改了设置页形态——
> §6 表里的「设置单页 6 区块常驻」仍成立，但导航从 Toolbar tab 换成了**左分组导航 +
> scroll-spy**（区块照旧全部常驻挂载）；队列抽屉从 `.player-dock` 绝对定位换成了
> kit Popover 的 portal 浮层（§6 表里「宽 360px 不推动布局」的断言仍由
> `.queue-float` 保持）。上列三条后续仍未逐项点过，留待真机走查。
