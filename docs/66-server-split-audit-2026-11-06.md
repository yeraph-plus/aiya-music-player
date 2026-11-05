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
