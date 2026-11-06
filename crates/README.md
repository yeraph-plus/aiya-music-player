# crates/（现状）

核心逻辑全部下沉为纯 Rust 库，按依赖方向分层。**无历史兼容**：数据库从 v1 开始，不迁旧库。

| crate | 职责 | 不允许依赖 |
|---|---|---|
| `aiya-player` | 领域类型、队列、状态归约（`PlayerActor`）、命令/事件、错误 | Tauri / tokio / rusqlite / windows / 音频后端 |
| `aiya-data` | SQLite v1、Repository、扫描、封面缓存、设置 | Tauri / 音频平台细节 / `aiya-player` |
| `aiya-audio` | **v1 主方案**（ADR-0006）：Symphonia 解码 + wasapi-rs 输出 + DSP/时钟/链内 tap；sidecar 线协议（`proto`）也定义在这里 | DB / Tauri |
| `aiya-audio-host` | sidecar 二进制：v1 原生引擎入口；mpv supervisor 仅作对照/逃生 | DB / Tauri（只依赖 `aiya-audio`） |
| `aiya-beets-client` | beets 协议客户端：发现/起进程/清点协议；**零环境知识** | DB / Tauri / 任何插件名 |
| `aiya-beets-store` | beets 模块自有的派生数据（索引增强结论）：独立 SQLite 文件，不认识应用曲库 | 应用曲库（`aiya-data`）/ Tauri / `aiya-beets-client` |
| `aiya-dto` | IPC 命令/事件 DTO 与拖放 DTO 的共享契约源（`derive(TS)` 导出）；从桌面壳迁出供 server 复用 | tauri / 音频平台 |
| `aiya-host-core` | 宿主编排共享层：`BeetsHost` trait + beets/索引增强/进程收尸（beets.rs、enrichment.rs、proc.rs 自桌面壳迁出）；桌面壳与 server 各自实现宿主面注入 | tauri / `aiya-audio` / 音频平台 |
| `aiya-server` | Web 版服务端壳（docs/65）：axum 命令路由、Range 流式、静态托管、SSE；浏览器出声 | tauri / `aiya-audio` / 音频平台 |

早期规划中的 `aiya-audio-client`（管道/JSON IPC 客户端）、`aiya-app`（PlayerActor
编排）、`aiya-system`（SMTC、托盘、单实例）没有单独成 crate，职责落到现有单元：

* 协议类型与帧收发在 `aiya-audio::proto`（Rust 源，契约以 Rust 为源，见 AGENTS.md §3 C4）；
  宿主侧的管道客户端、重连与降级在 `src-tauri/src/sidecar.rs`——那是宿主集成层，
  依赖 Tauri 类型，不适合做成库 crate。
* `PlayerActor` 就在 `aiya-player`（C2 的"播放状态机唯一真源"），不另设 app 层。
* SMTC/托盘/电源/单实例留在 `src-tauri`（Windows 宿主集成，C2/C7 的边界靠
  "只调 Actor 命令、不碰第二套状态"维持）。

`beets-tool/` 是 Python 项目（beets 2.14.x，独立发布单元），不是 Rust crate；
宿主通过 `aiya-beets-client` 走 stdio NDJSON 调用。除 `src-tauri` 外**任何 crate
都不许依赖它**——没装 beets 的机器必须能完整播放，这条由 `npm run check:boundary` 守。

库间依赖方向（web 分支拆分后的现状，逐条见各 Cargo.toml，机器守卫 `npm run check:deps`）：

- `aiya-audio-host → aiya-audio`（sidecar 唯一依赖）；
- `aiya-dto → aiya-data / aiya-player`（共享形状引用领域类型）；
- `aiya-host-core → aiya-data / aiya-player / aiya-dto / aiya-beets-client / aiya-beets-store`（编排层汇总）；
- `aiya-server → aiya-data / aiya-player / aiya-dto / aiya-beets-store / aiya-host-core`（命令面挂编排）；
- `aiya-data / aiya-player / aiya-audio` 三者互不依赖——UI 与引擎经宿主解耦
  （C1），播放状态机对引擎的语义由桌面壳/浏览器引擎两种 transport 各自适配。
