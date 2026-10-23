# AIYA Music — Tauri 2 重写

> 状态：**Phase 0 / 架构决策期**。本仓库先锁定边界与协议，再开始写产品代码。
> 基线：`../AIYA_MUSIC`（Electron 版，2026-10-23，`f99564e` + 未提交的频谱/倍速改动；27 个测试文件 / 255 用例全绿）。
> 本文所有行号引用均以该基线为准。

## 为什么要重写

现版的 UI 与领域模型是健康的，真正的问题是**音频所有权放错了位置**：

- 声音由渲染层 `<audio>` 出，播放状态机却主进程独占；窗口重载、关闭、Web Audio 建图、CORS、SMTC 都要靠补丁维持一致。
- Chromium 的音频栈做不了 WASAPI 独占、按源采样率开流、bit-perfect、设备级诊断；`--enable-exclusive-audio` 是进程级开关且失败静默回落。
- `MediaSession → SMTC` 在 Electron 里要靠特性开关与 AUMID 才成立；Tauri/WebView2 实测**不直通**，必须原生接管。
- 队列以 JSON 整行每 5 秒重写；5 万首时这是无谓的 WAL 与序列化开销。
- 音频问题（爆音、设备占用、独占失败、驱动差异）没有进程隔离与结构化诊断，排障只能靠猜。

因此新版的核心不是“把 Electron 换成 Tauri”，而是：

> **把音频、播放状态、曲库、文件与系统集成全部收进 Rust 后端；WebView 只负责显示与交互。**

## 五个已定决策

1. **Rust 拥有音频输出**：共享/独占 WASAPI、解码、DSP、设备与时钟都在 Rust；前端不再创建 `<audio>`、`AudioContext` 或 `MediaElementSource`。
2. **播放状态机在 Rust**：队列、随机排列、重复、恢复、播放历史都只有一份真相；前端是视图，不是扬声器。
3. **音频助手独立进程**：`aiya-audio` sidecar 通过命名管道/帧协议通信，崩溃可重启；宿主进程负责 UI、数据库与系统集成。
4. **契约以 Rust 为源**：Rust 类型生成 TypeScript，带协议版本与 schema 哈希；不再维护两份手写类型。
5. **单一数据库写入者**：只有宿主进程打开 SQLite；音频助手只接收 `trackId → path + format`，不碰库。

## 仓库结构（规划）

```
AIYA_MUSIC_TAURI/
├── docs/                  # 功能盘点、代码评审、目标架构、迁移计划、ADR
├── crates/
│   ├── aiya-core/         # 纯领域：队列、状态归约、命令/事件；无 IO/无 Tauri
│   ├── aiya-data/         # SQLite、迁移、Repository、扫描、任务
│   ├── aiya-audio/        # 解码、DSP、WASAPI、音频协议；无 DB/无 Tauri
│   ├── aiya-audio-host/   # sidecar 二进制：音频进程入口
│   ├── aiya-app/          # PlayerActor（mailbox/effects）、用例编排、端口 trait
│   └── aiya-system/       # SMTC、托盘、电源、单实例等平台集成
├── src-tauri/             # Tauri 2 宿主：commands/events、窗口、协议、supervisor
├── ui/                    # Svelte 5 + Vite；只依赖生成的 contracts
└── contracts/             # 协议说明、生成物、golden fixtures
```

依赖方向只有一条：

```
ui → generated TS → src-tauri → aiya-app → aiya-core
                         │           ├── aiya-data
                         │           └── aiya-audio-client ⇢ pipe ⇢ aiya-audio-host → aiya-audio
                         └── aiya-system
```

`aiya-core` 不允许依赖 `tauri`、`tokio`、`rusqlite`、`windows`；`aiya-audio` 不允许依赖数据库或 Tauri。违反这条，解耦就会在三个月内退化回现版。

## 文档

| 文档 | 内容 |
|---|---|
| [`docs/00-feature-inventory.md`](docs/00-feature-inventory.md) | 现版全部功能、状态、证据、重写归属 |
| [`docs/01-code-review.md`](docs/01-code-review.md) | 写得好的代码段、主要代码地图、不能照搬的部分 |
| [`docs/02-target-architecture.md`](docs/02-target-architecture.md) | 进程/分层/协议/数据流/错误恢复/测试 |
| [`docs/03-backend-boundary.md`](docs/03-backend-boundary.md) | 哪些必须进后端、哪些留在前端、怎么解耦 |
| [`docs/04-audio-engine.md`](docs/04-audio-engine.md) | WASAPI 共享/独占、格式协商、时钟、DSP、坑位 |
| [`docs/05-migration-plan.md`](docs/05-migration-plan.md) | 阶段、验收、复用/删除清单、风险与工期 |
| [`docs/06-open-questions.md`](docs/06-open-questions.md) | 需要拍板的 12 个分叉与建议 |
| [`docs/adr/`](docs/adr/) | 五条不可回退的决策记录 |

## 现在不做什么

- 不在旧仓库上继续补 WASAPI / EQ / DSD。
- 不把 PCM 放进 WebSocket、Tauri event 或任何 JSON IPC。
- 不追求一次 parity；先打通“能稳定出声、能恢复、能诊断”的最小闭环。
- 不在架构评审前写大量 UI 或数据库代码。

## 下一步

1. 评审 `docs/02` / `docs/03` 的边界，尤其是“播放状态机整体迁 Rust”与“音频 sidecar”两项。
2. 冻结旧库 schema（当前 `user_version = 4`），把 `AIYA_MUSIC/data/library.sqlite` 作为兼容性验收样本。
3. 起 `aiya-core` 与 `aiya-data` 的空壳 + 协议 golden fixtures，先跑通 5 万首查询与队列纯逻辑。
4. 再接 Tauri 壳与 `aiya-audio` 共享模式，最后做独占与 bit-perfect。