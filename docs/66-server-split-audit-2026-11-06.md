# 66 · server 拆分回审（2026-11-06）

- 类型：审计（W1/W2 落地后的分支隔离与迁移计划复核，含桌面版运行确认）
- 状态：closed（发现项已逐条定界：2 项转计划、3 项记录为已接受取舍）
- 关联：docs/65（方案与 W0–W6 批次，§11 为落地记录）；docs/57（分支方案原拍板）；
  docs/20（远程容器调研）；docs/64（i18n 批，其收尾随本审计一并完成）

## 0 · 审计问题

最初计划是"web 分支通过解除 tauri 壳与音频驱动，使大部分代码直接迁移"。
本审计回答三件事：

1. 迁移计划是否兑现——多少代码真的"直接迁移"了；
2. **server 的 IPC 与代码是否不进桌面版**；
3. **桌面版运行是否正常**。

## 1 · 分支隔离实证（编译图与前端 bundle）

| 断言 | 证据 | 结果 |
|---|---|---|
| 桌面编译图不含 aiya-server | `cargo tree -p aiya --edges normal,build` 无 aiya-server 节点 | ✅ 0 |
| server 编译图不含音频侧与 tauri | `cargo tree -p aiya-server` 无 aiya-audio / tauri / src-tauri 节点 | ✅ 0 |
| server 编译图 Linux 可构建 | W0 起持续以 rust:1 容器 cargo check 守护 | ✅ |
| 命令面单源不动 | 桌面 `generate_handler` 仍 95 条；ipc_contract / dto_wire 测试随 verify 全过 | ✅ |
| 前端 http 通道不进桌面主链 | `http-*.js` 仅以动态 `import()` 出现（vite 分包），静态引用 0 处；桌面 webview 走 `__TAURI_INTERNALS__` 分支，chunk 永不加载（与 mock 同机制） | ✅ |
| 共享面收敛 | DTO 单源 = `aiya-dto`（桌面与 server 同一份）；跨壳差异只存在于 transport 层与壳层 setup | ✅ |

**结论：分支形态成立且被机器证据钉死。**"解除 tauri 壳与音频驱动"落在两处
编译期边界：`aiya-server` 的 Cargo 依赖面（无音频侧）与前端 transport 的动态
分包；两边都不存在运行时 if。

## 2 · 迁移计划兑现度（对照 docs/65 §6.4 矩阵）

| 计划桶 | 实际 | 兑现度 |
|---|---|---|
| A 原样/轻编排（34） | 35 条直调共享 crate（`playlist_refresh` 实测为纯 `aiya-data` 调用，从 B 上调） | ✅ 超预期 |
| B 编排下沉（49） | player 18 + report 通道已落（Actor 服务端化闭环）；beets 长任务/扫描管线/事件泵 = W3 既定范围 | 部分（按批次计划推进中） |
| C 桌面专属（12） | 未进 server 路由（tests/contract.rs 的 desktop_only 断言守卫） | ✅ 边界干净 |
| 新增 | `player_report` 回报通道——浏览器作为引擎后，桌面壳"引擎事件泵"语义的最小等价物；这是计划文本里没有、落地时必需的一件 | 计划外必要件，1 条命令 |

"大部分代码直接迁移"的量化：命令面 95 条中 53 条已路由，其中 35 条是零逻辑
搬运（handler 直调 crate）；`aiya-data` / `aiya-player` / `aiya-dto` /
`aiya-beets-store` 四个 crate 原样进 server 编译图，改动为零。

## 3 · 桌面版运行确认（真机）

- `npm run dev`（tauri dev，真窗口）启动成功：AUMID/身份设置 OK、**音频
  sidecar 首次握手 1150ms**、队列恢复（hydrate）1 首、定时刷新 1381 首无错。
- `probe:ui`（CDP 真窗口探针）：**console error/warning 无、4xx/5xx 无（favicon
  404 为 dev server 既有现象）、未捕获异常无**；`--nav` 导航链点击正常。
  个别 MISSING 为探针脚本对现侧栏布局的过时预期（探针自身漂移，非应用错误）。
- i18n 收尾（本审计随批完成）：`cargo fmt --all` 清掉 aiya-audio 两文件手排
  残留；3 条 style 阻断注释（lib.rs / App.svelte / settings.ts 的 docs/64 引用
  与日期戳）改写；`check:fmt` ✅、`check:style:strict` 阻断归零（总命中 318
  = L2 延后 229 + report 89，维持既有口径）。

## 4 · 发现项与去向

1. **server 缺"语言首启"写库**（转 W3）：桌面壳首启按系统 UI 语言探测写
   `settings.language`（docs/64 的宿主侧职责），server 未实现——web 首访
   `language` 为空，UI 走 zh 兜底。容器形态应从 `Accept-Language` 头推导。
   归 W3（server 启动面既有工作）。
2. **`app_set_settings` 无 diff 副作用**（转 W3）：库内 roots 变更不触发重扫
   （server 扫描目前只在启动时增量跑）。与事件泵下沉同一批处理（docs/65 §11.5
   已登记）。
3. **前端播放条进度显示 0:00**（已接受取舍）：恢复装载 800ms 时 UI 首帧按
   秒取整显示 0:00，实际元素已 seek 至 0.8s。桌面同口径（秒级显示），不改。
4. **探针 --nav 脚本漂移**（已接受取舍）：probe 对侧栏"正在播放"入口的预期
   过时。归探针脚本自身维护，不阻塞。
5. **verify 两处 ✗ 已随本审计清零**：fmt 与 style 的 i18n 欠账修完后，
   `check:fmt` / `check:style:strict` 双绿；其余步（e2e 27 过、clippy、
   gen:types、beets 9 冒烟）见 docs/65 §11.4 记分卡。

## 5 · 结论

- 分支隔离：**成立**，且已有编译图 + bundle 分包 + 契约测试三层机器证据，
  后续批次靠 `tests/contract.rs` 与 Linux 容器检查持续守卫。
- 迁移计划：**兑现**，A 桶全额直迁，player 域以回报通道闭环，B 桶余量与
  C 桶按矩阵推进/围栏，无一处需要"让浏览器假装有 Windows 能力"。
- 桌面版：**运行正常**（真机启动、音频 sidecar、队列恢复、1381 首刷新、
  UI 探针全绿）。

## 6 · W3 落地补记（2026-11-06，B 桶收口）

### 6.1 编排下沉的最终形态

docs/57 §2.2 计划的 `aiya-host-core`（traits: EventSink/DialogEnv/EngineSink）
以收敛后的形状落地：**`BeetsHost` 单 trait**（emit / data_root /
library_roots / online_files / track_ids_by_paths / setting_str /
install_registry / try_set_installing / enrich_store / kick_library_refresh），
桌面壳与 server 壳各自实现。下沉内容：

- `beets.rs`（3181 行）+ `enrichment.rs`（620 行）+ `proc.rs` 迁入
  `crates/aiya-host-core`，tauri 触点（58+13 处）全部改走 trait；
- 会话机器本就以 `Arc<dyn EventSink>` 为出口（client crate 既有抽象），
  `Arc<dyn BeetsHost>` 经适配器直接充当 sink；
- 桌面壳 73 处调用点改传 `beets_host(&app)`；`AppState` 的安装登记两字段
  换成 `Arc<InstallRegistry>`（RunEvent::Exit 收尸语义不变）；
- server 壳 `ServerBeetsHost`：emit → SSE 总线、kick → 扫描管线。

### 6.2 server 侧新增

- **beets 域 22 条路由**接通（status/preview/apply/cancel/config 三件套/
  install/plugins/plugin_table/uninstall/adopt/update/duplicates/move/
  query/fields + enrich 三件），未安装时的 `found:false` 三态与桌面一致；
- **扫描管线**：library_scan（同步返回 ScanResult）/scan_cancel/roots_set/
  refresh_state + 定时器线程（refreshSeconds 口径）+ scan:progress 事件流
  （实测 walking→parsing→done 三帧）；
- **事件总线**：SSE 从心跳占位升级为真通道（6 通道生产者）+ 重连快照首推；
- **播放记账**：Ended → history 落行（秒数 = 最近位置回报）、位置回报 5s
  节流触发队列持久化（桌面泵同口径）；
- **语言首启**：Accept-Language 推导写库（实测 en 写入并持久化）。

### 6.3 门禁与实机

- 双壳 cargo 全绿 + clippy 0 + style 阻断 0 + 契约测试（denylist 随 W3 收口
  更新：beets/扫描域从桌面专属清单移入已路由）；
- 桌面 probe 全绿（beets 编排重构后的挂载期回归确认）；
- **e2e 44/44**——W2 期探测 404 挂掉的 17 条一并修复：探测机制退场，改为
  server 注入 `__AIYA_BOOT__` 引导标志（首页注入式下发，e2e/preview 无标志
  自然走 mock，运行期错误收集器零噪音）。

## 7 · W5 落地补记（2026-11-06，Docker 化——beets/liquidsoap 层按拍板暂缺）

- **多阶段 Dockerfile**：node:24-slim 构建前端（npmmirror）→
  rust:1-**bookworm** 构建 server（rsproxy sparse + BuildKit registry 缓存）→
  debian:bookworm-slim 运行时（aliyun apt，仅 ca-certificates + curl）。
  两个坑已钉：① workspace 成员清单解析要求 `[lib]` 路径文件存在——
  src-tauri 源码进构建上下文但不编译；② builder 与 runtime 必须**同
  glibc 线**（rust:1 默认是 trixie，对 bookworm runtime 报 GLIBC_2.39
  weak version 缺失）——builder 钉 rust:1-bookworm。
- **验证**（test-music 挂载 + aiya-data 命名卷）：容器内 bootstrap 扫描、
  `library_stats` 3 首、`/audio/{id}` Range 206（audio/wav）、beets 三态
  `found:false`、`/` 注入 `__aiya_boot.js`（浏览器 http transport 起动）、
  **数据卷跨容器持久**（重建容器 imported=0）、RSS 6.5 MiB。
- **compose.yaml**：`/music:ro` + `aiya-data` 卷 + 8787 端口 + 可选
  `AIYA_TOKEN`；README 增部署节。
- beets（Linux 套件包）与 liquidsoap 层按拍板暂缺：镜像内 beets 三态为
  "未安装"，UI 正常引导；接入时按 §6.1 的套件包路线叠加运行时层。

## 8 · 登录门（W5.5，线上部署前置）与 W6 数据原则

### 8.1 单管理员登录门

线上部署的安全模型拍板：**无多租户，单管理员**。静态令牌（`AIYA_TOKEN`）
升级为正经登录门，双凭据并行：

| 凭据 | 来源 | 面向 |
|---|---|---|
| 管理员口令 | `AIYA_ADMIN_PASSWORD` | 浏览器（登录闸 → 会话令牌） |
| 静态令牌 | `AIYA_TOKEN` | 脚本/curl（Bearer 直用） |

- **会话**：内存 HashMap + 7 天滑动过期（进程重启全体失效，浏览器重新进闸）；
  签发 32 字节随机令牌；口令核验恒时比较 + 失败固定延时回落。
- **端点**（公开）：`POST /auth/login`、`GET /auth/state`（UI 预检）、
  `POST /auth/logout`。鉴权中间件按路径分层：静态壳与 auth 端点公开，
  `/ipc`、`/events`、`/audio`、`/artwork`、`/stream`、`/hls` 凭据放行。
- **前端登录闸**：预挂载阻断（`/auth/state` 判定 → 闸渲染，应用零泄露）+
  运行期 401 拦截重登（会话过期出闸 → 重登 → 重载）；会话令牌存
  localStorage，`<audio>`/EventSource 经 `?token=` 携带。
- **实测**：curl 矩阵 7/7（state/401/错口令/签发/带会话/state 变真/撤销）；
  浏览器预挂载闸（应用未挂载即出闸）→ 错口令提示 → 登录 → localStorage
  令牌 → 受保护播放（playing/826ms 推进）；容器镜像同码复验（401 → 登录 →
  Range 206 → 静态面 200）。
- 门未开（两个变量都未设）= 局域网无鉴权模式，行为与既有部署完全一致
  （回归路径保持）。

### 8.2 W6 数据原则（拍板记录）

**DB schema 不做客户端分支**：桌面版与 server 版共用同一份 SQLite schema，
不为 web 形态建变体；桌面专属列（如用户域字段）在非桌面客户端下闲置在表里
——保留无碍，删除反而制造迁移面与分叉。原则落点：W6 收尾时任何"web 版要不要
瘦身表结构"的提议按此驳回。

### 8.3 登录闸视觉归位（补充）

首版登录闸是 transport 内的 plain-DOM 覆盖层（自带内联样式），与主应用
观感脱节。已重构为 **Svelte 原生视图**（`components/LoginView.svelte`）：
transport 收到 401 → 派发 `aiya:gate` 窗口事件 → App 条件渲染 LoginView
（事件驱动而非 store——lib 层不得反向依赖 stores，dep-graph 口径）。背景
复用 `AmbientBackdrop` 的固定渐变层，卡片按 `.k-dlg` 的 token 口径
（surface-3 / line-strong / 14px / shadow-5 / blur-modal）复刻；文案入
i18n shell 域五键（zh/en 奇偶）。登录成功整页重载，引导以新会话重建。

## 9 · 频谱复刻提前落地（W4 项提前，2026-11-06）

web 端频谱已可用——W4 计划项里的 AnalyserSpectrumSource 部分提前落地：

- **数据路径**：`<audio>` → `MediaElementAudioSourceNode` → `AnalyserNode`
  （fftSize 2048 / min -80dB / max -20dB / smoothing 0.55）→ 48 对数分带
  （公式对齐 `aiya-audio/spectrum.rs::compute_bands`：20Hz–20kHz、
  f_i = 20·1000^(i/48)、带内取峰）→ `spectrum:frame` 订阅回调（普通
  number[]，Remote 源逐帧归一化）→ 既有五形态渲染器 + 背景山形层，UI 零改动。
- **装配点**：http transport 的 `spectrum:frame` 订阅（引用计数启停，33ms
  推帧；暂停不推帧，分带冻结与桌面 tap 关闭同观感）。MediaElementSource
  对同一元素只能建一次，装配幂等；探测失败（Safari 静音前科）静默降级为
  无频谱，不影响出声。
- **实测坑**：autoplay 策略下新建的 AudioContext 是 suspended——音频经
  graph 出声，挂起 = 静音冻结（进度停在 0:00）。装配点总在用户手势后，
  立即 `ctx.resume()` + playing 态兜底恢复。
- **实测**：实播中背景山形随 880Hz 测试音隆起（截图），进度正常推进；
  e2e 回归见后台记录。EQ/ReplayGain 的 WebAudio 复刻仍留 W4。

## 10 · W6 前端收尾：后端能力面与桌面专属段落的隐身（2026-11-06）

共用 UI 按"宿主能力"收窄入口，而不是让 web 用户对着死开关：

- **能力 API**：`lib/backend-caps.ts`（eq / output / preventSleep / shell 四面；
  判据 = `__AIYA_WEB__`，与 boot 三态同源，tauri/mock 恒为桌面能力——e2e 的
  桌面形态断言不受影响）。设置字段照常落库（§8.2 原则），只是不渲染。
- **设置页隐身清单**（web 下不渲染）：播放组——输出模式 / 缓冲 / 时钟 /
  暂停释放（WASAPI 与引擎专属），整组换成一条静态说明「由浏览器解码播放」；
  防休眠（SetThreadExecutionState 桌面专属）。系统组——禁用系统缩放（webview
  补偿）、关闭到托盘（浏览器无窗口壳）。**保留且核实有效**：自动回放 / 记忆
  位置（服务端 restore）、音量归一（增益在 Actor 折进音量，浏览器引擎只收
  音量值）、自动刷新 / 历史保留（服务端定时器与清理）、外观全组（频谱为 §9
  的 AnalyserNode 复刻）、快捷键与语言主题。
- **EQ 入口**：播放条 EQ 触发键整键隐去——抽屉唯一入口即此键，键不渲染抽屉
  即不可达（无快捷键入口）。EQ 的 WebAudio 复刻仍是 W4 可选项。
- **登录门桌面不触发**：唯一派发点是 http 传输的 401 处理（动态 chunk，桌面
  永不加载）；LoginView 再改为 `{#await import()}` 动态分包，桌面构建里该
  chunk 不进主包也不加载，"桌面没有门禁实现"由装载路径钉死。顺带把 W5 建好
  却一直没接线的 `/auth/state` 预检接上：main.ts 在 mount 前探测，门开启时
  登录卡首帧即渲染（此前主界面先闪一帧、等 401 才切闸）。实测踩中并修掉一个
  预检缺陷——探测请求必须携带已存令牌，否则持有效会话刷新会被误拦回登录卡。
- **门禁**：svelte-check 0 错 / e2e 44/44（mock=桌面能力，形态不变）/
  style:strict 阻断 0 / web 实机四态核验：无令牌→登录卡首帧渲染（主界面
  不闪现）、口令进闸→应用、有效会话刷新→直进应用、无口令实例→无闸直进；
  登录后设置页隐身清单生效、播放条无 EQ 键。

## 11 · W6 收尾二：真机矩阵、http e2e、多根种子（2026-11-06）

§10 之后把 W6 清单剩余项做完：

- **window-chrome 裁剪**：顶栏窗控三键此前在 web 下是 disabled 摆着，改为
  `isNative` 分支整组不渲染（与 `backend-caps.shell` 同源的壳能力判定）。
- **浏览器解码灰标**：`lib/browser-codecs.ts`——`canPlayType` 按容器名探测
  （lofty FileType Debug 串），未登记容器与未知值一律按可播，不给未知格式
  判死刑；桌面 `isNative` 短路恒真，灰标随宿主形态自动消失。数据面给
  `TrackRow` 加 `container` 列（扫描即写、不依赖 beets；codec 列仍是 beets
  同步的领地，刻意不动）。已知盲区：ALAC 藏在 Mp4 容器里与 AAC 无从区分，
  列表层标不了灰，点播时由浏览器解码失败走播放条报错兜底。
- **web 添加目录**：曲库卡在 web 宿主没有系统选择器，换绝对路径手输入
  （`library.saveRoots` 追加 + 立即索引），与桌面选目录追加同语义。
- **多根种子语义（拍板对齐）**：`AIYA_MUSIC_DIR` 降级为**首启种子**——仅
  `library.roots` 键不存在（全新库）时写入；此后容器重启不回填，应用内
  删掉的根不会复活。曲库根的权威一直在设置里（应用内配置），宿主把任何
  目录挂进容器后在应用里配路径——常规容器应用口径。无种子且全新库 =
  零根启动。
- **http 形态 e2e**（`npm run test:e2e:http`）：`tools/http-e2e-server.mjs`
  拉起真 aiya-server（cargo 增量构建 + fixtures 临时曲库 + 口令 8799），
  5 用例全绿：登录闸首帧（主界面不闪现）/ 种子根入库 3 首 + AIFF 行灰标
  恰 1 / `<audio>` 真装载走表（/src=/audio/，hidden append 进 DOM 才可
  观测）/ 设置页 web 形态（隐身清单 + webDecode + 路径手输）/ 会话刷新
  直进 + English 切换。mock 套件主配置 `testIgnore` 掉 http 目录，两套
  互不串。
- **Firefox 真机矩阵**（`npm run test:e2e:firefox`）：mock 套件 44/44 全绿
  （bundled Firefox 首跑，3.3 分钟）。Safari 需要 macOS/iOS 设备，Windows
  本机无法回归，清单上保持未测。
- **实测坑**：headless Edge 的 `Accept-Language` 是 en-US，服务端首启写库
  后整库 UI 是英文——e2e 的文案断言要么双语兜底要么先点语言钮固定语向；
  语言切换控件是 RadioGroup（role=radio），不是 button。`new Audio()` 是
  游离元素，querySelector 摸不到——引擎构造时 hidden append 进 DOM。

## 12 · 全量审查与修复批（三路子代理，2026-11-06）

web 分支增量（基线 10f6000，含工作区）由三路子代理并行全量审查（后端 web 面 /
前端 / 部署与数据面），发现 2 项 P0、6 项 P1、约 10 项 P2，全部修复（beets
venv 与 liquidsoap 继续延期除外）：

- **扫描标志卡死悬案破案（P0，已复现条件明确）**：`run_scan` 的两个早退分支
  （DB 读失败 / **根集为空**）只 finish 不复位 `scanning`——无根 server 的
  第一个 timer tick 即把标志永久卡死（打一条 scanned=0 日志后全部扫描静默
  跳过、`library_scan` 恒 busy），与此前"发生一次、三次复现失败"的现场逐项
  吻合（复现失败是因为三次都带了种子根，走了会复位的全路径）。修复：桌面壳
  refresh.rs 的 FlagGuard 同型 **ScanFlagGuard**（构造置位 / Drop 复位），
  spawn_scan 与 run_scan_inline 共用，早退/panic/spawn 失败全路径兜底；
  run_scan_inline 的 busy 闸收归守卫自持。
- **`app_set_settings` 漏路由（P0）**：dispatch 有实现、ROUTED 缺席——web
  形态每次改设置 404 且永不落库。补路由之外，contract.rs 增**反向契约闸**
  （UI 调用集 − transport 内部 − 桌面专属 ⊆ ROUTED），首跑即再抓到两条漏网
  （`playlist_export`/`playlist_import`，需文件选择器/上传下载 API，登记桌面
  专属待 W4+）。
- **P1 批**：web 播放失败零反馈（audio error 只 emit error 字段、被 10Hz
  快照冲掉、`failed` 回报从不发出 → server Actor 死分支）——error 监听改为
  落 `status:'error'` + 上报 failed，与桌面引擎归约同径；两壳 `library_tracks`
  手拼 SELECT 漏 `t.genre`（row_to_track 的 genre 位吃到 codec 的哑雷，
  TrackDetail 不搬 genre 故暂无 wire 影响）——补列并把音频规格索引顺移
  19..=24，列序契约写进两侧注释；登录失败延时移出同步路径（std sleep 睡死
  tokio worker → handler 内 tokio sleep）；`CorsLayer::permissive` 撤销，
  默认不挂 CORS（同源不需要），跨域走 `AIYA_CORS_ORIGIN` 白名单；
  `/stream` 加固（scheme 白名单 + 环回/私网/CGNAT/组播目标拒绝 + 段读取
  64MB 上限 + 清单判定按路径）；会话令牌换 `getrandom`（OS 熵）；镜像 ENV
  摘除 `AIYA_MUSIC_DIR`（种子语义归运行者，compose 删行即零根；空串 env
  不再写 `[""]` 坏根）。
- **P2 批**：SSE 改单条 EventSource 多路复用（HTTP/1.1 每主机 6 连接上限，
  `<audio>` Range 长连接再占一条）+ 转发任务 `tx.closed()` 断连退出；position
  回报带 token、服务端与 Loaded/Ended 同口径丢弃迟到帧（多标签互相污染）；
  `/auth/state` 增 `mode`（password/token），登录卡据此切换凭据语义——只发
  静态令牌的部署此前是死路；gate 态跳过整条启动链（登录卡背后不再 1Hz 401
  噪音）；预检加 4s 超时；整理库 OrganizePanel 的文件选择器动作套 `isNative`
  门控（web 上曾是死按钮）；browser-codecs 判据统一到 `__AIYA_WEB__`（mock/
  预览保持桌面能力口径）并补 Ape/WavPack/Mpc/Speex 四个已知不可解容器键；
  删两处死代码（detectHttpBackend、引擎内 updateMediaSession——媒体会话全权
  归 App 层）。
- **live_open / player_open_paths 未路由落账（W4 已知断点）**：web Live 页
  点外部流"连接"会 404（路由面无此二命令，浏览器引擎 stream 播放也未建），
  此前 docs/66 漏记这笔降级——现明确为 W4 直播线遗留（与推流同批）。
- 结构文档对账：AGENTS 树/依赖图/边界句、crates/README 补 `aiya-host-core`
  行并重写"只有一条库间依赖"的失真陈述（现依赖网逐条按 Cargo.toml 核实）；
  docs/65 头部状态改 closed；README 修 crate 计数与 beets 三态位置。

- **验证期追加**：`app_set_settings` 补路由后揭出返回值形状差——桌面壳返回
  `Ok(settings)`（client 成功分支 `mergeSettings(响应)`），server 一直返回
  `null`；此前被 404 掩盖（走 catch 保住本地值），路由修好后语言切换被
  mergeSettings(null) 顶回默认。server 改为返回落库后的设置，与桌面同形。
  教训：命令面的响应形状也是契约（dto_wire 只锁 DTO 结构，不锁"命令返回
  什么"），跨壳对账要连同返回值一起比。
