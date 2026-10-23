# 02 — Tauri 2 目标架构

> 本文定义“新版长什么样”。所有实现选择都要能回答一个问题：**它是否让音频、状态、数据、UI 四者的边界更清楚？**
> 关联：[`03-backend-boundary.md`](03-backend-boundary.md)、[`04-audio-engine.md`](04-audio-engine.md)、[`05-migration-plan.md`](05-migration-plan.md)。

## 1. 目标与非目标

### 目标

1. **音频稳定**：WASAPI 共享/独占、设备热插拔、按源格式开流、错误可见、崩溃可恢复。
2. **单一真相**：队列与播放状态只在 Rust 后端；UI 重载/关闭不影响出声。
3. **可诊断**：设备、模式、格式、缓冲、欠载、独占结果、回退原因都是一等数据。
4. **可替换**：输出后端、解码器、UI 框架、传输层都能替换而不改领域逻辑。
5. **可测试**：核心纯逻辑、音频引擎（假设备）、协议、UI 纯函数都有自动化测试。

### 非目标（v1）

- 不做跨平台音频（先 Windows/WASAPI；Linux/macOS 用共享后端占位）。
- 不做 native DSD / ASIO；DSD 只考虑 DoP（后续）。
- 不做在线音乐、账号、云同步。
- 不做多窗口复杂同步；先单主窗口 + 隐藏 SMTC 窗口。
- 不把 PCM 放进 WebSocket/JSON IPC。

## 2. 进程模型：三个角色

```
┌──────────────────────────────┐
│ WebView2 / Svelte 5          │
│ 视图、交互、歌词、频谱绘制    │
│ 无文件/DB/音频 API            │
└──────────────┬───────────────┘
               │ Tauri invoke / events（JSON，类型生成）
┌──────────────▼───────────────┐
│ Tauri host（Rust）            │
│ commands / events             │
│ PlayerActor（唯一状态机）      │
│ SQLite / 扫描 / 任务           │
│ SMTC / 托盘 / 电源 / 单实例    │
│ artwork:// 协议               │
│ audio supervisor              │
└──────┬───────────────────────┘
       │ 命名管道，长度前缀帧（控制/事件，无 PCM）
┌──────▼───────────────────────┐
│ aiya-audio sidecar（Rust）    │
│ 解码线程 → 环形缓冲 → DSP     │
│ 渲染线程 → WASAPI             │
│ 设备/格式/时钟/欠载            │
│ 频谱 tap（可选）               │
└──────────────────────────────┘
```

### 为什么音频要独立进程

| 维度 | 同进程线程 | 独立 sidecar |
|---|---|---|
| 崩溃隔离 | 解码器/COM 崩溃拖垮整个应用 | 只崩音频，宿主重启它 |
| 实时性 | 受 Tauri runtime/数据库/GC（Rust 无 GC）干扰较小，但仍共享进程 | 独立调度，音频线程更干净 |
| 设备占用 | 退出/重启整应用才释放 | 可单独重启、单独诊断 |
| 调试 | 附加一个进程 | 可单独跑 `--selftest`、单独看日志 |
| 复杂度 | 低 | 中（IPC、supervisor、打包） |
| 打包 | 单 exe | 多一个几 MB 的 exe |

**决策：v1 就用 sidecar。** 理由不是“微服务化”，而是音频问题的排障成本远高于 IPC 成本：控制命令每秒个位数，位置 10Hz，频谱 30Hz；这些数据量很小。PCM 永远不过管道。

> 如果后续发现 IPC 是瓶颈，`AudioPort` trait 允许把 sidecar 降级为同进程线程；协议与领域层不变。

## 3. 分层与依赖规则

```
┌─────────────────────────────────────────────┐
│ ui/ (Svelte)                                │
│ 只依赖 generated contracts + 本地 UI 状态     │
└──────────────────┬──────────────────────────┘
                   │ invoke / events
┌──────────────────▼──────────────────────────┐
│ src-tauri/ (adapter)                        │
│ commands、events、窗口、协议、supervisor      │
└──────────────────┬──────────────────────────┘
                   │ 调用用例
┌──────────────────▼──────────────────────────┐
│ aiya-app/ (application)                     │
│ PlayerActor、LibraryService、ScanService     │
│ 端口 trait：AudioPort / TrackRepo / StateStore│
└───────┬──────────────────────┬──────────────┘
        │                      │
┌───────▼────────┐   ┌─────────▼─────────────┐
│ aiya-data/     │   │ aiya-audio-client/     │
│ SQLite/扫描/任务│   │ 管道客户端/重连/帧协议  │
└───────┬────────┘   └─────────┬─────────────┘
        │                      │
┌───────▼────────┐   ┌─────────▼─────────────┐
│ aiya-core/     │   │ aiya-audio/            │
│ 纯领域类型/队列 │   │ 解码/DSP/WASAPI        │
│ 状态机/错误     │   │ 无 DB/无 Tauri          │
└────────────────┘   └───────────────────────┘
```

### 硬规则

1. `aiya-core` 不依赖 `tauri`、`tokio`、`rusqlite`、`windows`、`serde_json`（可用 `serde` 派生，但不做 IO）。
2. `aiya-audio` 不依赖 `aiya-data`、`src-tauri`；只依赖 `aiya-core` 的音频契约与平台库。
3. `aiya-data` 不依赖 `aiya-audio`、`src-tauri`；只依赖 `aiya-core`。
4. `aiya-app` 是唯一同时看到“数据”和“音频”的地方；它负责编排，不实现 SQL/解码。
5. `src-tauri` 只做适配：参数校验、调用用例、事件序列化、系统集成；不写业务规则。
6. `ui` 不直接 `invoke` 任意命令；只通过生成的 `api` 模块与 stores。

> 违反第 1/2 条，三个月后就会重新长出“播放状态机知道 WASAPI 设备”的耦合。

## 4. 领域核心：PlayerActor

### 4.1 为什么是 Actor，而不是一把大锁

播放命令来自 UI、托盘、SMTC、sidecar 事件、定时器；如果每个入口都锁同一个 `Mutex<PlayerState>`，很容易在持锁时调用音频/数据库，形成死锁或长阻塞。Actor 模型：

```
        ┌───────────── PlayerActor ─────────────┐
UI ────►│ mailbox: Command | AudioEvent | Tick   │
SMTC ──►│ 单线程消费，顺序处理                    │──► Effect::Audio(cmd)
sidecar►│ 状态只在这里改                          │──► Effect::Persist(...)
timer ─►│                                        │──► Effect::Emit(snapshot)
        └────────────────────────────────────────┘
```

- mailbox 用 `tokio::sync::mpsc`（有界，控制命令不丢；高频位置可合并）。
- Actor 内部没有 `async` 的音频调用；只产出 `Effect`，由外层执行。
- 测试时用 `FakeAudio` 收集 `Effect::Audio`，同步驱动 Actor，无需 runtime/设备。

### 4.2 状态

```rust
pub struct PlayerState {
    pub queue: Queue,
    pub status: PlaybackStatus,
    pub position_ms: u64,
    pub duration_ms: Option<u64>,
    pub volume: f32,
    pub muted: bool,
    pub rate: f32,
    pub error: Option<AppError>,
    pub current: Option<Track>,
    pub generation: u64,
    pub revision: u64,
}
```

- `Queue` 持有 `Vec<TrackId>`、`index`、`repeat`、`shuffle`、`order: Option<Vec<usize>>`。
- `current` 是展示快照，来自 `TrackRepo`；不把完整 `Track` 塞进队列。
- `generation` 每次 `load` 自增；sidecar 事件必须带 generation。
- `revision` 每次状态变化自增；UI 用它丢弃乱序快照。

### 4.3 命令与效果

```rust
pub enum PlayerCommand {
    PlayTracks { ids: Vec<TrackId>, start: usize },
    PlayQuery { query: TrackQuery, start: usize },
    Toggle, Next, Prev, Seek { ms: u64 },
    SetVolume { value: f32 }, ToggleMute,
    SetRate { value: f32 }, SetRepeat { mode: RepeatMode },
    SetShuffle { on: bool },
    Enqueue { ids: Vec<TrackId>, mode: QueueInsert },
    RemoveAt { index: usize }, Move { from: usize, to: usize },
    Clear, Stop, Restore(PersistedPlayer),
}

pub enum Effect {
    Audio(AudioCommand),
    Persist(PersistOp),
    Emit(StateEvent),
    Log(LogEvent),
}
```

`PlayQuery` 由 app 层先解析成 ID 列表（同一个 `TrackRepo` 查询），再交给 Actor；Actor 不碰数据库。

### 4.4 关键不变量

1. `index ∈ [-1, len)`；空队列 `index = -1`。
2. `order` 要么 `None`，要么是 `0..len` 的排列；`cursor = order.position(index)`。
3. 任何队列结构变化都要同步重映射 `order`（参考 `queue.ts:183-211`）。
4. `generation` 单调递增；旧 generation 的 `Ended/Error/Position` 丢弃。
5. `position` 只能由 sidecar 的时钟事件推进；UI 不直接写状态。
6. 持久化失败不回滚播放；记录 warning，下次重试。

### 4.5 自动推进

```
AudioEvent::Ended { generation }
  ├─ generation != current.generation → ignore
  ├─ repeat == One → Audio(Load { same track, start: 0 }) + Play
  ├─ next_index() == None → status = Paused/Stopped at duration
  └─ Some(next) → load(next, start=0, autoplay=true)
```

`Ended` 由 sidecar 在缓冲区真正播完后发；不要在 UI 的 `timeupdate` 里猜。

## 5. 数据层

### 5.1 SQLite 所有权

- **只有宿主进程打开 SQLite。** 音频 sidecar 不 import `rusqlite`。
- `aiya-data` 用一个专用 DB 线程拥有连接（`rusqlite::Connection` 不是 `Sync`），通过有界 channel 接收 `DbRequest`，返回 `oneshot`。
- 启动：`PRAGMA journal_mode=WAL; foreign_keys=ON; synchronous=NORMAL; busy_timeout=5000;`，然后跑迁移。
- 读多写少；先用单连接串行，5 万首查询 2–30ms 足够。若 UI 卡顿，再加只读连接池（WAL 允许），不要过早。

### 5.2 Schema 策略

- **保持现有 schema 与 `user_version`**，让新版能直接打开旧库；这是最便宜的迁移验收。
- v1–v4 迁移 SQL 原样移植；新版只追加 v5（队列规范化）。
- 旧 `play_queue` JSON 行在 v5 中迁移到 `play_queue_items`，迁移完成前保留旧列只读。
- 新表：

```sql
CREATE TABLE play_queue_state (
  id           INTEGER PRIMARY KEY CHECK (id = 1),
  position     INTEGER NOT NULL DEFAULT 0,
  position_ms  INTEGER NOT NULL DEFAULT 0,
  repeat_mode  TEXT    NOT NULL DEFAULT 'off',
  shuffle      INTEGER NOT NULL DEFAULT 0,
  volume       REAL    NOT NULL DEFAULT 0.85,
  muted        INTEGER NOT NULL DEFAULT 0,
  rate         REAL    NOT NULL DEFAULT 1,
  updated_at   INTEGER NOT NULL
);

CREATE TABLE play_queue_items (
  queue_id  INTEGER NOT NULL,
  position  INTEGER NOT NULL,
  track_id  TEXT    NOT NULL REFERENCES tracks(id) ON DELETE CASCADE,
  PRIMARY KEY (queue_id, position)
) WITHOUT ROWID;
```

- 进度写 `position_ms` 一个字段（5s 节流），队列编辑才重写 items；不再每 5 秒序列化 2 万 ID。
- 队列上限：产品上统一为 50,000（或“当前视图全量”），在 app 层夹取并返回明确错误；不要 IPC 2 万、内部 5 万的暗差异。

### 5.3 查询与分页

- 保留 cursor + 物化 offset 双路径；保留 `library_order`。
- 保留 `title_norm` + `LIKE`，不要上 FTS5（两字中文的坑已验证）。
- `list_all_ids` 用于“点行播整库”；分页列表只取窗口。
- 所有查询返回 `took_ms`，由 app 层汇总到 diagnostics。

### 5.4 扫描

- `walkdir` 遍历，`rayon` 或固定 8 路 `spawn_blocking` 解析；单文件 5s 超时用 `CancellationToken` + 子任务。
- `lofty` 读标签/封面/ReplayGain；`image` 生成 96/256/512 缩略图；`sha1` 保持现有 ID 算法兼容。
- 两阶段：先入库可播，再补封面；进度按 120ms 节流。
- 扫描任务由 `ScanService` 统一入口 `request_scan(reason)`；忙时 dirty 补跑；取消优先。
- watcher 只做触发器，正确性靠周期性对账（后续）。

## 6. 传输层：Tauri commands + events

### 6.1 命令

按领域分组，参数强类型；不要一个万能 `invoke`。

```rust
#[tauri::command]
async fn player_command(
    cmd: PlayerCommand,
    app: tauri::State<'_, AppHandle>,
) -> Result<PlayerSnapshot, AppError>;

#[tauri::command]
async fn library_page(
    query: TrackQuery,
    app: tauri::State<'_, AppHandle>,
) -> Result<TrackPage, AppError>;

#[tauri::command]
async fn queue_slice(
    offset: usize, limit: usize,
    app: tauri::State<'_, AppHandle>,
) -> Result<QueueSlice, AppError>;
```

- `PlayerCommand` 是 tagged enum，生成 TS union。
- mailbox 消息带 `oneshot` reply；命令返回的是 **Actor 应用之后**的快照，不是发送前的缓存。批量操作有显式上限；超限返回 `AppError::InvalidArg`，不静默截断。
- 状态变化另走 event，供其他窗口/异步推进（自动下一首）使用。

### 6.2 事件

```rust
#[derive(Serialize, Clone)]
pub struct EventEnvelope {
    pub seq: u64,
    pub revision: u64,
    pub event: AppEvent,
}

#[derive(Serialize, Clone)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum AppEvent {
    PlayerState(PlayerSnapshot),
    QueueChanged { revision: u64, length: usize },
    ScanProgress(ScanProgress),
    LibraryChanged { revision: u64 },
    AudioStatus(AudioStatus),
    Spectrum(SpectrumFrame),
    Toast(Toast),
}
```

规则：

- **低频全局事件**（player state 4–10Hz、scan progress、library changed）走 Tauri event。
- **高频流**（position 10Hz、spectrum 30fps）可用 Tauri `Channel` 按窗口订阅；不要把所有高频事件广播给所有窗口。
- 位置事件 10Hz 足够；进度条/歌词前端外推。
- `seq` 单调；UI 丢弃 `seq <= last_seq` 的事件。
- `QueueChanged` 只带 revision/length，UI 需要时用 `queue_slice` 拉可见窗口，不传全队列。

### 6.3 错误

```rust
#[derive(Serialize)]
pub struct AppError {
    pub code: ErrorCode,       // invalid_arg / not_found / busy / io / db / audio_device_lost / ...
    pub message: String,
    pub details: Option<Value>,
    pub retryable: bool,
}
```

- 不把 `anyhow` 字符串直接丢给 UI。
- 音频错误保留原始 `HRESULT`/WASAPI 码、设备 ID、格式、模式，放 `details`。
- UI 只按 `code` 分支，文案可本地化。

### 6.4 契约生成

- Rust 结构体派生 `serde` + `specta::Type`，用 `tauri-specta` 生成 `ui/src/bindings.ts`。
- CI 校验：生成物与提交一致；协议 schema hash 写入 `contracts/schema.hash`。
- sidecar 协议与 Tauri 协议分离：sidecar 不依赖 Tauri 类型，便于单独测试/复用。
- 版本策略：`PROTOCOL_VERSION` 在 handshake 中校验；不兼容直接拒绝并提示升级，不做静默降级。

## 7. 音频 sidecar 协议

详见 [`04-audio-engine.md`](04-audio-engine.md)。这里只定边界：

```
Host → Audio:
  Hello { protocol, client_version }
  OpenDevice { device_id: Option<String>, mode: Shared|Exclusive|Auto, policy }
  Load { generation, track_id, path, format_hint, start_ms, autoplay, gain_db, rate }
  Play/Pause/Seek/SetVolume/SetRate/SetEq/Stop
  SetSpectrum { enabled, bands, fps }
  Shutdown

Audio → Host:
  Ready { protocol, devices, capabilities }
  Loaded { generation, duration_ms, format }
  Started/Paused { generation, position_ms }
  Position { generation, position_ms, playing }
  Ended { generation }
  Error { generation?, code, hresult?, message, device? }
  DeviceLost { device_id, reason }
  Underrun { count }
  Spectrum { bands: [u8; N] }
  Stats { ... }
```

- 帧格式：4 字节小端长度 + JSON（v1，易调试）；频谱可后续换 `postcard`/共享内存。
- 传输：Windows 命名管道（`\\.\pipe\aiya-audio-{pid}`）；sidecar 启动后创建，宿主连接。命名管道比 stdio 更适合重启/多通道，也不会和日志混在一起。
- 心跳：2s；5s 无响应 → kill + restart。
- 背压：控制命令有界队列不丢；`Position`/`Spectrum` 可丢旧帧。
- 日志：sidecar 写自己的滚动日志到 `data/logs/audio-*.log`，stderr 只输出致命错误。

## 8. 系统集成

| 能力 | 归属 | 方案 |
|---|---|---|
| SMTC | host | `windows` crate，隐藏顶层窗口 + `ISystemMediaTransportControlsInterop::GetForWindow`；元数据/封面/时间轴/按钮 |
| 托盘 | host | Tauri tray API；菜单随播放状态重建 |
| 关闭到托盘 | host | `WindowEvent::CloseRequested` 隐藏窗口；`RunEvent::ExitRequested` 在托盘模式下 `prevent_exit`，只有 `quitting` 放行 |
| 全局媒体键 | 系统/SMTC | 不注册 `globalShortcut` 媒体键，避免掐掉 SMTC 会话 |
| 防休眠 | host | `SetThreadExecutionState(ES_CONTINUOUS | ES_SYSTEM_REQUIRED)` 或 Tauri 电源插件；允许熄屏 |
| 单实例 | host | Tauri single-instance 插件；二次启动唤起窗口 |
| 窗口状态 | host | Tauri window-state 插件；首帧底色用配置 |
| 自定义协议 | host | 仅 `aiya-artwork://`；按 artworkId 解析缓存，ACAO + immutable |
| 更新/签名 | host | Tauri updater + Authenticode；sidecar 一起签 |

### SMTC 的坑（来自 spike）

- interop **拒绝 message-only 窗口**（`E_INVALIDARG`），必须用隐藏的顶层窗口。
- `TimeSpan` 字段名是 `Duration`（大写）；`SetTitle` 收 `HSTRING`。
- WebView2 的 `navigator.mediaSession` 不会出现在系统 SMTC（实测），不要依赖。
- 关闭 `globalShortcut` 媒体键，否则会话可能消失（Electron 实测；Tauri 也按此原则）。

## 9. 前端架构

```
ui/src/
├── app/
│   ├── App.svelte           # 只做 shell + 路由
│   └── providers/           # 事件订阅、错误边界、主题
├── features/
│   ├── library/             # 列表、搜索、筛选、批量
│   ├── player/              # 播放条、队列、速率、音量
│   ├── playlists/
│   ├── lyrics/
│   ├── spectrum/
│   ├── settings/
│   └── diagnostics/
├── components/              # 无业务状态的通用组件
├── stores/                  # player/library/selection/ui（只缓存，不拥有真相）
├── lib/                     # 纯函数：virtual/selection/track-window/drag/lyrics-clock/format
└── bindings.ts              # 生成物，不手改
```

规则：

- 组件不直接 `invoke`；通过 feature store 的 action。
- store 不复制后端状态；保存 `PlayerSnapshot` + `pending` 覆盖层（例如 seek 拖动中的本地值）。
- 播放命令返回快照；不要乐观改成 `playing`，等 `started` 事件。
- 列表/队列虚拟化继续用纯函数；选择集继续用视图下标。
- 歌词高亮、进度条都用锚点 + `performance.now()` 外推；事件只负责对时。
- 频谱只画后端给的 bands；不要在前端建 `AudioContext`。
- `App.svelte` 目标 < 300 行；每个 feature 一个容器组件。

## 10. 启动、关闭与恢复

### 启动

```
host start
  → init logging / paths
  → open DB + migrate (backup first if version changes)
  → load settings + player state
  → start PlayerActor (restored queue, paused)
  → spawn audio sidecar + handshake
  → open device (mode from settings; fallback policy)
  → if queue and autoplay: Actor.load(current, position)
  → create window (show when ready)
  → emit initial snapshot / events
```

### 关闭

```
quit requested
  → stop accepting commands
  → Actor.pause + persist(force)
  → audio Shutdown (fade 100ms, release device)
  → flush DB WAL, close
  → exit
```

### 恢复矩阵

| 事件 | 行为 |
|---|---|
| UI 重载/崩溃 | 音频不停；UI 重连后拉 snapshot + queue_slice |
| 音频 sidecar 崩溃 | host 标记 `audio_lost`；暂停状态；重启 sidecar；从最后位置 load；UI 提示 |
| 设备拔出 | sidecar `DeviceLost`；暂停；重新枚举；恢复默认设备；从原位置继续（可配置） |
| 独占失败 | 按策略：`auto` → 共享 + 明确提示；`exclusive_required` → 暂停 + 错误 |
| DB 写失败 | 播放继续；记录错误；下次重试；不阻塞音频 |
| 应用崩溃 | 退出前 5s 进度已落库；重启恢复到最近保存点 |

## 11. 安全与打包

- WebView 无文件系统权限；所有路径由后端解析，UI 只拿 ID。
- 自定义协议只暴露 artwork ID，不暴露任意路径。
- Tauri capabilities 最小化：只开 window/tray/dialog/opener 必要权限。
- sidecar 只接受宿主连接；命名管道 ACL 限制当前用户。
- 安装包签名 host + sidecar；WebView2 用系统运行时，安装器带 bootstrapper 或检测提示。
- 日志脱敏：不记录完整路径到遥测；本地日志可含路径，但滚动/限制大小。

## 12. 性能预算

| 指标 | 预算 | 验证 |
|---|---|---|
| 5 万首冷启动 | ≤ 450ms | criterion/启动计时 |
| 列表首屏 | ≤ 100ms | 查询 + IPC + 渲染 |
| 深分页 | ≤ 5ms | DB 查询 |
| 搜索 | ≤ 20ms | 5 万行 |
| 播放命令往返 | ≤ 20ms（不含设备重建） | host→sidecar→ack |
| 位置事件 | 10Hz，单帧 < 200B | 协议统计 |
| 频谱 | 30fps，48 bands，单帧 < 100B | 协议统计 |
| 内存稳态 | 播放 1h 增长 < 30MB | soak |
| 音频欠载 | 0（正常设备） | stats |

## 13. 测试金字塔

| 层 | 工具 | 内容 |
|---|---|---|
| 领域纯逻辑 | `cargo test` | 队列/随机/重复/恢复/令牌；property test |
| 应用层 | `cargo test` + FakeAudio | 命令→效果序列；自动推进；错误恢复 |
| 数据层 | `cargo test` + temp SQLite | 迁移幂等/旧库兼容/分页/搜索/事务 |
| 音频引擎 | 单元 + 硬件自检 | 解码 PCM 哈希、seek 精度、格式协商、独占锁 |
| 协议 | golden JSON | 命令/事件双向、版本、错误 |
| UI 纯函数 | Vitest | 虚拟列表/选择/拖拽/歌词时钟/快捷键 |
| UI 组件 | Vitest + Testing Library | 播放条、队列、设置关键交互 |
| E2E | tauri-driver / WebDriver | 启动→扫描→播放→seek→重启恢复 |
| 性能 | criterion + 启动脚本 | 预算回归 |

**音频测试原则**：所有 WASAPI 测试分两类——`#[ignore]` 的硬件测试（需要真实设备/独占权限）与 `NullSink`/`FileSink` 的确定性测试（CI 必跑）。不要用“能听到声音”作为唯一验收。

## 14. 架构决策索引

| ADR | 决策 |
|---|---|
| [0001](adr/0001-rust-owns-audio.md) | Rust 拥有音频输出，WebView 不再出声 |
| [0002](adr/0002-audio-sidecar.md) | 音频助手独立进程，命名管道通信 |
| [0003](adr/0003-backend-authoritative-player.md) | 播放状态机在 Rust，UI 只读 |
| [0004](adr/0004-rust-contract-source.md) | Rust 契约单一来源，生成 TS |
| [0005](adr/0005-artwork-protocol-only.md) | 只为封面保留自定义协议，音频不再走 media:// |

## 15. 一句话总结

> **UI 负责“看起来在播”，Rust 负责“真的在播”。**
> 两者之间只传状态与命令，不传 PCM，不传文件路径，不传第二套状态机。