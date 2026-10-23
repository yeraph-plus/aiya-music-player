# crates/（规划）

实现从 P1 开始，按依赖方向落地：

| crate | 职责 | 不允许依赖 |
|---|---|---|
| `aiya-core` | 领域类型、队列、状态归约、命令/事件、错误 | Tauri / tokio / rusqlite / windows |
| `aiya-data` | SQLite、迁移、Repository、扫描、封面缓存、jobs | Tauri / WASAPI |
| `aiya-audio` | 解码、DSP、WASAPI、音频协议 | DB / Tauri |
| `aiya-audio-host` | sidecar 二进制、命名管道服务 | DB / Tauri |
| `aiya-audio-client` | 管道客户端、supervisor 侧协议 | DB / Tauri UI |
| `aiya-app` | PlayerActor（mailbox/effects）、用例编排、端口 trait | Tauri 类型 |
| `aiya-system` | SMTC、托盘、电源、单实例 | UI / DB 细节 |

先建 `aiya-core` 与 `aiya-data`；`aiya-audio` 用 `NullSink` 跑通协议后再接 WASAPI。