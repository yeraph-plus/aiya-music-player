# 19 — UI/UX 重设计规范与依赖选型（2026-10-25）

> 状态：本轮只完成调研、设计规范与依赖决策。**未安装依赖、未改页面、未改后端、未做逐页分配。**
> 适用范围：下一批 UI 重设计。页面数量、路由/视图分配与逐页线框由下一批另行确认。

## 0. 决策摘要

### 技术栈

- **沿用 Svelte 5 + TypeScript + Vite**。现有组件、runes store、typed IPC、mock 与验收链都已成立，换框架没有可量化收益。
- **Tailwind CSS 4 保留并开始实际使用**。它已经在 `ui/package.json` 与 Vite 插件中，但目前只有 `@import 'tailwindcss'`，尚未形成 utility 体系。
- 新增 **Bits UI**：只承担 Dialog、Menu、Popover、Tooltip、Tabs、Switch、Slider 等交互原语。
- 新增 **@lucide/svelte**：替换手写 `Icon.svelte` 的 29 个路径，统一 24px 网格与 stroke。
- 首批**不新增**表格、虚拟列表、拖拽、路由、通知、主题管理库；现有实现已经更贴合本项目。
- **Bits UI 是 headless，不是现成皮肤**：它减少的是焦点管理、键盘导航、碰撞定位等行为代码，不会自动减少视觉 CSS。仍需要一层很薄的 AIYA `components/ui` 封装。

### 设计方向

**“本地音乐工作台”，不是流媒体网站。** 深色优先、浅色完整；扁平中性、封面显色；列表主导、播放常驻；高级能力按需出现。

不做：大面积渐变、层层玻璃、营销式 hero、大卡片瀑布流、所有按钮胶囊化、为了“现代感”牺牲信息密度。

## 1. 现状调研与保留边界

### 1.1 已核实的基础

| 现状 | 证据 | 本轮结论 |
|---|---|---|
| Svelte `5.57.2`、Vite `8.3.5`、TS `5.9.3` | `ui/package.json` | 版本新且匹配 Bits UI / Lucide，保留 |
| Tailwind `4.3.3` 与 `@tailwindcss/vite` 已配置 | `ui/vite.config.ts`、`ui/src/app.css` | 不重复安装，不引 PostCSS 旧配置 |
| Rust 生成类型、54 命令 typed IPC、浏览器 mock | `docs/17`、`docs/18` | UI 重设计不动契约与 store 数据语义 |
| 应用壳已有侧栏 / 主区 / 右侧情境面板 / 常驻播放条 | `components/AppShell.svelte` | 保留信息架构骨架，重画视觉与交互细节 |
| 明暗主题、52/42px 两档列表密度 | `styles/tokens.css`、`LibraryView.svelte` | 保留双主题与密度语义，重做 token 命名和色彩 |
| 29 个手写 SVG path | `components/Icon.svelte` | 用 Lucide 替换，品牌图形不在图标库范围内 |
| 菜单、焦点陷阱、弹层定位、外部点击为手写 | `ContextMenu.svelte`、`ConfirmDialog.svelte`、`PlayerBar.svelte` | Bits UI 最有收益的区域 |
| 固定行高虚拟化只有 60 行纯函数 | `lib/virtual.ts` | 不换库；它简单、可测，且已接入稀疏分页 |
| 5 万行游标分页、选择集、拖拽与滚动恢复已工作 | `TrackList.svelte`、`docs/18` | 不以“标准化”为由重写 |
| 窗口默认 1280×800，最小 960×600 | `src-tauri/tauri.conf.json` | 三档窗口验收，不设计移动端 |

### 1.2 与 docs/18 的关系

`docs/18` 的“不引 UI 组件库”服务于当时的**结构重构、行为不变**目标，已经完成。本文件开启的是新的视觉与交互阶段，只替代这一旧决策，不回退以下成果：

- `components → stores → ipc` 的依赖方向；
- Rust 生成类型为唯一 DTO 来源；
- 后端播放状态为唯一真相；
- 高频播放事件 rAF 合帧、频谱 canvas 不进响应式；
- 稀疏分页、虚拟列表、队列映射与选择集语义；
- 真窗口 / 浏览器 mock / `probe:ui` 双验收链。

## 2. UX 原则与全局骨架

### 2.1 三条任务优先级

1. **找歌并播放**：搜索、排序、筛选、列表、播放状态必须始终清楚。
2. **管理当下聆听**：当前曲、进度、队列、歌词、音量与输出状态不打断浏览上下文。
3. **整理本地曲库**：扫描、beets 预览/差异/应用是可恢复的任务流程，不伪装成普通小弹窗。

### 2.2 全局区域

| 区域 | 建议尺寸 | 职责 | 约束 |
|---|---:|---|---|
| 左导航 | 216–224px | 曲库入口、历史入口、播放列表、设置入口 | 导航位置稳定；技术路径与库统计降为次级信息 |
| 主内容 | 剩余宽度 | 当前视图的标题、工具条、主列表/主任务 | 永远只有一个 dominant region；不用等权卡片拼盘 |
| 右侧情境面板 | 320–336px | 队列 / 歌词 / 频谱等当下上下文 | 同时只开一个；打开不丢主列表滚动位置 |
| 底部播放条 | 88–96px | 当前曲、传输控制、进度、音量、面板开关 | 全局常驻；任何任务页都不能挤掉它 |
| 浮层 | 按内容 | 菜单、确认、短表单、tooltip | 复杂任务不塞浮层；浮层不叠浮层 |

### 2.3 窗口适配

- **≥1280px**：三栏可完整停靠，设计基准 1440×900，同时验证默认 1280×800。
- **1100–1279px**：右栏降至 304px；主列表先压缩专辑列，不动标题与播放控制。
- **960–1099px**：右侧情境面板改为覆盖层；播放条隐藏次要音频规格和音量滑杆，保留静音与核心控制。
- **高度 <700px**：减少头部留白，不压缩列表行到不可读；设置与诊断使用自身滚动区。
- 使用容器查询处理播放条和主列表，不只依赖 `window.innerWidth`。

### 2.4 浏览与播放行为

建议在下一轮改成桌面播放器惯例：**单击选择、双击播放、Enter 播放、Ctrl/Shift 多选**。当前“单击立即播放”会让浏览和选择太容易误切歌。

这是行为变更，不是换皮；下一轮页面分配时需明确确认。若决定保留单击播放，也必须提供清晰 hover/焦点与防误触反馈。

搜索文案必须匹配真实范围：只搜标题就写“搜索标题”，不能写成“搜索艺人、专辑”或“全局搜索”。

## 3. 视觉设计规范

### 3.1 色彩角色

界面保持中性，封面是主要彩色内容。动作色与封面氛围色分开：

- `accent-action`：按钮、焦点、选中、进度等交互语义，固定在可达对比度范围。
- `accent-artwork`：只用于当前曲封面周围的低强度氛围、装饰性渐变或频谱。
- 若保留“封面主色”设置，映射到动作色前必须做对比度/饱和度约束；不能直接拿任意封面色当文字和焦点色。

#### 深色主题（默认）

| Token | 值 | 用途 |
|---|---|---|
| `bg-canvas` | `#0F1115` | 应用底色 |
| `bg-sidebar` | `#12151B` | 左导航 |
| `bg-surface` | `#171A21` | 面板、播放条 |
| `bg-elevated` | `#1D2129` | 菜单、对话框 |
| `bg-hover` | `#22262F` | hover |
| `bg-selected` | `#293147` | 当前项/选中项 |
| `border-subtle` | `#282D36` | 分隔线、默认边框 |
| `border-strong` | `#3A414D` | 输入框、强边界 |
| `text-primary` | `#F2F4F8` | 标题、主要内容 |
| `text-secondary` | `#A7AFBD` | 艺人、说明、次级操作 |
| `text-tertiary` | `#7D8797` | 计数、非关键元数据 |
| `accent-action` | `#8EA6FF` | 焦点、进度、激活态 |
| `accent-on` | `#101322` | 实色主按钮上的文字/图标 |
| `success` | `#63C695` | 完成状态 |
| `warning` | `#E7B65C` | 可继续但需注意 |
| `danger` | `#FF7D85` | 删除、错误、危险操作 |

已计算 canvas 上的普通文字对比度：primary **17.16:1**、secondary **8.56:1**、tertiary **5.21:1**、accent **8.14:1**、danger **7.67:1**。

#### 浅色主题

| Token | 值 | 用途 |
|---|---|---|
| `bg-canvas` | `#F4F5F7` | 应用底色 |
| `bg-sidebar` | `#ECEFF3` | 左导航 |
| `bg-surface` | `#FFFFFF` | 面板、播放条 |
| `bg-elevated` | `#FFFFFF` | 菜单、对话框 |
| `bg-hover` | `#E7EAF0` | hover |
| `bg-selected` | `#E2E8FA` | 当前项/选中项 |
| `border-subtle` | `#D9DDE5` | 分隔线、默认边框 |
| `border-strong` | `#BEC5D1` | 输入框、强边界 |
| `text-primary` | `#171B23` | 标题、主要内容 |
| `text-secondary` | `#525C6B` | 次级文字 |
| `text-tertiary` | `#626C7B` | 非关键元数据 |
| `accent-action` | `#3F5FCC` | 焦点、进度、激活态 |
| `accent-on` | `#FFFFFF` | 实色主按钮上的文字/图标 |
| `success` | `#2F7D55` | 完成状态 |
| `warning` | `#8A5A00` | 注意状态 |
| `danger` | `#B42332` | 删除、错误、危险操作 |

已计算 canvas 上的普通文字对比度：primary **15.81:1**、secondary **6.21:1**、tertiary **4.87:1**、accent **5.17:1**、danger **5.97:1**。语义色在白色 surface 上也均达到普通文字 AA。

### 3.2 字体与数字

- 字体栈：`Segoe UI Variable, Segoe UI, Microsoft YaHei UI, PingFang SC, system-ui, sans-serif`；不增加网络字体依赖。
- 基础正文 **13px / 1.45**；列表主标题 **13px / 1.4**；次级元数据 **12px / 1.4**。
- 视图标题 **20px / 600**；面板标题 **14px / 600**；不设置营销式大标题。
- 11px 只用于非关键 badge/计数，不用于长说明和主要操作。
- 时长、序号、播放次数、采样率、码率、时间戳统一 `font-variant-numeric: tabular-nums`，不为这些数字单独引入等宽字体。
- 中文标签不做大写化和大字距；字距只可用于 `AIYA MUSIC` 品牌区。

### 3.3 间距、尺寸、圆角与层级

- 4px 基础网格：`4 / 8 / 12 / 16 / 20 / 24 / 32`，局部微调可用 2/6。
- 控件高度：小按钮 24、默认按钮/输入 32、主要按钮 36、主播放键 40。
- 图标：默认 16，导航/传输 18，主播放 20；stroke 统一 1.75 或 2，不逐图标调整。
- 列表行高沿用 **52 / 42px**，虚拟化与 CSS 必须继续读取同一个值。
- 圆角：badge 4、按钮/输入 6、菜单/面板 8、对话框 10；不把所有元素做成胶囊。
- 主区域以留白和 1px 分隔线分层，不用卡片阴影；阴影只属于浮层。
- 默认无 backdrop blur。若低性能模式或减少透明度设置出现，应能不改变层级地关闭所有模糊。

### 3.4 动效

| 场景 | 时长 | 方式 |
|---|---:|---|
| hover / 颜色变化 | 80–120ms | color/background/border |
| 菜单 / tooltip | 100–120ms | opacity + 2–4px transform |
| 对话框 / 侧面板 | 140–180ms | opacity + transform，不动画布局尺寸 |
| toast | 120–160ms | opacity + 4px transform |

- 不使用弹簧、回弹、页面级 slide、无限装饰动画。
- `prefers-reduced-motion: reduce` 时关闭非必要动效、平滑滚动与播放条 equalizer 动画。
- 频谱是实时数据可视化，不属于装饰动效；仍保持 canvas 自绘，不进入 Svelte 高频响应式。

## 4. 组件与交互规范

### 4.1 基础组件映射

| AIYA 组件 | 实现 | 关键规范 |
|---|---|---|
| Button / IconButton | 原生 button + Tailwind/语义类 | primary/secondary/ghost/danger；图标按钮必须有 `aria-label` 与 tooltip |
| ToggleButton | 原生 button，`aria-pressed` | 随机、循环、面板开关不能只靠颜色表示激活 |
| Dialog | Bits `Dialog` | 焦点陷阱、初始焦点、关闭后焦点还原、Esc 与外部点击策略 |
| ConfirmDialog | Bits `AlertDialog` | 危险操作默认焦点在取消；说明影响范围与是否可撤销 |
| ContextMenu / DropdownMenu | Bits 对应原语 | 碰撞翻转、typeahead、方向键、子菜单、危险项分隔 |
| Tooltip | Bits `Tooltip` | 统一 Provider；仅放补充信息，不承载唯一说明或错误 |
| Popover | Bits `Popover` | 倍速等短控制；Esc 关闭并还原焦点 |
| Tabs / SegmentedControl | Bits `Tabs` / `RadioGroup` | 设置页签支持方向键；即时互斥选项用 segmented |
| Switch | Bits `Switch` | 必须有可见文字标签，状态改变立即反馈 |
| Slider | Bits `Slider` | 音量可持续更新；seek/倍速拖动预览、松手提交 |
| Select | Bits `Select` | 选项较多或需解释时；2–4 个短选项优先 segmented |
| ScrollArea | 原生 overflow 优先 | 普通面板可后评 Bits；虚拟列表不套 ScrollArea |
| Toast / InlineError | 保留现有 notify store，重做样式 | 成功用 toast；阻断/持续错误用 inline，不滥用 modal |

Bits `Slider` 已有 `onValueChange` 与 `onValueCommit`，可保留“拖动只更新预览，松手才发 seek”的语义，避免高频 IPC。替换时需同时测鼠标、触控板与键盘。

### 4.2 曲目列表

- 列表是主区视觉中心，表头吸顶，标题列优先，时长右对齐；不用卡片包裹每一行。
- 当前播放、选中、键盘焦点是三种不同状态：当前曲用播放标识 + 标题色；选择用整行底色；焦点用 2px outline。
- hover 才显示的行内动作不能成为唯一入口；所有动作同时进入右键菜单/键盘菜单。
- 搜索高亮不只用颜色，保留 `mark` 背景与可读对比；不破坏标题截断。
- 加载只画可视区 skeleton；空数据、搜索无结果、分页失败分别呈现，不共用“没有音乐”。
- 保持固定行高、overscan、稀疏分页与滚动恢复；虚拟列表外不再包一层自定义滚动容器。
- ARIA grid 使用 roving tabindex：只有当前行进入 Tab 序列，不能每行都 `tabindex=0`；补齐行列标题、行号与选中状态语义。

### 4.3 播放条

- 左：封面、标题、艺人、格式；中：传输控制与进度；右：歌词/频谱/队列、音量。
- 无当前曲时显示明确空态，核心按钮按能力禁用，不显示假的 `0:00` 可拖进度。
- 加载、播放、暂停、错误必须可区分；音频错误提供“查看诊断”，不只 console.warn。
- 进度拖动时显示预览时间，提交前不改变后端位置；拖动中不被 10Hz 状态快照抢值。
- 倍速非 1× 时持续显示数值；提供“复位 1.0×”，不依赖用户知道双击。
- 随机、循环、静音、面板开关用 `aria-pressed` 或等价状态语义。
- 格式徽章只表达真实数据；无损/Hi-Res/未知三态不猜测。

### 4.4 队列、歌词、频谱与 beets

- 右侧面板标题区说明当前对象与数量；关闭不清空队列、不停止频谱之外的播放行为。
- 队列拖拽有来源态、插入线、自动滚动与取消；当前曲不能被误显示成普通可删除后“停止”的项。
- 歌词允许文本选择；用户手动滚动后继续遵守“不抢滚动条”，当前行不只靠颜色区分。
- 频谱设置不再展示实际无差别的“专业/简洁”选项；在 pro 真正可用前隐藏该选择，只保留频谱开关。
- beets 预览/差异/apply 是主任务流：有步骤、进度、取消、冲突与结果摘要；不能压缩成 320px 右侧栏里的长表单。
- 写标签/移动文件期间，关闭动作必须变成取消或明确禁止，并说明后果；成功后给可复查的结果，不只 toast 一闪而过。

### 4.5 反馈与异常

| 情况 | 呈现 |
|---|---|
| 首次无曲库 | 主区空态 + 选择目录主按钮；解释只读扫描，不暗示上传 |
| 搜索无结果 | 保留查询词 + 清除搜索；不显示“曲库为空” |
| 扫描中 | 非阻断进度条，显示阶段/计数/取消；主列表仍可操作 |
| 可重试失败 | 视图内错误条 + 重试；保留旧数据，不清空整页 |
| 操作成功 | 简短 toast；结果需要复查时同时留在任务结果区 |
| 删除/清空 | AlertDialog，写明数量与范围；仅索引移除和物理删除必须措辞不同 |
| 后台忙碌 | 控件显示进行态并防重复提交，不只禁用后无解释 |

## 5. 可访问性与键盘

- 普通文字对比度至少 4.5:1；图标、边框、焦点指示至少 3:1。状态不能只靠红/绿或明暗区分。
- 所有交互可只用键盘完成；焦点顺序与视觉顺序一致，任何浮层关闭后回到触发点。
- 焦点环统一 2px `accent-action` + 2px offset，不因鼠标样式而永久移除 `:focus-visible`。
- 桌面指针目标以 28–32px 为主；密集列表使用整行作为目标，不硬套移动端 44px。
- 装饰图标 `aria-hidden`；独立图标按钮必须有可读名称。tooltip 不能替代名称。
- toast 使用 polite live region；只有真正阻断的错误才使用 assertive，避免播放状态 10Hz 更新被朗读。
- `body user-select:none` 不能扩散到歌词、诊断、路径、错误详情、设置说明和 beets 差异内容。
- 快捷键录制期间屏蔽全局快捷键；Esc 只取消录制，不能连带关闭设置。
- 支持 `prefers-reduced-motion`；后续补一轮 Windows forced-colors 基本检查。

## 6. 依赖选型

### 6.1 首批直接新增

版本为 2026-10-25 查询 npm registry 与官方文档得到的当前值；实际安装沿用仓库 caret 范围并提交 lockfile。

| 包 | 建议范围 | 当前核实 | 许可 | 用途与理由 |
|---|---|---|---|---|
| `bits-ui` | `^2.20.2` | peer `svelte ^5.33.0` | MIT | Svelte 原生 headless；覆盖本项目最费时且最易出错的浮层/焦点/键盘行为 |
| `@lucide/svelte` | `^1.61.0` | peer `svelte ^5` | ISC；部分 Feather 图标 MIT | 统一图标网格、类型与 tree-shaking，删除手写 path 维护 |

补充：

- Bits UI 的 npm peer 还声明 `@internationalized/date ^3.8.1`。首版不使用日期组件，不在业务代码 import；安装时让 npm 解析并在 lockfile 固化，若包管理检查要求显式 peer，再作为依赖补齐。
- Lucide 使用具名导入或 `@lucide/svelte/icons/*` 直接导入；集中维护“语义动作 → 图标”映射，禁止动态导入整包图标。
- 保留现有 `Icon.svelte` 作为薄适配层也可以，但内部只映射 Lucide 组件，不再保存 SVG path。

### 6.2 已有依赖继续用

| 包 | 决策 |
|---|---|
| `tailwindcss` + `@tailwindcss/vite` | 已配置，继续；通过 `@theme inline` 映射语义 token，不在组件里写原始色值 |
| `zod` | 继续只做磁盘/外部边界校验，不用于每个 UI 组件 |
| `audiomotion-analyzer` | 暂保留现状，不扩大使用；pro 无真实 AudioNode 前不宣传其能力 |
| `@tauri-apps/api` | 继续只存在 IPC transport 等宿主边界 |

### 6.3 明确不引入 / 暂缓

| 候选 | 决策 | 原因 |
|---|---|---|
| `@tanstack/svelte-virtual` | 暂缓 | 当前固定行高计算仅 60 行且已接通稀疏分页；换库收益小、回归面大。出现可变行高/专辑网格后再评 |
| `@tanstack/svelte-table` | 不引入 | 排序、筛选、游标、选择集由后端查询与现有 store 决定，table 状态只会形成第二真相 |
| 新 DnD 库 | 不引入 | 当前行拖拽与随机队列重映射已验证；先修表现，不换行为引擎 |
| Router | 不引入 | 桌面单窗口视图状态已足够，无 URL/SSR 需求 |
| `svelte-sonner` 等通知库 | 不引入 | 现有 notify store 已区分常驻错误和 toast，重做展示即可 |
| `mode-watcher` | 不引入 | 现有主题设置、持久化与宿主首帧逻辑更完整 |
| Melt UI / 第二套 headless | 不引入 | 与 Bits UI 职责重复 |
| 完整 shadcn-svelte 主题/组件拷贝 | 不整套引入 | 会带入不需要的站点风格与组件量；只借鉴“薄封装、自有源码”方式 |
| Bootstrap / 完整 CSS 框架 | 不引入 | 与 Tailwind、token 体系冲突，桌面密度也不匹配 |
| `clsx` / `tailwind-merge` | 暂不引入 | Svelte `class:` 与少量本地 class 组合已够；真实出现冲突再补 |

### 6.4 Tailwind 使用边界

- Token 是唯一色彩来源；Tailwind 负责布局、间距、尺寸、状态和响应式，不出现 `text-[#...]`、任意 z-index、任意行高散落。
- 高复用且带语义的组合封装为 `components/ui/*`，不在每个视图重复十几行 utility。
- 复杂虚拟列表、播放条和 canvas 周边可保留语义 CSS；不追求“全部 utility 化”。
- 语义命名优先 `bg-surface/text-secondary/border-subtle`，禁止在业务组件直接引用 blue-500、gray-700 等原始调色板。

## 7. 下一轮实现时的建议结构（本轮不创建）

```text
ui/src/
  components/
    ui/                 Button、IconButton、Dialog、Menu、Tooltip、Slider 等薄封装
    icons.ts            语义动作到 Lucide 的唯一映射
    ...                 现有领域组件
  styles/
    tokens.css          原始语义变量、明暗主题、密度
    theme.css           Tailwind @theme inline 映射
    base.css            reset、focus、selection、reduced motion
    ...                 复杂领域组件样式
```

建议迁移顺序：**token → 基础控件 → 浮层 → 播放条 → 列表 → 各页面**。每一步都可独立验收，避免一次性换肤后无法定位回归。

## 8. 验收口径

### 功能与性能

- 后端契约、IPC 参数、store 数据语义不变；`gen:types:check` 通过。
- `npm run check` 0 error / 0 warning；`cargo test --workspace` 全绿；`npm run build:ui` 通过。
- 真窗口与 mock 都跑 `probe:ui --play`；无未捕获异常、无 console.error、无非预期资源 404。
- 5 万行滚动仍只渲染可视区 + overscan；滚动恢复、搜索、排序、Ctrl/Shift、多选批量、右键、拖拽逐项通过。
- 播放进度拖动只在 commit 时 seek；切歌、暂停、队列编辑、窗口隐藏恢复无回归。
- 依赖安装前先记录 bundle 与交互基线，完成后比较主 chunk、首屏与列表帧率，不只报告“构建成功”。

### 视觉与可访问性

- 960×600、1280×800、1440×900；深色/浅色；52/42 两档密度全部覆盖。
- 核心流程只用键盘可完成；菜单/对话框焦点不逃逸，关闭后回到触发点。
- 文本对比度达 AA；焦点可见；状态不只靠颜色；reduced-motion 生效。
- 无文本溢出遮挡：长中文/英文标题、长路径、未知元数据、0/5 万计数都要测。

## 9. 下一批页面分配时需要确认的事项

1. 曲目行为是否改为“单击选择、双击播放”；本规范推荐改。
2. 设置保持大 Dialog，还是成为主区独立页面；诊断更适合与设置分区共存。
3. beets 导入整理改为占主区的分步任务流，右侧栏不再承担该流程。
4. 封面主色只驱动氛围色，还是经过对比度约束后也驱动动作色；本规范推荐前者。
5. 页面清单、各页主任务与导航归属；确认后再出逐页线框和组件分配。

## 10. 调研来源

- 本地：`ui/package.json`、`ui/vite.config.ts`、`components/AppShell.svelte`、`TrackList.svelte`、`PlayerBar.svelte`、`SettingsPanel.svelte`、`ContextMenu.svelte`、`ConfirmDialog.svelte`、`lib/virtual.ts`、`styles/*`、`docs/17`、`docs/18`、`src-tauri/tauri.conf.json`。
- Bits UI 官方介绍与组件文档：<https://www.bits-ui.com/docs/introduction>、Dialog / ContextMenu / Slider / Tooltip / Tabs。
- Lucide Svelte 官方指南：<https://lucide.dev/guide/svelte/getting-started>。
- Tailwind Vite 官方指南：<https://tailwindcss.com/docs/installation/using-vite>。
- TanStack Virtual 官方指南：<https://tanstack.com/table/latest/docs/framework/svelte/guide/virtualization>。
- npm registry：`bits-ui@2.20.2`、`@lucide/svelte@1.61.0`、`@tanstack/svelte-virtual@3.13.48`。
