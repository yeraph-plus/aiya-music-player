# 17 · 前端重做的契约与错误模型（2026-10-25）

> 触发问题：前端要整体重做，从哪儿开始？需不需要 zod 之类的接口约定方案？
>
> 本文**只定约定，不写实现**。所有数字都来自当前代码实测（`src-tauri/src/lib.rs`、
> `ui/contracts`、`ui/compat`），不是估计。

---

## 0. 四条决定（先看这个）

| # | 决定 | 一句话理由 |
|---|---|---|
| 1 | 新命令返回 `Result<T, ApiError>`，**不用** `IpcResult` 信封 | 现在错误有两条通道（Tauri reject + `{ok:false}`），UI 每处判两次；信封还逼着返回 `Value`，typed struct 永远进不来 |
| 2 | 类型从 Rust **生成**（`ts-rs`），TS 侧不再手写 domain 类型 | 1301 行手写契约纯人工同步，已出过两次事故 |
| 3 | zod 只用于 **3 个真·动态边界** + dev 采样校验 | 热路径（10Hz / 30fps）过校验会直接崩性能；类型该交给生成而不是校验 |
| 4 | 事件分两类：**快照型**与**增量型**，高频事件 UI 侧必须合帧 | `player:state` 10Hz、`spectrum:frame` 30fps，直接驱动渲染会抖 |

信封仅在老 `compat/` 壳的存活期内保留，**不进新 UI**。

---

## 1. 现状盘点

### 1.1 数字

| 项 | 实测 |
|---|---|
| 注册命令 | **54** 个 |
| 返回 typed 的命令 | **0** 个 —— 54/54 都返回 `Value`（`{ok,data}` / `{ok:false,error}` 手拼） |
| `json!` 手拼处 | `src-tauri/src` 共 48 处（lib.rs 37 + beets.rs 等 11）；`-> Value` 出现 62 次 |
| 参数已强类型 | **48/54**（只有 6 个走 `Value`：`app_set_settings`、`app_set_ui_state`、`library_list`、`player_play_query`、`library_ids_for_selection`、`player_report`） |
| TS 契约 | `ui/contracts` 1301 行 / 7 文件，**纯类型零校验** |
| zod | `zod ^4.6.7` 在 deps，**0 处 import** |
| 事件 | 5 条（下节），其中 2 条手拼 JSON |

**关键结论**：债不在参数（多数已强类型），债全在**返回**。所以收口成本比想象的
低——不需要一次改 54 个签名，只需要给"UI 要用的返回值"定义 DTO。

### 1.2 已被这两处债咬过

1. **`TrackWindowQuery` 嵌套形状**：Rust 侧参数结构与 TS 侧不一致，查询 silently 返回错数据（ddaf83d 才修）。
2. **`exclusiveMode: bool` → `output: {...}`**：设置项形状变了，靠人肉同步。

### 1.3 顺手发现的一处现存契约 bug

`crates/aiya-player/src/state.rs:55` 注释写「path … 不出现在对外 DTO」，但字段是
`#[serde(default)] pub path: String`，**没有 skip**——`PlayerState.current` 是
`TrackRef` 直接序列化下发的。也就是说 UI 现在就能从 `player:state` 里拿到绝对路径：

```rust
/// 引擎装载用；不出现在对外 DTO（DTO 侧由 trackId 查询）
#[serde(default)]
pub path: String,
```

实测 UI 侧没有任何地方读它（`grep .path ui/src/stores/player.svelte.ts` 为空）。
修法二选一：`#[serde(skip_serializing)]`，或新 DTO 把 `path` 拆出去（见 §3）。
**注释与代码不一致本身就是契约债**，记在这里，随 B1 一起修。

---

## 2. 错误模型

### 2.1 形状

```rust
#[derive(Debug, Clone, serde::Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct ApiError {
    pub code: ApiErrorCode,
    /// 面向用户的一句话（可直接显示）
    pub message: String,
    /// 面向上帝的技术细节（日志/诊断页）；默认不下发到 UI 文案
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    /// UI 是否值得给"重试"：设备断开、扫描冲突 = true；参数错 = false
    pub retryable: bool,
}

#[derive(Debug, Clone, Copy, serde::Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub enum ApiErrorCode {
    Internal, NotFound, InvalidArg, Busy, Io,
    BeetsMissing, Unsupported, DeviceLost,
}
```

TS 侧由生成器产出，等价于：

```ts
export type ApiErrorCode = 'internal' | 'notFound' | 'invalidArg' | 'busy'
  | 'io' | 'beetsMissing' | 'unsupported' | 'deviceLost';
export interface ApiError {
  code: ApiErrorCode; message: string; detail?: string; retryable: boolean;
}
```

### 2.2 旧码 → 新码映射（现有 9 个码，实测出现次数）

| 旧码 | 次数 | 新码 | 备注 |
|---|---|---|---|
| `E_INTERNAL` | 26 | `internal` | 现在是个大口袋，其中"DB 锁/文件被占"应拆 `io` |
| `E_NOT_FOUND` | 5 | `notFound` | |
| `E_BEETS_NOT_FOUND` | 2 | `beetsMissing` | UI 要区分"没装 beets"和"找不到曲子" |
| `E_INVALID_ARG` / `E_BAD_QUERY` / `E_INVALID` | 5 | `invalidArg` | 三个同义码合并 |
| `E_BAD_NAME` | 2 | `invalidArg` | 带 `detail` 说明规则 |
| `E_BAD_PATH` | 1 | `invalidArg` | |
| `E_SCAN_BUSY` | 1 | `busy` | `retryable = true` |

`E_INTERNAL` 占 26/41（63%）说明现有错误分类几乎没信息量——这是重做时值得拆的主要收益之一。

### 2.3 传输语义

- Rust 返回 `Result<T, ApiError>` → Tauri 把 `Err` 序列化后作为 **Promise reject 的值**。
  注意它**不是** `Error` 实例（没有 `stack`、`instanceof Error` 为假），所以新 UI 需要一个归一化：

```ts
// src/lib/ipc/errors.ts
export function toApiError(e: unknown): ApiError {
  if (e && typeof e === 'object' && 'code' in e) return e as ApiError;
  return { code: 'internal', message: String(e ?? '未知错误'), retryable: false };
}
```

- 不再有 `{ok:false}`。老 `compat/api.ts` 在**唯一一处**把 `Result` 包回 `{ok,data}`
  形状给旧组件用，这层随旧 UI 一起删除。

### 2.4 播放错误必须结构化（会动事件契约）

现状 `PlayerState.error: Option<String>` 只有一句话，UI 无法决定"该不该自动跳过/该不该提示"。
建议升级：

```rust
pub struct AudioError { pub code: AudioErrorCode, pub message: String }
// AudioErrorCode: fileMissing | decode | deviceLost | unsupportedFormat | sidecarDown
```

代价：动 `PlayerState`（`player:state` 事件）与 SMTC 通知路径，属于**独立任务**，
不与 UI 重做混在一个提交里。但必须在写播放条之前定，否则 UI 会把 `error: string`
的假设写死。

### 2.5 UI 处理约定

| code | 呈现方式 | 例子 |
|---|---|---|
| `invalidArg` | 内联（表单下方），不 toast | 歌单名字为空 |
| `notFound` / `beetsMissing` | toast + 可操作按钮（去设置/去安装） | 点开已删除的曲子 |
| `busy` | toast，`retryable` → 提供"稍后重试" | 扫描进行中又点扫描 |
| `io` | toast + 诊断页入口 | 文件被占用 |
| `deviceLost` | 顶部持久横幅（不是 toast） | 拔了 USB 声卡 |
| `internal` | toast「出错了」+ 详情进诊断页 | 兜底 |

原则：**能不打断就不打断**。toast 只用于"用户动作失败"，后台失败（扫描、封面、beets 同步）走状态提示。

---

## 3. DTO 约定

| 项 | 约定 | 现状对照 |
|---|---|---|
| 字段命名 | Rust `snake_case` + `#[serde(rename_all = "camelCase")]` | 已是如此（`PlayerState`/`TrackRef` 都对） |
| 枚举 | `#[serde(rename_all = "lowercase")]`（或 camelCase） | `RepeatMode`/`PlaybackStatus` 已 lowercase |
| 命令名 | 保持 snake_case（Tauri 现状） | `library_list` 等 |
| 时间 | 一律**毫秒** `*Ms: number`（epoch ms 或时长 ms），**不出现裸 `duration`** | 现状混用：`duration_ms` 毫秒，但 `added_at` 是 **UTC unix 秒**（`migrations/0001_init.sql:43` 注释明写），`last_played_at` 同为秒（`now_secs()`）。TS 侧 `addedAt: number` / `lastPlayedAt: number \| null` **没有任何单位标注** |
| 大整数 | JS number 安全区，`u64` 的 `version/token` 实际远小于 2^53，可直传 | 现状 `queueVersion: u64` 直传 |
| id | 一律 `string`（不透明），**不要** number | 现状一致 |
| 缺值 | `null`（`Option` → `null`），**禁止 -1 哨兵** | `PlayerState.index/cursor: i64 = -1` 表示空队列 ← 要改 `Option<u32>` |
| 分页 | 游标式 `{items, nextCursor: string|null, total?, tookMs}` | 现 `TrackPage{items,total,limit,mode:'cursor'\|'offset'}`，保留但去掉 `mode`（只留 cursor） |
| 列表 vs 单曲 | 列表不带 audio spec，`library_tracks` 才带 | 已有约定（`docs/03`），保留 |
| 队列 | `queueVersion` 单调递增，UI 只在变化时重拉 | 已落地（`player.svelte.ts`） |

**示例：`library_list` 迁移前后**

```rust
// 之前：参数、返回都是 Value，错误靠 fail("E_BAD_QUERY")
fn library_list(state: State<'_, AppState>, query: Value) -> Value { ... }

// 之后：参数沿用已有强类型 TrackWindowQuery，返回 DTO
#[tauri::command]
fn library_list(
    state: State<'_, AppState>,
    query: TrackWindowQuery,
) -> Result<TrackPage, ApiError> { ... }

#[derive(serde::Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct TrackPage {
    pub items: Vec<TrackDto>,
    pub next_cursor: Option<String>,
    pub total: i64,
    pub took_ms: u32,
}
```

注意参数从 `Value` 改成 `TrackWindowQuery` 的**副作用**：形状不对时 Tauri 自己会 reject
（错误码是 Tauri 的，不是我们的）。所以需要一个 `#[serde(default)]` + 校验的中间层，
或保留 `Value` 参数在 lib.rs 里 `from_value` 后转 `ApiError::invalid_arg`。
**建议保留 Value 参数 + 立即反序列化**（现状做法是对的，只是错误码要换），
理由：错误分类权留在我们手里，不被框架文案污染。

### 3.1 时间单位的实例（这条现在就会咬人）

`Track` 同一个对象里混了两种时间单位：

| 字段 | 单位 | 证据 |
|---|---|---|
| `durationMs` | 毫秒 | `scanner.rs`: `props.duration().as_millis()` |
| `addedAt` | **UTC unix 秒** | `migrations/0001_init.sql:43`：`added_at INTEGER NOT NULL, -- UTC unix 秒` |
| `lastPlayedAt` | **秒** | `lib.rs:1570`：`let played_at = now_secs();` |

TS 侧只有 `addedAt: number;`（`contracts/src/domain.ts:32`），**没有单位标注**。
后果很具体：新 UI 做「添加于」列时写 `new Date(track.addedAt)` 会得到 1970 年，
写 `* 1000` 才对——而这种"对不对"目前只能靠读 SQL 注释发现。

约定：**DTO 一律毫秒**，`addedAt`/`lastPlayedAt` 在 DTO 层 `*1000` 后改名
`addedAtMs` / `lastPlayedAtMs`。DB 仍存秒（排序走 SQL `t.added_at`，不动 schema，
不动索引）。改名带单位是刻意的——**名字里没有单位的时间字段迟早出错**。

---

## 4. 事件契约

| 事件 | 载荷 | 频率 | 丢帧 | 订阅条件 | 现状 |
|---|---|---|---|---|---|
| `player:state` | `PlayerState`（typed） | ~10 Hz（节流） | 可丢 | 常驻 | ✓ 已消费 |
| `spectrum:frame` | `Vec<u8>`（48 带，0-255） | 30 fps | 可丢 | 仅面板打开时 `spectrum_set(true)` | ✓ 已消费 |
| `scan:progress` | 扫描进度（typed，宿主已 100ms 节流） | ≤10 Hz | 可丢 | 扫描期间 | ✓ |
| `beets:import` | **union 4 型**（`sessionEnd` / `librarySync*` / Python 侧透传） | 不定 | 终态不可丢 | 导入会话 | ✓ |
| `audio:mode` | **手拼 JSON**（requested/mode/clock/bufferMs/format/reason，无 token） | 偶发 | 不可丢 | 常驻 | ✗ **无人消费** |

规则：

1. **快照型**事件必须自带 `version`/`token`（`player:state` 有 `queueVersion`/`token`，
   过期直接丢——ADR-0003 已定）。
2. **增量型**（`beets:import`）必须有终态（`sessionEnd` / `*Failed`），UI 才能收尾会话。
3. 订阅统一在 store 层，**组件不直接 `listen`**（现在 `compat/api.ts` 已经这么做了，保留）。
4. `audio:mode` 要补 `token` 并改为 typed struct（现在手拼、且无人消费——正好趁重做补齐）。
5. 高频事件在 store 层合帧：`player:state` 只留最后一帧 + 用 rAF 批量刷新的字段
   （进度条），而不是每个事件都触发一次 Svelte 更新。

---

## 5. 迁移清单（54 命令分三批）

**B1 · 首屏必需（12）** —— 第一批做，做完就有了新壳能跑起来的最小集
```
app_info  app_get_settings  app_set_settings  app_diagnostics
app_get_ui_state  app_set_ui_state
library_list  library_tracks  library_stats
player_state  player_queue  spectrum_set
```

**B2 · 播放与队列（13）** —— 播放条 + 队列面板
```
player_play_tracks  player_play_query  player_toggle  player_next  player_prev
player_seek  player_set_volume  player_toggle_mute  player_set_rate
player_set_repeat  player_set_shuffle  player_report  lyrics_get
```

**B3 · 其余（29）** —— 按页面推进时逐个收口，不预先改
```
playlist_*（10）：list detail create rename delete reorder add_tracks remove_at move clear
beets_*（5）：status preview apply cancel sync
library_*（6）：scan scan_cancel choose_root reveal_track ids_for_selection remove_tracks
player_*（5）：enqueue insert_next remove_at move clear
history_*（2）：stats clear
app_open_data_dir（1）
```

迁移状态用一个显式清单维护（`docs/17` 的勾选表或代码里的 `// TODO(ipc-v2)` 标记 +
`grep` 计数），**不允许"感觉都迁完了"**。

---

## 6. 校验与漂移防线

三道，成本从低到高：

1. **类型生成**：`ts-rs` → `ui/src/lib/ipc/generated/`；生成物**入库**，
   `npm run gen:types && git diff --exit-code` 挂进 `verify`。改 Rust 不改生成物 = CI 红。
2. **Rust 侧快照测试**：每个新 DTO 加一条 `serde_json::to_value(...)` 字面量比对
   （与 `proto.rs` 的 `wire_shapes_are_stable` 同风格）。这样"字段名/嵌套形状"两端都锁死。
3. **dev 运行期校验**（zod，只在开发态）：
   - 对象：6 个 `Value` 参数命令 + 3 个动态边界（`settings`、`beets:import`、`scan:progress`）
   - 触发：`import.meta.env.DEV` 且 `VITE_IPC_VALIDATE=1`；每个通道**只校验前 20 次**，
     之后关掉（避免热路径开销）
   - 失败：`console.error` 带字段路径 + 一次性 toast，不抛（开发期不想被校验挡住流程）

**热路径不校验**：`player:state`（10 Hz）、`spectrum:frame`（30 fps）永不进 zod。
它们靠生成的静态类型 + Rust 快照测试保证。

---

## 7. 明确不做

1. **不做 zod 全量**（54 个 schema）——与 ts-rs 重复劳动，改一处要改两处，且没有任何收益。
2. **不把信封带进新 UI**——只在 `compat/` 存活期内存在。
3. **不引 router**——桌面播放器不需要 URL；沿用 view 状态机（现 `lib/view.ts` 思路）。
4. **不自建 schema 语言 / 不上 gRPC / 不搞 OpenAPI**——Rust 是唯一源（ADR-0004）。
5. **不在这一批做 `player:state` 的结构化错误**——它是独立任务（§2.4），但必须在写播放条前落地。

---

## 8. 待拍板（3 项，附我的建议）

| # | 问题 | 选项 | 建议 |
|---|---|---|---|
| 1 | `PlayerState.error` 是否升级为 `AudioError{code,message}` | 现在改 / 等 UI 稳定 | **现在改**。晚改等于 UI 先按 string 写完再返工 |
| 2 | 命令名是否 camelCase（`libraryList`） | 保持 snake / 全改 | **保持 snake**。Tauri 生态默认如此，改只是风格收益，但会牵动 54 个命令 + 所有日志 |
| 3 | `ts-rs` 生成物是否入库 | 入库 + diff 检查 / 构建时生成 | **入库**。CI 能发现"改了 Rust 忘了更新"，且 clone 后无需先跑 codegen 就能类型检查 |

---

## 9. 落地状态（2026-10-25 第一批已实施）

### 9.1 已完成

| 项 | 落点 | 证据 |
|---|---|---|
| 类型生成管道 | `tools/gen-types.mjs` + `npm run gen:types` / `gen:types:check`；生成物 23 个类型在 `ui/src/lib/ipc/generated/` | `gen:types:check` 已实际抓到过一次"改了 Rust 忘记重新生成"（TrackRef.ts 变更） |
| `ApiError` 错误模型 | `src-tauri/src/dto.rs`，8 个分类 | `tests/dto_wire.rs::api_error_wire_shape` 断言键集合 + camelCase 码 + `detail` 恒为 null |
| 播放错误结构化 | `aiya_player::{AudioError, AudioErrorCode}`；`PlayerState.error` 从 `Option<String>` 换成 `Option<AudioError>` | 分类在**引擎侧**由 `AudioError::kind()` 决定，随协议 v5 的 `kind` 字段传到宿主，宿主不解析文案 |
| 协议 v5 | `EngineEvent::Error { token, kind, message }` | `proto.rs` wire 断言 `{"t":"error","token":5,"kind":"fileMissing",...}` |
| B1 十二命令 | 全部 `Result<T, ApiError>` | 键集合测试 + 契约测试（命令名/参数名对齐） |
| `TrackRef.path` 不再下发 | `#[serde(default, skip_serializing)] #[ts(skip)]` | `dto_wire.rs::player_state_error_is_structured_and_path_is_hidden` |
| 时间统一毫秒 | `TrackRow.added_at_ms` / `last_played_at_ms`，DB 仍存秒 | `dto_wire.rs::track_row_has_no_path_and_uses_ms`；`repo.rs` 的 `MostPlayed` 测试把 `Some(600)` 改成 `Some(600_000)`，顺带锁住换算 |
| 新旧契约唯一接缝 | `ui/src/compat/api.ts` 的 `tauriInvoke`：裸载荷→信封、reject→`{ok:false}` | 只在旧壳存在期间有效；旧壳删除时一并删 |
| 契约测试扩展 | `tests/ipc_contract.rs` 现在同时扫 `compat/api.ts` 与新的 `lib/ipc/client.ts` | 4 条测试通过；解析器加了 `call<T>(` 泛型剥离 |

### 9.2 与本文原计划的偏差（都写明原因）

1. **`PlayerState.index/cursor` 仍是 `-1` 哨兵**，没换成 `Option`。原因：这两个值参与 `cursor`/`order` 的全部算术（advance、shuffle、repeat），换类型等于重写状态机的下标逻辑，风险远大于收益，且与前端无关。留给"状态机整理"单独一批。
2. **`TrackPage.mode` 保留**（原计划只留 cursor）。offset 分页还在 `library_ids_for_selection` 等路径用，删字段要连带改 UI 选择逻辑。
3. **zod 只落了 `settingsSchema` / `uiStateSchema`**，`beets:import` / `scan:progress` 的 schema 推迟到对应页面接入时再写——现在没有消费者，提前写等于猜形状。
4. **`devValidate()` 只定义了函数**，还没接进 store：等新 UI 的 store 落地时在 3 个边界调用（现在没有新 store 可接）。

### 9.3 验证

```
cargo test --workspace                  178 passed / 0 failed（+1 ignored perf_50k）
cargo build --workspace --all-targets   0 warning
npm run check (svelte-check)            0 errors / 0 warnings
npm run gen:types:check                 ✓ 23 个类型与 Rust 一致
npm run smoke:output                    六项全过（真机，协议 v5 握手）
应用启动                                 无 ERROR/WARN，sidecar protocol=5
```

> 老 UI 的点击级验证没做（窗口截图被安全策略拦）。旧壳路径由「类型检查 + 契约测试 +
> wire 快照 + 干净启动」覆盖；真正的端到端确认留给新 UI（新版走 `lib/ipc/client.ts`，
> 没有适配层可错）。
