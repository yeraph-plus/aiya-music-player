# AGENTS.md — AIYA Music

本文件是本项目协作约定的单一真源，面向在本仓库工作的开发者与 AI 代理。
分五节：结构（§1）、开发操作（§2）、约束（§3）、注释与文档文风（§4）、
文档制度（§5）。修改任何一条约束时，同批更新本文件并在对应编号文档记录来由
（流程见 §5.3）。注释不携带指向 docs 的路标；"为什么这样写/这个坑当年
怎么回事"按 §4.5 路由表反向查询。

项目定位：Tauri 2 本地音乐播放器，Windows 优先。Rust 宿主 + 原生音频 sidecar
进程 + Svelte 5 前端；beets 集成为可选增强。剩余项以
[docs/35-closing-ledger-2026-10-27.md](docs/35-closing-ledger-2026-10-27.md)
为唯一真源，进行中的批次以 docs/ 下编号最新的文档为准。

## 1. 项目结构与依赖边界

### 1.1 目录（实际）

```
AIYA_MUSIC_TAURI/
├── AGENTS.md              # 本文件：结构、操作、约束、文风、文档制度
├── docs/                  # 编号记录：调研/计划/审计/账本；ledger/ 为架构域流水账，制度见 §5
├── crates/
│   ├── aiya-player/       # 纯领域：队列、状态归约、命令/事件；不依赖 tauri/rusqlite/windows
│   ├── aiya-data/         # SQLite v1、Repository、扫描、封面缓存、设置；不依赖 tauri
│   ├── aiya-dto/          # IPC DTO 与拖放 DTO 的共享契约源（ts-rs 导出）；桌面壳与 server 共用
│   ├── aiya-host-core/    # 宿主编排共享层：BeetsHost trait + beets/enrichment/proc（桌面壳与 server 共用）
│   ├── aiya-audio/        # 原生引擎：Symphonia 解码 + wasapi-rs 输出 + DSP/频谱/EQ（桌面独占）
│   ├── aiya-audio-host/   # 音频 sidecar 进程入口（命名管道行 JSON，不传 PCM；桌面独占）
│   ├── aiya-beets-client/ # beets 协议客户端：发现/起进程/说协议；零环境知识
│   ├── aiya-beets-store/  # beets 索引增强的派生数据：独立 SQLite 文件，不认识应用曲库
│   └── aiya-server/       # Web 版服务端壳（docs/65）：axum 命令路由 + Range 流式 + 静态托管
├── beets-tool/            # beets 侧全部能力（独立发布单元，Python 包 aiya-beets）
├── src-tauri/             # Tauri 宿主：命令、事件桥、协议注册、窗口、日志
├── ui/                    # Svelte 5 + Vite：views / stores / 组件分 content·shell·ui
├── protocol/              # beets 协议 v2 schema 的本仓镜像（字节副本 + sha256）
├── tools/                 # 类型生成、依赖图、CDP 探针、notice 生成、门禁脚本、打包辅助、run.mjs
├── contracts/             # 契约说明 README
├── scripts/               # 图标生成等辅助脚本
└── licenses/              # 随包分发的许可材料（npm run notices 生成部分）
```

各 crate 的细分说明见 [crates/README.md](crates/README.md)；`ui/`、`src-tauri/`
各有子说明 README。

### 1.2 依赖方向（门禁强制）

```
ui ── lib/ipc/client.ts ──▶ src-tauri(aiya) ──┬─▶ aiya-data
                                             ├─▶ aiya-player ──▶ aiya-audio
                                             ├─▶ aiya-beets-store
                                             ├─▶ aiya-dto
                                             ├─▶ aiya-host-core（beets/enrichment 编排）
                                             └─▶ aiya-audio      ⇠ 同一 crate 也被 sidecar 用
浏览器 ── HTTP/SSE ──▶ aiya-server ──┬─▶ aiya-data / aiya-player / aiya-dto / aiya-beets-store
                                    ├─▶ aiya-host-core（与桌面壳共用同一份编排）
                                    └─▶ aiya-audio 不得进其编译图
src-tauri ⇠ 命名管道行 JSON ⇢ aiya-audio-host
src-tauri ──▶ aiya-beets-client ⇠ stdio NDJSON ⇢ aiya-beets（独立安装，可能根本没装）
```

- `aiya-player` 不依赖 `tauri`、`tokio`、`rusqlite`、`windows`；`aiya-data`
  不依赖 `tauri` 与音频平台；`aiya-beets-store` 不依赖应用曲库（`aiya-data`）；
  `aiya-dto` 不依赖 tauri 与音频平台（桌面壳与 server 共用的契约源）；
  `aiya-host-core` 不依赖 `tauri`/`aiya-audio`（编排层被两壳共用，宿主差异
  走 `BeetsHost` trait 注入）；`aiya-server` 不依赖 `aiya-audio`/`src-tauri`
  （音频侧不进 server 编译图，Linux 容器 cargo check 为证）。违反视为结构
  破坏，合并前必须修正。
- 核心链路 `aiya-audio` / `aiya-audio-host` / `aiya-player` / `aiya-data` 不依赖
  `aiya-beets-client`，"没装 beets 也能完整播放"是编译期事实。门禁：
  `npm run check:boundary`。
- UI store 的依赖方向与环检测：`npm run check:deps`。

## 2. 开发操作

### 2.1 环境

- Windows 10+；Rust stable（MSVC 工具链）；Node + npm。
- npm 脚本一律在**仓库根**执行；Rust 相关脚本经 `tools/run.mjs` 转发
  （把 `%USERPROFILE%\.cargo\bin` 前置进 PATH）。
- `beforeBuildCommand` 的 cwd 是调用 tauri CLI 时的目录：`tauri build` / `tauri dev`
  只在仓库根执行，不在 `ui/` 内。
- beets-tool 开发与套件包构建需要 Python 3.10+；播放器本身无此依赖。
- e2e 使用 mock 后端 + 系统 Edge，无需真实后端与声卡。

### 2.2 常用命令（仓库根执行）

| 命令 | 作用 |
|---|---|
| `npm run dev` | 开发：`tauri dev`（自动起 vite dev server） |
| `npm run dev:ui` | 只起前端（浏览器 mock 数据，不开 Tauri） |
| `npm run build` / `bundle` | 出安装包：`vite build` → 编 sidecar → release 编译 → NSIS（不含 beets） |
| `npm run portable` | 出便携版 zip：两个 exe + 许可 + `portable.txt` |
| `npm run beets:bundle` | 出 beets 单轨发布物 `beets-tool/dist/beets-portable-win-x64.zip` |
| `npm run build:ui` | 只构建前端到 `ui/dist` |
| `npm run check` | `svelte-check`（0 error / 0 warning 才算过） |
| `npm run test` | Rust 全量测试（1 条 `--ignored` 为 `bench:50k` 性能基准） |
| `npm run test:data` / `test:audio` / `test:player` / `test:app` | 单 crate 测试 |
| `npm run test:all` | 先编 sidecar → Rust 全量 + UI 类型检查 + 依赖/边界/分层/协议门禁 + UI 构建 |
| `npm run test:e2e` | Playwright（mock 后端 + 系统 Edge） |
| `npm run test:e2e:http` | Playwright × 真 aiya-server（8799，cargo 增量构建 + 临时曲库，登录闸/播放/隐身清单） |
| `npm run test:e2e:firefox` | 同 test:e2e 跑 Playwright bundled Firefox（真机矩阵；需先 `npx playwright install firefox`） |
| `npm run verify` | `test:all` + `test:e2e` + `check:fmt` + `check:clippy`（-D warnings）+ `gen:types:check` + `check:style:strict` + beets 九项冒烟（apply/maint/proto/config/catalog/bins/uninstall/lock/enrich） |
| `npm run gen:types` / `gen:types:check` | 从 Rust 生成 TS 类型 / 只校验不写盘 |
| `npm run smoke:engine` / `smoke:offset` / `smoke:switch` / `smoke:rate` / `smoke:mode` | 真机 WASAPI 出声冒烟（需要声卡） |
| `npm run smoke:supervisor` | sidecar 握手/心跳/看门狗冒烟（不需要声卡） |
| `npm run probe:clock` | 时钟诊断：独占下 `GetCurrentPadding` 与 `IAudioClock::GetPosition` 哪个可信、事件/轮询两种时钟的写入策略是否喂饱设备 |
| `npm run smoke:output` | 宿主↔sidecar 接缝冒烟：起真子进程 `aiya-audio-host`，v4 协议送 `SetOutputConfig`、读回 `audioMode` 事件（wire 形状与接缝行为，区别于直接驱动引擎的 `smoke:mode`） |
| `npm run smoke:beets` / `smoke:maint` | beets apply 五场景 / 维护四件套（隔离 `BEETSDIR`，不动真实库） |
| `npm run smoke:proto` / `smoke:config` / `smoke:catalog` / `smoke:bins` / `smoke:uninstall` / `smoke:lock` / `smoke:enrich` | beets 契约与能力冒烟（隔离 `BEETSDIR`） |
| `npm run bench:50k` | 5 万首查询性能基准（release，`--ignored`） |
| `npm run check:boundary` | 边界门禁：核心链路不依赖 beets 工具、client 无环境知识 |
| `npm run check:deps` | store 依赖方向/环检测 |
| `npm run check:style` / `check:style:strict` | 注释文风门禁（report-only / strict 阻断，见 §4.4） |
| `npm run check:ui-layers` | UI 分层门禁 |
| `npm run proto:check` / `proto:sync` | 协议门禁：镜像与工具侧真源字节一致 / 同步镜像 |
| `npm run notices` | 刷新 `licenses/CARGO-DEPS.md`（依赖变更后跑） |
| `npm run probe:ui` | UI 运行时探针：CDP 在真窗口读 DOM、抓 console.error 与未捕获异常（默认探结构，`--actions` 探交互；有未捕获异常或 console.error → 退出码 1）。目标先起其一：Tauri 真窗口 `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9222 npm run dev`，或浏览器预览 `msedge --headless=new --remote-debugging-port=9222 http://localhost:1420/`（dev:ui 起的 1420 端口） |
| `npm run probe:exclusive` | 独占格式探测：枚举默认输出设备**实际接受**的独占 (采样率, 位深) 组合——`open_exclusive` 的设计依据，也是"独占没生效"的排障工具 |

### 2.3 验证门禁

- 提交前基线：`npm run verify`（含 e2e、fmt、clippy -D warnings、类型生成校验、
  文风门禁、beets 冒烟）。
- 干净检出直接跑 Rust 测试会因缺 sidecar 二进制失败；`test:all` 已把编 sidecar
  纳入第一步，单独 `cargo test` 前先编 sidecar。
- svelte-check 要求 0 error / 0 warning。
- `///`、`//!` 注释参与 doctest：改动契约层注释时同样跑 `cargo test` 与
  `svelte-check`。

## 3. 约束（既定决策事实）

> 每条是现行约束事实，标注来源编号。**修改本节即修改约束**，须在批次文档中
> 记录来由（流程见 §5.3）。

- **C1 后端拥有音频输出**：解码、DSP、设备枚举、播放时钟全部由 Rust 音频进程
  负责；WebView 不创建 `<audio>` / `AudioContext` / `MediaElementSource`，不接触
  音频文件路径；UI 经 `AudioPort` 抽象与引擎解耦，不感知引擎实现。
- **C2 播放状态机唯一真源在 Rust**：`PlayerActor` 独占队列、随机、重复、位置、
  音量、速率；UI 经命令与快照事件交互，不判断"下一首"、不维护第二套队列。
  `PlayerState` 不含完整队列（`queue_slice` 分片拉取）；每个 `load` 带
  `generation`、事件带 `revision`/`seq`，过期与乱序事件丢弃；UI 仅保留 pending
  覆盖层（拖动 seek、输入中的音量），不拥有权威状态。系统媒体键、托盘、关窗
  控制同一个 Actor。
- **C3 音频独立 sidecar 进程，命名管道行 JSON**：只传命令/状态/错误/频谱 band，
  不传 PCM。`PROTOCOL_VERSION` 握手，不兼容拒绝启动、不静默降级；控制命令有界
  队列不丢，`Position`/`Spectrum` 可丢旧帧；2s 心跳、5s 超时 kill + restart；
  心跳由 sidecar 独立线程发出、不等引擎初始化完成（初始化慢不等于进程死）；
  sidecar 不打开 SQLite、不依赖 Tauri 类型。
- **C4 契约以 Rust 为源**：命令、事件、错误、DTO 定义在 Rust，`#[derive(TS)]`
  （ts-rs，经 `cargo test` 导出）由 `npm run gen:types` 直接写进
  `ui/src/lib/ipc/generated/`（`TS_RS_EXPORT_DIR` 指向该目录，桶文件 index.ts
  同脚本生成）；不手改生成物，`gen:types:check` 校验一致。UI 只走
  `lib/ipc/client.ts` 一层访问 IPC；参数侧复合对象收 `Json` 手动解析（否则参数
  不合法会绕开 `ApiError` 分类）。事件/命令用 tagged enum；sidecar 协议独立
  定义，不复用 Tauri 类型。
- **C5 自定义协议仅保留封面**：`aiya-artwork` 协议只接受 artworkId
  （`/artwork/{id}?size=96|256|512`），不接受任意文件路径；handler 只按 ID 查
  缓存，404 与错误响应不落盘；音频文件路径只存在于 host 与 sidecar 的 `Load`
  命令中。Windows 运行时 URL 形态为 `http://aiya-artwork.localhost/...`。
- **C6 原生音频引擎**：引擎 = Symphonia 解码 + `wasapi` crate 输出，经
  `aiya-audio-host` sidecar 交付。DSP 链固定为 `decode → [resample(共享)] →
  [ReplayGain] → [EQ] → tap → [volume/fade] → WASAPI`，tap 在 EQ 之后、主音量
  之前。tap 为订阅制必要采样复制（SPSC 无锁环，分析线程 30Hz：FFT → 48 对数
  分带 `u8[48]` 事件），仅在 UI 开启频谱时生效，关闭时零拷贝零分配，不破坏
  bit-perfect。不采用 Rodio/CPAL（模型与自有 DSP 链、位置时钟、独占模式冲突）；
  mpv 仅作对照实验，不进发布包。
- **C7 单一数据库写入者**（docs/02）：只有宿主打开 SQLite；音频/beets sidecar
  不碰宿主库。
- **C8 数据库无迁移链**（docs/02）：schema 自 v1 起，不提供旧库迁移路径。
- **C9 beets 是增强能力，不是播放依赖**（docs/33）：协议 v2 已冻结，真源在
  `beets-tool/protocol/v2.schema.json`，本仓镜像 `protocol/aiya-beets-v2.json`
  （字节副本 + sha256）。守线各管一面：`smoke:proto`（工具输出合 schema）、
  `cargo test -p aiya-beets-client`（serde 形状对齐镜像）、`proto:check`（镜像
  未被单方面修改；改镜像只能经 `proto:sync`）、运行时 `schemaHash` 比对
  （宿主与工具必须是同一版契约字节，见 `beets.rs`）。beets 为写权威，
  `aiya-beets-store` 保存派生数据（读模型），不用 beets 查询语法替代内置索引
  （docs/29）。
- **C10 分发形态**（docs/33/35/53/54/58）：安装包（NSIS，当前用户级）不含
  beets；beets 单轨分发——免安装套件包（自带 embeddable 解释器与外置工具，
  整目录复制可用）。播放器侧安装流程：下载 → 展开 → 记入口 → 跑一次 `status`
  验证；**成功判据是探测到的 `found`，不是退出码**。下载物按 zip 容器形状校验
  （GitHub 未发布资产时返回 200 的 HTML 错误页）。不做代码签名，SmartScreen
  提示为预期，以便携版 zip 作为补偿轨道。
- **C11 数据位置唯一真源**（`src-tauri/src/paths.rs`）：便携判定只看 exe 同目录
  有无 `portable.txt`（不探测目录可写性）；命中则数据根为 `<exe目录>/data`，
  否则 `%APPDATA%\dev.aiya.music`。全仓只有 `paths.rs` 回答"数据放哪"。
- **C12 发布基址覆盖**（docs/33）：仅经 `AIYA_BEETS_RELEASE_BASE` 环境变量覆盖，
  高于内置常量；不设 `beets.releaseBase` 配置键。

### 3.1 范围边界（现在不做什么）

- 前端不建第二条音频路径：WebView 不创建 Web Audio 节点，频谱只来自引擎链内
  tap（C1/C6）。
- 不做 SSR / 路由库 / URL 状态；不做主题编辑器；不做移动端适配（保证 960px
  以上不破）。
- 数据库无迁移链（C8）；自动更新后置到 v2。

## 4. 注释与文档文风

### 4.1 两个语域

| | 交付物文风 | 记忆文风 |
|---|---|---|
| 载体 | 代码注释、`///` 文档、README | docs/ 编号记录 |
| 回答 | 现在是什么、为什么这样设计、证据在哪 | 何时、何故、经何过程做出决定 |
| 时态 | 现在时陈述句 | 过去时允许，日期必须真实 |
| 历史叙事 | 禁止（归 git 历史与编号文档） | 允许且必要 |
| 第一/第二人称 | 禁止 | 第一人称允许，第二人称禁止 |
| 共同禁令 | 拟人比喻；口语词（人话、拍板、顺手、坑、翻车、挂了、搞定、抽风、收尸）；训诫式祈使 | 同左 |

判据：注释中出现日期、轮次、人称、旧实现对比、指向 docs 的引用，即违反
交付物语域；编号文档中把结论写成面向读者的禁令，即越界为训诫。

### 4.2 交付物注释两段法

按"是什么 → 为什么"组织，两段不必齐备：

1. **是什么**：该代码/契约的行为、单位、边界、不变量（现在时）。
2. **为什么**：当前约束来源——性能、正确性、平台行为、本文件 §3 的某条
   约束编号（`C1`–`C12` 为现行约束编号，可注）。

历史叙事、事故链、迭代轮次一律不进注释：档案已有全文的直接删——git 历史
是修订承接位（blame/log 可考），不必逐字搬运；档案没有的先归档再删，归档
去向见 §4.5 路由表。**注释内禁止指向 docs 的引用**（`docs/NN`、`§`、`ADR-`、
`notes/`、`AGENTS.md`，任何注释形态含 `///`、`//!`、块注释）；字符串字面量
中的引用不受限（UI 文案）。证据只能指向仓内真实目标（测试名、schema、
实测数字），不造名。

规约型注释（文件头"如何扩展"段）允许中性祈使，但不用面向人的训诫与强调
修辞。

### 4.3 不算违例的情形

技术复合词（镜像、像素、识别、区别、别名）；规约段的中性要求句；字符串
字面量中的 UI 文案与引用。

### 4.4 自动检查

`tools/check-style.mjs` 接入 `verify` 与 CI（`check:style:strict`），扫描 Rust
（crates/、src-tauri/）、前端（ui/src/、ui/e2e/，排除生成物）、构建脚本
（tools/*.mjs）与样式/迁移注释（.css、.sql——注释形态块注释与 `--`）。日期戳（含月戳与 `YYYY 年`）与 docs 引用（`docs/NN`、
`notes/`、`ADR-`；裸 `§` 为 GPL 条款/源码定位等合法引用，不拦）在**任何注释
形态**（`//`、`///`、`//!`、块注释、JSDoc、HTML 注释）都是阻断，无延后桶。
存量经 `tools/check-style-allowlist.json` 豁免并按**棘轮**只减不增：分区
治理每清一处删对应豁免条目，烧完回归裸阻断，不新增豁免。语域 report =
高精度禁词（人话、拍板、顺手、翻车、搞定、抽风、收尸）与历史叙事信号词
（旧实现、此前、曾经、以前、旧版、事故），report-only 观察误报，不阻断。
宽泛词（像、别、坑、必须）误报率高，不进自动门禁，由批次人工处理。

beets-tool 的 Python 侧由 `tools/comment-gov/scan.py --check` 覆盖（npm
`check:style:py`）：`#` 注释与 docstring 同为注释面，日期/引用与 Rust/前端
同桶同白名单，已接入 `verify` 与 CI。同目录另含批次治理与验收工具
apply / verify / test_scan（`verify.py` 剥注释零差异终验，干净检出恒过）。

### 4.5 历史查询路由（注释不设路标）

注释里没有指向 docs 的路标；"这段为什么这样写 / 当年怎么回事"反向查本表，
先近后远：

| 要查什么 | 先查 | 再进 |
|---|---|---|
| 现行约束与边界（某条铁律的依据） | 本文件 §3（条目带来源编号） | 来源编号文档全文 |
| 一行式取舍/小修订的来龙去脉 | [docs/ledger/](docs/ledger/README.md) 主题索引（代码域 → 账本文件） | 条目内"相关档案"链接 |
| 复杂事故链、多轮实测过程 | 编号文档的踩坑章节（形态见 docs/50 §2.5、docs/67 §3） | 全文 |
| 执行流程、命令序列 | 本文件 §2.2 表 | — |
| 剩余项与未完成承诺 | [docs/35](docs/35-closing-ledger-2026-10-27.md) 收尾账本 | — |
| 当前批次做了什么 | docs/ 下编号最新的文档 | git log（每区一提交，提交信息带处置摘要） |

检索方向：域 ledger →（条目指向）编号文档 → git 历史；新增踩坑的归档
路由：复杂事故链进编号文档踩坑章节（该事故已有编号档则只补不重写），
一行式进域账本，执行流程进 §2.2（不是 ledger）。

## 5. 文档制度

### 5.1 docs/ 四型制

| 类型 | 职责 | 生命周期 |
|---|---|---|
| 调研 | 问题、选项、证据、结论 | 完成即 closed，结论由本文件约束/后续文档承接 |
| 计划 | 批次拆分、验收判据 | 批次收尾标 closed，剩余项转入账本 |
| 审计 | 体检、问题清单 | 发现项转入计划或账本后 closed |
| 账本 | "还剩什么"的唯一真源（docs/35） | 持续更新，不 closed |

每份编号文档头部固定 3 行元信息：`类型 / 状态 / 关联`（supersedes、superseded-by、
相关编号）。

### 5.2 编号与命名

- 命名 `NN-slug-YYYY-MM-DD.md`，日期为写作日；新编号取现存最大值 +1。
- 历史撞号登记（不重排，交叉引用以编号为键）：10 号两份并存
  （`10-full-code-review-2026-10-24.md` / `10-plan-alignment.md`）。
- 标题体例统一为 `# NN · 标题（YYYY-MM-DD）`。
- 会话级临时物（`.commitmsg-*.txt`、探针/自测目录等）不进 docs/、不入库。

### 5.3 架构决策的登记

决策过程与证据落在所在批次的编号文档；决策事实（现行约束）同批更新本文件
§3 并注明来源编号；推翻既有约束时直接修改 §3 对应条目，并在新编号文档记录
来由。需要登记的情形：跨进程/跨仓边界的契约决定；难以回退的决定（数据格式、
分发形态、签名与信任）；推翻既有约束；约束后续多个批次的架构选择。

### 5.4 相关文档

- [docs/ledger/](docs/ledger/README.md)：架构域流水账，历史叙事与设计取舍的
  唯一承接位（主题索引见其 README）
- [docs/35-closing-ledger-2026-10-27.md](docs/35-closing-ledger-2026-10-27.md)：
  发布前收尾账本（剩余项唯一真源）
