# sidecars/

| 目录 | 语言 | 作用 |
|---|---|---|
| `beets/` | Python 3.10–3.14 + beets 2.14.x | 导入预览、候选/diff、写标签、移动/复制；stdio JSON |
| `audio/`（规划） | Rust（`aiya-audio-host`） | v1 原生引擎 sidecar（ADR-0006）；`mpv.exe` 仅作 spike 对照/逃生 |

规则：

- sidecar 不打开宿主 SQLite；
- 不把 PCM 放进 JSON/IPC；
- 每个 sidecar 有自己的日志/版本/自检；
- 崩溃由 host 重启，不自动重试“写文件”类操作。
