# AIYA Music — Tauri 2 重建

> 状态：**Phase 0 / UI 兼容 + 引擎选型**。
> 基线：`../AIYA_MUSIC`（Electron demo，已锁定于 tag `demo-final` = 代码终版 `4b5edfc`；255 测试全绿，仓库已归档只作参考）。
> **无历史兼容**：不迁移旧库、不兼容旧 `media://`、不保留 Electron IPC。旧仓库只做 UI/行为参考。

## 这次重建的重点

不是“把 Electron 换成 Tauri”，而是三件事：

1. **音频所有权归后端**：WebView 不再创建 `<audio>` / `AudioContext` / `MediaElementSource`；由 Rust host 编排音频 sidecar，关窗/重载/崩溃不影响出声。
2. **UI 先原样搬**：demo 的视觉/交互已经验证过，现在重写是负收益。先用 `window.api` 兼容层 + `RemoteAudioSink` 跑起来，后续按 feature 重写。
3. **beets 是第一等功能**：不是“调一下 `beet import`”，而是把候选、差异、写标签、移动/复制做成可审阅、可解释的 UI。

## 六条已定决策

1. **后端拥有音频输出**：共享/独占 WASAPI、解码、设备、时钟都在音频 sidecar（v1 原生：Symphonia 解码 + wasapi-rs 输出，ADR-0006；mpv 仅 spike 对照）；Rust host 编排；前端只显示状态。
2. **播放状态机在 Rust**：队列、随机、重复、恢复只有一份真相；托盘/SMTC/关窗都控制同一个 Actor。
3. **音频独立进程 + PCM 链内 tap**：`aiya-audio-host` sidecar 承载原生引擎；DSP 链内建订阅制“必要采样” tap（动画级频谱，关闭零开销、不破坏 bit-perfect），EQ、LRC、WASAPI 输出联合设计一次定形。见 04 §5、ADR-0006。
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
│   ├── aiya-audio/        # 原生音频（wasapi-rs + symphonia/FFmpeg）——v1 主方案（ADR-0006）
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
| [`09-audio-engine-choice.md`](docs/09-audio-engine-choice.md) | mpv vs wasapi-rs 对比、GPLv3、spike 门槛（含 §3.1 修订：原生首发） |
| [`10-plan-alignment.md`](docs/10-plan-alignment.md) | **底盘规划对齐评估**：一致/冲突裁决（FTS5、UI 复用、引擎改判等）、ADR-0006 依据 |
| [`11-review-round2-2026-10-24.md`](docs/11-review-round2-2026-10-24.md) | 第二轮全量审查：交叉验证矩阵、5 个 P0、16 个 P1、假绿测试清单 |
| [`12-e2e-backend-fix-2026-10-24.md`](docs/12-e2e-backend-fix-2026-10-24.md) | 后端修复与 E2E 验收（114→124 测试、真机冒烟、5 万首性能） |
| [`13-packaging-2026-10-24.md`](docs/13-packaging-2026-10-24.md) | **打包**：externalBin/许可进包、安装→运行→卸载闭环、仍缺项 |
| [`adr/`](docs/adr/) | 五条不可回退的决策记录 |

## 构建 / 测试 / 打包

常用命令都收进根 `package.json`，在**仓库根**执行 `npm run <script>`。Rust 相关
脚本统一经 `tools/run.mjs` 转发（把 `%USERPROFILE%\.cargo\bin` 前置进 PATH），
所以从 CMD / PowerShell / IDE 里跑都不会报「'cargo' 不是内部或外部命令」。

| 命令 | 作用 |
|---|---|
| `npm run dev` | 开发：`tauri dev`（自动起前端的 vite dev server） |
| `npm run dev:ui` | 只起前端（浏览器里跑 mock 数据，不开 Tauri） |
| `npm run build` / `bundle` | 出安装包：`vite build` → 编 sidecar → release 编译 → NSIS |
| `npm run build:ui` | 只构建前端到 `ui/dist` |
| `npm run build:sidecar` | 只编 `aiya-audio-host` 到 `src-tauri/binaries/<name>-<triple>.exe` |
| `npm run check` | `svelte-check` 类型检查（0 error 才算过） |
| `npm run test` | Rust 全量测试（当前 **124 条**） |
| `npm run test:data` / `test:audio` / `test:player` / `test:app` | 单 crate 测试 |
| `npm run test:all` | Rust 全量 + UI 类型检查 + UI 构建 |
| `npm run verify` | `test:all` + beets 五场景冒烟 |
| `npm run smoke:engine` / `smoke:offset` / `smoke:switch` / `smoke:rate` | 真机 WASAPI 出声冒烟（需要声卡） |
| `npm run smoke:supervisor` | sidecar 握手/心跳/看门狗冒烟（不需要声卡） |
| `npm run smoke:beets` | beets apply 五场景（隔离 `BEETSDIR`，不动真实库） |
| `npm run bench:50k` | 5 万首性能基准（release，`--ignored`） |
| `npm run notices` | 刷新 `licenses/CARGO-DEPS.md`（依赖变更后跑） |
| `npm run icon:source` | 重新生成占位图标源图（再用 `tauri icon` 出全套） |
| `npm run clean` | `cargo clean` |
| `npm run verify:release` | `test:all` + 打包，发版前一把过 |

打包前置会做两件事：`vite build` 出前端产物，`tools/build-sidecar.mjs` 编出
`aiya-audio-host` 并放到 `src-tauri/binaries/<name>-<triple>.exe`（Tauri
`externalBin` 的约定路径）。产物：`target/release/bundle/nsis/*.exe`。
细节与验证记录见 [`13-packaging-2026-10-24.md`](docs/13-packaging-2026-10-24.md)。

> `beforeBuildCommand` 的 cwd 是**调用 tauri CLI 时的目录**，所以别在 `ui/` 里
> 跑 `tauri build`（会找不到 `src-tauri` 与 `tools/`）。

## 现在不做什么

- 不迁移旧数据库、不兼容旧 `media://`、不保留 Electron preload/IPC。
- 不为“以后可能要”保留 `MediaAsset/MediaLocation` 多来源模型；需要时再加。
- 不在前端建第二条音频路径；不为了 pro 频谱把 Web Audio 搬回来。
- 不先把 EQ 实现/DSD/crossfade/watcher/在线做进去（EQ 的链路位置与协议已随 ADR-0006 定形，实现后置）。
- 不在 UI 兼容层跑通前写大量新界面。

## License

项目以 **GPL-3.0-or-later** 发布，全文见 [`LICENSE`](LICENSE)。

分发相关材料（都在安装包的 `licenses/` 下）：

| 文件 | 内容 |
|---|---|
| `licenses/THIRD-PARTY-NOTICES.md` | 随包分发/链接的第三方组件与许可；**只列真在包里的**（v1 无 mpv/FFmpeg） |
| `licenses/CARGO-DEPS.md` | 525 个 Rust 依赖的名称/版本/许可（`node tools/gen-notices.mjs` 生成） |
| `licenses/SOURCE-OFFER.txt` | GPL-3.0 §6(d) 源码获取要约 |

设计取舍与"为什么不用 mpv"见 [`09-audio-engine-choice.md`](docs/09-audio-engine-choice.md) §5。

## 进度与剩余

**已完成**：P0 宿主+UI、P1 数据+只读、P2 播放纵切（含 sidecar 进程化）、P3 beets
预览/应用、P4 体验/系统、P5 第一刀打包。124 测试全绿，安装包安装→运行→卸载闭环
通过（[`13`](docs/13-packaging-2026-10-24.md)）。

**剩余**（完整清单与理由见 `docs/12` §六、`docs/13` §七）：

| 类别 | 待办 |
|---|---|
| 后端小刀 | 日志落盘（全仓无 `tracing_subscriber` 初始化）、设置四项接线（`preventSleep`/`autoplayOnStart`/`rememberPosition`/`normalizeVolume`）、音量逐样本 ramp、封面缓存失效、歌词负缓存、诊断页三段硬编码 |
| 音频 | 独占模式（完成定义要求「独占可选且失败可解释」，当前只有共享） |
| UI | 整体重做；重做时一并处理 `11` 的 P0-5（空格双派发）与 P1-19~24（歌词早退、ConfirmDialog Enter、`.side-panel` 缺失、频谱 pro 降级、队列行 Enter、key 重复）、快捷键设置页、主题/密度消费者。**UI 目前零测试** |
| 发布 | 代码签名（host 与 sidecar 都要签）+ 杀软误报测试、4h soak（长播/万文件扫描/100 张专辑导入/设备切换）、CSP 与 capabilities 收窄、exe 版本资源 |

自动更新按计划后置到 v2。