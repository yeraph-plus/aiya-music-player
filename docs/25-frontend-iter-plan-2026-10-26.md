# docs/25 — 前端迭代批：EQ 抽屉化 · 顶栏对齐 · 信息架构重排 · 正在播放改版 · 毛玻璃四层

日期：2026-10-26。状态：**已落地（T1–T4b 全部完成，同日）**。前置：docs/24 B0–B6 已收官（`637b4bd`），P5 打包搁置。

本批是**纯前端批**：不新增后端命令（EQ/队列/频谱的命令与事件都已在协议 v6 就位），不动契约层。五项按依赖关系切成五刀，刀与刀之间可独立提交、独立验收。

**落地提交链**：T1 `0f6c8d7`（信息架构）→ T2 `7eaabcc`（顶栏）→ T3 `9f0ea33`（EQ 抽屉）→ T4a `2b1be59`（两栏改版）→ T4b `265b2f5`（背景四层）。最终验证：svelte-check 0/0、check:deps 0 环（102 模块）、e2e 19/19、JS 412 kB / CSS 53.3 kB。

---

## §1 EQ 整体抽屉化（操作栏，与播放列表同级）

### 现状
- EQ 是正在播放页三块可切（歌词/频谱/均衡器）之一，藏在页面里（`NowPlayingView.svelte` 的 SegmentedControl 第三项）。
- 播放列表（队列）已经是播放条右侧的 Popover 抽屉（B5a：裸 button 触发 + bits-ui Popover，几何在 `player.css .queue-float`）。

### 方案
- `PlayerBar.svelte` 右侧键区加一颗 EQ 键（`SlidersHorizontal` 图标，EQ 启用时常亮/打点），复用队列抽屉的**同一套壳**：裸 button + `Popover` + `onopenchange` 受控。
- `EqPanel.svelte` 原样搬进抽屉内容（store 不动；面板宽度和队列抽屉对齐，约 340px）。
- `NowPlayingView` 的 NowTab 从三值缩到两值（歌词/频谱）——第 §4 刀会把这两个也拆掉，本刀只是先摘 EQ。
- **互斥规则**：EQ 抽屉与队列抽屉同侧同层，一个打开时关另一个（`ui` store 里两个开合状态互相清理，`setQueue(false)` / `setEqDrawer(false)`）。

### 影响面
`PlayerBar`（+2 prop：`eqOpen`/`oneqopen`）、`ui` store（+`eqOpen` 状态）、`NowPlayingView`（删 eq 分支）、e2e nowplaying 的「EQ 预设」用例改为从播放条入口进入。UI 持久化不涉及（抽屉开合是会话态，与队列一致）。

---

## §2 顶栏：高度、品牌区对齐侧栏、内容对齐主区边距

### 现状（tokens.css / TitleBar.svelte / layout.css）
- `--titlebar-h: 46px`；品牌区 `padding-left: 12px` 就地起排，与 216px 的侧栏（`--sidebar-w`）**没有对齐关系**。
- 返回键和搜索框紧跟品牌区，与主区内容的 22px 边距（`--space-6`，`.page-scroll`/`.page-pad`）也没有对齐关系。
- TitleBar 是全宽横条（`grid-column: 1 / -1`），这条几何事实是对齐方案的前提。

### 方案
- `--titlebar-h: 46px → 54px`（播放控制更舒展，也匹配增加后的品牌区视觉重量）。
- 品牌区定宽 = `var(--sidebar-w)`（216px），内部左内距与侧栏导航文字的缩进一致（10px 容器 + 9px 项内距 ≈ 品牌标记的落点）；品牌区右缘正好压在侧栏分隔线的延长线上。
- 返回键左缘 = 侧栏宽 + 22px（`margin-left: var(--space-6)`），与 `.page-pad` 的内容左边距同一条竖线；搜索框跟在返回键后，同一节奏。
- 窗口三键区不动。中段 `.tb-drag` 拖拽余量自动吸收差值。

### 影响面
纯 `TitleBar.svelte` 样式 + `tokens.css` 一个变量。零逻辑改动；e2e 不受影响（没有对顶栏几何的断言）。

---

## §3 侧栏信息架构重排

### 现状
- 一级导航八项：首页 / 曲库 / 最近播放 / 常听 / 专辑 / 艺人 / 正在播放 / 设置。
- 曲库页内置 tab 三项：全部曲目 / 最近播放 / 常听（三视图共用同一套曲目列表）。
- FM 电台在 B3 被移出导航（`RadioView` 是占位组件，路由值 `'radio'` 还在 `AppPage` 里）。

### 方案（按用户指定顺序）
侧栏一级导航新排布：

```
正在播放  →  page 'nowPlaying'
推荐      →  dashboard（原「首页」改名：歌单宫格 + 三段预览本来就是推荐位）
曲库      →  builtin 'library'
最近播放  →  builtin 'recent'（独立页，不再作为曲库 tab）
常听      →  builtin 'mostPlayed'（独立页，不再作为曲库 tab）
FM 电台   →  page 'radio'（占位页回归入口）
设置      →  page 'settings'（保持末位不动——用户没提，默认保留）
```

曲库页内置 tab 换血：

```
全部曲目（library） / 专辑（albums） / 艺人（artists）
```

- **实现是导航式而不是嵌套式**：tab 点击走 `nav.go({ kind: 'browse', view: 'albums' | 'artists' })`，`albums`/`artists` 两个宫格页已存在，`AppPage` 13 值、路由快照、`isListPage` **全部零改动**。
- tab 的显示条件从 `page ∈ {library, recent, mostPlayed}` 改为 `page ∈ {library, albums, artists}`；active 判定直接比 `ui.page`（现有 `Toolbar active={ui.page}` 机制不用动）。
- 最近播放 / 常听从 tab 里摘掉后，其页头/动作按钮（清空历史）逻辑不变，只是少了一层「共享列表」的视觉暗示；`switchView` 的白名单同步收窄。
- 侧栏删掉专辑/艺人两个一级项（宫格页仍可达：曲库 tab + 搜索结果页聚合卡 + 右键菜单）。
- 「最近播放/常听」保留在侧栏 = 用户原话「不需要…共用这个页面列表」的正确解读：它们仍是完整页面，只是不再寄居在曲库的 tab 行里。

### 影响面
`Sidebar.svelte`（PAGES 数组重排 + 删两项 + 首页改名）、`LibraryView.svelte`（BUILTIN_TABS/showBuiltinTabs/switchView）、`HomeView.svelte`（Header title「首页」→「推荐」）。e2e：route.spec 8 条与 smoke 的侧栏相关选择器要跟着改文案（「首页」→「推荐」）。

---

## §4 正在播放页改版：拆卡片，左信息右歌词，频谱拆出

### 现状
`NowPlayingView` = hero 卡片（132px 封面 + 标题 + 艺人·专辑 + 音频规格徽章）+ 三块可切的切换条 + 内容卡。整个页面是「卡片摞卡片」。

### 方案
取消卡片区域，改两栏沉浸布局：

```
┌──────────────────────────────────────────────┐
│ [背景四层结构，见 §5]                          │
│                                              │
│   ┌ 左列（约 40%） ┐   ┌ 右列（歌词） ──────┐ │
│   │ 大封面（≤280px）│   │  LyricsPanel      │ │
│   │ 标题（大字号）  │   │  （居中逐行高亮，  │ │
│   │ 艺人 · 专辑     │   │   现有滚动逻辑）   │ │
│   │ 音频规格徽章    │   │                   │ │
│   └───────────────┘   └───────────────────┘ │
└──────────────────────────────────────────────┘
```

- 左列内容 = 现 hero 的全部信息（封面/标题/艺人/专辑/规格），但**去卡片化**：无边框无底色，直接浮在背景上（可读性由 §5 的遮罩层负责）。
- 右列 = LyricsPanel 原样，外框也去掉（容器样式改透明），逐行歌词保留 `.selectable`。
- SegmentedControl 整个删除；LyricsPanel 的 `onclose` 空置。
- **频谱拆出**：SpectrumPanel 不再作为页面内容块。频谱数据（48 带 tap + `spectrum:frame`）转投 §5 的背景环境层。pro 五形态渲染器的去留见 §7 待拍板项 2。
- EQ 已在 §1 迁往播放条抽屉。

### 影响面
`NowPlayingView.svelte` 大改（结构 + 样式）、`pages.css` 的 now-playing 段重写、e2e nowplaying.spec 的三块可切用例重写（改为验证两栏布局 + 背景层存在）。

---

## §5 毛玻璃四层结构（重点调研项）

### 用户设想
第一层背景图/背景色 → 第二层频谱叠在背景上 → 第三层毛玻璃遮罩层 → 第四层当前界面布局。

### 可行性结论：**结构成立，但第三层的实现方式是性能生死线**

调研到的关键事实：

1. **backdrop-filter 的开销模型**（多个来源一致）：它把元素背后的所有内容光栅化进离屏 buffer → 滤镜 → 再合成。**背后内容每变一帧，离屏 buffer 就重建一次**。频谱 canvas 是 rAF 连续动画，等于要求合成器每帧对全屏区域做一次大半径高斯模糊——这是最贵的用法。
2. **业界产品的实际做法**：Apple Music / 网易云 / QQ 音乐的沉浸播放页，背景一律是**预模糊**——复制封面图层，对图片本身 `filter: blur(60~100px) + saturate()`（静态内容，光栅化一次就被缓存，之后零每帧成本），上面再叠一层主色底和暗色渐变保证可读性。**没有一家把动画内容垫在 backdrop-filter 下面**。
3. **Apple Liquid Glass（iOS 26）**：折射/高光是系统级 shader，web 只能近似（backdrop-filter + 边框高光渐变）；且 Apple 的准则也是「用于小控件，不是全屏垫底」。
4. **安全阈值**（来源给的实测量级）：blur 半径越大成本非线性上涨；固定面板 ≤5px、模态 ≤8px 是中低端机的安全线。桌面独显上全屏 blur(20px+) 通常可行，但 WebView2 环境必须实测。

### 推荐实现（B 方案：预模糊背景 + 频谱自发光 + 局部玻璃）

```
第 1 层  背景层：当前曲目封面铺满 → filter: blur(64px) saturate(1.4) + 主色底
         （静态，浏览器缓存光栅化结果；无封面时退回主题底色 + 现有 glow 渐变）
第 2 层  频谱环境层：透明 canvas，48 带 tap 驱动，低透明度 + 发光绘制
         （在画布内部做柔化——先画粗柱再 ctx.filter='blur()' 或 drawImage 降采样放大，
          频谱自己就是"叠在背景上的光"，不依赖外层玻璃）
第 3 层  可读性遮罩层：从下往上的暗色/亮色渐变（跟随主题）保证文字对比度；
         真正的 backdrop-filter 毛玻璃只给**局部小面板**（标题信息块、控制键）
第 4 层  内容层：§4 的两栏布局
```

### 备选（A 方案：忠于字面四层，全屏 backdrop-filter）
第三层做成全屏 `backdrop-filter: blur(24px)` 盖住频谱——频谱透过毛玻璃变成"光晕"，效果最接近字面描述。**代价**：播放期间每帧全屏重模糊。这不是否决项，桌面 GPU 上很可能 60fps，但必须：CDP 探针实测帧率（tap 打开时 vs 关闭时）→ 达标才转正；设置里留「背景效果：完整/精简」降级开关；`contain: paint` 限制重绘面。

### 建议的推进方式
**先做 A/B 的 30 分钟 spike**（一个临时页面两种实现，DevTools Performance + 探针测帧率），用数据定第三层。A/B 的第 1/2/4 层完全相同，spike 期间互不阻塞。

### T4d 增补（同日晚，用户四点反馈）

1. **顶栏/播放条同档半透明**：统一 90%，底色分别用 surface-2 / surface-3——同档不同阶，叠起来有一点色差变化。
2. **背景脱离封面**：预模糊封面与封面主色提取整条移除（`lib/accent.ts` 删除、`accentFromArtwork` 设置删除、App.svelte 主色 effect 删除）；改为 **5 套固定 CSS 渐变 + 配套强调色**（aurora 极光 / ember 流霞 / meadow 苔原 / dusk 暮紫 / mono 素白），`html[data-ambient]` 切换，深浅主题各一份渐变与强调色三件套，设置页「背景方案」SegmentedControl 切换。`--ambient-bg` 是新 token；强调色覆盖 `--accent/--accent-weak/--accent-text`（aurora/mono 用主题默认蓝）。spec 加了 CSS 特异性坑备注：暗色方案块必须排在浅色主题块之后（同为 (0,1,1) 靠顺序），浅色变体用双属性选择器 (0,2,1) 压回。
3. **沉浸档撤销遮罩**：正在播放页「看不到频谱」的根因就是第 3 层遮罩底部 72% 的 surface-1 把 0.55 透明度的频谱光压没了——`immersive` 时 veil 整个不渲染；standard 档保留。
4. **正在播放信息区**：去 `.np-meta` 玻璃（无底色无边框）；艺人/专辑各占一行；标题 `-webkit-line-clamp: 2`；封面 300→240px；歌词面板摘头部（「歌词」标题与格式小字），`.np-lyrics .lyrics-panel` 网格改单行（player.css 的 `auto 1fr` 是给带头版的）。

影响范围核对（第 2 项，改动前已核）：settings.ts（类型+默认值+方案清单）、schema.ts zod、look.ts（Look+ambient、镜像校验）、env.d.ts（`__AIYA_LOOK__` 形状）、App.svelte（look effect 三字段、删主色 effect）、SettingsPanel（行替换）、AmbientBackdrop/AppShell（img→渐变 div、删 bgArt）、tokens.css（5×2 方案块）、e2e（启动断言改渐变层常在、沉浸页断言 veil count 0）。`artwork-blob.ts` 保留（Media Session 封面仍用）。

### T4c 增补（同日，用户反馈「四层应是全应用而非本页局部」）

背景从 NowPlayingView 局部组件升为 **AppShell 根部的全应用层**，四区席位共享一套四层结构：

- `AmbientBackdrop` 绝对定位在 `.app` 根部（z-index:0）；四区席位（顶栏/侧栏/主区）补 `position:relative; z-index:1` 才能压在其上——**静态元素会被定位元素盖住（含文字）**，不补席位层级直接翻车。player-dock 自带 z-20、BatchBar z-30 不变。
- **持久壳层禁用 backdrop-filter**（上表「全应用玻璃一致性」一条作废，红线升格为全应用规则）：顶栏/侧栏/播放条/歌词·队列·频谱面板组的 blur 全部摘除，换半透明底色（`color-mix` 80–88%）。玻璃只留给瞬时浮层（对话框/菜单/Popover，打开时才付钱）与 `.np-meta` 一个展示性小块。
- 遮罩双档：`immersive` prop（正在播放页，浅遮罩放光）/ standard（其余页面，近均匀重遮罩保列表可读性）；封面背景层透明度同理双档（0.85 / 0.5）。频谱环境光全应用生效（设置 + reduced-motion 门控不变）。
- 歌词面板在沉浸页真透明（player.css 给 `.lyrics-panel` 的 surface-2 底色在 `.np-lyrics .lyrics-panel` 掀掉——T4a「透明容器」的意图这才真正落地）。
- 频谱源与背景小图的派生随背景一起搬进 AppShell；NowPlayingView 只剩内容层（第 4 层）。
- e2e：启动用例补全应用断言（推荐页有遮罩层、未播放无封面/频谱层）；原有沉浸页断言不变。

### T4e 增补（同日晚，用户五点反馈：抽屉化收口 + 播放条按键重排）

1. **沉浸档频谱去虚化直出**：正在播放页（immersive 档）频谱绘制不再过 `ctx.filter: blur(26px)`——柱子贴底满幅、不透明度 0.9 直出（本页主视觉）；standard 档（列表页）保留"柔化成环境光"的画法。
2. **正在播放入口迁移**：侧栏撤掉「正在播放」席位（六项：推荐/曲库/最近播放/常听/FM 电台/设置）；唯一入口 = 播放条左侧歌曲区域（`.now` 整体变 button，无曲目也可进空态页）。
3. **SideDrawer 替代气泡**（`ui/SideDrawer.svelte` 新组件）：队列/EQ 抽屉从 bits-ui Popover 气泡迁到**窗口左缘滑出**的固定抽屉——气泡的定位由 floating-ui 每次测量+碰撞翻转，开合有可见抖动；抽屉几何确定（贴左缘、全高、定宽）零测量。Esc/遮罩点关/焦点还原手写（对齐 bits-ui Dialog 行为）。**两个实测坑**：① `.sd-root` 全屏容器自己会拦截指针，必须 `pointer-events:none` + 子层恢复，否则遮罩让出的播放条区域照样点不到；② 遮罩底部让出 `--player-h` + 触发键改 toggle——抽屉开着时另一个触发键可直接点（互斥切换不被遮罩吃掉点击）。倍速保持 Popover 气泡（小面板贴键无抖动）。
4. **播放条按键重排**：主五键 = 加入歌单（新增，复用全局 addToPlaylist 对话框）/ 上一首 / 播放 / 下一首 / **模式单键循环**（列表循环→单曲循环→随机→顺序；后端无"模式"字段，UI 把 repeat+shuffle 两正交开关组合成四态，`player.setMode/cyclePlayMode`）；右侧 = 队列抽屉键 / EQ 键 / 倍速 chip / 静音 / 音量（正在播放入口键删除——入口已迁歌曲区域）。快捷键动作 id `cycleRepeat` 是持久化键不改名，实现指向 `cyclePlayMode`。
5. `.k-pop.queue-pop` 覆盖类随气泡壳一起删除；`queue-float`/`eq-drawer-body` 几何归零（全出血铺满抽屉面板）。

验证：svelte-check 0/0；check:deps 0 环（102 模块）；e2e 19/19（正在播放入口断言改播放条歌曲区、路由用例补 `via:'player'` 分支 + 正在播放页侧栏高亮数 0、互斥双向断言不变仍绿）；JS 424 kB / CSS 58.9 kB。

---

## §7 待拍板项（开工前确认）

1. **设置入口**：用户列的侧栏顺序没有「设置」，按保留在末位处理（若要移去别处请指出）。
2. **pro 频谱五形态的去留**：环境频谱用哪个形态？推荐——背景环境层用现有 compact 引擎的低透明度发光绘制（数据同源、实现最薄）；pro 五形态（径向/LED/镜像/反射）暂留代码，入口后补（如「沉浸模式」按钮或设置页选择），本批不删能力。
3. **毛玻璃第三层 A/B**：全屏真毛玻璃（A，需 spike 实测 + 降级开关）vs 渐变遮罩 + 局部玻璃（B，推荐，性能安全）。spike 数据出来再定。
4. **「推荐」是否承载更多**：只改名（零逻辑），还是顺便把首页三段预览的内容/排序调整？默认只改名。

## §8 批次切分与验收

| 刀 | 内容 | 依赖 | 验收 |
|---|---|---|---|
| T1 | §3 信息架构重排（侧栏 + 曲库 tab + FM 回归 + 改名推荐） | 无 | ✅ `0f6c8d7` e2e 全绿（含侧栏高亮恰好一个；抓到并修掉"宫格页侧栏零高亮"缺口） |
| T2 | §2 顶栏对齐 | 无 | ✅ `7eaabcc` 纯样式（--titlebar-h 54px、品牌区=--sidebar-w、返回键 margin-left=--space-6） |
| T3 | §1 EQ 抽屉化 | 无（可与 T1/T2 并行） | ✅ `9f0ea33` 开合/互斥/Esc/焦点还原 e2e 断言；**踩坑：Menu 0×0 锚点必须 portal 到 body**（transform 祖先内 position:fixed 退化为容器相对定位，菜单飞出视口） |
| T4a | §4 正在播放两栏改版 | 建议 T3 先行（少一个 tab 分支） | ✅ `2b1be59` 两栏 + 无卡片；hero/tabs/页内频谱块拆除；SpectrumPanel 源码保留（pro 入口后补），生产构建摇掉 |
| T4b | §5 背景四层 + spike | T4a | ✅ `265b2f5` **用户拍板走方案 B，spike 免做**：预模糊封面（静态 blur 64px）+ 频谱低分辨率 buffer 放大自柔化 + 主题遮罩；真 backdrop-filter 只给 .np-meta 局部小块；reduced-motion 降级 |
| 收尾 | docs/25 各章节实测 + docs/23 §7 遗留项核对 | — | ✅ workspace 全测 + e2e 19/19 + check 0/0 + 0 环 + 包体积 412 kB JS / 53.3 kB CSS |

### 落地增补记录（2026-10-26）

- **拍板结果**：设置留侧栏末位；pro 五形态暂留代码（入口随沉浸模式后补）；玻璃第三层直接走 B（不跑 A/B spike——业界调研已一致，A 的全屏 backdrop-filter 垫动画 canvas 是最贵用法，不值得花 30 分钟验证一个已知更贵的路）；「推荐」只改名。
- **T1 实测缺口**：专辑/艺人并入曲库 tab 后宫格页侧栏零高亮 → `isActive` 让曲库项吸收 `albums`/`artists` 两页（e2e「高亮恰好一个」抓住）。
- **T3 实测坑（Menu 坑 2）**：bits-ui Popover.Content 用 transform 定位，transform 祖先内的 `position:fixed` 退化为"相对该容器定位"——Menu 的 0×0 点击坐标锚点在 EQ 抽屉里直接飞出视口。修法：锚点 portal 到 body（对 body 层宿主的右键菜单零差异）。
- **T4b 结构**：`AmbientBackdrop.svelte` 三层（背景 img blur 64px / 频谱 128×72 buffer 放大 + ctx.filter blur 26px / color-mix 遮罩），第 4 层为两栏内容。频谱 tap 按需订阅（进页面 start、离开 stop、设置关 = 引擎零开销）。
- **e2e 覆盖变更**：pro 五形态 chip 用例随页内频谱块拆除而移除，待沉浸模式入口回归时补；新增两栏布局/背景层/EQ 抽屉互斥/曲库 tab 导航断言。
- **仍未做（移交后续）**：真机走查（docs/23 §7 的快捷键 16 动作、主题密度目视、960px 队列抽屉、forced-colors）+ 本批新 UI 的真机目视（背景四层观感、顶栏对齐）；pro 五形态入口；P5 打包。

顺序建议：T1 → T2 → T3 → T4a → T4b（T2/T3 可并行）。全部是前端批，每刀跑 `npm run test:e2e` + `check`，收尾跑全量。
