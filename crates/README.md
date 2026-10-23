# crates/（规划）

实现从 P0/P1 开始，按依赖方向落地。**无历史兼容**：数据库从 v1 开始，不迁旧库。

| crate | 职责 | 不允许依赖 |
|---|---|---|
| `aiya-player` | 领域类型、队列、状态归约、命令/事件、错误 | Tauri / tokio / rusqlite / windows |
| `aiya-data` | SQLite v1、Repository、扫描、封面缓存、设置 | Tauri / 音频平台细节 |
| `aiya-audio` | **B 方案**：wasapi-rs + 解码/DSP/时钟 | DB / Tauri |
| `aiya-audio-host` | sidecar 二进制：v1 做 mpv supervisor；B 方案做原生引擎入口 | DB / Tauri |
| `aiya-audio-client` | 管道/JSON IPC 客户端、重连、帧协议 | DB / UI |
| `aiya-app` | PlayerActor（mailbox/effects）、用例编排、端口 trait | Tauri 类型 |
| `aiya-system` | SMTC、托盘、电源、单实例 | UI / DB 细节 |

`sidecars/beets/` 是 Python 项目（beets 2.14.x），不是 Rust crate；host 通过 stdio JSON 调用。

先建 `aiya-player` 与 `aiya-data`；P0 先跑 UI compat + mpv/beets spike，再决定 P2 是否走 `aiya-audio` 原生。