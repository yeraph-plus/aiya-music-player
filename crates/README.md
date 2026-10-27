# crates/（规划）

实现从 P0/P1 开始，按依赖方向落地。**无历史兼容**：数据库从 v1 开始，不迁旧库。

| crate | 职责 | 不允许依赖 |
|---|---|---|
| `aiya-player` | 领域类型、队列、状态归约、命令/事件、错误 | Tauri / tokio / rusqlite / windows |
| `aiya-data` | SQLite v1、Repository、扫描、封面缓存、设置 | Tauri / 音频平台细节 |
| `aiya-audio` | **v1 主方案**（ADR-0006）：wasapi-rs + 解码/DSP/时钟/链内 tap | DB / Tauri |
| `aiya-audio-host` | sidecar 二进制：v1 原生引擎入口；mpv supervisor 仅作对照/逃生 | DB / Tauri |
| `aiya-audio-client` | 管道/JSON IPC 客户端、重连、帧协议 | DB / UI |
| `aiya-beets-client` | beets 协议客户端：发现/起进程/清点协议；**零环境知识** | DB / Tauri / 任何插件名 |
| `aiya-app` | PlayerActor（mailbox/effects）、用例编排、端口 trait | Tauri 类型 |
| `aiya-system` | SMTC、托盘、电源、单实例 | UI / DB 细节 |

`beets-tool/` 是 Python 项目（beets 2.14.x，独立发布单元），不是 Rust crate；
宿主通过 `aiya-beets-client` 走 stdio NDJSON 调用。除 `src-tauri` 外**任何 crate
都不许依赖它**——没装 beets 的机器必须能完整播放，这条由 `npm run check:boundary` 守。

先建 `aiya-player` 与 `aiya-data`；P0 先跑 UI compat + 原生引擎/beets spike（mpv 仅对照），P2 落 `aiya-audio` 原生引擎（ADR-0006）。