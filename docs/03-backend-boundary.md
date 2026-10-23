# 03 — 后端边界：什么该进 Rust，什么必须留在前端

> 这不是“Rust 比 TS 快”的性能讨论，而是**所有权讨论**：谁拥有设备、文件、时钟、数据库，谁就该拥有对应的状态。
> 现版音频问题的根因不是代码写得差，而是“设备/时钟在 Chromium，状态机在 JS 主进程，UI 在第三个地方”。

## 1. 一条判断标准

对每个模块问四个问题：

1. **它是否直接接触 OS 资源？**（音频设备、文件、注册表、COM、SQLite、窗口句柄）
2. **它是否拥有一个不能被 UI 生命周期打断的时钟/连接？**（播放、扫描、设备热插拔）
3. **它是否需要在窗口关闭/重载/崩溃后继续存在？**
4. **它的错误是否需要原始系统码才能诊断？**（HRESULT、设备 ID、格式）

四个里有两个“是”，就应该进 Rust 后端。只有“显示/交互/布局”进前端。

## 2. 必须进后端（Hard Requirements）

| 能力 | 现版位置 | 为什么必须迁 | 新版归属 |
|---|---|---|---|
| WASAPI 共享/独占输出 | Chromium `<audio>` | Chromium 无法按源采样率开流、无法证明独占、失败静默回落 | `aiya-audio` |
| 解码 | Chromium 媒体栈 | 需要统一 PCM、seek 精度、DSD/格式兜底、解码错误码 | `aiya-audio` |
| 设备枚举/选择/热插拔 | 无 | `enumerateDevices()` 拿不到 WASAPI 端点能力/独占格式 | `aiya-audio` |
| 播放时钟/位置 | `<audio>.currentTime` | 设备时钟是唯一可信时间；窗口挂起会漂 | `aiya-audio` |
| 播放状态机/队列 | JS 主进程 | 与音频同生命周期；避免跨进程双状态 | `aiya-app` / `aiya-core` |
| 音量/静音/速率/DSP | Web Audio/`audio.volume` | 独占下没有 `audio.volume`；EQ/归一化必须在输出前 | `aiya-audio` |
| 音频文件读取/Range | `media://` | 只为喂 `<audio>` 存在；原生引擎直接读文件 | 删除（见 §4） |
| 元数据/标签解析 | `music-metadata` (JS) | 扫描/封面/ReplayGain 与文件系统同域；避免 JS 解析大库 | `aiya-data` |
| SQLite/迁移 | `better-sqlite3` | 单写入者、事务、WAL、旧库兼容 | `aiya-data` |
| 文件扫描/对账/watcher | JS 主进程 | 长任务、可取消、崩溃恢复；不应阻塞 UI | `aiya-data` |
| 封面解码/缩略图 | `sharp` (JS) | 与扫描同域；缓存/哈希/尺寸白名单 | `aiya-data` |
| 播放历史/统计落库 | JS 主进程 | 与播放状态同一事务边界 | `aiya-app` + `aiya-data` |
| 设置持久化 | JS 主进程 | 单一校验点；前端只拿 canonical 值 | `aiya-data` |
| SMTC | Chromium MediaSession | Tauri/WebView2 实测不直通；需隐藏顶层窗口 + WinRT | `aiya-system` |
| 托盘/关窗到托盘 | Electron | Tauri 宿主窗口生命周期 | `src-tauri` |
| 防休眠 | Electron `powerSaveBlocker` | 平台 API；与播放状态绑定 | `aiya-system` |
| 单实例/窗口状态 | Electron | 宿主职责 | `src-tauri` |
| 全局快捷键（非媒体键） | Electron | 平台集成；媒体键交给 SMTC | `aiya-system` |
| 歌词文件读取/编码 | 主进程 | 文件 IO 不应在 WebView；编码探测用 `encoding_rs` | `aiya-data` 或 app |
| 自定义封面协议 | `media://` | 只暴露 artworkId，不暴露路径 | `src-tauri` |

## 3. 必须留在前端（Do Not Move）

| 能力 | 为什么留在前端 | 现版可复用 |
|---|---|---|
| 虚拟列表/滚动/行高 | DOM 布局与滚动是浏览器的最优解 | `lib/virtual.ts`、`TrackList.svelte` |
| 选择集/Shift 连选 | 视图下标是 UI 概念 | `lib/selection.ts` |
| 拖拽排序 | Pointer 事件 + DOM 反馈 | `lib/drag.ts`、`QueuePanel.svelte` |
| 右键菜单/对话框/Toast | 纯 UI 编排 | `lib/menu.ts`、`ContextMenu` 等 |
| 键盘绑定匹配 | 键盘事件在 WebView；后端只执行动作 | `contracts/shortcuts.ts` |
| 歌词高亮时钟 | 需要 rAF 与帧同步；后端只给位置 | `lib/lyrics-clock.ts` |
| 进度条平滑 | 锚点 + `performance.now()` 外推 | 同上模式 |
| 频谱绘制 | canvas/DOM 绘制是前端职责 | `SpectrumPanel.svelte`、`lib/spectrum.ts` |
| 主题/密度/布局 | CSS/DOM | `app.css`、`SettingsPanel` |
| 封面主色提取 | canvas 取色，无需 IPC | `lib/accent.ts` |
| 搜索输入防抖/高亮 | UI 交互 | `App.svelte`/stores |
| 格式化（时间/码率/大小/日期） | 展示逻辑 | `lib/format.ts`、`contracts/audio-format.ts` |

## 4. 应该删除的层

| 删除项 | 原因 | 替代 |
|---|---|---|
| `renderer/src/lib/audio-sink.ts` | `<audio>` 执行器，播放绑死 WebView | Rust `AudioEngine` |
| Web Audio 图（`AudioContext`/`GainNode`/`AnalyserNode`） | 独占后渲染层无 PCM；CORS/静音/生命周期问题 | Rust DSP + 频谱 tap |
| `media://track/*` 音频协议 | 只为 `<audio>` 服务；Range/流计数是补丁 | 原生引擎直接读文件 |
| `preload/index.ts` + `contextBridge` | Tauri 不需要；IPC 由 commands/events 替代 | 生成 bindings |
| `main/player-host.ts` 的 `did-finish-load` 补发 | UI 生命周期不再影响音频 | 常驻 Actor |
| `main/media-session.ts` | WebView2 不直通 SMTC | Rust SMTC |
| `audiomotion.ts` 的 AnalyserNode 接入 | 依赖前端音频图 | 后端 bands + 自研绘制 |
| `queue.repo.ts` 的 JSON 整行写入 | 大队列每 5s 重写 | 规范化 `play_queue_items` |

> 不要为了“少改”保留一个 WebView 音频兜底。两条音频路径 = 两套时钟、两套错误、两套设备状态，最终一定互相打架。

## 5. 灰区决策

### 5.1 播放状态机：前端还是后端？

**结论：后端。** 不是因为它“重”，而是因为：

- 它与音频引擎必须原子地共享 generation、position、duration；
- 托盘/SMTC/全局快捷键在窗口之外，仍要能控制；
- UI 重载不应中断播放；
- 队列/恢复/历史写入需要与 DB 同域。

前端只保留 `PlayerSnapshot` 的只读缓存和 `pending` 覆盖（拖动 seek 的本地值）。不要在前端做“下一首是谁”的判断。

### 5.2 设置：前端还是后端？

**结论：后端持久化 + 前端表单。** 后端是 canonical（默认值、白名单、迁移、枚举），前端只渲染和提交 patch。避免现版 `readSettings`/`sanitizeSettings` 两份逻辑。

### 5.3 歌词：解析在前端还是后端？

**结论：后端读文件 + 解析，前端只做时钟/滚动。** 理由：文件 IO、编码探测（GB18030/Big5）属于后端；解析是纯函数，放 Rust 可测。返回：

```ts
type Lyrics = { found: boolean; synced: boolean; lines: LyricLine[]; offsetMs: number; encoding: string | null };
```

歌词文本很小，可以一次传完；不要为歌词建自定义协议。

### 5.4 频谱：后端算还是前端算？

**结论：后端 tap + 算 bands，前端绘制。** 独占模式下渲染层没有 PCM；回环采集会混入系统声音且独占时不可用。引擎在输出前 tap 是最准确的。协议只传 48 个 `u8`（30fps），不要传 FFT/PCM。

### 5.5 封面主色：后端算还是前端算？

**结论：前端。** 它只影响视觉，canvas 取色成本低；后端算会把 UI 审美写进数据层。保留 `accent.ts`，但封面图必须允许 CORS（自定义协议带 ACAO）才能 `getImageData`。

### 5.6 搜索：SQL 还是前端过滤？

**结论：SQL。** 5 万行不能全量拉到前端；保留 `title_norm` + `LIKE`。前端只做输入防抖和高亮。

### 5.7 歌词/封面缓存：放 data 还是 media？

**结论：`aiya-data` 管文件与索引，`src-tauri` 只做协议适配。** 不要让协议层知道 SQL。

## 6. 解耦的六个具体手法

### 6.1 端口/适配器，而不是“工具函数”

```rust
pub trait AudioPort: Send {
    fn apply(&mut self, cmd: AudioCommand) -> Result<(), AudioError>;
    fn poll(&mut self) -> Vec<AudioEvent>;
    fn stats(&self) -> AudioStats;
}

pub trait TrackRepo: Send + Sync {
    fn get(&self, id: &TrackId) -> Result<Option<Track>, DataError>;
    fn page(&self, q: &TrackQuery) -> Result<TrackPage, DataError>;
    fn all_ids(&self, q: &TrackQuery) -> Result<Vec<TrackId>, DataError>;
}

pub trait StateStore: Send {
    fn load_player(&self) -> Result<Option<PersistedPlayer>, DataError>;
    fn save_player(&self, p: &PersistedPlayer) -> Result<(), DataError>;
}
```

`PlayerActor` 只依赖这些 trait；测试用 `FakeAudio`、`InMemoryRepo`。Tauri、SQLite、WASAPI 都是适配器。

### 6.2 传输无关的协议

`AudioCommand`/`AudioEvent` 定义在 `aiya-core`/`aiya-audio-api`，不依赖 Tauri、不依赖管道。sidecar 只是“把同一组类型序列化到管道”。这样：

- 单元测试可以直接调用；
- 以后换 in-process 线程，不改协议；
- 以后加远程音频（另一台机器）也不用改领域层。

### 6.3 单一数据库写入者

只有 host 打开 SQLite；sidecar 只收 `path`。这避免：

- 两个进程各自迁移 schema；
- WAL 锁竞争；
- 音频进程崩溃时事务状态不明；
- 数据层被音频平台细节污染。

### 6.4 契约以 Rust 为源

```
Rust structs → specta → ui/src/bindings.ts
                     ↘ contracts/schema.json + hash
```

- 前端不手写同名 interface。
- CI 检查生成物 diff；协议变更必须带版本/迁移说明。
- 事件/命令都有 `seq`/`revision`，避免“最后一条覆盖新状态”。

### 6.5 UI 不拥有播放状态

- 所有播放动作走 `player_command`；返回 snapshot。
- 不乐观修改 `status`；等 `started` 事件。
- 拖动 seek 时本地显示 `dragMs`，松手发命令，收到 snapshot 后清 pending。
- 队列面板用 `queue_slice`，不缓存全队列；拖动只发 `Move{from,to}`。

### 6.6 错误分层

| 层 | 错误类型 | UI 看到 |
|---|---|---|
| WASAPI/解码 | `AudioError`（HRESULT、device、format） | `AppError.code` + details |
| 数据 | `DataError`（SQLite code、constraint） | `db` / `not_found` |
| 应用 | `AppError`（业务语义） | 文案 + retryable |
| Tauri | 序列化/权限 | `internal` / `forbidden` |

不要让 HRESULT 直接出现在 toast；也不要把业务错误压成 `anyhow!("failed")`。

## 7. 关于“音频 WS”

如果“ws”指 **WebSocket 音频通道**，结论是：**不要用它传 PCM，也不要用它做播放主链路。**

| 方案 | 问题 |
|---|---|
| WebSocket 传 PCM 到前端播放 | 额外拷贝/编码、抖动、无设备时钟、无法独占；延迟与爆音不可控 |
| WebSocket 传 PCM 到 sidecar | 本机多一跳；命名管道/共享内存更简单；仍要解决设备时钟 |
| WebSocket 做控制面 | 可以，但只应是远程控制/插件接口，且 terminate 在 host，不要让 UI 直连 |
| WebSocket 做频谱 | 30fps 小帧可以，但本机没必要；Tauri Channel/命名管道足够 |

**规则：PCM 只在本机音频进程内部流动；跨进程只传命令、状态、band 能量。** 如果确实需要网络音频（DLNA/远程播放），单独设计 `RemoteAudioTransport`，不要污染本地播放链路。

## 8. 边界检查清单（PR 用）

- [ ] 这个改动是否让 `aiya-core` 依赖了 OS/Tauri/DB？
- [ ] 是否在前端创建了 `AudioContext`/`<audio>`/`MediaElementSource`？
- [ ] 是否把 PCM/大数组放进了 JSON event？
- [ ] 是否让 sidecar 打开了 SQLite？
- [ ] 是否在 UI 里复制了“下一首是谁/队列索引”的判断？
- [ ] 新错误是否有 `code` 和可诊断 details？
- [ ] 新事件是否有 `seq`/`revision`，UI 是否会丢弃乱序？
- [ ] 是否新增了第二条音频路径/兜底？
- [ ] 契约生成物是否更新？协议版本是否兼容？
- [ ] 这个功能在窗口关闭/重载后是否仍然正确？

## 9. 一句话

> **后端拥有“正在发生什么”，前端拥有“看起来怎样”。**
> 任何让前端知道文件路径、设备句柄、PCM 缓冲的设计，都是把旧问题换个地方重写。