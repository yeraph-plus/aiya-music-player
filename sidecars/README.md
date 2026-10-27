# sidecars/

只剩音频侧车。beets 已经不在这里（docs/33 两侧分离：它搬到了 `beets-tool/`，
按独立产品发布，播放器只通过协议调用）。

| 目录 | 语言 | 作用 |
|---|---|---|
| `audio/`（代码在 `crates/aiya-audio-host`） | Rust | v1 原生引擎 sidecar（ADR-0006）；`mpv.exe` 仅作 spike 对照/逃生 |

规则：

- sidecar 不打开宿主 SQLite；
- 不把 PCM 放进 JSON/IPC；
- 每个 sidecar 有自己的日志/版本/自检；
- 崩溃由 host 重启，不自动重试"写文件"类操作。
