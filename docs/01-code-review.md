# 01 — 代码评审：值得保留的、主要的、不能照搬的

> 现版约 10,000 行 TS（`apps/desktop` 3,246 行 main + 132 行 preload + 5,584 行 renderer，`packages` 约 3,800 行；不含测试）。
> 结论先行：**领域建模、协议语义、测试与文档质量高；失败集中在“音频出口放在 WebView”与“主进程 JS 承担了本该由原生层承担的状态”。**

## 1. 写得最好的十段代码

### 1.1 音频出口被收敛成一个接口

`apps/desktop/src/main/player-host.ts:37-56`

```ts
export interface AudioSink {
  apply(command: PlayerCommand): void;
}

function createRendererSink(broadcast): AudioSink {
  return { apply(command) { broadcast(CH.playerCommand, command); } };
}
```

这是现版最重要的架构资产。播放状态机不关心 `<audio>`、文件路径、解码器；换出口只换装配。**新版保留这个思想，但把实现从“广播到渲染层”换成“写进音频 sidecar”。**

### 1.2 `load` 指令只带 `trackId`，不带 URL

`packages/contracts/src/player.ts:45-75`

```ts
export type PlayerCommand =
  | { kind: 'load'; token: number; trackId: string; startMs: number; autoplay: boolean }
  | { kind: 'play'; token: number }
  ...
```

`media://` 被锁在 `audio-sink.ts` 一处。这是“换解码器不改协议/状态机/测试”的前提，也是新版 `AudioCommand` 的直接原型。新版可以进一步去掉 `token`（由后端 generation 统一），但**“出口自己决定怎么把 trackId 变成声音”这条不能丢**。

### 1.3 状态里不放队列

`packages/contracts/src/player.ts:11-43`

```ts
export interface PlayerState {
  queueLength: number;
  queueVersion: number;
  index: number;
  ...
  current: Track | null;
}
```

理由写在注释里：5 万 ID 每秒两次广播等于每秒序列化 2MB。新版应保留“状态快照 + 队列按版本/分片拉取”的分离，但队列分片改成 `queue_slice(offset, limit)`，连 2 万 ID 的一次性快照也不要传。

### 1.4 过期回报用令牌丢弃，而不是计时器猜测

`packages/playback/src/player.ts:446-486`

```ts
report(report: PlayerReport): PlayerCommand[] {
  if (report.token !== this.tokenValue) {
    if (report.kind === 'time' || report.kind === 'duration') return [];
    ...
  }
  ...
}
```

快速切歌时，旧 `<audio>` 的 `ended`/`error` 可能晚到。令牌让“谁属于当前装载”成为可证明的事实，而不是 200ms 防抖。新版用 `generation: u64` 原样保留，并让音频 sidecar 的每条事件都带 generation。

### 1.5 随机播放是排列，不是“每次随机取一首”

`packages/playback/src/queue.ts:53-90`

```ts
function shuffled(indices, random) { /* Fisher–Yates */ }

export function setShuffle(queue, on, random = Math.random) {
  ...
  return { ...queue, shuffle: true, order: [queue.index, ...shuffled(rest, random)] };
}
```

一轮不重不漏、切随机不打断当前曲目、`cursor()` 可显示“12 / 40”，都能被单测证明。新版用 Rust `Vec<usize>` 重写，测试用例直接翻译。

### 1.6 队列编辑时同步重映射随机排列

`packages/playback/src/queue.ts:183-211`

```ts
const order = queue.order
  ? queue.order.map((position) => shiftIndex(position, from, to))
  : null;
```

只保证 `order` 是“合法排列”不够——它可能仍指向错误的歌，随机播放会漏歌/重复。这段注释和 `shiftIndex()` 是现版最容易被忽略、也最值得带走的正确性细节。

### 1.7 分页同时支持 cursor 与物化 offset

`packages/database/src/tracks.repo.ts:237-323` + `packages/database/src/order.ts`

- cursor 用于顺序翻页（0.5ms）。
- offset 用于滚动条随机跳转（2.3ms），走 `library_order` 物化位置表，不用裸 `LIMIT/OFFSET`。
- `mode` 会回传给 UI，调用方知道这次走的是哪条路。

新版继续用 SQLite，这段 SQL 与 `library_order` 的 `WITHOUT ROWID` 设计可以原样移植；这是 5 万首体验的地基。

### 1.8 点行播放用“整库 ID”而不是当前窗口

`apps/desktop/src/main/player-host.ts:223-236` + `packages/database/src/tracks.repo.ts:344-363`

```ts
const ids = listAllIds(db, query);
return respond(controller.playTracks(ids, start));
```

列表只持有 2,400 行窗口；用窗口建队列会“播完就停”。整库取 5 万 ID 实测 24.7ms。新版把这个决策留在 `PlayerActor::play_query()`，但队列持久化改成规范化表，避免 5 万 ID 的 JSON 每 5 秒重写。

### 1.9 Range / 流关闭 / CORS 三个坑都有可复用实现

`packages/media/src/protocol.ts:131-193`、`195-254`；`packages/storage/src/range.ts:18-48`

- `stream: true` 缺了会让 30 分钟文件整曲进内存，`buffered` 谎报全缓冲。
- `finish()` 同步计数 + `close` 幂等兜底，解决“取消后立即断言读到滞后计数”。
- 建 Web Audio 图后跨源媒体会整体静音，必须 `ACAO: *` + `crossOrigin='anonymous'`。

新版音频不再走 WebView，**Range/流计数可以删**；但封面协议仍可复用“按 ID 解析 + 不可变缓存”的思路。CORS 教训保留为“自定义协议也要正确响应头”的 checklist。

### 1.10 播放结算、歌词解析、歌词时钟：三个纯函数范本

- `apps/desktop/src/main/play-log.ts:38-80`：完成/跳过/不足 3 秒三档口径，且注释解释了为什么 `recordPlay` 与 `recordSkip` 不能混。
- `packages/library/src/lrc.ts:38-105`：多时间戳、offset、行内时间戳、纯文本降级；`116-132` 先 UTF-8 严格解码再退 GB18030，顺序不能反。
- `apps/desktop/src/renderer/src/lib/lyrics-clock.ts:20-32`：500ms 上报 + rAF 外推，误差压到一帧；这是“后端低频上报、前端高频渲染”的通用模式，新版进度条也应采用。

## 2. 其他主要代码段（按职责）

| 职责 | 现版位置 | 评价 / 新版处理 |
|---|---|---|
| 播放状态机 | `packages/playback/src/player.ts`（536 行） | 结构清晰、方法返回命令数组、易测；**整体迁 Rust**，测试当规格 |
| 队列纯模型 | `packages/playback/src/queue.ts`（217 行） | 保留语义；Rust 重写 |
| 播放宿主/装配 | `apps/desktop/src/main/player-host.ts`（369 行） | `AudioSink` 好；启动补发/多窗口广播是 WebView 时代的补丁，新版删除 |
| IPC 边界 | `apps/desktop/src/main/ipc.ts`（513 行） | 信封/错误映射好；但与 Electron `ipcMain` 绑死，迁 Tauri commands |
| 数据库 schema/迁移 | `packages/database/src/migrations.ts`（271 行） | `user_version` runner、追加式迁移、`WITHOUT ROWID` 都好；SQL 原样移植 |
| 查询/分页 | `packages/database/src/tracks.repo.ts`（589 行） | 核心资产；注意 `SELECT` 子查询与 `audio` 单曲填充策略 |
| 播放列表 | `packages/database/src/playlists.repo.ts`（298 行） | 事务内全量重排 position；规模几千首，简单正确 |
| 扫描 | `packages/library/src/scanner.ts`（509 行） | 两阶段、并发池、超时、单文件隔离；Rust 用 `walkdir` + `lofty` 重写 |
| 扫描调度 | `apps/desktop/src/main/services/library.ts:149-197` | dirty 补跑合并风暴；Rust 保留 |
| 封面缓存 | `packages/media/src/artwork-cache.ts`（121 行） | 并发生成去重、临时文件 rename、尺寸白名单；Rust 重写 |
| 播放历史 | `packages/database/src/history.repo.ts`（131 行） | 写路径与统计口径清楚；Rust 保留 |
| 设置清洗 | `apps/desktop/src/main/app-settings.ts`（114 行） | 默认值 + 白名单；Rust serde 后只保留一份 |
| 快捷键 | `packages/contracts/src/shortcuts.ts`（160 行） | 纯函数、可测；前端保留 |
| 媒体会话 | `renderer/src/lib/media-session.ts`（175 行） | 封面 blob、generation、metadataKey 防抖都值得学；**Tauri 下整体迁 Rust SMTC** |
| 诊断 | `main/ipc.ts:427-468`；`SettingsPanel.svelte:256-353` | 指标选得好；新版改为 Rust 结构化 metrics |
| UI 纯逻辑 | `renderer/src/lib/{virtual,selection,track-window,drag,list-keys,spectrum}.ts` | 与框架无关、有测试；**原样保留** |
| 渲染层状态 | `renderer/src/stores/*.svelte.ts` | 已从 App 抽出；新版继续拆，App 目标 < 400 行 |

## 3. 不能照搬的八件事

### 3.1 渲染层 `<audio>` 作为唯一出口

`audio-sink.ts` 本身写得不错（注释、错误映射、速率重放、图生命周期），但它把“播放”绑在 WebView 生命周期上：

- 窗口重载/关闭，声音就没了；要靠 `did-finish-load` 补发 load。
- 换 `src` 会把 `playbackRate` 重置为 1，必须手动重放。
- 建 Web Audio 图后，跨源媒体静音，必须补 CORS。
- 无法做独占、按源采样率、bit-perfect、设备枚举、DSD/EQ。

**新版删除整个文件，不保留兼容层。** 唯一保留的是它对 `PlayerCommand` 的翻译语义（作为 Rust `AudioCommand` 的规格）。

### 3.2 主进程“补发 load”与多窗口广播

`player-host.ts:126-155` 的 `did-finish-load` 补发是 WebView 时代的必要补丁，不是架构。新版音频常驻 sidecar，UI 重载只需重新拉状态；不存在“窗口没加载好所以没声”。

### 3.3 队列 JSON 整行每 5 秒重写

`queue.repo.ts:67-94` 每次 `savePlayer` 都 `JSON.stringify(trackIds)` 并更新整行。20,000 首约 400KB/5s，5 万首更糟。新版：

- `play_queue_items(queue_id, position, track_id)` 规范化；
- `play_queue_state` 只存 index/repeat/shuffle/volume/muted/rate；
- 进度单独节流写，队列结构变化才写 items。

### 3.4 两套手写契约

现版 `contracts` 是 TS 单一来源，主/渲染共用，做得好；但迁 Tauri 后 Rust 是后端，继续手写 TS 会产生漂移。新版以 Rust 为源生成 TS，并加 schema hash/golden fixtures。

### 3.5 `window.api` 分散调用

现版已收敛到 stores + `main.ts`，但 `App.svelte` 仍有 1,100 行、对话框/菜单编排都在里面。新版从第一天就按 feature 拆，不把 `App.svelte` 当路由器 + 状态仓库 + 对话框管理器。

### 3.6 字符串错误

`toFailure()` 用 `message.startsWith('E_INVALID_CURSOR')` 映射错误码。新版全链路 `AppError { code, message, details, retryable }`，音频 sidecar 也返回结构化错误；不要在 UI 里匹配文案。

### 3.7 把日志/诊断当附属品

现版诊断页是亮点，但音频没有“设备/格式/缓冲/欠载/独占结果”这些原生指标。新版从第一版就把 `AudioStats` 放进协议：device、mode、sample rate、bits、period、buffer、underruns、exclusive、fallback reason。

### 3.8 Pro 频谱依赖 Web Audio 节点

`audiomotion.ts` 依赖 `AnalyserNode`（`audioMotion.connectInput(audioNode)`），与“音频在 Rust”冲突。新版：

- 后端在输出前 tap PCM，算 48 个对数分带，30fps 推给前端；
- 前端只做 canvas 绘制（compact 自研，pro 可后续做，不必绑 AGPL 引擎）；
- 如果一定要 audioMotion，需要重新评估其输入 API，不能为了它把音频图搬回 WebView。

## 4. 现版最值得带走的“非代码”资产

1. **255 个测试**：尤其 `player.test.ts`、`queue.test.ts`、`shuffle.test.ts`、`reorder.test.ts`、`protocol.test.ts`、`play-log.test.ts`。它们是重写的规格，不是包袱。
2. **ADR 与实测数据**：`docs/02-技术决策记录.md`、`03`、`04`、`10`；独占缓冲对齐、事件频率、格式矩阵等坑位直接复用。
3. **bench baseline**：5 万首冷启动/深分页/搜索口径，重写后按同一口径验收。
4. **fixtures**：`fixtures/` 与 `scripts/seed.mjs` 生成 5 万首合成库；新版 CI 直接复用。
5. **smoke/soak 思路**：扫描→入库→206→点击真实行→出声→seek→暂停→落库恢复；新版改成“扫描→入库→原生引擎出声→seek→重启恢复”。

## 5. 代码规模与重写判断

| 区域 | 现版规模（约） | 判断 |
|---|---:|---|
| `contracts` | 1,150 行 | 语义保留，Rust 重写 + 生成 TS |
| `playback` | 750 行 | Rust 重写，测试翻译 |
| `database` | 1,500 行 | SQL/schema 复用，Repository 重写 |
| `library` | 730 行 | Rust 重写（lofty/walkdir/image） |
| `media` / `storage` | 630 行 | 音频协议删除；封面缓存重写 |
| `jobs` | 60 行 | Rust 持久化队列重写 |
| `main` | 3,250 行 | Tauri host 重写，系统集成迁 Rust |
| `preload` | 130 行 | 删除 |
| `renderer` | 5,580 行 | 大部分保留；API 层、App 拆分、音频图删除 |

**结论：不是“全重写”。** UI 纯逻辑、视觉、交互、测试规格、数据库设计、ADRs 都能复用；真正重写的是“主进程 + 音频 + 文件/媒体协议”。把重写范围说清楚，才不会把 5,000 行 UI 也推倒。