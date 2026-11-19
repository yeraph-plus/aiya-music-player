# ui/

Svelte 5 + TypeScript + Vite。桌面运行于 Tauri 2 / WebView2，Web 运行于浏览器。

## 结构与组件

- `views/`：页面编排；`stores/`：业务状态，依赖方向由 `check:deps` 校验。
- `components/ui/`：通用 kit，复杂交互使用 Bits UI；图标使用 Lucide。
- `components/content/`：Header、Toolbar 等内容结构；`components/shell/`：应用骨架与窗口功能。
- `styles/tokens.css`：主题、尺寸、字体和层级参数；页面专用布局归页面或业务组件。
- `lib/ipc/client.ts`：页面和 store 的命令入口；契约类型来自 `lib/ipc/generated/`，由 Rust 生成。
- `lib/ipc/`：Tauri、HTTP/SSE 与 mock transport；`dev/`：只在 Vite 开发态装配的场景与资源。

播放权威状态由宿主维护。桌面 UI 不创建 Web Audio 节点；Web 的浏览器媒体播放与分析由 HTTP transport 的音频适配层管理。

## Kit 组合

- `Button` 基于 Bits `Button.Root`，透传原生按钮属性与事件，默认 `type="button"`，表单提交显式使用 `type="submit"`。`IconButton` 组合相同基础与 Bits Tooltip，`label` 同时提供可访问名称；不另加原生 title。loading 保持内容占位并禁用按钮。
- `Field` 使用关联 label 的原生 input，支持 password、name、autocomplete、pattern、inputmode 等原生属性。默认外观用于表单，`variant="search"` 用于顶栏胶囊搜索；prefix/trailing 放图标与清除操作。`onenter` 排除 IME 合成回车；原生 form 使用 onsubmit。错误和 hint 自动关联描述，外部 aria-describedby 可合并。
- `Dialog` 拥有 header、body、actions 的内边距，body 默认 16px；列表型主体显式设 `bodyPadding="none"`。调用方只负责内部字段间距。首焦点有业务要求时使用 autofocus 回调；异步提交设置 `dismissible={false}`，同时禁用取消/输入/提交，不能只在 onclose 内检查忙碌。
- `AlertDialog` 使用 Bits 确认语义，与 Dialog 共用 dialog.css；autofocus 显式指向取消，外点不关闭，Esc 可取消。异步确认可用 dismissible 阻止 Esc；动作槽由调用方等待结果后关闭，不使用自动关闭的 Action 包装。
- `Card` 的紧凑入口与独立 actions 卡使用 8px 内边距，带 children 的正文卡使用 16px；调用方负责正文内部间距，不再添加一层外边距容器。
- 真正互斥面板使用 Bits `Tabs.Root` / `Tabs.Content` 与 `TabList`；`TextTabs` 用于设置滚动导航、列表排序，不宣称 tabpanel 语义。两者共享 styles/tabs.css 的外观。
- `SideDrawer` 使用静态 Bits Popover，保持固定右侧几何，关闭焦点陷阱和滚动锁；顶栏与底栏可操作，队列/EQ 互斥仍由业务状态控制。Tooltip 的 Esc 让外层浮层优先处理，嵌套 Select/Menu 先关闭自身。

控件高度 sm/md/lg 为 28/32/36px；普通控件文字 13px，小按钮与说明 12px。圆角、间距和浮层面统一引用 tokens.css，页面负责业务布局。

## 浏览与播放操作

专辑、艺人和歌单详情的播放全部位于页面标题操作区；曲库与搜索位于列表工具条。宫格过滤、分页及滚动位置随返回栈恢复，只保存在当前会话。

队列的播放、拖排把手和移除是独立按钮。播放按钮使用 Enter/Space 起播，移除按钮使用 Enter/Space 只移除；播放项或把手聚焦时可用 Delete 移除、Alt+上下键排序。歌单列表同样支持 Alt+上下键重排。成功操作后焦点跟随目标或相邻项，空队列回到关闭按钮。

EQ 预设使用共享 Select，十个频段和三个宏观增益使用共享 Slider，支持方向键步进和 Home/End 边界；可访问值包含 dB。预设身份按十频段判断，三个宏观增益独立。

## 配置与任务操作

设置锚点在内容区滚动，减少动态效果时立即定位；窄容器将控件放到说明下一行。频谱形态使用 Select，父开关关闭时禁用并保留当前值。快捷键录制中 Esc 取消、Backspace 清空绑定；空串表示禁用，恢复默认单独执行。

歌单编辑等待命令结果后关闭，失败保留草稿并显示错误，忙碌时不允许重复提交或关闭。动态歌单的改名和规则保存仍是两个顺序命令。

推流周格保持连续拖选，每次新增一条时段，与星期/起止输入同步，最后统一保存。网格在窄窗内横向滚动，键盘方向键移动、Enter 添加；时间行保持单行并可局部滚动，受限文本支持 HH:MM 和结束 24:00。Beets 插件使用整行 Checkbox 标签，安装未知阶段使用不确定 Progress。

## 源码预览

所有 npm 命令在仓库根执行：

```powershell
npm run dev:ui
```

入口 `http://localhost:1420/` 使用 mock，源码修改通过 HMR 更新。`vite preview` 服务已有 dist，适用于产物验证；日常 UI 迭代使用 dev server。

开发场景通过查询参数选择；每次带 `page` 打开都会得到确定的起始数据，交互修改保留于当前内存，刷新后重置。

| 参数 | 可用值 / 行为 |
|---|---|
| `backend=mock` | 强制 mock；省略时没有宿主标记也默认 mock |
| `host=web` | mock 采用正式 Web 能力判定；显示推流和容器内目录输入，隐藏桌面音频配置 |
| `page` | `home/library/playlist/albums/artists/album/artist/search/nowPlaying/settings/beets/live/broadcast`；推流需要 `host=web` |
| `scene` | `standard` 默认；`long` 中英长文本；`empty` 空曲库/歌单/频道；`issues` 缺失节目来源；`runtime-error` 推流进程错误 |
| `theme` / `lang` / `scale` | `dark/light`、`zh/en`、`90/100/110/125/150`；未指定时，带页面的场景为深色/中文/100% |
| `lyrics=none` | 移除默认同步歌词样例；播放页默认装载一首歌，暂停在 24 秒 |
| `gate=password` / `gate=token` | 真实登录组件的展示预览；输入 `preview` 进入应用，其他值显示错误，不请求认证或保存凭据 |
| `delay` | 所有 mock 命令延迟，单位 ms，上限 3000，用于观察加载态 |
| `failCmd` | 指定命令失败，如 `library_list`、`station_get` |
| `beets=absent` / `missing=2` / `live=<url>` | 未安装 Beets、缺失位置提示、直播播放态 |

示例：

- [Web 频道列表及工作台](http://localhost:1420/?backend=mock&host=web&page=broadcast)：点击频道配置进入，支持内存新建/编辑/删除、保存/应用和开停播。
- [浅色英文长文本播放页](http://localhost:1420/?backend=mock&host=web&page=nowPlaying&scene=long&theme=light&lang=en&scale=125)。
- [Web 空曲库](http://localhost:1420/?backend=mock&host=web&page=library&scene=empty)。
- [口令登录预览](http://localhost:1420/?backend=mock&host=web&page=library&gate=password)。

搜索直达页不带查询；在顶栏输入后回车即可检查结果。Beets 子页、队列、EQ、菜单与模态从应用内入口进入。mock 不出真实声音、不执行 Liquidsoap、不提供可播放 HLS；频道状态用于 UI 检查。场景不会写回 mock 的设置和路由存储，应用外观镜像仍沿用正常行为。没有场景参数时使用普通 mock 的持久化设置和路由。

## HTTP 联调

先启动真实服务，再给 Vite 配置目标并重启 dev server：

```powershell
$env:AIYA_DEV_HTTP_TARGET = 'http://127.0.0.1:8799'
npm run dev:ui
```

浏览器访问 `http://localhost:1420/?backend=http`。代理覆盖 `/auth`、`/ipc`、`/events`、`/artwork`、`/audio`、`/stream`、`/hls`，保留查询、认证头、SSE 流式响应及媒体 Range。未设置目标时不启用代理。`host/page/scene/gate` 等开发 fixture 参数只作用于 mock。

## 验证与基线

```powershell
npm run check
npm run check:deps
npm run check:ui-layers
npm run test:e2e:dev -- --project=regression
npm run test:ui:baseline
npm run audit:ui-styles
```

开发 Playwright 使用系统 Edge，连接或启动 1420，可复用正在运行的 Vite。`test:e2e:dev` 不指定 project 时同时运行回归和基线；正式 `test:e2e` / Firefox 的 dist 测试排除开发专用用例。

浏览/播放回归覆盖搜索晚到响应与失败重试、独立 facet 加载、分页返回、键盘移除和排序、千首含重复项队列的虚拟化，以及 EQ 控件行为。另有 960×600、明暗主题、125/150% 缩放的七页布局矩阵；这些检查使用 mock，不验证真实音频输出。

配置专项覆盖快捷键、父子禁用、确认初焦点/外点/Esc、原生表单提交、保存慢响应/失败/重试和周格连续拖选。配置布局矩阵覆盖设置、Beets 三子页、收听、频道工作台与两种登录，使用同样的 960×600 明暗放大场景；同时检查容器边界、gutter 实际值和周格刻度缩放。

截图覆盖曲库、设置、频道工作台、播放页 × 明暗主题 × 960×600 / 1280×800 / 1440×900。`ui/test-results/ui-baseline/` 保存页面、队列/时段截图和 metrics.json，包括 viewport/DPR、token 与实际计算样式。这是当前界面取证，尚未建立自动截图差异阈值，也不等于页面设计已通过验收。

样式盘点输出 `ui/test-results/ui-style-inventory.json`，包含全部 kit 的 Bits 引用和八类 CSS 声明位置。字面量只是复核线索，包含合理几何值；行内与动态样式另行审查。测试产物不入库。

组件与频谱的分批范围、基线结论和后续验收见 [客户端 UI/UX 计划](../docs/77-client-ui-ux-and-spectrum-plan-2026-11-18.md)。

频谱使用同一 WebGL 舞台组合环境光、当前背景与播放页前景。背景为山形、极光
绸幕、漫射光帘；前景为轨道光环、声纹丝带、韵律晶格、回声轮廓、游弦单线、
双岸声场。播放页只登记 DOM 区域，由舞台裁切绘制，共用单个 canvas 与调度订阅。
山形光效只在选择山形时显示；无硬件 WebGL、关闭效果或 reduced motion 时背景
使用 CSS 静态渐变，前景停止绘制。前后景各自申请源引用，独立开关与释放。

源码预览可用 `bg=ridge|aurora|curtain|off`、`fg=orbit|ribbon|lattice|contour|string|shore|off`、`ambient=aurora|ember|meadow|dusk|mono`
与 `theme=dark|light` 对照；`track=1` 装载一首暂停曲目，点击播放观察 mock 频谱。
例如 [极光与游弦播放页](http://localhost:1420/?backend=mock&page=nowPlaying&bg=aurora&fg=string)。
这些参数只作用于开发 mock，不增加生产设置或音频分析链。

背景专项包含数值平滑、真实 GL 像素回读、暂停与断流、20 次切换释放、上下文恢复、
静态降级与 30 张配色截图；background-perf 为每种背景记录 30 秒 CPU 提交与帧间隔，
输出在指定输出目录的各用例目录，不能据此推断其他 GPU 或真实宿主表现。

保留独立频谱取证可以运行：

```powershell
npm run test:e2e:dev -- --project=regression e2e/dev/background.spec.ts e2e/dev/background-perf.spec.ts --output test-results/background-verification
npm run test:e2e:dev -- --project=regression e2e/dev/foreground.spec.ts e2e/dev/foreground-perf.spec.ts --output test-results/foreground-verification
```

前景专项包含六 shader 像素回读、暂停/断流/静音、切换释放、前后景独立开关、
上下文恢复、静态降级、DPR 1/2 与 CSS 缩放裁切，以及 48 张组合明暗截图。
foreground-perf 对六前景各跑 30 秒，三背景各占 10 秒，记录 CPU 提交、帧间隔、
慢帧与资源释放；测量期间保持源码稳定，禁止 HMR 或并发 GPU 测试。

Playwright 每次运行会清理对应输出目录；独立目录用于保留截图与测量，避免被
日常单项回归覆盖。


播放页对比度与明暗五配色截图：

```powershell
npm run test:e2e:dev -- --project=regression e2e/dev/contrast.spec.ts --output test-results/contrast-verification
```

动态矩阵使用高能量分带检查 180 个合成场景，数值对比度覆盖播放页元信息，歌词字形阴影通过截图检查；代表页正文与焦点使用静态面检查。
这些是实际采样证据，未覆盖全部动画时刻。各标准宿主的 Playwright 输出分别落在
production-regression / http-regression / firefox-regression，避免清理其他专项产物。
