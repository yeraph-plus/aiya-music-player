# AIYA Music — Tauri 2 重建

> 状态：**Phase 0 / UI 兼容 + 引擎选型**。
> 基线：`../AIYA_MUSIC`（今天写的 Electron demo，`f99564e` + 未提交改动；255 测试全绿）。
> **无历史兼容**：不迁移旧库、不兼容旧 `media://`、不保留 Electron IPC。旧仓库只做 UI/行为参考。

## 这次重建的重点

不是“把 Electron 换成 Tauri”，而是三件事：

1. **音频所有权归后端**：WebView 不再创建 `<audio>` / `AudioContext` / `MediaElementSource`；由 Rust host 编排音频 sidecar，关窗/重载/崩溃不影响出声。
2. **UI 先原样搬**：demo 的视觉/交互已经验证过，现在重写是负收益。先用 `window.api` 兼容层 + `RemoteAudioSink` 跑起来，后续按 feature 重写。
3. **beets 是第一等功能**：不是“调一下 `beet import`”，而是把候选、差异、写标签、移动/复制做成可审阅、可解释的 UI。

## 六条已定决策

1. **后端拥有音频输出**：共享/独占 WASAPI、解码、设备、时钟都在音频 sidecar（v1 mpv / B 方案 wasapi-rs）；Rust host 编排；前端只显示状态。
2. **播放状态机在 Rust**：队列、随机、重复、恢复只有一份真相；托盘/SMTC/关窗都控制同一个 Actor。
3. **音频独立进程**：v1 推荐 `mpv` sidecar（`--audio-exclusive`/`--wasapi-exclusive-buffer`/全格式）；`wasapi-rs` 原生是 B 方案。见 [`09`](docs/09-audio-engine-choice.md)。
4. **契约以 Rust 为源**：生成 TS；但为了搬 UI，先保留旧 `AiyaApi` 形状作为“UI 兼容契约”（不是旧数据兼容）。
5. **单一数据库写入者**：只有宿主开 SQLite；音频/beets sidecar 不碰宿主库。
6. **全新 schema v1**：没有 v1–v5 迁移链；demo 数据可丢，直接从零建库。

## 仓库结构（规划）

```
AIYA_MUSIC_TAURI/
├── docs/                  # 功能盘点、UI 复用、beets、引擎选型、计划、ADR
├── crates/
│   ├── aiya-player/       # 纯领域：队列、状态归约、命令/事件；无 IO/无 Tauri
│   ├── aiya-data/         # SQLite v1、Repository、扫描、封面、设置
│   ├── aiya-audio/        # 原生音频（wasapi-rs + symphonia/FFmpeg）——B 方案
│   ├── aiya-audio-host/   # sidecar 入口：mpv supervisor 或原生引擎
│   ├── aiya-app/          # PlayerActor（mailbox/effects）、用例编排、端口 trait
│   └── aiya-system/       # SMTC、托盘、电源、单实例
├── sidecars/
│   └── beets/             # Python + beets 2.14.x；stdio JSON
├── src-tauri/             # Tauri 2 宿主：commands/events、窗口、协议、supervisor
├── ui/                    # 复制 demo renderer + compat 适配
└── contracts/             # 协议、生成物、golden fixtures
```

依赖方向：

```
ui → compat API → src-tauri → aiya-app → aiya-player
                         │           ├── aiya-data
                         │           └── aiya-audio-client ⇢ pipe ⇢ aiya-audio-host
                         └── aiya-system

src-tauri ⇢ stdio JSON ⇢ sidecars/beets
```

`aiya-player` 不依赖 `tauri`、`tokio`、`rusqlite`、`windows`；`aiya-audio` 不依赖数据库或 Tauri。违反这条，三个月后会退化回现版。

## 文档

| 文档 | 内容 |
|---|---|
| [`00-feature-inventory.md`](docs/00-feature-inventory.md) | 现版 125 项功能/状态/证据（只作参考，不是 parity 承诺） |
| [`01-code-review.md`](docs/01-code-review.md) | 值得保留的代码段、不能照搬的模式 |
| [`02-target-architecture.md`](docs/02-target-architecture.md) | 进程/分层/协议/数据流/恢复/测试 |
| [`03-backend-boundary.md`](docs/03-backend-boundary.md) | 后端/前端/删除边界 |
| [`04-audio-engine.md`](docs/04-audio-engine.md) | wasapi-rs 原生路径：WASAPI、时钟、DSP、坑位 |
| [`05-rebuild-plan.md`](docs/05-rebuild-plan.md) | **4–6 周重建计划**、阶段、验收、任务 |
| [`06-open-questions.md`](docs/06-open-questions.md) | 仍需拍板的 9 个分叉 |
| [`07-ui-reuse.md`](docs/07-ui-reuse.md) | **UI 原样搬**：54 个调用点、5 个适配器、优先级 |
| [`08-beets.md`](docs/08-beets.md) | **beets 集成**：Python sidecar、预览/diff/应用 |
| [`09-audio-engine-choice.md`](docs/09-audio-engine-choice.md) | mpv vs wasapi-rs、GPLv3、spike 门槛 |
| [`adr/`](docs/adr/) | 五条不可回退的决策记录 |

## 现在不做什么

- 不迁移旧数据库、不兼容旧 `media://`、不保留 Electron preload/IPC。
- 不为“以后可能要”保留 `MediaAsset/MediaLocation` 多来源模型；需要时再加。
- 不在前端建第二条音频路径；不为了 pro 频谱把 Web Audio 搬回来。
- 不先把 EQ/DSD/crossfade/watcher/在线做进去。
- 不在 UI 兼容层跑通前写大量新界面。

## License

项目计划以 **GPL-3.0** 发布。第三方依赖（mpv/FFmpeg/beets/Tauri 等）的许可与分发注意事项见 [`09-audio-engine-choice.md`](docs/09-audio-engine-choice.md) §5；发布前补全 `NOTICE`/源码提供方式。

## 下一步

1. 复制 renderer → `ui`，写 `compat/api.ts` + `RemoteAudioSink`，让旧界面在 Tauri 里显示（见 `07`）。
2. 跑 2 天引擎 spike：mpv 独占/seek/设备/1h vs wasapi-rs；按 `09` §4 打分，拍板。
3. 跑半天 beets spike：`beet --version`、`Library` 读取 10 条；确认 Python sidecar 路线。
4. 建 `aiya-data` v1 + 扫描；接真实列表/封面。
5. 接播放纵切；然后做 beets 预览/应用。