# 00 — 现版功能全量表

> 基线：`../AIYA_MUSIC`，`f99564e` + 工作区未提交改动（频谱、倍速、音频规格、CORS 修复）。
> 验证：`npm run typecheck` 通过；`npm test` **27 文件 / 255 用例全绿**（2026-10-23）。
> 状态：✅ 已实现并可用；⚠️ 部分实现/有已知边界；❌ 未实现。
> **这是“今天的 demo”快照，不是新版的 parity 承诺。** 新版无历史兼容、全新库；v1 只做 UI/beets 相关的高价值项，优先级见 [`07-ui-reuse.md`](07-ui-reuse.md) §4。
> “新版归属”是目标架构的建议，不是现状。

## 1. 曲库、扫描与文件

| # | 功能 | 状态 | 现版实现与证据 | 新版归属 |
|---|---|---|---|---|
| 1.1 | 本地优先曲库，无在线依赖 | ✅ | SQLite + `media://`；`packages/database` | Rust `aiya-data` |
| 1.2 | 多音乐根目录，追加式添加 | ✅ | `ipc.ts:156-175`；`getLibraryRoots` / `setLibraryRoots` | Rust host |
| 1.3 | 首次启动引导选择目录 | ✅ | `App.svelte` 空态按钮；`library.chooseRoot` | 前端 + host dialog |
| 1.4 | 手动扫描 | ✅ | `ipc.ts:118-150`；`scanScheduler.request('manual')` | Rust `aiya-data` |
| 1.5 | 扫描取消 | ✅ | `ipc.ts:152-154`；`AbortSignal` 透传 | Rust cancellation token |
| 1.6 | 扫描进度（walking/parsing/artwork/done） | ✅ | `scanner.ts:68-168`；`ScanProgress` 事件 | Rust event channel |
| 1.7 | 启动后延迟增量扫描 | ✅ | `main/index.ts:322-330`（2.5s 后） | Rust host |
| 1.8 | 扫描请求合并（忙时 dirty 补跑） | ✅ | `services/library.ts:149-197` | Rust 调度器 |
| 1.9 | 单文件失败隔离 + 5s 解析超时 | ✅ | `scanner.ts:103-142`、`466-482` | Rust `aiya-data` |
| 1.10 | mtime/size 增量跳过 | ✅ | `scanner.ts:190-200` | Rust `aiya-data` |
| 1.11 | 音频扩展名白名单 | ✅ | `scanner.ts:18-21`（mp3/flac/m4a/m4b/aac/ogg/oga/opus/wav/wma/aiff/aif） | Rust；按解码能力收窄 |
| 1.12 | 标签解析（标题/艺人/专辑/序号/年份/时长） | ✅ | `scanner.ts:202-222`（music-metadata） | Rust `lofty` |
| 1.13 | 技术规格解析（codec/container/采样率/声道/码率/位深/无损） | ✅ | `scanner.ts:258-270`；现版 schema v4 | Rust `lofty`/`symphonia` |
| 1.14 | ReplayGain 标签读取（track gain/peak） | ⚠️ | `scanner.ts:219-222`；只用了 gain，peak 未参与限幅 | Rust；DSP 内实现 |
| 1.15 | 内嵌封面提取 + 原图缓存 | ✅ | `scanner.ts:238-246`；`artwork-cache.ts` | Rust `image` + cache |
| 1.16 | 封面缩略图 96/256/512、并发去重、临时文件 rename | ✅ | `artwork-cache.ts`；测试 `artwork-cache.test.ts` | Rust 后端 |
| 1.17 | 封面内容哈希去重 | ✅ | `makeArtworkId(bytes)`；`artworks.content_hash UNIQUE` | Rust 保留 SHA-1 兼容 |
| 1.18 | Track / MediaAsset / MediaLocation 三分离 | ✅ | 现版 schema；`domain.ts:20-51` | v1 可简化为 tracks；多来源后置 |
| 1.19 | 文件改名不影响 trackId/播放 | ✅ | 身份由标签指纹生成；`ids.ts` | Rust 保留算法 |
| 1.20 | 多位置/可用性（online/offline/missing） | ⚠️ | schema 与 `findPlayable` 支持；UI 无多位置管理 | Rust 数据层 |
| 1.21 | 从曲库移除（只删索引，不动文件） | ✅ | `ipc.ts:227-242`；`tracks.repo.ts:443-461` | Rust host |
| 1.22 | 在资源管理器中显示 | ✅ | `ipc.ts:189-200`；仅 local source | Rust `opener`/shell |
| 1.23 | 文件 watcher | ❌ | 未实现；chokidar/fs.watch 均无 | Rust `notify` |
| 1.24 | 周期性对账 / 缺失文件标记 | ❌ | Phase 4.5 未做 | Rust 后台任务 |
| 1.25 | 持久化任务队列（崩溃恢复/去重/重试） | ❌ | `packages/jobs` 只是内存 `TaskRegistry` | Rust `aiya-data` jobs |
| 1.26 | beets 集成 / 交互式导入 | ❌ | 仅有调研文档 | Rust worker + sidecar |
| 1.27 | 在线元数据 / 账号 / 云同步 | ❌ | 明确不做 | 不做 |

## 2. 列表、搜索、选择与批量

| # | 功能 | 状态 | 现版实现与证据 | 新版归属 |
|---|---|---|---|---|
| 2.1 | 5 万行虚拟列表 | ✅ | `TrackList.svelte`；`lib/virtual.ts`；固定 52px | 前端保留 |
| 2.2 | 稀疏分页窗口（24 页上限 + generation） | ✅ | `lib/track-window.ts`；`App.svelte:154-183` | 前端保留 |
| 2.3 | cursor 分页 | ✅ | `tracks.repo.ts:289-323`；`query.ts` | Rust `aiya-data` |
| 2.4 | offset 分页走物化位置表 | ✅ | `tracks.repo.ts:246-283`；`database/order.ts` | Rust 保留 |
| 2.5 | 深分页 5 万首 2.3ms 基线 | ✅ | `docs/bench-baseline.md` | Rust 复测 |
| 2.6 | 排序：标题/添加时间/艺人/专辑，升降序 | ✅ | `tracks.repo.ts:50-55`；表头点击 | Rust 查询 + 前端控件 |
| 2.7 | 中文两字搜索（`title_norm` + LIKE） | ✅ | `tracks.repo.ts:146-149`；`text.ts` | Rust 保留 SQL |
| 2.8 | 搜索通配符转义 | ✅ | `likePattern`；测试 `database.test.ts` | Rust 保留 |
| 2.9 | 搜索高亮 | ✅ | `TrackList.svelte`；`highlight` 相关 | 前端保留 |
| 2.10 | 艺人/专辑钻取筛选 | ✅ | `App.svelte:193-211`；`filter.artistId/albumId` | 前端 + Rust query |
| 2.11 | 最近播放 / 常听视图 | ✅ | `source.kind = recent/mostPlayed`；`tracks.repo.ts:191-206` | Rust query |
| 2.12 | 多选（Ctrl/Shift） | ✅ | `lib/selection.ts`；`selection.svelte.ts` | 前端保留 |
| 2.13 | 选择集是“视图下标”而非 ID | ✅ | `SelectionIdsQuery`；上限 5000 | 前端 + Rust 解析 |
| 2.14 | 全选 5 万行不传 5 万 ID | ✅ | `idsForSelection`；`listIdsAtIndices` | Rust 保留 |
| 2.15 | 批量加入队列 / 播放 / 加入播放列表 / 移除 | ✅ | `App.svelte:275-322`；`BatchBar.svelte` | 前端 + Rust commands |
| 2.16 | 右键菜单：下一首播放/加入队列/按艺人/按专辑/在文件夹显示 | ✅ | `lib/menu.ts`；`ContextMenu.svelte` | 前端 + Rust commands |
| 2.17 | 右键菜单：加入播放列表 / 从曲库移除 | ✅ | `App.svelte` 菜单编排 | 前端 + Rust |
| 2.18 | 空态/加载/错误/toast | ✅ | `App.svelte` 空态与 error 区块 | 前端保留 |
| 2.19 | 滚动位置恢复 | ✅ | `UiState.scrollTop`；`App.svelte:474-486` | 前端 + 后端持久化 |
| 2.20 | 列表键盘导航（↑/↓/Enter/Esc） | ✅ | `lib/list-keys.ts`；测试 | 前端保留 |
| 2.21 | 5 万首冷启动 366–403ms 基线 | ✅ | `docs/bench-baseline.md` | Rust 复测 |

## 3. 播放内核与队列

| # | 功能 | 状态 | 现版实现与证据 | 新版归属 |
|---|---|---|---|---|
| 3.1 | 点列表任意行播放“当前视图整库” | ✅ | `playerPlayQuery` → `listAllIds`；`player-host.ts:223-236` | Rust `PlayerActor` |
| 3.2 | 播放一批曲目 | ✅ | `playTracks`；`player.ts:212-230` | Rust |
| 3.3 | 队列最大 2 万（IPC 入参）/ 视图可 10 万 | ⚠️ | `MAX_QUEUE=20_000`；`listAllIds` 上限 10 万 | 统一上限策略 |
| 3.4 | 播放/暂停/恢复 | ✅ | `player.ts:232-260` | Rust |
| 3.5 | 上一首：>3s 先回开头 | ✅ | `player.ts:295-308`；`RESTART_THRESHOLD_MS` | Rust 保留语义 |
| 3.6 | 下一首 / 自动播完进下一首 | ✅ | `player.ts:262-293`、`477-478` | Rust |
| 3.7 | 队列末尾自然停止 | ✅ | `player.ts:277-285` | Rust |
| 3.8 | 单曲循环自动重播 | ✅ | `player.ts:266-275` | Rust |
| 3.9 | 列表循环 / 不循环 | ✅ | `queue.ts:92-122` | Rust |
| 3.10 | 随机播放用排列，一轮不重不漏 | ✅ | `queue.ts:53-90`；测试 `shuffle.test.ts` | Rust 纯逻辑 |
| 3.11 | 切随机不打断当前曲目 | ✅ | `queue.ts:69-90` | Rust |
| 3.12 | 队列：下一首播放 / 追加 / 移除 / 拖动 / 清空 | ✅ | `queue.ts:124-211`；`QueuePanel.svelte` | Rust + 前端 |
| 3.13 | 随机模式下队列编辑正确重映射 | ✅ | `queue.ts:183-211`；测试 `reorder.test.ts` | Rust 保留 |
| 3.14 | seek 夹取到时長 | ✅ | `player.ts:310-319` | Rust |
| 3.15 | 音量/静音 | ✅ | `player.ts:323-333` | Rust + audio |
| 3.16 | 倍速 0.5–2.0，0.1 网格 | ✅ | `contracts/audio-format.ts`；`player.ts:341-347` | Rust + audio |
| 3.17 | 倍速不变调 | ✅ | `<audio>.preservesPitch=true`；`audio-sink.ts:225-232` | Rust 时间伸缩 |
| 3.18 | 换源后保持速率（重放 rate） | ✅ | `audio-sink.ts:204-215`；`player.ts:200-205` | 由 audio 引擎自然保持 |
| 3.19 | 播放令牌与过期回报丢弃 | ✅ | `contracts/player.ts:55-84`；`player.ts:446-450` | Rust generation |
| 3.20 | 连点 20/200 次下一首不越界 | ✅ | `tests/player.test.ts` | Rust property test |
| 3.21 | 播放进度 500ms 上报 | ✅ | `audio-sink.ts:19-21` | 后端 4–10Hz + 前端外推 |
| 3.22 | 播放队列面板虚拟化 | ✅ | `QueuePanel.svelte`（46px 行高） | 前端保留 |
| 3.23 | 队列元数据按可见区间批量取 | ✅ | `QueuePanel.svelte:62-77` | 前端 + Rust batch |
| 3.24 | 队列面板显示总时长 | ✅ | `QueuePanel.svelte:51-56` | 后端可提供聚合 |
| 3.25 | 恢复队列/索引/进度/音量/静音/速率 | ✅ | `hydrate`；`queue.repo.ts` | Rust |
| 3.26 | 5s 节流落库 + 切歌/退出 flush | ✅ | `player-host.ts:97-107`、`160-182`；`queue.repo.ts` | Rust |
| 3.27 | 启动自动播放 / 记住进度开关 | ✅ | `AppSettings.autoplayOnStart/rememberPosition`；`player.ts:169-208` | Rust |

## 4. 音频出口与媒体协议

| # | 功能 | 状态 | 现版实现与证据 | 新版归属 |
|---|---|---|---|---|
| 4.1 | `media://` 自定义协议，`stream: true` | ✅ | `protocol.ts:10-28`；30 分钟文件不全量进内存 | 删除音频用途；封面保留协议 |
| 4.2 | Range 单段 206 / 416 / 后缀范围 | ✅ | `protocol.ts:131-193`；`range.ts:18-48` | 音频不再需要；封面可选 |
| 4.3 | 流关闭计数（finish + close 双保险） | ✅ | `protocol.ts:195-254` | 删除 |
| 4.4 | 音频 `no-store` / 封面 immutable | ✅ | `protocol.ts:46-47` | 封面保留 |
| 4.5 | CORS `ACAO: *` 修复 Web Audio 静音 | ✅ | `protocol.ts:143-154`；`audio-sink.ts:109-113` | 删除音频图后不再需要 |
| 4.6 | `<audio>` 执行播放指令 | ✅ | `audio-sink.ts` 全文件 | **删除**，改 Rust 引擎 |
| 4.7 | Web Audio 图（Gain → Analyser → destination） | ✅ | `audio-sink.ts:49-104`；ADR-037 | **删除**，DSP/频谱回后端 |
| 4.8 | ReplayGain 音量归一化 | ⚠️ | `audio-sink.ts:155-185`；无 peak/loudness | Rust DSP |
| 4.9 | 音频规格徽章（codec/采样率/位深/kbps/Hi-Res） | ✅ | `contracts/audio-format.ts`；`PlayerBar.svelte` | 前端格式化 |
| 4.10 | 媒体错误分类（不支持/解码失败/读取失败） | ✅ | `audio-sink.ts:321-333` | Rust 结构化错误 |
| 4.11 | 输出设备枚举/选择 | ❌ | 未实现；仅调研 `enumerateDevices` | Rust WASAPI |
| 4.12 | 共享模式 WASAPI | ❌ | 目前是 Chromium 默认共享 | Rust 首发 |
| 4.13 | 独占模式 WASAPI | ❌ | 方案已有；未实现 | Rust P2 |
| 4.14 | 按源采样率开流 / bit-perfect | ❌ | 未实现 | Rust |
| 4.15 | 淡入淡出 / 交叉淡化 | ❌ | 未实现 | Rust + Player 决策 |
| 4.16 | gapless | ❌ | 未实现 | Rust |
| 4.17 | EQ | ❌ | 仅预留位置 | Rust DSP（P3） |
| 4.18 | DSD / DoP | ❌ | 仅调研 | Rust（P4，可选） |
| 4.19 | 频谱数据源（compact / pro） | ✅ | `spectrum-source.ts`；`SpectrumPanel.svelte`；`audiomotion.ts` | 后端推分带，前端绘制 |
| 4.20 | 输出失败可见降级/诊断 | ❌ | 无原生引擎可诊断 | Rust + diagnostics |

## 5. 歌词、外观与系统集成

| # | 功能 | 状态 | 现版实现与证据 | 新版归属 |
|---|---|---|---|---|
| 5.1 | LRC 解析：多时间戳/offset/元信息/行内时间戳 | ✅ | `lrc.ts:38-105`；测试 `lrc.test.ts` | Rust 或前端纯函数 |
| 5.2 | 纯文本歌词降级 | ✅ | `lrc.ts:86-93` | 同上 |
| 5.3 | 编码探测 UTF-8 → GB18030 → Big5 | ✅ | `lrc.ts:116-132` | Rust `encoding_rs` |
| 5.4 | 歌词高亮自走时钟（rAF 外推） | ✅ | `lyrics-clock.ts:20-32`；`LyricsPanel.svelte:24-54` | 前端保留 |
| 5.5 | 点击歌词跳转 | ✅ | `LyricsPanel.svelte:56-60` | 前端 + Rust seek |
| 5.6 | 用户滚动后 2s 不抢滚动条 | ✅ | `LyricsPanel.svelte:65-76` | 前端保留 |
| 5.7 | 无歌词/加载/纯文本态 | ✅ | `LyricsPanel.svelte` | 前端保留 |
| 5.8 | 深/浅主题、标准/紧凑密度 | ✅ | `App.svelte:657+`；`app.css`；settings | 前端保留 |
| 5.9 | 封面主色提取 | ✅ | `lib/accent.ts` | 前端保留（canvas） |
| 5.10 | 首帧主题不闪（窗口底色 + 启动参数） | ✅ | `window.ts:8-37`；ADR-027 | Tauri 窗口配置 |
| 5.11 | 窗口尺寸/位置/最大化记忆 | ✅ | `window-state.ts`；`window.ts:65-104` | Tauri window-state 插件 |
| 5.12 | 单实例锁，二次启动唤起窗口 | ✅ | `main/index.ts:132-139` | Tauri single-instance |
| 5.13 | 托盘：显示/隐藏/播放/上一首/下一首/退出 | ✅ | `tray.ts` | Tauri tray |
| 5.14 | 关闭到托盘 vs 直接退出 | ✅ | `main/index.ts:282-313` | Tauri host |
| 5.15 | 播放时阻止系统休眠（允许熄屏） | ✅ | `power.ts` | Tauri/Windows API |
| 5.16 | 系统媒体控制 SMTC（标题/封面/状态/控制） | ✅ | `media-session.ts`；Electron 特性开关 + AUMID | **Rust 原生 SMTC** |
| 5.17 | 系统媒体键由系统路由，不抢 globalShortcut | ✅ | 删除 media-keys；ADR-034 | Rust SMTC |
| 5.18 | 17 个可改键快捷键 + 冲突检测 | ✅ | `contracts/shortcuts.ts`；`SettingsPanel.svelte` | 前端匹配 + 后端动作 |
| 5.19 | 设置页五个页签与即时生效 | ✅ | `SettingsPanel.svelte`；`app-settings.ts` | 前端 + Rust settings |
| 5.20 | 设置清洗/旧配置兼容 | ✅ | `app-settings.ts:8-114` | Rust serde 默认值 |
| 5.21 | 播放列表 CRUD/重命名/删除/排序/封面（首曲自动） | ✅ | `playlists.repo.ts`；`Sidebar.svelte` | Rust `aiya-data` |
| 5.22 | 播放列表内加歌去重/移除/拖动/清空 | ✅ | `playlists.repo.ts:155-290` | Rust |
| 5.23 | 播放列表选择器 / 创建 / 确认对话框 | ✅ | `PlaylistPicker.svelte`；`PromptDialog`；`ConfirmDialog` | 前端保留 |
| 5.24 | 播放历史记录与统计（最近/常听/Top5） | ✅ | `history.repo.ts`；`play-log.ts` | Rust |
| 5.25 | 播放/跳过/完成口径 | ✅ | `play-log.ts:38-80`；测试 | Rust 保留 |
| 5.26 | 清空播放历史 | ✅ | `history.repo.ts:71-78` | Rust |
| 5.27 | 诊断页：库统计/流计数/查询耗时/错误/快捷键冲突 | ✅ | `SettingsPanel.svelte:256-353`；`ipc.ts:427-468` | Rust + 前端 |
| 5.28 | 诊断页：媒体键/防休眠/托盘/平台能力 | ✅ | `Diagnostics`；`platform.ts` | Rust |
| 5.29 | 开发态截图 / smoke / soak | ✅ | `main/dev/*`；`npm run smoke/soak` | Rust test harness |
| 5.30 | 自动更新 / 安装包签名 | ❌ | 未做 | Tauri updater |

## 6. 现版明确没做的能力

这些不应被误认为“已有基础”：

- 文件 watcher 与周期性对账（Phase 4.4/4.5，代码为零）。
- 持久化任务队列（`packages/jobs` 只有内存 `TaskRegistry`）。
- beets 调度、Python worker、交互式导入。
- 输出设备选择、WASAPI 共享/独占、bit-perfect、EQ、DSD、crossfade、gapless。
- 在线元数据、账号、云同步。
- Linux/macOS 的完整验证（当前主要面向 Windows 11）。

## 7. 行为验收参考（不是 v1 全量 parity）

下面这些行为值得保留，但 **v1 只承诺 UI/beets 相关的高价值项**；完整优先级见 [`07-ui-reuse.md`](07-ui-reuse.md) §4。旧库/旧协议不在验收范围。

1. 5 万首冷启动 ≤ 450ms；深分页 ≤ 5ms；搜索 ≤ 20ms（同口径）。
2. 点击列表任意行，播放顺序等于当前视图整库顺序；播完最后一首才停。
3. 连点 200 次下一首，索引不越界、不出现旧歌回报覆盖新歌。
4. 切歌/暂停/退出后重启，队列、索引、进度、音量、静音、速率一致。
5. 独占模式：换到 44.1k/16bit 曲目仍能按源开流；失败有可见原因，不静默回落。
6. 设备拔出/默认设备切换：播放暂停并给出原因，恢复后能从原位置继续。
7. 关窗到托盘后音乐继续；UI 重载/崩溃不影响出声。
8. SMTC 在真实 Windows 会话中可见，播放/暂停/上一首/下一首/进度双向生效。
9. 开频谱不改变音频路径、不引入静音/爆音；独占模式下仍有频谱（引擎内 tap）。
10. 扫描 1 万文件时 UI 可交互、可取消、进度不倒退。