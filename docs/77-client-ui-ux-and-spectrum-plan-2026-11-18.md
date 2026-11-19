# 77 · 客户端 UI/UX 统一治理与频谱替换计划（2026-11-18）

- 类型：计划
- 状态：closed（P0、A1–A3、B1、B2、C 完成；扩展设备与长时走查归 docs/35）
- 关联：docs/19、23、24、27、46、74、75、76、78；docs/35 近期迭代；仓库外 spectrum-lab-2026-11-18（docs/78 为本批整批质量审查）

## 1. 目标与范围

本轮把逐次手动调整形成的界面收拢成可复用、可检查的设计体系。交付分两部分：

1. **工作包 A：逐页组件 kit 治理。** 优先采用 Bits UI 交互原语，统一视觉参数，清理绕过 kit 的局部实现；覆盖应用壳、全部页面、工具子页与浮层。
2. **工作包 B：频谱视觉替换。** 背景采用 B0 / B1 / B4，播放页前景采用 F1 / F2 / F4 / F5 / F6 / F7，接入已有频谱源与共享 Three.js 舞台。

本文件承接调研、实施方案与分批验收。P0 的开发预览与现状基线、A1–A3 的共享控件和逐页治理、B1/B2 的九种频谱与共享舞台、C 的整体终验与真实宿主检查均已完成；本机证据及未实测边界见 §17。

### 1.1 已确认的边界

- 沿用 Svelte 5、TypeScript、Vite、Bits UI、Lucide、现有导航与 store 分层；不更换框架，不增加路由库或 URL 状态。
- 保留左侧栏、标题栏、内容区、底部播放条的骨架，不增加第三列；支持桌面与 Web 宿主，最低窗口仍为 960×600。
- 保留普通页面的「装饰英文 + 大标题 + 描述行」。正在播放页保持沉浸布局；频道内层工作台保持小标题，不加第二套装饰英文。
- 保留 docs/75 已确定的推流页面结构与交互，保留 docs/76 的桌面目录选择器 / Web 容器内路径输入差异。
- 不改变音频后端、播放器权威状态、队列语义、DTO、Liquidsoap 节目模型；不重新设计扫描、Beets 或部署流程。
- 干净替换旧视觉、旧枚举和失效样式，不增加旧版视觉兼容层、设置映射或迁移页。

### 1.2 治理方式

先完成共享零件与代表页定标，再逐页应用。每次替换同时移除原来的局部交互和样式，避免 kit 与页面两套实现并存。组件数量减少不是独立指标：静态卡片和业务时间网格仍需要应用代码，复杂焦点、选择与弹层行为优先交给 Bits UI。

## 2. 联网调研与设计取向

检索与核对日期：2026-11-18。以下结论来自官方页面、官方截图与文档；没有安装或运行这些产品客户端，也没有把营销图片当作完整操作验证。

| 参考 | 本次观察 | 应用到 AIYA 的部分 | 不采用的部分 |
|---|---|---|---|
| [Feishin 官方截图](https://github.com/jeffvli/feishin#screenshots) | 曲库表格、专辑曲目、艺人专辑区、封面与歌词的层次清楚，底部播放条稳定 | 作为本地音乐工作台最接近的布局参考；统一详情标题与曲目工具条，明确浏览卡片和管理表单的密度差异 | 不照搬其导航与服务端概念，不增加右侧常驻栏 |
| [Apple Music Windows 指南](https://support.apple.com/zh-cn/guide/music-windows/welcome/windows) | 资料库入口、专辑封面与文字的等级稳定，歌词成为播放场景的焦点 | 稳定导航、封面优先的浏览、克制的分隔、明确的当前歌曲层级 | 不引入商店与订阅推荐入口，不把传输控制搬到顶部 |
| [Plexamp 官方介绍](https://www.plex.tv/plexamp/) | 封面、模糊色彩与可视化构成播放氛围，正文仍有独立阅读区域 | 背景退后、前景有轮廓、歌词有衬底；以真实页面叠加检查频谱 | 不把移动端比例直接放大成桌面布局，不复制宣传页的大面积装饰 |
| [Spotify 桌面与 Web 重设计说明](https://newsroom.spotify.com/2021-03-25/introducing-a-new-spotify-experience-across-desktop-app-and-web-player/) | 2021 年文章强调桌面/Web 一致性、歌单整理、拖放、队列与快捷键 | 同一任务在不同页面、宿主保持相同操作顺序和反馈；拖动提供键盘替代 | 这是历史设计说明，不作为 2026 当前外观证据；不引入商业信息流和三列布局 |

方向定为：**曲库浏览保持紧凑，配置页面保持清楚，播放页面承载氛围。** 页面之间共享字体、间距、控件和反馈，密度按任务区分，不把整个应用做成同一种大卡片布局。

### 2.1 Bits UI 与审查依据

- [Bits UI 组件索引](https://bits-ui.com/llms.txt)与 [Button](https://bits-ui.com/docs/components/button)、[Dialog](https://bits-ui.com/docs/components/dialog)、[Popover](https://bits-ui.com/docs/components/popover)、[Tabs](https://bits-ui.com/docs/components/tabs)、[Time Range Field](https://bits-ui.com/docs/components/time-range-field) 文档已核对，并对照仓库安装的 `bits-ui 2.20.2` 类型与实现。
- Bits UI 是 headless：它提供行为、语义、状态与组合接口，圆角、字号、透明度仍由应用 token 与薄样式层定义。
- 使用 [Web Interface Guidelines](https://github.com/vercel-labs/web-interface-guidelines/blob/main/command.md) 检查标签、焦点、表单、键盘、动效、溢出与反馈。该通用指南的 URL 状态建议与本项目既定范围不同，本轮不引入 URL 状态。
- 频谱技术案例继续引用仓库外 `D:\MUSIC_Player\spectrum-lab-2026-11-18\RESEARCH.md`；本轮选型以用户指定的九种效果为准。

## 3. 现状审查与证据

### 3.1 覆盖情况与实测边界

静态盘点覆盖 `views/` 的 10 个视图文件、13 个 AppPage、`components/ui/` 的 25 个组件，以及 shell、content、station、Beets、队列、EQ、歌词、全局浮层与登录入口。

本轮通过 Vite mock 预览实际走查：首页、曲库、专辑/艺人浏览、专辑/艺人详情、歌单、搜索、设置、Beets 安装/分析/整理、直播收听、播放页、队列抽屉、EQ 抽屉、添加流弹窗。播放页检查的是 mock 曲目与无歌词态，封面占位图属于样例数据，不作为真实曲库缺陷。

推流入口/频道工作台、登录闸、Web 目录输入本轮主要为静态检查：默认 mock 使用桌面能力，缺少 station 命令与 Web 能力预览开关。长列表、错误、长文本、明暗主题、不同缩放和真实歌词仍需在实施验收矩阵中逐项验证。

运行走查的实际视口约为 1127×1354；尝试的浏览器尺寸覆盖未改变该标签的实际尺寸，因此不计作 1280×800 已通过。临时覆盖已复原。走查结束时捕获的 console error / warning 为空；这不代表所有未覆盖状态都已通过。

### 3.2 已定位的问题与治理任务

代码位置按本次审查时的行号记录，实施后以组件名与任务编号追踪。

| 编号 | 证据 | 问题与影响 | 实施切口 |
|---|---|---|---|
| A01 | `ui/src/components/ui/Select.svelte:126,143`；`Menu.svelte:111,122`；`Popover.svelte:72`；`Dialog.svelte:121` | 下拉外框 10px、选项 7px、气泡 12px、弹窗 14px 分散硬编码；7/12px 脱离圆角阶梯，其他值也绕过 rem 缩放 | 统一面板与条目 token，同步所有状态与 Portal 内容 |
| A02 | `ui/src/components/ui/Dialog.svelte:181` | body 没有默认内边距，调用方各自添加，易产生贴边或重复留白 | body 默认 16px，列表型明确选择无内边距；逐个调用迁移，header/body/actions 统一所有者 |
| A03 | `ui/src/components/ui/Field.svelte:17`；`ui/src/components/LoginView.svelte:86` | Field 缺少 password 与若干原生属性；登录输入没有关联 label 或 aria-label，表单另写视觉样式 | 扩展原生属性透传，登录复用 Field/Button/InlineError；保留现有认证流程 |
| A04 | `ui/src/views/BrowseView.svelte:114`；`ui/src/components/shell/TitleBar.svelte` | 页面过滤与全局搜索各自实现输入外观、焦点和尺寸 | Field 的搜索组合共享皮肤；全局胶囊与页内常规输入允许明确变体 |
| A05 | `ui/src/components/content/Header.svelte:80`；`ui/src/styles/pages.css` | 标题和页面节奏仍混用 px 与 token；pages.css 同时承担多个页面样式 | 保留标题层，尺寸进入 token；共享规则归 kit，业务布局归对应页面，清理失效选择器 |
| A06 | `ui/src/views/AlbumView.svelte:75`；`ArtistView.svelte:60`；`LibraryView.svelte:188`；`ui/src/components/ListToolbar.svelte` | 详情标题与列表工具条重复显示“播放全部”，运行预览可复现 | 详情保留标题主操作，工具条只放选择集动作；曲库/搜索没有标题播放键时保留工具条播放 |
| A07 | `ui/src/components/SettingsPanel.svelte:761` 及快捷键冲突列表 | 设置直接渲染 SHORTCUT_LABELS 的翻译键，界面出现 `shell.keys.*` | 在渲染和冲突描述中翻译；同时查动态键、分页名称、Tooltip 与英文文案 |
| A08 | `ui/src/components/QueuePanel.svelte:235,280` | role=button 的整行内还有移除按钮；外层 Enter/Space 处理未区分子按钮，存在键盘事件串到播放动作的风险 | 行容器与播放、移除按钮分离，事件按动作归属；验证移除不播放，保留拖排与虚拟化 |
| A09 | `ui/src/components/ui/SideDrawer.svelte` | 手写焦点转移、恢复与 Esc；固定几何与非模态播放条交互有明确业务理由 | 优先验证 Bits Popover.ContentStatic，保留固定定位与非模态语义；见 §5 |
| A10 | `ui/src/components/station/WeekTimePicker.svelte:124–136` | 小时刻度 9px、固定网格与输入宽度，需要复核缩放后的可读性与点击空间 | 保留连续多时段拖选和单行输入，统一间距、字体与有效命中区；不更改节目模型 |
| A11 | `ui/src/components/SettingsPanel.svelte` 外观区 | 新六种前景继续用一排分段按钮会过密；目前关闭前景后形态仍可操作，依赖状态表达不够清楚 | 统一两组效果 Select，父开关关闭时子项禁用但保留选值 |
| A12 | `ui/src/components/NowPlayingSpectrum.svelte`；`AmbientBackdrop.svelte` | 背景为 Three.js，播放页仍为大段 Canvas 2D；直接复制实验室会产生第二套舞台与循环 | 工作包 B 接入现有 GlStage / scheduler / SpectrumSource，删除被替换绘制分支 |
| A13 | `ui/README.md` | 仍有早期 demo/compat 与复制说明，不能指导本轮开发 | 更新为实际分层、Vite HMR、mock 场景、HTTP 联调与组件用法 |

这些是首轮可执行发现，不把所有裸 px 或所有原生 HTML 都认定为问题。后续每页仍检查 hover、focus、active、disabled、loading、error、empty 与长文本状态。

## 4. 设计基线与样式所有权（工作包 A）

### 4.1 初始基准与拟定映射

现有 tokens 已包括尺寸、字体、面、阴影、模糊与层叠体系。本轮优先整理用途及引用，不重新命名整套变量，也不先把所有全局字号统一放大。

| 维度 | 现有基准 | 拟定治理规则 / 定标内容 |
|---|---|---|
| 骨架 | 顶栏 54、侧栏 216、底栏 92px | 首批保留；页面治理不以改变四区尺寸解决局部拥挤 |
| 控件 / 列表 | 默认控件 32、曲目行 52px | 保留基础密度；小图标按钮 28px、常规 32px，图形大小与实际命中区分开 |
| 间距 | 4 / 6 / 8 / 12 / 16 / 22px | 图标内距 4/6、同组控件 8、表单项 12、卡片/弹窗 body 16、页面横边距 22；大节间距复用组合，确有复用时才加语义 token |
| 圆角 | 4 / 6 / 8 / 10 / 14px + pill | 小标签 4，菜单项 6，控件 8，卡片和下拉面板 10，模态/抽屉 14；封面按尺寸映射，头像圆形；胶囊限搜索、徽标等有意场景 |
| 字体 | 10 / 10.5 / 11.5 / 12 / 13 / 14 / 17 / 21 / 26 / 30px | 目标角色：控件/常规正文 13、曲名 14、说明/元数据 12、装饰英文 10–11、小标题 17、区块重点 21、页标题 26、歌词主句 30；先在代表页校准，再逐用途映射，不直接改 fs-base 影响全站 |
| 字重 / 行高 | 页面存在局部差异 | 正文 400、关键操作/子标题 500–600；说明约 1.5、正文约 1.4；单行控件垂直居中，不用 line-height 修补错误高度 |
| 数字 / 路径 | 已有 mono | 时间、频段、路径等使用等宽；时长/计数使用 tabular-nums；普通歌名与艺人不用等宽 |
| 面与透明度 | 深色 surface-2 .72 / 3 .86 / 4 .97；浅色 .74 / .90 / .98 | 侧栏/基础面、内容卡、密集浮层分别使用对应面；菜单和队列接近实色，避免叠多层透明卡片；页面不自行乘一层 opacity |
| 文字透明度 | 深色 text-1 .92 / 2 .58 / 3 .55；浅色 .92 / .68 / .64 | 按角色继承；以新频谱最亮合成背景重测，必要时调整面或 scrim，不把说明文字继续调淡 |
| 禁用 / 状态 | disabled-opacity .45，已有 danger/warn/ok | 统一禁用档位，避免父子重复压暗；忙碌状态保留按钮宽度，阻止重复提交；红绿状态同时有文字/可访问名称 |
| 深度 | shadow 1–5、blur 4/8/18/28px、z 阶梯 | 保留并统一引用；不通过页面额外阴影、模糊和 z-index 修补层叠 |
| 图标 / 焦点 | Lucide、局部尺寸 | 通常 14/16/18px，基于统一 stroke；仅图标操作有 aria-label 和 hover/focus Tooltip；统一清晰的 focus-visible 环，键盘焦点不只改变颜色 |
| 动效 | 120 / 180ms，已有 reducedMotion | 控件只动颜色/透明度/必要变换；弹层开关遵循同一节奏；reduced motion 不保留持续形变和飞入 |

数值均为 100% 缩放下的逻辑 px；文字、尺寸、圆角和间距通过 rem/token 跟随应用缩放。1px 发丝线、必要的 shader 像素量及阴影模糊可以保留 px，需有用途理由。

### 4.2 所有者与使用规则

- `tokens.css`：唯一视觉参数源。组件不复制 rgba、字号、圆角和阴影档位。
- `components/ui`：通用交互、控件视觉与状态；直接使用 Bits 的公开 props、child/snippet 与 data-state，不再建立一套复杂配置协议。
- `components/content`：页面 Header、Toolbar 等结构。标题与工具条各自成层，不合成新的全能页面组件。
- `shell`：四区布局与窗口功能；`views` / 业务组件：数据编排与专用布局。页面不穿透修改 `.k-*` 私有类来改变通用外观。
- 共享皮肤由组件或明确共享样式拥有；页面专属规则逐步从大型 pages.css 移回对应位置。只随实际迁移清理，不为拆文件单独重写业务。
- 代表页：曲库（密集列表）、设置（表单）、频道工作台（复杂编辑）、播放页（动态背景）。四者共同通过后扩展全站。

### 4.3 可访问性与文案

普通文字对比度目标 4.5:1，关键非文本控件/焦点指示 3:1。测量对象包括 B0/B1/B4 亮部、浅色主题、浮层遮罩及歌词衬底。不能只用 surface 的原始色值计算。

输入都有持久标签，错误与提示通过描述属性关联；表单支持 Enter 提交，IME 合成回车不误提交。弹层的初始焦点、Esc、焦点返回、菜单关闭后的恢复对象都纳入验证；虚拟行已回收时返回仍存活的列表容器。删除与关闭使用约定图标，含义由文案和 aria-label 表达。

中英文同步审查：页面标题、工具提示、空态、错误、动态参数、快捷键与能力提示。长歌名/路径/URL 以完整可访问名称和可复制内容保留信息；截断只用于视觉。格式化使用现有本地化入口，不拼接翻译碎片。

## 5. 组件决策表

当前 25 件 UI kit 中 11 件直接引用 Bits UI。下面是本轮目标，不以“已有封装”视为已经完成治理。

| 当前组件 | 目标实现 | 治理重点 |
|---|---|---|
| Button | Bits Button.Root + 薄样式 | 原生属性/事件透传、type、disabled、loading、图标与尺寸 |
| IconButton | 同一 Button 基础的图标变体 + Bits Tooltip | aria-label、hover/focus 提示、active/pressed，避免两套按钮规则 |
| CheckCell | Bits Checkbox（保留） | checked/indeterminate、整格命中、键盘与禁用 |
| Switch | Bits Switch（保留） | 持久标签、描述、表单提交与禁用关联 |
| Slider | Bits Slider（保留） | 横/纵向、单位与 valuetext、键盘步长、提交时机 |
| Knob | 以 Bits Slider 替换 | EQ 三项增益使用统一滑杆与数值，不继续维护自写旋钮拖动/wheel/keyboard |
| SegmentedControl | Bits RadioGroup（保留） | 互斥且不可空；少量短选项使用，不用可取消的 single ToggleGroup 替代 |
| Select | Bits Select（保留） | 外框/选项共用浮层皮肤；键盘、滚动、英文宽度、Portal 层级 |
| Dialog | Bits Dialog（保留） | body 留白、标题描述、初始焦点、关闭/提交；不在每页重写 |
| Menu | Bits DropdownMenu（保留） | 保留坐标 Portal 以适配虚拟行；焦点恢复与危险项皮肤；不为每个虚拟行增加 ContextMenu 实例 |
| Popover | Bits Popover（保留） | 密集设置皮肤、标题标签、outside/Esc、焦点与播放器按键互不干扰 |
| SideDrawer | 优先 Bits Popover.ContentStatic + Portal | 固定右侧几何，trapFocus=false、preventScroll=false；先完成非模态行为样板再移除手写焦点代码 |
| Tooltip | Bits Tooltip（保留） | 全局时延、字号、长文本上限、键盘可见 |
| TextTabs | 按语义拆用 Bits Tabs / 导航按钮 | Beets 等互斥面板用 Tabs；设置滚动锚点仍用 nav/button；曲库排序按钮保留排序语义 |
| Pagination | Bits Pagination（保留） | 页码本地化可访问名称、忙碌/禁用、焦点和位置稳定 |
| Progress | Bits Progress（保留） | 明确可量化与不确定进度、单位与错误状态；不借用 Slider 伪装只读进度 |
| Field | 原生 input/textarea + 薄组合，必要时 Bits Label | Bits 无通用 TextInput；补 password/name/autocomplete 等属性透传、描述与受限输入，不造表单框架 |
| Card | 静态容器（保留并收窄变体） | 明确 compact/content/form 的内边距，避免 actions/children 任意组合导致每页修补；无对应 Bits Card |
| Cover | 静态封面（保留） | 固定比例、尺寸、失败占位与 alt；不额外引图片库 |
| Chip | 静态标签（保留） | 字号、圆角、状态色；可选择标签改用适合的按钮/Checkbox 语义 |
| EmptyState | 静态组合（保留） | 空库/空筛选/失败分开，动作可见、避免重复主操作 |
| InlineError | 静态反馈（保留） | 可恢复动作、错误描述、live region，避免重复播报 |
| Skeleton | 静态占位（保留） | 匹配内容几何，减少动效时静止，不闪动整页 |
| StatBlock | 静态统计（保留） | 数字/标签层次、长单位、列宽，不自行定义卡片皮肤 |
| Toast | 现有轻提示宿主（保留） | live region、持续时间、重复合并、可关闭；Bits 没有 Toast，不新增通知库 |

### 5.1 新增原语与有限业务自制

- 删除确认优先使用 Bits AlertDialog，沿用同一面板皮肤与现有确认需求，不扩大确认范围。提示与可编辑表单仍使用 Dialog。
- 真实 tabpanel 的结构由 Bits Tabs 承担；若使用 Toolbar，需连同键盘模型一起落地，不能只贴 role。
- 已安装版本的 Dialog.Content 虽有 trapFocus 属性，但内部 aria-modal 固定为 true，不能仅关闭 trapFocus 就当作非模态抽屉。Popover.ContentStatic 已有静态定位接口，作为首选验证切口；焦点进入、返回、底栏按钮切换、内嵌菜单和 Esc 全部通过才替换 SideDrawer。
- 若 ContentStatic 无法满足既有固定抽屉行为，保留一个有明确理由的薄业务适配，不引入另一套弹层库，也不把队列改为阻断播放条的全模态窗口。
- 周时段网格、虚拟曲目表、队列拖排、歌词跟随属于领域交互，继续复用现有实现。Bits Calendar/RangeCalendar 不承担周循环时间排程。
- Bits TimeField / TimeRangeField 确实存在，但本项目结束时间需要 `24:00`，其业务边界尚未得到组件验证；本轮延续用户已指定的受限 `HH:MM` 输入，不回退到内嵌时间选择器。

## 6. 逐页实施矩阵

表中“状态验收”均为实施期任务。共用视图仍按用户实际入口分别验收。

| 页面 / 表面 | 组件与布局切口 | 保留的行为与状态验收 |
|---|---|---|
| 全局 TitleBar / Sidebar / PlayerBar | 搜索复用 Field；按钮/Tooltip/Slider/Popover 统一；清理局部字号与间距 | 导航选中、返回、歌单排序、窗口拖动与窗控、流与文件标签、播放/暂停/seek/音量/速率；960px 不遮挡核心操作 |
| 首页 home | 歌单横列、统计、常听/最近列表使用统一 Cover/Card/StatBlock/列表皮肤；统一区块标题 | 保留首页内容顺序和轻量浏览；无历史、无歌单、加载/失败、长标题、横向滚动和键盘进入 |
| 曲库 library | Header/Toolbar/TrackListPanel/Field/Select/Popover/CheckCell；工具条分组与筛选反馈 | 单击选择/双击播放、多选、排序、分页、虚拟化、滚动恢复、当前行定位；空库引导仍复用 AddLibraryFolder |
| 歌单 playlist | 统一详情 Header 与列表操作，移除重复播放键；编辑/清空/删除复用模态 | 普通/动态歌单、规则编辑、顺序拖排、空歌单、缺失曲目；取消编辑不写回 |
| 专辑浏览 albums | BrowseView 搜索、Cover、小卡尺寸、网格间距、Pagination | 封面缺失、长专辑名、空过滤、加载、卡片键盘进入，不为每张卡加无用管理按钮 |
| 艺人浏览 artists | 与专辑共享浏览尺寸系统，保留头像差异 | 长艺人名、无图、计数与分页；不把头像强改为专辑方封面 |
| 专辑详情 album | Header 与 TrackListPanel 主操作分工，曲目表皮肤复用 | 一个清楚的播放主操作；保留专辑曲序、艺人跳转、选择集操作与返回滚动位置 |
| 艺人详情 artist | 小型头像标题与曲目区共享规则，移除重复播放键 | 维持现有曲目为主的内容，不借参考产品新增业务区；空态、长名、专辑跳转 |
| 搜索 search | 复用列表与分组标题、搜索状态、统一空态 | 区分输入中/加载/完成/无结果/失败；切换查询时不短暂把旧结果数当新结果；长中英文查询 |
| 正在播放 nowPlaying | 封面、曲名/艺人/专辑、歌词与频谱区域重新定标，保留沉浸结构 | 无曲/本地/临时文件/直播、无歌词/滚动歌词/手动浏览恢复、长标题、窗口高度变化；前景不穿过文字和底栏 |
| 设置 settings | 统一 SettingRow 的标题/说明/控件对齐；统计与目录卡共用 body；外观区两组 Select；修复快捷键翻译 | 锚点导航保留；桌面/Web 能力差异、缩放、冲突录制/取消/清空、目录状态、更新/错误/关于；长英文不挤压控件 |
| Beets 安装 | Field/Button/CheckCell/Progress/InlineError、描述与插件行密度统一 | 未安装/检测中/已安装/失败、下载与插件变更；不改变安装与探测判据 |
| Beets 分析 | 条件行、Select、受限数值、结果表、Tabs 和空态统一 | 条件/原始语法、添加删除条件、查询中/失败/0 条/结果、分页和存歌单；不改变分析库语义 |
| Beets 整理 | 目录选择、动作、候选差异/展开区、确认操作与状态统一 | 未设置/预览中/无候选/差异/应用中/失败；保留先预览后应用，不把长任务塞进小弹窗 |
| 直播收听 live | Header 常驻；最近流条目、当前流舞台、添加表单与卡片皮肤统一 | 只负责收听；添加/移除/上一流/下一流、无流/连接/播放/失败/无时长，保留明确的舞台操作 |
| 推流频道列表 broadcast | Header 常驻；频道条目共用 compact 行卡和 IconButton | 不加搜索框；频道名（公开/私密），第二行可点击 m3u8 地址；开播红绿灯、切歌、复制、配置按既定顺序，图标按钮 hover/focus 提示 |
| 频道工作台 | 只切换 Header 下的列表/工作台组件；返回、编辑、删除同一行，保存/取消组合贴右；内层小标题 | 新建填全信息并区分必填/选填；编辑与删除为模态；系统歌单含全曲库，小封面卡右侧加号；队列整卡、顺序旁统一优先级 |
| 周时段编辑 | 治理网格/提示/分隔线与输入尺寸，不增加新的编辑层级 | 指定时段 switch 顺排；拖一次建一条、多段半透明显示、点击输入同步；周按钮与起止时间单行、区间用 -、X 删除；无嵌套卡和“全部安排”重复开关，允许全选与 24:00，一次保存 |
| 登录 LoginView | Field/Button/InlineError 与统一卡片 | password/token 两模式、Enter、提交中、错误、输入焦点与可访问名称；不改变凭据存储和认证逻辑 |
| 全局 OverlayHost | 歌单创建/编辑、加入歌单、曲目/歌单菜单、确认与 Toast 共用新 kit | 首焦点/Tab/Esc/回焦、长内容滚动、嵌套 Select 层级、取消不提交、忙碌防重复、动态歌单规则 |
| 队列 / EQ / 播放速率 | 非模态固定抽屉、独立行操作、纵向 Slider 与统一数值；Popover 皮肤 | 队列与 EQ 互斥，底栏可用；移除不误播放、拖排有键盘替代、虚拟化；EQ 关闭禁用、预设切换、增益单位与键盘步长 |

逐页完成条件同时包括：没有同功能的遗留局部组件；共享 token 不被页面覆盖；中英文完整；状态和能力分支均有可复现入口。仅静态默认截图一致不足以销项。

## 7. 频谱选型与接入（工作包 B）

### 7.1 确定的九种效果

实验室目录为 `D:\MUSIC_Player\spectrum-lab-2026-11-18`。B/F 编号只作为研发与验收索引，产品设置显示中英文名称。

| 场景 | 编号 | 中文 / 英文建议 | 设置语义 ID 建议 | 接入目标 |
|---|---|---|---|---|
| 背景 | B0 | 山形 / Ridge | ridge | 以现有山形为视觉基准，保留满意的包络与光效，不以新实现重画成尖峰 |
| 背景 | B1 | 极光绸幕 / Aurora Veil | aurora | 保留修订后的平滑光带，消除窄尖峰与突然拉高的亮线 |
| 背景 | B4 | 漫射光帘 / Diffuse Curtain | curtain | 大面积柔和明暗，文字上方维持低干扰，避免亮度覆盖导航与表格 |
| 前景 | F1 | 轨道光环 / Orbital Halo | orbit | 环形空间感，轮廓清楚，避让封面与歌词 |
| 前景 | F2 | 声纹丝带 / Spectral Ribbon | ribbon | 横向多束丝带，保留流动层次，边缘渐隐 |
| 前景 | F4 | 韵律晶格 / Rhythm Lattice | lattice | 48 光条的节奏与层次，避免高频闪烁和文字后方高亮 |
| 前景 | F5 | 回声轮廓 / Echo Contours | contour | 多层有机轮廓，控制线密度与叠加亮度 |
| 前景 | F6 | 游弦单线 / Drifting String | string | 横向连续单线与游走高光，保持简洁轮廓与平滑转折 |
| 前景 | F7 | 双岸声场 / Twin Shores | shore | 上下镜像曲线围成半透明声面，低能量时无尖角 |

不引入 B2/B3/F3/F8，也不复制实验室的候选卡片、收藏/导出、模拟音频与浏览器文件解码。仅移植选中的 shader、曲线和平滑工具；Three.js 使用仓库依赖，不再复制 vendor。

建议初始默认组合 B0 + F1；背景/前景开关沿用现有默认开关值，前景不会因枚举替换自动开启。默认组合属于方案建议，用户已确认的是可选效果集合。

### 7.2 渲染结构与切口

B1/B2 已落地以下结构：`AmbientBackdrop.svelte` 组合 Flow、当前背景与当前前景，`NowPlayingSpectrum.svelte` 只登记区域。背景 RippleLayer 和播放页 Canvas 2D 均已删除。

```text
既有 SpectrumSource（48 带、按需引用计数）
           ├─ 背景选中图层：ridge / aurora / curtain
           └─ 前景选中图层：orbit / ribbon / lattice / contour / string / shore
                         ↓
一个 GlStage、一个 WebGLRenderer、一个 subscribeRender 调度
                         ↓
背景全窗绘制 + 播放页内容区 viewport/scissor 裁切
```

1. `lib/render/context.ts` 保持唯一舞台职责；增加最小的图层注册/区域绘制能力。舞台 host 通过明确接口向播放页提供注册与释放，不把 renderer 塞入业务 store。
2. 前景挂载时登记播放页视觉区域的 DOM rect，卸载时移除图层。用舞台与区域坐标差计算 viewport/scissor，正确转换 WebGL 的纵坐标；统一 DPR、应用缩放、浏览器缩放和窗口 resize。布局变化时更新区域，避免每帧反复测量与写布局。
3. 每个区域绘制后恢复全窗 viewport/scissor 和 renderer 状态，避免前景设置污染下一帧背景；同一帧只在舞台入口 clear，区域图层不清除其他内容。
4. GL canvas 仍位于 DOM 内容下方。前景是相对背景的视觉层，不代表盖在歌词上方；封面/歌词/scrim 用既有 DOM 层级保证可读性。播放页图形不进入标题栏、侧栏或播放条。
5. B0 先以现有 RidgeLayer 与曲线作对照；其余按职责拆为有限 GlLayer 类。公共滤波/主题参数共享，形态计算独立，不把整个 effects.js 搬成一个大开关组件。
6. 图层切换只保留当前背景与当前前景的 GPU 资源；几何、材质、纹理与 render target 在切换/卸载/HMR 时释放。固定缓冲复用，禁止逐帧新建数组、几何或材质。

### 7.3 输入、平滑、流速与停止

- 后端 48 带、30Hz 和当前源接口保持不变。桌面继续消费 Rust tap，不创建 AudioContext；Web 继续复用 http transport 已有的浏览器分析链，不再添加第二个 AudioContext。
- B1 以实验室修订值为初始基线：约 240ms 时间平滑、两次半径 6 / sigma 2.2 的 Gaussian 带间滤波、Catmull-Rom 重采样至 256 点。具体常量与外部实现核对后落地；验收看低能量、重低音和换曲时是否出现尖峰，不能只测平稳模拟信号。
- F2/F6/F7 的连续曲线共享合适的带间平滑与重采样工具；F4 保留柱间节奏，不对所有效果无差别套同一强滤波。所有平滑以 dt 表达，30/60/高刷新率观感接近。
- 流速和能量响应分开。以实验室背景 0.65、前景 1.50 的艺术速度作初始基准，前景明显更流动；不通过加快音频采样、修改播放时钟或虚构 band 来制造速度。
- 暂停时冻结形态运动，幅度平滑退到静息态；无输入时不模拟音乐跳动。停止、无当前曲目、数据源不可用各有明确静态结果，恢复播放不积算隐藏期间的大 dt。
- document.hidden 停止逐帧绘制；reduced motion 使用静态/低变化呈现，停止持续游走。全局效果关闭后释放图层与源引用；前景关闭或离开播放页只释放前景引用，不误关背景 tap。

### 7.4 设置与旧代码清理

- 保留全局动态效果、背景、播放页前景三个开关关系；背景/前景各选一种。六种前景用 Bits Select，不挤成六段按钮。两组选择器共用尺寸、标签和说明结构。
- 主题颜色与效果形态是不同轴：切换形态不改主题，切换主题不重置选择。shader 从现有主题色得到明暗配色，不搬实验室固定底色。
- B0 的 soft/balanced/neon 光效先保留为山形专属；选 B1/B4 时不显示无效的山形参数。本轮不把实验室全部 gain/speed 调试滑杆搬到产品设置。
- 修改 `lib/settings.ts` 的类型、选项、schema/default、设置 UI 和中英文文案；旧 mirror/radial/ripple/wave 等值及对应文案、绘制路径随替换删除。schema 对非法值正常回默认，不建立旧值到新效果的映射表。
- 若引入效果缩略图，使用静态资源或单舞台一次生成；首批采用 Select 即可，不为九张预览卡持续启动九个 renderer。

### 7.5 能力与降级

现行 capability 收口为**WebGL 动态效果 + CSS 静态背景**：GL 不可用时前景关闭绘制，应用与播放正常工作。九种效果共用 GL 路径，不维护 2D 副本。设置显示包含背景静态与前景停用的双语能力说明。

这是对 docs/46 降级策略的修订。B1 落地背景两条路径，B2 删除播放页 Canvas 2D；capability、测试、ui-render 域账本与 AGENTS.md 同步。保留硬件能力诊断和 context lost/restored 处理。上下文丢失期间隐藏共享画布并释放两侧源引用，恢复后按当前选择重订阅并校准尺寸。

### 7.6 视觉与性能验收

- 九种效果分别在低能量、重低音、高频丰富、静音、暂停/恢复、换曲、无数据场景看动态；三背景 × 六前景的 18 个组合做可读性和裁切冒烟，代表组合做持续性能采样。
- 明暗主题、五套主题色，确认没有发白边、过曝、叠加变脏、线条锯齿与 F7 中轴尖角；B1 无窄尖峰，F2/F6/F7 横向差异清楚。
- 实测记录设备/GPU、WebView2/浏览器版本、窗口、DPR、播放素材、背景/前景组合；记录 draw 时间 p50/p95、帧间隔和 draw calls。相同环境先测 B0 基线，再比较新组合，不用实验室 FPS 作为生产结论。
- 60Hz 设备以稳定交互和帧预算为目标；代表组合连续观察不少于 30 秒，记录 p95 超过 16.7ms 的情况与原因。无真实设备数据时不标“性能通过”。
- 反复切换 20 次、进出播放页、全局开关、隐藏恢复、HMR、context loss/restore 后，图层数、源引用、纹理/几何数量回到稳定值。无独立 rAF、常驻隐藏特效与每帧 Svelte 状态更新。

## 8. Vite 预览与验证工作流

### 8.1 日常预览

```powershell
# 仓库根目录
npm run dev:ui
```

地址为 `http://localhost:1420/`，直接读取源码并 HMR。本轮已用该入口完成走查并保留预览。用户提出“不每次构建”，技术实现采用 Vite dev server；`vite preview` 只服务已有 dist，无法让源码修改即时生效，不作为日常迭代入口。

默认是 mock；现有 `?backend=mock`、`?failCmd=library_list`、`?beets=absent`、`?missing=2` 等场景入口继续使用。P0 已补齐以下开发能力：

1. `?backend=mock&host=web` 使用正式 Web 能力判定；能力标记只在 DEV 装配，不通过浏览器脚本注入 store。
2. `page` 支持全部 13 个 AppPage；`scene` 提供 standard/long/empty/issues/runtime-error；theme/lang/scale 选择主题、语言和应用缩放，delay/failCmd 复现加载与命令失败。station 内存回执覆盖列表、频道信息、系统歌单/全曲库、节目时段、保存/应用及运行状态，不模拟 Liquidsoap 调度或 HLS 出流。
3. 三幅本地 SVG 封面与同步歌词不依赖外网；播放页装载队列并暂停在 24 秒。`lyrics=none` 检查无歌词，`gate=password|token` 使用真实登录展示组件和开发认证适配，输入 preview 后进入应用，不请求认证和保存凭据。
4. `test:e2e:dev` 连接或启动 Vite 1420，可复用已有 server，无构建；`-- --project=regression` 只跑交互，`test:ui:baseline` 只跑截图。正式 Edge / Firefox 的 dist 测试排除 DEV 专用用例。
5. `AIYA_DEV_HTTP_TARGET` 显式开启开发代理，覆盖 `/auth`、`/ipc`、`/events`、`/artwork`、`/audio`、`/stream`、`/hls`。`?backend=http` 使用真实 HTTP transport；未设置目标时无代理，不将 mock 通过视为 HTTP 联调通过。

开发预览能力、fixtures 与诊断仅在开发入口存在；不把实验室页面、合成音频或测试目录重新带入发布版本。

### 8.2 检查节奏

日常修改通过 HMR 看页面；修改共享接口/组件后运行 `npm run check`、`npm run check:deps`、`npm run check:ui-layers` 与相关开发态 e2e。纯间距/圆角调整不新增镜像实现的单元测试；焦点、输入边界、事件串扰、渲染生命周期使用必要的行为回归。

每个批次完成跑对应页面和状态矩阵；两个工作包收尾后执行一次完整 `npm run verify`，并完成最终 UI 构建和真 WebView2 视觉/播放冒烟。Vite 用于迭代速度，不代替发布产物和原生宿主的最终验证。

## 9. 实施顺序与批次验收

| 批次 | 工作 | 产出与完成判据 | 依赖 |
|---|---|---|---|
| P0 预览与基线 | 开发态 Web/mock 场景、开发 e2e、样式盘点、四代表页参数校准 | 13 入口与关键浮层可复现；形成 token/变体使用表及明暗代表截图；不再为看 UI 反复构建 | 起点 |
| A1 共享 kit 与壳 | Button/Field/浮层/选择器/Tabs/抽屉样板，统一 Header 与操作分组 | A01–A04/A09 的共享切口完成；焦点、缩放、层叠与底栏交互通过；所有调用同步迁移 | P0 |
| A2 浏览与播放操作 | 首页、曲库、歌单、专辑/艺人浏览和详情、搜索、队列/EQ | A06/A08 完成；一页一个清楚的播放主操作；选择/虚拟化/拖排/滚动恢复不回退 | A1 |
| A3 配置与任务页面 | 设置、Beets、直播、推流工作台、登录与剩余全局浮层 | A07/A10/A11 完成；逐页矩阵无遗漏，docs/75/76 已确定交互保持，双语/能力状态完整 | A1，A2 的共享零件 |
| B1 舞台与背景 | 生命周期、B0 对照、B1/B4、背景降级调整 | 一舞台一循环；B0 对照、B1 平滑、背景文字对比与关停正确 | P0、A1 的 token |
| B2 六种前景 | 共享区域注册、六 GlLayer、区域裁切、暂停与流速、设置枚举/双语、删除旧 2D 形态 | 六效果完整、横向/环形差异明确、18 组合冒烟、生命周期与性能记录完成 | B1；与 A3 外观区同步 |
| C 收尾 | 全矩阵回归、失效样式/文案/代码清理、README/账本/约束同步、最终 verify | 只有一套视觉实现；变更/验证/未实测项如实登记，台账销项 | A2、A3、B2 |

建议按 P0 → A1 → A2 → A3 → B1 → B2 → C 顺序实施。两个工作包分别记录验收，避免 UI 页面已完成却因频谱尚未接入而无法判断状态。各批完成即更新本文件，不把全部实现留到最后一次审查。

### 9.1 统一回归矩阵

| 维度 | 覆盖要求 |
|---|---|
| 窗口 | 960×600、1280×800、1440×900；播放页另查高窄窗口与最大化恢复 |
| 缩放 | 100% 全入口；125% / 150% 检查壳、列表工具条、设置、频道时段、弹层；90% / 110% 做共享 kit 冒烟；区分应用缩放与系统 DPI |
| 文案 / 主题 | 中英 × 明暗；长标题/路径/URL、缺图、0/大量计数；五套主题色查动态背景合成 |
| 输入 | 鼠标、键盘、拖动、滚轮、IME；焦点次序、嵌套菜单、Esc、回焦、受限时间 00:00/23:59/24:00/非法值 |
| 状态 | 默认、hover、focus、pressed/selected、disabled、loading、empty、error；播放暂停/停止、流连接/失败、任务执行中与取消 |
| 宿主 | mock 用于复现与视觉；HTTP 真服务验证登录/目录/媒体/推流；Tauri WebView2 真窗口验证窗控、目录选择、播放、GPU；能力隐藏和禁用各有判据 |
| 渲染 | 新效果单项、18 组合冒烟、无 GL、reduced motion、隐藏/恢复、context loss、卸载/HMR、两源引用与资源稳定性 |

## 10. 当前交付与待实施清单

- [x] 联网参考四类音乐播放器并明确采用范围。
- [x] 盘点页面、25 件 kit、现有 token、Bits 原语与频谱接入点。
- [x] 用 Vite 源码预览走查代表页面，记录实际覆盖与限制。
- [x] 固定九种视觉候选，写出两工作包、逐页切口、批次与验收矩阵。
- [x] P0 预览覆盖与现状定标；证据见 §11。
- [x] A1 共享 kit 与应用壳；证据见 §12。
- [x] A2 浏览与播放操作；证据见 §13。
- [x] A3 配置与任务页面；证据见 §14。
- [x] B1 三种背景与舞台生命周期；证据见 §15。
- [x] B2 六种前景与共享区域裁切；证据见 §16。
- [x] C 整体回归与文档收尾；证据与限制见 §17。

P0 修改了开发装配、mock 注入、登录展示与认证适配的边界、Vite 代理、回归入口与说明；没有实施共享组件换皮或频谱替换，没有修改后端和部署。现存 docs/76 相关未提交实现保持原状。

## 11. P0 实施与验收记录（2026-11-18）

### 11.1 可复现入口与隔离

`ui/src/dev/` 通过 `import.meta.env.DEV` 动态加载，频道、封面和歌词 fixture 不放入 public，不复制仓库外频谱实验室。`MockSeed` 注入设置、路由与数据，额外命令由 mock transport 派发，业务组件继续调用既有 IPC client。mock 卸载停止播放与频谱计时器并清除 transport。

带 page/scene/theme/lang/scale 的场景不读取历史 mock 设置和路由作为起点，修改仅保留内存；应用外观镜像仍沿用正常写入。普通 mock 的原有持久化测试继续保留。查询参数只用于开发初始装配，没有新增产品 URL 路由或状态同步。

登录组件提取可注入的 LoginAuth 接口，默认 HTTP 实现保留口令换令牌、静态 token trim/存储和整页重载；展示预览使用不访问认证端点的适配。输入组件皮肤与标签问题 A03 仍待 A1/A3。

入口、参数全集、HTTP 环境变量及命令见 [ui/README.md](../ui/README.md)。常用入口：

- [Web 频道](http://localhost:1420/?backend=mock&host=web&page=broadcast)：点击配置进入工作台。
- [Web 曲库](http://localhost:1420/?backend=mock&host=web&page=library)。
- [播放页](http://localhost:1420/?backend=mock&host=web&page=nowPlaying)。
- [登录](http://localhost:1420/?backend=mock&host=web&page=library&gate=password)。

### 11.2 实测尺寸与变体使用表

系统 Edge、DPR 1、应用缩放 100%、root font-size 16px；四代表页 × 明暗 × 960×600 / 1280×800 / 1440×900，共 24 组。每组记录实际 viewport、token 与 computed style；没有根文档横向溢出。此结论不涵盖内部滚动容器、长英文和更高缩放的全部布局。

| 用途 / 变体 | P0 实测或实现现状 | 后续使用与定标 |
|---|---|---|
| 壳 / 密度 | 顶栏 54、侧栏 216、底栏 92、曲目行 52px | 保留骨架；间距 4/6/8/12/16/22，圆角 4/6/8/10/14px，按 §4.1 映射 |
| Header | 页面标题 26px，描述 12px；频道工作台内标题 13px | 外层装饰英文+标题+描述常驻；内层小标题，最终字号在 A1 定标 |
| Button md / sm | md 高 32、字 12、圆角 8、横内距 12；sm 高 28、字 11.5、圆角 6、横内距 8px | primary 主操作、secondary 常规、ghost 次要、danger 删除、link 行内跳转；A1 统一透传与皮肤 |
| IconButton | ghost/solid、sm/md/lg、圆角方/round，独立于 Button | 普通图标操作与唯一实心播放键各有语义；A1 合并基础及 Tooltip |
| Field / Select | 工作台均高 32、圆角 8、横内距 8；Field 13px、Select 12px | 同行字号不一致需要 A1 处理；无通用 Bits TextInput，Field 保持薄封装 |
| Card | 入口、带 actions、带 body 三种组合，已有圆角 10px 与紧凑间距 | compact/content/form 是拟定用途，P0 未增加新 props；A1 明确内边距所有权 |
| 设置行 | 字 13px，纵内距 12、样例行高约 62.4px | 保留标题/说明/控件关系，A3 统一长英文与窄窗口布局 |
| 曲目行 | 高 52、字 13、圆角 8、横内距 12px | A2 按曲名/元数据角色校准，不直接全局放大 fs-base |
| 周时间网格 | 1280 窗口下 658×200px；小时字 9px | A10 可读性与缩放待治理；保留连续多段拖选和单行输入 |
| 播放页 | 样例封面 240×240、独立圆角 16；歌名 30px；普通歌词 14px/21px、内距 10×14、opacity .55 | 独立圆角、正文与透明度继续纳入 A/B；P0 未改变视觉 |
| 明暗文字 / 面 | text-2 深 .58 / 浅 .68；surface-2 深 .72 / 浅 .74；surface-4 深 .97 / 浅 .98 | 沿用 token；新频谱最亮合成背景的对比度尚待 B/C 实测 |

截图已人工核对四代表页明暗、960 与 1280 窗口，并补充工作台队列和完整网格视图。960 高度下设置与节目编辑需要内容区滚动，不能从首屏截图推出全部表单可见；底栏保持固定。自动化使用 reduced motion 和暂停 fixture，普通浏览器另核对频道及播放场景，当前旧前景环形频谱仍可见。

### 11.3 静态盘点与产物

`npm run audit:ui-styles` 输出 `ui/test-results/ui-style-inventory.json`：25 件 kit，11 件直接引用 Bits，八类 CSS 声明共 900 条。扫描剔除 CSS 注释并保留源行号，不把 scroll-padding 等同名后缀计作 padding；尚不涵盖 inline style、动态数值与所有 CSS 属性。

| 属性 | 声明数 | 含字面量线索 |
|---|---:|---:|
| border-radius | 128 | 70 |
| font-size | 195 | 3 |
| padding | 140 | 110 |
| gap | 165 | 133 |
| opacity | 58 | 24 |
| background | 181 | 3 |
| box-shadow | 23 | 6 |
| backdrop-filter | 10 | 0 |

字面量计数含 0px、几何值和合理发丝线，是初筛线索，不是缺陷总数；token 引用也可能有用途错误。A01–A12 仍按页面及交互逐项处理。A13 的 UI README 已更新。

`ui/test-results/ui-baseline/` 每组保存 page.png 与 metrics.json；频道额外保存 queue.png、schedule.png。产物被 gitignore，命令可重新生成；本轮没有建立截图比对阈值，也没有把现有视觉标为最终通过。

### 11.4 验证范围

- `npm run test:e2e:dev -- --project=regression`：55/55 通过，包括 10 个新增开发场景/登录适配用例与既有 45 个回归。13 页面入口在一个用例中逐一访问，搜索页额外输入查询后检查结果。
- `npm run test:ui:baseline`：24/24 通过，记录三窗口实际尺寸、明暗主题、封面/歌词装载与运行错误；频道补充截图覆盖编辑区。
- `npm run check`：0 error / 0 warning；`check:deps`、`check:ui-layers`、`check:style:strict`、`git diff --check` 通过。
- 以本地临时 Node HTTP 服务和独立 Vite 1421 验证七类代理路径：路径/查询、POST body/认证头、206 Content-Range/字节内容，以及 SSE 在响应结束前到达首段；测试服务已退出。
- 本轮无构建、无提交、无后端/Compose 修改。没有运行真实 HTTP 服务端到端、Tauri 真窗口、Firefox 或 GPU 性能测试；DEV 分支的产物剔除由代码边界约束，最终构建检查留到 C。

P0 后续的 A1 实施见 §12；频谱候选和 B 的渲染方案尚未实施。

## 12. A1 实施与验收记录（2026-11-18）

### 12.1 共享切口

A01–A04、A09 的共享组件切口已完成，A05 的 Header/Toolbar 与壳层节奏已落地；页面业务布局继续归 A2/A3。没有改动后端、部署或频谱实现。

| 部分 | 落地结果 |
|---|---|
| Button / IconButton | 使用已安装 Bits Button.Root；原生属性、事件、type 与 ref 透传。图标按钮组合相同基础与 Tooltip，删除重复皮肤和旋转器；loading 保留内容占位、可访问名称并禁止重复操作 |
| Field / 登录 | 原生输入属性透传，关联 label、合并描述与错误；搜索前后缀和胶囊变体。IME 合成回车不触发 onenter。登录使用 Field/Button/InlineError 与原生表单，失败后等待 disabled 更新再回焦 |
| Dialog / Card | Dialog 主体默认 16px，歌单选择列表显式 none，清除调用方重复外内距；标题、说明和动作区各自拥有留白，主体在短窗口内滚动。Card 保留紧凑入口、独立操作、带正文三种已有结构，未新增配置协议 |
| 浮层 | Menu/Select/Popover 的面板圆角 10px、条目 6px，文字与控件字号同族；Tooltip 12px，限制长文本宽度。Portal 和碰撞处理继续由 Bits 拥有 |
| 抽屉 | Bits Popover.ContentStatic + Portal；trapFocus=false、preventScroll=false，无 aria-modal=true。固定上下栏间几何，顶栏/底栏可操作；删除手写全局 Esc、焦点记录与转移代码。Tooltip 的 Esc defer 交给外层，菜单/选择器仍先关闭自身 |
| Tabs | 新增薄 TabList，Beets 使用 Bits Tabs.Root/Trigger/Content，方向键切换、禁用与面板关联生效，只有当前业务面板挂载；TextTabs 保留设置锚点和排序导航，两者共用 tabs.css |
| 应用壳 | 顶栏与浏览过滤复用 Field；窗控和倍速复用按钮基础。Header 保留标题层并允许操作区换行，Toolbar、侧栏和页面边距引用 token。底栏右组按实际控件宽度占位，避免放大后侵入播放控制区 |

100% 缩放下，小/常规/大控件高 28/32/36px，常规文字 13px、小按钮与提示 12px；默认控件圆角 8px、小控件 6px、浮层 10px、模态与抽屉 14px。骨架 54/216/92px、页标题 26px、全局 fs-base 12px 保持。没有把整站正文统一放大，也没有新加常驻侧栏。

### 12.2 验证与证据

- `npm run test:e2e:dev -- --project=regression`：72/72 通过。新增 17 项 kit 行为用例，覆盖 IME、登录错误与原生提交、Beets Tabs、抽屉与底栏共存、两层 Esc 和焦点恢复；包含明暗两主题 × 90/110/125/150% 的 960×600 频道弹窗与 Select，以及四档缩放的桌面能力底栏。底栏断言检查按钮边界和彼此重叠，不只检查页面总宽度。
- `npm run test:ui:baseline`：24/24 通过，四代表页 × 明暗 × 三窗口。人工复核曲库、设置、频道工作台、播放页与弹窗；原 P0 基线保留在忽略目录 p0-baseline，当前 ui-baseline 为 A1。基线不是像素差异阈值测试。
- `npm run check`：0 error / 0 warning；`check:deps`、`check:ui-layers`、`check:style:strict`、`git diff --check` 通过。样式盘点为 26 件 kit，其中 14 件直接引用 Bits，八类 CSS 声明 863 条；这只是盘点，不作为设计质量分数。
- Vite HMR 实地核对频道表单、可见焦点、浅色放大播放栏和 EQ 抽屉；当前预览捕获的 console error/warning 为空。截图为 ui/test-results/a1-preview.jpg，代表页拼图为 a1-baseline-contact.jpg，均不入库。IAB 尺寸覆盖所得实际 CSS 视口为 1200×750，不能计作 960×600；精确窗口证据来自正式 Playwright 矩阵，临时覆盖已复原。
- 没有运行构建、提交、真实 HTTP 服务端到端、Tauri 真窗控/播放或 GPU 性能测试。桌面能力 mock 的通过不等于原生窗口验证；最终 verify/build 与原生冒烟仍归 C。

### 12.3 后续边界

下一批 A2 处理首页、曲库、歌单、专辑/艺人、搜索以及队列/EQ 的业务交互与局部布局，包括重复播放主操作、队列嵌套按钮和 Knob 替换。A3 再收拢设置、Beets、收听/推流与剩余全局浮层，处理快捷键翻译键、时间网格可读性、删除确认的 AlertDialog 等页面切口。B1/B2 的九种频谱尚未接入。

## 13. A2 实施与验收记录（2026-11-18）

### 13.1 浏览与播放交互

A06/A08 的浏览与播放操作完成，首页和播放页同步收拢局部布局。页面标题层、左侧栏、列表行高和虚拟化数学保持既有结构。

| 部分 | 落地结果 |
|---|---|
| 播放主操作 | 专辑、艺人和歌单只保留 Header 的播放全部；曲库和搜索保留列表工具条入口。加载或失败时禁用详情页播放。工具条允许换行，操作组不宣称没有键盘模型支撑的 toolbar 语义 |
| 列表与首页 | 文本列允许收缩，时长与日期列保留可读宽度；封面播放键在键盘焦点进入时可见，子按钮双击不触发行播放。首页按内容容器宽度将双表改为上下排列，统计区回流 |
| 搜索与浏览状态 | 搜索输入立即使旧请求代次失效，晚到响应不覆盖新词；加载时隐藏旧词结果和统计。专辑、艺人分别维护加载与错误状态，失败提供重试，不显示为空库 |
| 返回位置 | 宫格的搜索词、分页与滚动位置进入导航返回栈，列表恢复不被焦点跟随覆盖。仅保存会话态，没有增加 URL 或持久化协议 |
| 队列 | 拖排把手、播放与移除使用独立兄弟按钮；Enter/Space 移除不会播放，Delete 移除，Alt+上下键排序。成功快照后焦点移到相邻或目标项，空队列回到关闭按钮；失败不留下待恢复焦点 |
| 歌单 | 保留拖排、批量选择与虚拟滚动，增加 Alt+上下键重排和目标焦点恢复；操作结果仍由宿主快照确认 |
| EQ | 预设改共享 Select，删除自制 Knob；十个频段和三个宏观增益全部使用共享 Slider，提供 dB 可访问值和关闭按钮。预设身份仍按十频段判断，宏观增益独立；十三个控件不表示十三频段 |
| 播放页 | 封面尺寸受可用栏高约束，信息回到文档流并允许滚动，网格行显式允许收缩，避免 150% 缩放时歌词固有高度撑出主区；长标题保留完整 title |

队列元数据分批、负缓存、重复曲目以及后端权威队列状态保持。mock 曾将歌单重排结果重新按曲名排序，与 Rust 的 position 顺序不一致；排序集中到 sortForSource，列表、播放查询和选择查询共用，并保留重复项。未修改后端。

队列拆成独立把手后，播放按钮移除共享 click suppression 判断，避免拖排后立即按 Enter 被误吞；相关回归包含拖排结束后的立即播放。

### 13.2 验证与证据

- `npm run test:e2e:dev -- --project=regression`：90/90 通过；最后移除队列多余 suppression 后，补跑 browse-playback.spec.ts 全部 18 项通过。覆盖唯一播放主操作、批量选择、鼠标拖排、键盘排序/移除与回焦、千首含重复项队列、分页和非零滚动返回、搜索乱序、独立加载与失败重试、EQ 步进/边界/禁用与预设。
- 新增矩阵为 960×600 × 明暗主题 × 125/150% × 七个浏览/播放页面，共 28 张截图；暗色中文、浅色英文。检查内部内容边界、页面错误和 console.error；截图为当前取证，没有设像素差异阈值。
- `npm run test:ui:baseline`：24/24 通过，四代表页 × 明暗 × 三窗口。A1 基线保存在忽略目录 a1-baseline，当前 ui-baseline 为 A2。
- `npm run check`：0 error / 0 warning；`check:deps`、`check:ui-layers`、`check:style:strict`、`git diff --check` 通过。样式盘点为 25 件 kit、八类 CSS 声明 868 条，仅用于定位复核范围，不作为质量分数。
- Vite HMR 预览核对首页日期、浅色英文长文本播放页、键盘移除保持暂停、EQ Select 和十三个 Slider、Esc 回焦，捕获的 console error/warning 为空。截图为 ui/test-results/a2-preview.jpg、a2-queue-preview.jpg，均不入库。IAB 实际大视口约 1500×1354，精确 960×600 证据来自 Playwright 矩阵。

### 13.3 后续边界

下一批为 A3：设置、Beets、收听/推流、登录和剩余全局浮层。B1/B2 的九种频谱尚未接入，现有绘制保持。本批没有构建、提交、后端或 Compose 修改；真实 HTTP 服务、Tauri WebView2 播放/窗控与 GPU 性能未实测。完整 verify、最终构建和真宿主冒烟仍归 C。

## 14. A3 实施与验收记录（2026-11-18）

### 14.1 配置与任务界面

A07、A10、A11 的配置侧切口，以及设置、Beets、收听/推流、登录与全局确认浮层完成治理。保留左侧栏、普通页面标题层、频道内页小标题、系统歌单与全曲库来源、连续多时段拖选和一次保存；目录的桌面/Web 能力差异继续沿用 docs/76。

| 部分 | 落地结果 |
|---|---|
| 设置 | 背景/前景形态和历史保留周期使用共享 Select；父开关关闭时禁用子项并保留选值，山形参数只在山形启用。快捷键名称、冲突和录制名称走中英文翻译；清空写入空绑定，恢复默认独立处理 |
| 布局与状态 | 设置行和快捷键样式归组件，窄内容区控件换行；统计与目录卡取消负 gutter。Switch 几何随 rem 缩放，删除父子重复透明度；修正设置/收听页未定义 gutter 引用及插件标签边框 token |
| 确认与保存 | 新增 Bits AlertDialog，共享 Dialog 的皮肤；歌单/曲目、频道、Beets 删除确认初焦点落取消，外点不关闭，Esc 取消并回焦。Dialog 的 dismissible 接入 Bits Escape/Outside 层，歌单、频道、目录和 Beets 保存时同时阻止关闭与重复提交 |
| 歌单编辑 | 原生表单提交，等待创建/改名/规则更新结果；失败保留名称、类型和规则草稿，模态内显示错误。已有动态歌单先改名再存规则，两个命令仍为顺序执行，不宣称跨命令原子性 |
| Beets | 插件改用 Bits Checkbox，整行标签可切换；安装/卸载互斥禁用，未知阶段使用不确定 Progress。安装事实表、查询条件、结果与整理差异视图统一留白和 token，长路径/文本约束在业务容器内 |
| 推流 | 频道新建/编辑支持原生 Enter 提交；工作台操作组可换行。周格小时使用 fs-sm（100% 时 11.5px），网格最小 38rem、最大 48rem，窄窗口局部横滚；星期与起止仍为单行，支持键盘箭头/Enter 与结束 24:00 |
| 收听与反馈 | 保留层叠轮播；卡片在窄短窗口按可用高度缩小，箭头留在内容边界，焦点进入显示移除键。添加流使用原生表单。登录可滚动，Toast 使用 status 语义、长文换行与减少动态效果 |

曲库扫描未知进度改为不确定 Progress；既有目录删除 popover、清空曲库内联确认仍保持，不重复增加确认步骤。频谱形态在此批只治理控件与依赖状态，尚未替换任何绘制或设置枚举。

### 14.2 验证与证据

- 配置专项 13 项覆盖快捷键清空/默认/冲突、频谱依赖、Web 目录验证、频道 Enter、确认焦点/外点/Esc、周格多次拖选与间隙命中、Beets 整行勾选，以及歌单慢保存、失败保留与规则重试。新增的慢保存回归定位了只检查关闭回调仍会被 Bits 内部关闭的问题，现由交互层阻止。
- 新增布局矩阵为 960×600 × 明暗主题 × 125/150%，暗色中文、浅色英文；覆盖设置、Beets 安装/分析/整理、收听、推流、两种登录，共 32 张截图。断言检查内部宽度、卡面高度、小时刻度缩放与页面错误，不以根文档不溢出代替内部检查。
- Vite HMR 实地核对设置外观、Beets 安装、频道工作台与完整周视图、浅色英文收听和添加流弹窗，捕获的 console error/warning 为空。截图为 ui/test-results/a3-preview.jpg 和 a3-schedule-preview.jpg；IAB 实际视口约 1500×1354，精确小窗口取证来自 Playwright。
- `npm run test:e2e:dev -- --project=regression`：103/103 通过；最后 gutter 与插件边框修正后，配置专项 13/13 补跑通过。`npm run test:ui:baseline`：24/24 通过，当前 ui-baseline 为 A3，A2 保存在忽略目录 a2-baseline。所有截图为当前取证，没有像素差异阈值；预览与测试产物不入库。
- `npm run check`：0 error / 0 warning；`check:deps`、`check:ui-layers`、`check:style:strict`、`git diff --check` 通过。样式盘点为 26 件 Svelte kit、八类 CSS 声明 869 条，共享 dialog.css 参与声明盘点但不计组件数量。这只是复核线索，不作为设计质量分数。

### 14.3 后续边界

下一批 B1 接入背景 B0/B1/B4，再按 B2 接入前景 F1/F2/F4/F5/F6/F7。A3 无构建、无提交、无后端/Compose 修改；真实 HTTP 服务、Tauri WebView2 播放/窗控与 GPU 性能仍未实测。完整 verify、最终构建、对比度与真宿主冒烟继续归 C，计划保持 open。

## 15. B1 实施与验收记录（2026-11-18）

### 15.1 三种背景与设置

- B0 `ridge` 保留现有山形 shader、包络和 soft/balanced/neon 参数，只调整资源所有权及无输入处理；默认仍为山形柔雾。
- B1 `aurora` 使用五层宽光幕；240ms 时间平滑、Gaussian 半径 6 / sigma 2.2 两遍、Catmull-Rom 256 点与实验室修订一致。单点峰稳态低于 0.14，256 点相邻最大差低于 0.008；30/60Hz 一秒结果误差低于 0.00001，宽能量没有被削成平线。
- B4 `curtain` 使用十二列柔光，随平滑分带变化高度和宽度。两种新背景共用纹理、配色与预乘 over 合成，浅色用正常主题色加深，未移入实验室的负色输出或固定底色。
- 背景艺术速度为 0.65，与音频平滑分离；暂停冻结艺术时钟，幅度淡出，无数据不模拟音乐跳动。常驻 Flow 暂停时也冻结，主题或尺寸改变仍重绘静态场。
- 设置 Select、新枚举、schema 和中英文同步；B1/B4 隐藏山形参数。背景 `ripple` shader 与 Canvas 2D 整条路径删除，非法值直接回默认，没有建立旧值映射。

### 15.2 舞台与源生命周期

舞台累积有效 dt，visibilitychange 和 context lost/restored 重置帧间时钟，恢复不累计后台时间。上下文丢失时背景 canvas 隐藏、背景 tap 引用释放，CSS 渐变继续显示；恢复沿用当前图层并重新下发尺寸，Flow 的离屏场标记为脏。

GlStage 拥有所挂图层，remove 与舞台销毁统一释放，避免 Svelte effect 清理顺序造成重复 dispose。各图层拥有自己的几何体、纹理和材质，不留下模块级共享 quad。形态切换保留同一个 renderer、scheduler 订阅与 source 引用。只有成功创建的舞台申请背景 tap，硬件不可用时不创建 2D 背景。

RemoteSpectrumSource 的 live 改为最近一秒实际收到帧，订阅成功但没有数据不再视作实时；短帧清除尾部残值，dispose 清掉所有引用并关闭 tap。音频链与 48 带 / 30Hz 协议未修改。

共享播放页区域注册与 viewport/scissor 一并放在 B2：实际前景消费者尚未接入，本批不添加无人调用的注册接口。B1 先完成背景舞台的所有权、时钟、降级和恢复；单 renderer 的前后景组合仍需 B2 验收，不能将本批结果扩写为九种效果已完成。

### 15.3 浏览器取证

环境为 Windows、系统 Edge 157、ANGLE Intel Arc Graphics (0x00007D55) / D3D11，1280×800、DPR 1、应用缩放 100%。独立 GlStage 的三种背景各跑 30 秒，下表取独立归档复测值；Flow 与所选背景同时绘制，输入依次包含低能量、低频、高频、宽谱变化和静音。

| 背景 | JS 提交 p50 / p95 | 帧间隔 p50 / p95 | draw calls | 常驻 GPU 资源 |
|---|---|---|---|---|
| B0 ridge | 0.30 / 0.60ms | 16.7 / 16.8ms | 2–3 | 2 geometry / 2 texture / 3 program |
| B1 aurora | 0.30 / 0.60ms | 16.7 / 16.8ms | 2–3 | 同上 |
| B4 curtain | 0.30 / 0.60ms | 16.7 / 16.8ms | 2–3 | 同上 |

数值是前端 CPU 提交时间与 rAF 间隔，不是 GPU timer query，也不代表 Tauri WebView2、4K/DPR2 或前后景组合性能已通过。30 秒取证结束释放后 geometry / texture / program 均为 0；20 次切换后的常驻资源保持上表数量。

运行时证据单独归档在 ui/test-results/background-verification：background-gl.json、product-capability.json、三份 *-30s.json 和三背景 × 明暗 × 五配色的 30 张页面截图。像素回读验证各 shader 确实产生内容，暂停/断流后回到同一个静息基线；另覆盖实际 context loss/restored、恢复改高、隐藏时钟、全局开关与 reduced motion、无硬件 GL 的 CSS 降级。

内置浏览器人工对照了山形、暗色极光和浅色 meadow 光帘的页面叠加，未见负色、发白边或窄尖峰；完整数值对比度、长时间真实音乐与换曲观感仍随 B2/C 继续。Vite 因删除/重建组件保留了旧导入缓存，已重启开发服务恢复源码预览；没有构建。


### 15.4 最终检查与后续

- `npm run test:e2e:dev -- --project=regression`：113/113 通过，包含三背景各 30 秒取证及 30 张配色截图。
- `npm run test:ui:baseline`：24/24 通过。
- 背景专项独立归档复测 10/10 通过，使用独立输出目录保存取证：`npm run test:e2e:dev -- --project=regression e2e/dev/background.spec.ts e2e/dev/background-perf.spec.ts --output test-results/background-verification`，避免后续窄范围回归清理整批测量产物。
- `npm run check`：0 errors / 0 warnings；`check:deps`：160 模块 / 622 内部依赖，无循环；`check:ui-layers`、`check:style:strict`、`git diff --check` 通过。
- 独立源码探针做了 5 次组件 HMR，每次均为 1 canvas / 1 scheduler 订阅 / 1 source 引用；产品背景实际 context loss 时 source 引用降到 0，canvas 隐藏，恢复后回到 1；关闭全局效果后三项均为 0，无 console.error 或 pageerror。探针 JSON 归入 ui/test-results/background-verification/background-hmr.json。
- 人工预览截图位于仓库外 D:/MUSIC_Player/output/77-b1-aurora.jpg，30 组配色缩略对照为 77-b1-contact.png。

B1 完成。下一批 B2 接入 F1/F2/F4/F5/F6/F7 六前景及同舞台区域裁切，删除播放页独立 Canvas 2D，完成 18 组合与前后景引用计数验收。C 继续承接完整 verify、最终构建、真实 HTTP/Tauri 播放、WebView2 GPU 与数值对比度。没有提交、构建或后端/Compose 修改，现有其他未提交改动保持。

## 16. B2 实施与验收记录（2026-11-18）

### 16.1 六前景与输入

F1/F2/F4/F5/F6/F7 分别接入 orbit / ribbon / lattice / contour / string / shore。六个有限图层类共用采样、主题和预乘 alpha 合成，独立提供解析 shader；仅提取选中的形态，没有复制实验室模拟音频、文件解码、调试面板或 vendor。

原始 48 带采用 90ms 时间平滑；F2/F6/F7 连续曲线复用 240ms、两遍 Gaussian 与 256 点 Catmull-Rom。F4 保留逐柱节奏。所有缓冲与纹理就地更新，艺术速度为 1.50，与音频与播放时钟分开。暂停与断流冻结艺术时钟，120ms gate 衰减到透明；实时静音保留静息形态，稳定后停止艺术游走。

颜色取自应用 accent，辅色在同一主题内偏移；浅色模式在线性颜色空间降低亮度后转为 sRGB。累积 alpha 上限 0.82，颜色按相同比例缩放，控制多条轮廓交叠亮度。默认前景形态为 orbit，npSpectrum 默认仍关闭。设置、schema、白名单与中英文标签同步；旧形态或非法值落默认，没有旧值映射。

### 16.2 共享舞台与区域

AppShell 通过 Svelte context 提供最小 RenderHost；NowPlayingSpectrum 仅登记 target / source / style / paused，不再持有 canvas、订阅或绘图循环。AmbientBackdrop 管理唯一舞台、当前背景和前景；背景关闭时，播放页前景仍能维持舞台。前后景各持一个源引用，切换形态不重新订阅；暂停更新稳定请求，不重建图层。

RegionTracker 在 resize、滚动、祖先属性变化与布局 transition 期间测量区域，其余帧复用缓存；目标与 canvas rect 换算为逻辑像素，保留目标 viewport 尺寸并对 overflow 祖先与舞台求交。GlStage 先画全窗层再画区域层，每层之后恢复 render target、全窗 viewport 与 scissor。帧入口统一 clear，区域不覆盖标题栏和底部播放条。

播放页 Canvas 2D 绘制整段删除。无硬件 WebGL2、reduced motion 和全局关闭时只保留 CSS 背景；前景不绘制且不申请 tap。上下文丢失时共享画布隐藏，两侧引用释放；恢复重订阅并重新下发区域尺寸。该决策同步 AGENTS.md §3、ui-render 域账本和 ui/README.md。

### 16.3 组合与运行时取证

环境为 Windows、系统 Edge 157、ANGLE Intel Arc Graphics (0x00007D55) / D3D11，1280×800、DPR 1、应用缩放 100%；播放页登记区域 1180×654。六种前景各跑 30 秒，三种背景各占 10 秒，每段首秒不计统计，输入依次包含低能量、低频、高频、宽谱和静音。下表每格为 JS 提交 p50 / p95，单位 ms：

| 前景 | B0 ridge | B1 aurora | B4 curtain |
|---|---|---|---|
| F1 orbit | 0.30 / 0.80 | 0.30 / 0.80 | 0.30 / 0.70 |
| F2 ribbon | 0.30 / 0.80 | 0.30 / 0.60 | 0.30 / 0.60 |
| F4 lattice | 0.30 / 1.10 | 0.30 / 0.50 | 0.30 / 0.50 |
| F5 contour | 0.40 / 0.80 | 0.30 / 0.60 | 0.30 / 0.70 |
| F6 string | 0.30 / 0.60 | 0.30 / 0.60 | 0.30 / 0.90 |
| F7 shore | 0.30 / 0.70 | 0.30 / 0.60 | 0.30 / 0.60 |

18 组合的帧间隔 p50 为 17.7–17.9ms、p95 为 18.1–18.6ms，最大单帧间隔为 43.4ms；采样窗内超过 50ms 的帧为 0。draw calls 为 3–4，常驻资源均为 3 geometry / 3 texture / 4 program，释放后全部为 0。单前景实际只上传 shader 使用的一个采样纹理，另一 sampler 被编译器优化；单前景资源为 1 / 1 / 1。这里记录 CPU 提交与 rAF 间隔，没有 GPU timer query，不外推其他 GPU、4K/DPR2 性能或真宿主播放。

专项 17/17 通过，产物独立保存在 ui/test-results/foreground-verification。六 shader 像素回读确认实际内容、暂停冻结、断流退零、静音时钟冻结与 24 次切换资源恒定；产品用例确认背景/前景独立开关、退出播放页、context loss/restored、reduced motion 和关闭归零。DPR 1/2 × CSS zoom 100/125/150% 的六项像素检查覆盖移动、尺寸变化、overflow 祖先裁切与 GL 状态恢复。

18 组合明暗截图 36 张，加关闭背景的六前景明暗截图 12 张，共 48 张；已人工对照两张缩略总览，六种轮廓差异清楚，未见负色或发白边。内置浏览器另实际查看暗色 B1+F6、浅色 B4+F7 和 B0+F1，前景限制在内容区，歌词保持 DOM 衬底与层级。原始图与总览位于仓库外 output/77-b2-*.jpg。未把这轮目视检查记为数值对比度通过；五配色全前景矩阵、真实音乐与换曲观感仍归 C。

独立 HMR 探针连续更新 AmbientBackdrop 五次，每次保持 1 canvas / 2 source refs / 2 scheduler ticks（其中一个 tick 为歌词）；暂停后保留 1 个舞台 tick，关闭全局效果后 canvas / refs / ticks 均为 0，console.error 与 pageerror 为空。探针结果归档 foreground-verification/foreground-hmr.json。

首次组合测量期间源文件热更新，使测试持有的旧 DOM 节点失效；同时带时间戳的 Vite 模块与测试动态导入不是同一实例，原型探针未捕获舞台。该批测量已丢弃。补充每帧节点有效性断言与慢帧记录，重启 Vite 后保持源码稳定复测；18 组合 detachedFrames 均为 0，产品探针通过。HMR 五次单独放在测量之后验证，避免将开发热更新混入性能结果。

### 16.4 最终检查与后续

- 前景专项 17/17：`npm run test:e2e:dev -- --project=regression e2e/dev/foreground.spec.ts e2e/dev/foreground-perf.spec.ts --output test-results/foreground-verification`。
- 源码回归 119/119：`npm run test:e2e:dev -- --project=regression --grep-invert '持续绘制取证|30 秒绘制取证|18 种前后景组合|背景明暗与五配色'`；前景慢测和截图已单独通过，背景慢测与配色截图沿用 B1 取证，不重复计为本批新增验收。
- 四代表页明暗三窗口基线 24/24：`npm run test:ui:baseline`。
- `npm run check`：0 errors / 0 warnings；`check:deps`：164 模块 / 633 内部依赖，无循环；`check:ui-layers`、`check:style:strict`、`git diff --check` 通过。
- 共享舞台 HMR 五次通过；源码探针已还原，未留下页面探针属性或生产调试入口。六前景总览为 D:/MUSIC_Player/output/77-b2-preview.jpg。

B2 完成。C 继续承接完整 verify、最终构建、真实 HTTP/Tauri 播放与 WebView2 GPU、五配色前景和数值对比度检查；本批没有构建、提交或后端/Compose 修改。Vite 1420 保持运行，已有其他未提交工作保留。


## 17. C 整体验收与收尾（2026-11-18）

### 17.1 收尾修正

- 播放页阅读层增加明暗独立的 `np-text` 与广域渐隐衬底，当前句衬底随主题切换深浅。删除已播放歌词对 `text-3` 再叠 opacity 的双重透明；元数据和音频徽标共用阅读文字角色。键盘进入歌词时取消边缘 mask，避免焦点落在渐隐区。清除与现行深浅衬底矛盾的历史注释。
- 真实合成测量发现浅色装饰英文低于 4.5:1，Header 改用 `text-2`；保留字号、字距与标题层次。Tooltip 显式提供 `role=tooltip`，触发件继续有可访问名称。
- HTTP 回归从原生 title 断言改为实际 hover Tooltip；删除确认按 AlertDialog 语义检查。导航等待限定侧栏，避免恢复设置页时两个 nav 触发严格匹配错误。真实浏览器播放用例补充 AnalyserNode 非零采样及单共享画布检查，循环只作用于测试中的短音频。
- 原生三首小曲库扫描发现完成事件被 100ms 进度节流过滤：数据库已有三首，但设置仍停在遍历。宿主对 done / cancelled / failed 终态无条件发出，保留中间进度节流。隔离便携目录重编译复测收到 done，统计为三首。
- 标准 Edge、HTTP、Firefox 回归分别使用独立输出目录。此前默认 test-results 根目录会递归清理 B1/B2 的专项产物；文中历史测量数值与仓库外总览仍保留，本批不把已删除的原始文件描述成仍在目录中。

### 17.2 对比度与页面证据

`contrast.ts` 从真实文本节点取布局框，按 overflow 祖先裁切并排除被其他层遮住的区域；临时隐藏字形，保留面、衬底和几何，从浏览器合成截图读取文字后的背景，按文字 alpha 与祖先 opacity 计算 WCAG 相对亮度比值。歌词取中央 10%–90% 的阅读区域；焦点样本从轮廓外缘取实际背景，普通字目标 4.5:1、30px 当前句与焦点环目标 3:1。

播放页使用最大分带输入，覆盖三背景 × 六前景 × 明暗 × 五配色，共 180 组截图与数值。该输入只在测试页替换 SpectrumSource，不改变生产音频链。四代表页中的曲库、设置、频道工作台另以明暗五配色检查可见标题、说明、卡片、Select 和焦点；播放页由前述动态矩阵覆盖。静态文字与焦点样本 10/10 已通过。测量记录的是采样状态，不能推断所有动画时刻或全应用每个控件均满足 WCAG。

最终动态矩阵 10/10、180 组合全部通过，正文最小 4.94:1，当前句最小 5.41:1；代表页正文最小 4.67:1，焦点环最小 3.06:1。数字向下截取两位，仅代表这批采样。动态 JSON 与截图保存在 ui/test-results/contrast-verification，静态正文与焦点在 surface-contrast，数值汇总另存仓库外 output/77-c-contrast-summary.json。

最终检查：

- 完整 `npm run verify` exit 0：Rust 全量、svelte-check、HTTP transport、依赖/边界/分层/协议、UI 构建、45 项 dist e2e、9 项真实 HTTP、fmt、clippy -D warnings、生成类型校验、文风与 beets 冒烟均通过。
- 最后补强 HTTP 频谱探针后，真实 HTTP 9/9 再次通过；该次 webServer 构建包含最终 Header/播放页样式，是最终 UI 产物。Vite 有大于 500kB 的 chunk 提示，未作为构建错误或本批拆包任务。
- 源码交互与渲染回归 119/119 通过：排除已独立取证的持续性能、组合截图和两组对比度；没有把历史 B1/B2 的慢测数加入本批新回归数量。
- 四代表页 × 明暗 × 三窗口基线 24/24 通过；播放页 10 项对比度与其他代表页 10 项对比度独立通过。
- 最终 `npm run check` 零错误零警告；`check:style:strict`、`git diff --check` 通过。样式盘点为 26 件 kit、868 条声明，仅作复核索引。

### 17.3 真实宿主

原生使用最新 debug 宿主和实际音频 sidecar，便携数据根为仓库外 `output/77-c-native/data`。测试复制三首本地 MP3（Lovely Picnic / Morose Dreamer / Romantic Smile，48kHz / 320kbps），未改原始音乐目录。Windows 目录选择器实际选中该隔离目录并入库；标题栏最大化/还原、最小化及单实例入口恢复均实际操作通过。

运行时为 WebView2 / Edge 157、ANGLE Intel Arc Graphics (0x00007D55) / D3D11，逻辑窗口 1280×800、系统 DPR 1.5、应用缩放 100%。真实播放时钟推进，暂停位置稳定，恢复并换曲后频谱继续非零；WebView 音频元素数量为 0。前后景同时开启为 1 canvas / 2 source refs，关闭背景仍为 1 / 1，双关为 0 / 0，退出播放页保留背景 1 / 1。探针无 console.error / pageerror。

运行脚本、JSON 与原生截图位于仓库外 output/77-c-native.*。三代表组合各取 30 秒，真实音乐、DPR1.5、无并发 GPU 测试，得到以下 CPU 提交与 rAF 间隔（单位 ms）：

| 组合 | 帧数 | CPU p50 / p95 | 帧间隔 p50 / p95 / max | >50ms |
|---|---|---|---|---|
| B0 + F1 | 1801 | 0.50 / 1.00 | 16.7 / 17.0 / 28.3 | 0 |
| B1 + F6 | 1800 | 0.50 / 0.80 | 16.7 / 17.0 / 20.6 | 0 |
| B4 + F7 首测 | 1249 | 0.50 / 0.80 | 16.7 / 33.3 / 945.6 | 45 |
| B4 + F7 独立复测 | 1800 | 0.50 / 0.80 | 16.7 / 16.9 / 17.7 | 0 |

各段为 3–4 draw calls，常驻 3 geometry / 3 texture / 4 program。首次 B4+F7 出现长帧，保留原始结果；独立复测增加 longtask 与 visibilitychange 记录，两者均为空、页面可见且有焦点，未重现卡顿。首测没有对应归因记录，不能把它确定归因于系统调度或 shader，也不记为整段无掉帧通过；更长时间观察留在 docs/35 P2-14。约 16.9–17.0ms 的 p95 接近 60Hz，单次采样不外推所有设备。原始测量在 output/77-c-native-perf.json，复测为 77-c-native-perf-repeat.json。

HTTP 使用临时真 aiya-server，登录、目录添加/扫描、媒体 Range 播放、浏览器频谱、推流队列/时段、私有 HLS 分段授权与重连共 9 项通过。此处 HLS 测试使用本地清单与片段，没有验证真实 Liquidsoap 常驻推流。

### 17.4 验收边界

本批不生成安装包、不发布、不提交、不重新部署 Compose。4K / DPR2 原生性能、其他 GPU、macOS / Safari、原生 HLS 回退、长时间硬件音频 soak 与真实 Liquidsoap 出流不在本机这组证据内。DPR1/2 的布局与像素裁切已由源码 GL 用例覆盖，不能等同这些设备的持续性能。所有性能时间均为前端 CPU 提交和 rAF 间隔，不是 GPU timer query。


C 收尾完成，docs/77 关闭。docs/35 与相关域账本同步；Vite 1420 继续提供源码预览。原生隔离测试实例在取证后关闭，测试音乐、脚本和产物仅留在仓库外 output；未提交、未发布、未重新部署。

## 18. 歌词视觉反馈修订

按实地反馈移除歌词整区的渐隐衬底和当前句 halo 元素，删除对应 scrim token。当前句保留强调色、字号和字重，只以紧贴字形的轮廓阴影与柔影突出；深色主题使用黑色阴影，浅色主题使用白色阴影，长句换行时阴影随字形绘制。非高亮行不新增处理，歌词滚动、边缘渐隐与键盘焦点行为保持原样。

§17 的歌词数值是衬底版历史证据，不适用于本次阴影方案。现有矩形背景取样会移除 text-shadow，不能反映字形阴影的阅读效果，因此播放页动态用例改为元信息数值对比度与全页组合截图，歌词改为目视检查，不宣称现行歌词通过原数值矩阵。

验证：`npm run check` 为 0 error / 0 warning，`check:style:strict` 与 `git diff --check` 通过。源码专项 5/5 通过，包含 aurora 配色明暗主题各十八种频谱组合、播放页布局与 EQ/队列交互；已目视检查明暗 B0+F1 截图，无歌词区域色块。36 张组合截图保存在 ui/test-results/lyric-shadow。本次使用 Vite HMR，没有构建安装包或重新部署。
