# 05 — 迁移计划：从 Electron 到 Tauri 2 + Rust 音频

> 原则：**先证明边界，再追求 parity；先让音频稳定，再补体验功能。**
> 估计按“1 名熟悉 Rust/Windows 音频的开发者，全职”计算；含调试设备与打包，不含 beets/DSD 的深度实现。

## 1. 迁移策略

不是“在旧仓库里逐步替换 Electron”，而是**新仓库并行重写，旧仓库冻结**：

- 旧仓库继续可运行，作为行为参考与回滚路径。
- 新仓库按阶段实现，用同一份数据库副本/同一批 fixtures 验收。
- 每个阶段都有可运行的产物，不做“半年后一次大爆炸”。
- 先做“能稳定出声 + 能恢复 + 能诊断”的纵切，再横向补功能。

## 2. 阶段总览

| 阶段 | 目标 | 主要产物 | 退出条件 | 估时 |
|---|---|---|---|---|
| P0 | 决策与脚手架 | 本仓库、ADRs、协议草案、CI | 边界评审通过；`cargo test`/`vitest` 空跑 | 3–5d |
| P1 | 领域 + 数据 | `aiya-core`、`aiya-data`、旧库兼容 | 队列/随机/分页/迁移测试全绿；5 万库可查 | 1–1.5w |
| P2 | Tauri 壳 + 只读 UI | 窗口、commands/events、列表/搜索/封面 | 能打开旧库、浏览/搜索/封面；无播放 | 1–1.5w |
| P3 | 共享音频 + 播放纵切 | `aiya-audio` Shared、PlayerActor、播放条 | 点行出声、seek、暂停、恢复、关窗继续 | 1.5–2w |
| P4 | 独占 + 诊断 | Exclusive、格式协商、设备、 diagnostics | 独占/共享可切；bit-perfect 可证明；失败可见 | 1–1.5w |
| P5 | 体验 parity | 播放列表/历史/歌词/频谱/快捷键/托盘/SMTC | parity 清单 90%+；E2E 通过 | 1.5–2w |
| P6 | 后台与发布 | watcher/jobs/beets、安装包/签名/更新 | 长任务可恢复；安装包可升级 | 1–2w |

**总计约 7–10 周**（不含 DSD/EQ 深度、跨平台）。如果只有业余时间，按 2–3 倍估。

## 3. P0 — 决策与脚手架（3–5 天）

### 交付

- 本仓库的 5 份文档 + 5 条 ADR。
- `Cargo.toml` workspace、空 crate、`ui` Vite 骨架、CI（fmt/clippy/test/typecheck）。
- 协议草案：`AudioCommand`/`AudioEvent`、`AppEvent`、`AppError`、`PlayerCommand`。
- 旧库样本：复制 `AIYA_MUSIC/data/library.sqlite` 到 `fixtures/legacy/`（不进 git 大文件；用脚本生成/下载）。
- 基准脚本：5 万首 seed/query 的 Rust 版骨架。

### 验收

- [ ] 文档评审：音频 sidecar、状态机迁 Rust、契约以 Rust 为源，三项都拍板。
- [ ] `cargo fmt --check` / `cargo clippy -D warnings` / `cargo test` 通过。
- [ ] `pnpm typecheck` / `vitest` 通过（空壳）。
- [ ] 协议 golden fixtures 能双向序列化。

## 4. P1 — 领域与数据（1–1.5 周）

### 交付

- `aiya-core`：`TrackId`、`Queue`、`RepeatMode`、`PlayerState`、`PlayerCommand`、`AudioCommand`、错误类型。
- 队列纯逻辑：append/insert next/remove/move/clear、shuffle 排列、next/prev、cursor。
- `aiya-data`：`rusqlite` DB 线程、迁移 v1–v5、`TrackRepo`、`PlaylistRepo`、`HistoryRepo`、`QueueRepo`。
- 旧库兼容：复制 `user_version=4` 的库，在副本上迁移到 v5（新增队列表，保留旧 `play_queue`）；旧库保持可回滚。
- 5 万首 seed + 基准：冷启动、深分页、搜索。

### 验收

- [ ] 现版 `player.test.ts` / `queue.test.ts` / `shuffle.test.ts` / `reorder.test.ts` 的用例逐条翻译并通过。
- [ ] 5 万库：cursor 翻页不重不漏；offset 深分页 ≤ 5ms；搜索 ≤ 20ms。
- [ ] 迁移幂等；旧库迁移后旧表仍可读（回滚保险）。
- [ ] 队列持久化：改队列才写 items；进度只写 state 一行。

## 5. P2 — Tauri 壳与只读 UI（1–1.5 周）

### 交付

- Tauri 2 窗口（原生装饰、最小尺寸、首帧底色、窗口状态）。
- commands/events + 生成的 TS bindings。
- `aiya-artwork://` 协议 + 封面缓存读取。
- `ui`：侧栏、列表、搜索、排序、筛选、虚拟列表、空态；只读，不播放。
- `library_page`/`library_stats`/`playlists_list` 等命令。
- 诊断页骨架。

### 验收

- [ ] 打开旧库，5 万首列表可滚动；搜索/排序/筛选与现版一致。
- [ ] 封面 96/256/512 正常，缓存命中；无路径泄漏到 UI。
- [ ] UI 无 `window.api`；只 import `bindings.ts` 与 stores。
- [ ] 列表性能：首屏 ≤ 100ms，滚动稳定 60fps。

## 6. P3 — 共享音频与播放纵切（1.5–2 周）

### 交付

- `aiya-audio`：symphonia 解码、ring、NullSink、WASAPI Shared、时钟、事件。
- `aiya-audio-host` sidecar + 命名管道 + supervisor。
- `PlayerActor`：命令/事件/自动推进/恢复；`AudioPort` 适配。
- 播放条：播放/暂停/上一首/下一首/seek/音量/静音/速率（先 1.0 或简单变速）。
- 持久化：队列/进度/音量/静音；启动恢复。
- 关窗到托盘（先不接 SMTC）。

### 验收

- [ ] 点列表任意行，按当前视图整库顺序播放；播完最后一首停。
- [ ] 连点 200 次下一首不越界；旧 generation 事件不覆盖新状态。
- [ ] 暂停/恢复/seek 准确；seek 容差 ±100ms（共享模式）。
- [ ] 关窗到托盘，音乐继续；UI 重载后状态恢复。
- [ ] sidecar 崩溃：宿主重启它，从最后位置继续/暂停，UI 有提示。
- [ ] 长播 1h：无句柄泄漏、无爆音、位置漂移 < 100ms。

## 7. P4 — 独占、格式与诊断（1–1.5 周）

### 交付

- 设备枚举/选择/默认跟随/热插拔。
- Exclusive：格式协商、period=buffer、对齐恢复、失败分类。
- `Auto` / `Shared` / `ExclusiveRequired` 策略与可见回退。
- bit-perfect 模式（跳过 DSP；整数输出）。
- `AudioStats` + 诊断页音频区块。
- 硬件自检命令（`aiya-audio --selftest`）。

### 验收

- [ ] 44.1k/16bit、48k/24bit、96k/24bit 独占成功；第二客户端打开同设备失败。
- [ ] f32 独占被拒，错误可见，不崩溃。
- [ ] 44.1k/10ms 走 `GetBufferSize` 对齐恢复。
- [ ] 设备拔出/默认切换：暂停/恢复/提示符合策略。
- [ ] bit-perfect 下归一化/EQ/音量被禁用或明确提示；诊断显示 `bit_perfect=true`。
- [ ] 1h 独占播放 0 underrun（正常设备），内存稳态。

## 8. P5 — 体验 parity（1.5–2 周）

### 交付

- 播放列表 CRUD/排序/加歌/拖拽/封面。
- 播放历史/最近/常听/统计/清空。
- 歌词：后端读文件 + 解析 + 编码；前端时钟/滚动/点击跳转。
- 频谱：引擎 tap + 48 bands + 前端 compact 绘制；pro 可后置。
- 快捷键：17 动作、改键、冲突检测。
- 托盘菜单、SMTC、防休眠、单实例、主题/密度/主色。
- 批量选择/右键菜单/对话框/Toast。
- 诊断页完整指标。

### 验收

- [ ] 现版 parity 清单（`00-feature-inventory.md` §7）逐条通过。
- [ ] SMTC 在真实 Windows 会话可见，按钮/进度双向生效。
- [ ] 频谱开/关不改变音频路径；独占模式下也有数据。
- [ ] UI E2E：启动→扫描→播放→seek→切换→重启恢复。
- [ ] 255 个旧测试中适用的部分已翻译/替代，新增 Rust 测试覆盖核心。

## 9. P6 — 后台与发布（1–2 周）

### 交付

- `notify` watcher + 30min reconcile；watcher 只触发 `request_scan`。
- 持久化 jobs（去重/重试/取消/恢复）；扫描/分析/beets 任务。
- beets sidecar（隔离 `BEETSDIR`、禁 move、import 交互）。
- 安装包：host + sidecar 签名、WebView2 检测、updater。
- 长播 soak、扫描 10 万文件、崩溃恢复演练。

### 验收

- [ ] 扫描中杀进程，重启后任务可恢复/不重复。
- [ ] watcher 删除/改名/新增最终与对账结果一致。
- [ ] 安装包在干净 Windows 11 上可安装、启动、卸载；升级保留数据。
- [ ] 4h soak 无泄漏/无爆音/无设备丢失未处理。

## 10. 复用 / 重写 / 删除清单

| 资产 | 处理 | 说明 |
|---|---|---|
| `docs/02` ADRs、`03/04/10` 实测 | **复用** | 坑位与数据直接进新 ADR/测试 |
| `tests/*.test.ts` | **翻译** | 作为 Rust 规格；不要求 1:1 文件 |
| `fixtures/`、`scripts/seed.mjs` | **复用/改写** | 生成 5 万合成库；Rust seed 更好 |
| DB schema v1–v4 | **复用** | SQL 原样；新增 v5 只加表 |
| `packages/playback` | **重写** | Rust，语义保留 |
| `packages/database` | **重写** | rusqlite；SQL 复用 |
| `packages/library` | **重写** | lofty/walkdir/image |
| `packages/media` 音频协议 | **删除** | 封面缓存思路保留 |
| `packages/storage` | **删除/合并** | 原生引擎直接读文件 |
| `packages/jobs` | **重写** | 持久化队列 |
| `apps/desktop/src/main` | **重写** | Tauri host + 系统集成 |
| `preload` | **删除** | 无跨界桥 |
| `renderer` 视觉/纯逻辑 | **复用** | API 层与 App 拆分 |
| `audio-sink.ts` / Web Audio 图 | **删除** | Rust 引擎 |
| `audiomotion.ts` | **删除/后置** | 依赖 AnalyserNode |

## 11. 数据库迁移与回滚

### 迁移

1. **默认不原地升级旧库**：首次启动把 `library.sqlite` 复制为 `library-v2.sqlite`（或用户指定路径），在副本上迁移。旧版继续使用原库，互不干扰。
   - WAL 模式下不能只拷主文件：先让旧版退出/checkpoint，或用 SQLite backup API / `VACUUM INTO`。
2. 启动时检测 `user_version`；若低于目标，先备份到 `backups/`。
3. 在事务中按版本追加迁移；失败回滚并保留备份。
4. v5 只新增 `play_queue_state`/`play_queue_items`，**不删除旧 `play_queue`**，便于导入/比对。
5. 首次启动从旧 `play_queue` 导入队列；之后只写新表。
6. 校验：`PRAGMA integrity_check`、行数、队列长度、索引存在。

### 回滚

- **不能假设旧版能打开 v5 库**：旧版 `migrate()` 在 `to > maxKnown` 时会直接报错（`packages/database/src/db.ts:78-84`）。因此“双写旧表让旧版回滚”不成立。
- 采用副本策略时，回滚 = 继续用旧库/旧版，新库可保留用于对比。
- 若必须原地升级，回滚 = 停止新版、用备份恢复原库；不能直接启动旧版。
- 在正式切库前完成一次“升级 → 使用 → 回滚”演练，并记录备份路径。

## 12. 风险清单

| # | 风险 | 影响 | 缓解 |
|---|---|---|---|
| 1 | WASAPI 设备/驱动矩阵复杂 | 独占失败、爆音、设备丢失 | 先 Shared；硬件自检矩阵；错误可见；Auto 回退 |
| 2 | 解码格式覆盖不足（WMA/AIFF/DFF） | 曲库部分不可播 | 明确支持矩阵；可选 FFmpeg；UI 标注 |
| 3 | Tauri/WebView2 生态差异（SMTC/托盘/快捷键） | 系统集成回归 | spike 已验证 SMTC fallback；逐项 E2E |
| 4 | sidecar 打包/签名/杀软 | 安装失败/被杀 | 一起签名；安装器测试；日志/崩溃报告 |
| 5 | 旧库兼容/迁移失败 | 用户数据风险 | 备份；v5 只增；双写；回滚演练 |
| 6 | UI parity 工作量大 | 延期 | 先纵切；P5 按优先级；不追求一次全量 |
| 7 | 音频自动化测试依赖硬件 | 回归覆盖不足 | NullSink/FileSink + `#[ignore]` 硬件测试 |
| 8 | 范围蔓延（EQ/DSD/crossfade） | 核心延迟 | 明确 v1 不做；协议预留 |
| 9 | 性能回退（Rust IPC/序列化） | 列表/播放卡顿 | 预算 + criterion；队列分片；事件合并 |
| 10 | 单人长期项目节奏 | 半途而废 | 每阶段可运行；旧版可用；文档先行 |

## 13. 前 10 个具体任务

1. 建 workspace + CI；把本仓库 5 份文档纳入 review。
2. 写 `aiya-core::queue`，翻译 `queue.test.ts` / `shuffle.test.ts` / `reorder.test.ts`。
3. 写 `aiya-data` DB 线程 + v1–v4 迁移，打开旧库副本。
4. 写 `TrackRepo::page/all_ids`，用 5 万 seed 跑基准。
5. 写 v5 队列规范化 + 旧 JSON 导入 + 双写开关。
6. 写 `AudioCommand`/`AudioEvent` + golden fixtures。
7. 写 `NullSink` + 解码 + ring + 时钟；用 WAV fixture 跑 PCM 哈希。
8. 写 WASAPI Shared + 设备枚举；`--selftest` 输出 JSON。
9. 起 Tauri 窗口 + commands/events + 生成 bindings；只读列表接通。
10. 写 `PlayerActor` + FakeAudio；把 `player.test.ts` 的关键 10 条翻译成 Rust。

## 14. 完成定义（DoD）

新版可以替代旧版日常使用，当且仅当：

- 音频：Shared 稳定；Exclusive 可用且失败可解释；设备切换不崩；长播无泄漏。
- 状态：队列/进度/音量/速率/重复/随机重启一致；UI 重载/关闭不影响播放。
- 曲库：5 万首列表/搜索/排序/筛选/封面达到预算；旧库可直接打开。
- 系统：SMTC/托盘/防休眠/单实例/关窗到托盘与旧版等价。
- 体验：播放列表/历史/歌词/频谱/快捷键/批量操作可用。
- 工程：核心测试、协议 golden、E2E、性能基准、崩溃恢复演练全部有记录。

> 如果只能先做一件事：**把 P3 的“共享音频纵切”做扎实**。它一旦成立，后面的独占、EQ、DSD 都是在这个骨架上加东西；它不成立，做多少 UI 都是在旧问题上刷漆。