# docs/ledger · 架构域流水账

历史叙事、设计取舍、踩坑过程的**唯一承接位**（制度：docs/60 §2）。
代码注释里禁止出现指向本目录或任何 docs 的引用；方向永远单向：ledger → 代码。

## 主题索引

| 文件 | 覆盖代码 |
|---|---|
| [audio-engine.md](audio-engine.md) | `crates/aiya-audio/`、`crates/aiya-audio-host/` |
| [data-layer.md](data-layer.md) | `crates/aiya-data/`、`crates/aiya-beets-store/` |
| [player-core.md](player-core.md) | `crates/aiya-player/` |
| [beets-protocol.md](beets-protocol.md) | `crates/aiya-beets-client/`、`protocol/` |
| [tauri-host.md](tauri-host.md) | `src-tauri/` |
| [beets-tool.md](beets-tool.md) | `beets-tool/` |
| [ui-shell.md](ui-shell.md) | `ui/src/stores/`、`ui/src/views/`、`ui/src/App.svelte` |
| [ui-render.md](ui-render.md) | `ui/src/lib/render/`、`ui/src/components/` |
| [ui-ipc.md](ui-ipc.md) | `ui/src/lib/ipc/`、`ui/src/lib/settings*.ts` |
| [build-tooling.md](build-tooling.md) | `tools/`、`ui/e2e/`、CI |

## 条目 schema（append-only，新 → 旧）

    - YYYY-MM-DD · 代码位置 · 事实/取舍（现在时一句话）· 来龙去脉（可选，≤2 句）

## 纪律

1. 同一事实只在一处写全；落不进一行事实的内容交给 git 历史，不硬写。
2. 不从编号文档搬家；docs/NN 是历史档案，每篇头部"相关档案"是唯一纽带。
3. 新取舍默认进 ledger；跨进程契约、难回退决定等仍按 AGENTS §5.3 开编号文档。
4. 单篇 ≤400 行，超出即拆题。
