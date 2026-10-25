# 18 — 前端重做实现计划（2026-10-25）

> 前置：契约层已完工（`docs/17`，commit `3ae93db` + `70f6c22`）——
> 54/54 命令 typed，41 个类型由 Rust 生成，错误是 `ApiError` 对象。
> 本文件只讲 UI 本体怎么落地。

## 0. 一句话结论

**重写结构，不重画皮肤。** 现有 UI（16 组件 / 5 store / 1149 行 `App.svelte` /
1825 行 `app.css`）是已跑通的 demo 移植，视觉与交互验证过；要换的是**数据层与骨架**：
`window.api` 全局 + `compat/api.ts` 适配层 + 单文件 App 组织方式。

不重画的理由（不是偷懒）：
1. 现设计的 token 系统（`app.css` 顶部 40 个 CSS 变量 × 明暗两套）已经能表达全部状态，
   换皮收益是"主观好看"，成本是全组件重做 + 重新验证对比度；
2. 重构的收益是可测量的（去掉 75 处 `window.api`、删掉 compat 翻译层、拆 1149 行单文件），
   换皮的收益无法验收；
3. 视觉改造随时可以在 token 层做（改 40 个变量），不必等结构重写完。

如果之后要换视觉，改 `styles/tokens.css` 即可，不需要动组件——这正是把 token 抽出来的原因。

## 1. 现状盘点（实测）

| 项 | 数字 | 问题 |
|---|---|---|
| `App.svelte` | **1149 行** | 视图路由 + 全局快捷键 + 拖拽 + 批量操作 + 生命周期全在一个文件 |
| `app.css` | 1825 行 | 全局类名 + 组件样式混在一起，无分层 |
| 组件 | 16 个，2683 行 | 全部通过 `window.api.*` 取数，组件即数据层 |
| store | 5 个，718 行 | 每个都自己 `window.api` 调用 + 自己处理 `IpcResult` 信封 |
| `window.api` 调用点 | **75 处** | 全局单例，无法 tree-shake、无法类型收窄、测不了 |
| `compat/` 被依赖 | 5 个文件 | `api.ts`（适配）+ `mock.ts`（假后端）+ 3 个音频/封面适配 |
| `ui/contracts/` | 1301 行手写类型 | 与 Rust 生成的 41 个类型**重复**，已开始漂移（时间字段两套名字） |

## 2. 目标结构

```
ui/src/
  main.ts                 只做两件事：非 Tauri 时动态装 mock + mount（~40 行）
  App.svelte              127 行：装配 + 4 个 document 级副作用（批次 3 落地）
  styles/                 tokens / base / shell / lists / player / overlays（批次 1）
  lib/
    ipc/                  ✓ 完整（client 54 命令 / schema / events / transport / generated 41 类型）
      transport.ts        命令+事件的唯一换气口（批次 4：预览靠替换它，UI 零分支）
      mock/               浏览器预览假后端（backend 类 + 54 命令派发表，独立 chunk）
    audio-format.ts       速率夹取 + 格式徽章/规格/tooltip（批次 4 从 contracts 搬来）
    beets.ts              beets:import 事件与预览形状（手写镜像 sidecar 协议）
    query.ts              zod 查询 schema + TrackSort/TrackSource + 游标编解码
    settings.ts           AppSettings + DEFAULT_SETTINGS（与 schema.ts 分工见文件头）
    shortcuts.ts          快捷键清单/默认键位/解析匹配/冲突检测
    lyrics.ts             findActiveLine（二分）
    artwork.ts            artworkUrl()（唯一源）
    keyboard.ts           快捷键**执行层**（判断层是 list-keys.ts 的纯函数）
    boot.ts               启动引导：同步注册订阅 + 异步首屏
    focus-search.ts       搜索框聚焦的模块级注册表
    errors.ts             ApiError → 可显示文案（它不是 Error 实例）
    spectrum-source.ts    RemoteSpectrumSource（订阅 spectrum:frame）
    format/selection/view/track-window/list-keys/media-session/menu/drag ...
  stores/                 依赖可多向，但**必须无环**（详见 §3 批次 3 的判据）
    ui.svelte.ts          视图/筛选/面板/设置/歌单/歌词/诊断 + scrollTop
                          ★ 不依赖任何其他 store（被最多 store 依赖）
    library.svelte.ts     统计/扫描进度/分页取数（只依赖 ipc，最底层）
    player.svelte.ts      快照 + 队列 + 频谱源（rAF 合帧）
    selection.svelte.ts   纯内存选择集
    list.svelte.ts        查询 + 分页窗口 + reload + 播放当前列表 + 批量 + 搜索排序
    playlists.svelte.ts   歌单组合动作（CRUD → 刷新 → 视情况重载 → toast）
    scan.svelte.ts        选目录 → 扫描 → 收尾（3 个入口共用）
    history.svelte.ts     清空历史 + 连带刷新
    overlay.svelte.ts     右键菜单 / 对话框（两份菜单互斥）
    notify.svelte.ts      错误条（常驻）+ toast（2.6s）
    beets-session.svelte.ts  直连 ipc.beets.* + events.beetsImport
  views/
    LibraryView.svelte    主区（顶栏 + 视图头 + 表 + 空态），不收 prop
    （PlaylistView / HistoryView 不拆了：它们与曲库共用同一张表，
      差别只在 `view.kind` → `source`，拆开等于复制一遍 TrackList 的接线）
  components/             非视图组件；AppShell 零 prop，OverlayHost 自己读 store
```

**依赖方向铁律**：`components → stores → ipc`。组件里出现 `ipc.` 或 `window.api`
就是错的，`grep` 可查。store 之间按"依赖别人者承担编排"排布（§3 批次 3）。

## 3. 分批落地（每批可独立验证）

### 批次 1 · 地基（无可见变化，但后面全部依赖它）
- [x] `app.css` 拆成 `styles/` 分层，`app.css` 只剩 `@import`
      （实际拆成 6 个文件而非计划的 3 个，理由见 §3.1）
- [x] `lib/artwork.ts` 承接 `artworkUrl()`，`contracts/` 不再被新代码引用
      （`contracts/src/ipc.ts` 改成 re-export，不留第二份实现）
- [x] `App.svelte` 拆壳：布局（侧栏 / 主区 / 侧面板 / 批量条 / 播放条）
      移入 `components/AppShell.svelte`，`App.svelte` 只留视图切换 + 行为回调 + 全局副作用
- [x] 验收：`svelte-check` 0/0；构建产物 CSS **逐字节一致**；`probe:ui` 无异常

#### 3.1 与计划的偏差：`utilities.css` 实际没落地

计划里写的是 `tokens / base / utilities` 三层，落地时改成
`tokens / base / shell / lists / player / overlays`。原因是实测：1825 行里
**没有**一组可复用的原子类（`.btn` 只有 3 个变体、`.cover` 只有 4 档尺寸，
其余全是"某个组件的某个部件"），硬抽 utilities 只能抽出 20 行还更绕。
真正的分区边界是组件，所以按组件切：

| 文件 | 行数 | 内容 |
|---|---|---|
| `tokens.css` | 127 | `:root` / `[data-theme=light]` / 密度变量 —— 换皮只动这里 |
| `base.css` | 184 | reset、`body`、滚动条、全局动效 |
| `shell.css` | 266 | 顶栏、视图头（播放列表 / 统计） |
| `lists.css` | 234 | 曲目列表 |
| `player.css` | 555 | 队列 / 歌词面板、播放条、批量条 |
| `overlays.css` | 452 | 右键菜单、弹层、设置、提示 |

`app.css` 1825 → 18 行。

**等价性证明**（不是"看着一样"）：把 HEAD 的 `app.css` 还原构建一次、
换成分层版本再构建一次，两份 `dist/assets/index-*.css` **31845 字节逐字节相同**，
连 content hash（`index--7pPwuck.css`）都一样。
另有一层更细的检查：按"选择器 + 声明"把旧文件拆成 245 个规则块，
与新文件合集做多重集比对，245 = 245 无差异。

### 批次 2 · 数据层换血（关键一步）
- [x] `player` store 改直连 `ipc.player.*` + `events.playerState`（10 Hz 合帧）
- [x] `library` store 改直连 `ipc.library.*`（游标分页，`TrackQueryInput`）
- [x] `ui` store 的 `getSettings/setSettings` 走 `ipc.app.*` + `settingsSchema` 校验
- [x] `beets` store 承接命令（`chooseAndPreview/apply/cancel/syncNow`），`ImportPanel` 归零
- [x] `history` 暂不单独建 store（只有 `clear` 一个写操作，理由见下）
- [x] 验收：`grep -r "window.api" ui/src` 只剩 `compat/` 自身 —— **实测 0 处**

#### 2.1 与计划的偏差

**`history` store 没拆**：计划里写"playlists / history / beets 从组件里抽出来"，
但实测 `history` 只有 `clear()` 一个调用，真正的历史视图（播放次数榜）在批次 3
才落地。为一个 `clear` 建空 store 是仪式感，先留在 `library.clearHistory()`，
批次 3 接 `history.stats` 时再拆（已在代码注释里写明）。

**顺便删掉 4 个死文件**：`player` store 一旦不再用 AudioSink，
`lib/audio-sink.ts` → `compat/audio-sink.ts` → `compat/spectrum.ts` /
`lib/spectrum.ts` 就是一条闭合的死链（`AnalyserSource` 无人实例化，
WebView 里早就没有音频图）。留着不仅浪费打包体积，还会让"`window.api`
只出现在 compat"这条验收线上多一个假阳性。

#### 2.2 播放器 store 的三处结构变化

1. **`<audio>` 元素和 `AudioSink` 一起删了**。ADR-0006 之后音频在 Rust 出声，
   `<audio>` 是死挂载点、`AudioSink.apply()` 是空实现。旧代码里
   `player.init(audioEl)` 还带个 `if (audioEl)` 守卫 —— 元素一旦没绑上，
   整个状态订阅就不启动。现在 `init(hooks)` 没有可空的入参。
2. **10 Hz 事件用 rAF 合帧**：事件先写非响应式快照，rAF 刷进 `$state`。
   但 `onState` 观察者（系统媒体中心）**同步**调用，不等 rAF ——
   窗口隐藏时浏览器把 rAF 降到 0，而 OS 的媒体卡片必须立刻知道换歌了。
   UI 落后一帧没观感影响，两者需求不一致，分开处理。
3. **`alignVolume` / `setNormalization` 删除**：Rust 侧注释自己写着
   "旧 UI 侧 setNormalization 已是 no-op"，归一化由 `normalizeVolume`
   设置标志驱动（`lib.rs` 事件泵每拍读）。UI 再"对齐"一次只会制造第二条真相。

#### 2.3 错误语义：什么时候抛、什么时候降级

新契约失败即 reject（`ApiError`），所以每个调用点都要表态。本批的分工：

| 场景 | 处理 | 理由 |
|---|---|---|
| 列表 / 播放 / 歌单 CRUD / 清空历史 / 扫描 | **抛出** → 调用方写错误条 | 用户显式动作，失败必须让他看到 |
| 播放条单键命令（播放/暂停/音量/静音…） | `catch` → `console.warn` | 按不动不会崩，状态里已体现；但绝不能变 unhandled rejection |
| 统计 / 扫描进度 / 数据目录 / 诊断 / beets 探测 | 降级为空 | 侧栏统计拉不到不该挡住整个界面 |
| 歌词 | 降级为空态 | 没有 `.lrc` 是常态（纯音乐） |

#### 2.4 类型归位时挖出的两个真问题

类型对齐不是收尾工作，它是**本批最有价值的产出**——两处旧类型在说谎：

1. **`PlayerState.current` 在 `contracts` 里被声明成完整 `Track`**
   （带 `artistId` / `albumId` / `trackNo` / `addedAtMs` 等 9 个字段），
   而 Rust 实际下发的是 `TrackRef`（只有 8 个字段，且**不含 path**）。
   也就是说 `playerState.current.artistId` 一直是 `undefined`，类型却说是
   `string | null`。现在 `PlayerState` 全部改用生成类型，谎话消除。
2. **`AudioFormat.lossless` 在 Rust 侧根本不存在**
   （`crates/aiya-player/src/state.rs` 的 `TrackAudio` 没有这个字段）。
   而 contracts 把它当必填，`isHiRes()` / `audioTooltip()` 全靠它分流
   —— 于是 Tauri 版播放条的无损/有损徽章**一直是"未知"**，
   这是从 v1 重写时丢掉的功能，被类型分叉藏了一个多月。
   批次 2 先把类型改成可选（如实反映 payload），**已于同日后端补齐**，见 §2.5。

这两处都验证了 §1 表格里那句"`ui/contracts/` 与生成类型重复且已分叉"——
分叉的方向是旧类型「更乐观」，正好是最容易静默错误的那个方向。
不做这批，批次 4 删 `contracts/` 时会一次性炸掉，而那时更难分辨是重构错还是类型错。

### 2.5 后端补齐 `lossless`，顺带挖出 `bitrate` 单位 bug（批次 3 之前单独一批）

**修法：派生，不落库。** `TrackAudio::infer_lossless(codec, container)` 是纯函数，
在 DTO 出口算一次（`crates/aiya-player/src/state.rs`，两处构造点调用）。

- 不加 DB 列的理由：`lossless` 是 `codec`/`container` 的纯函数，加列等于让同一事实
  有两个来源（beets 改 codec 后列不跟着变，或者反过来），还要 schema 迁移 +
  老库回填 + 扫描器/beets 两条写路径都记得维护它。
- **不改扫描器**的理由：`container` 列存的本来就是 lofty 的 `FileType`
  （`Flac` / `Wav` / `Mpeg` / `Aac` / `Opus` / `Vorbis`），足够判掉绝大多数文件。
  唯一判不了的是 `Mp4`（ALAC 无损 / AAC 有损共用），所以返回 `None` 而不是猜。
  要分它得读 MP4 box 结构，或等 beets 的 `format` 填进 `codec`（现已覆盖）。

**必须保守**：用户在拿这个字段判断音质。误判 `true` 会把 AAC 标成
「无损 / 44.1 kHz / 16 bit」，比老实显示「未知」更糟。所以三态齐全：
codec 认得出按 codec；codec 缺失按容器；容器也说不了 `Mp4/Ogg` 就 `None`。

**顺带发现的第二个静默 bug：`bitrate` 单位是 kbps，不是 bps。**
前端 `formatAudioSpec` / `audioTooltip` 都写了 `Math.round(bitrate / 1000)`，
于是 320 kbps 的 MP3 显示成 **`0 kbps`**、48k/32bit 的 WAV 显示成 **`3 kbps`**。
权威依据：lofty 源码 `pub fn audio_bitrate(&self) -> Option<u32>` 的文档注释写
`/// Audio bitrate (kbps)`；beets 文档 §Path Formats 写
`bitrate (in kilobits per second, with units: e.g., "192kbps")`。
**后端两条写路径本来就一致，错的是前端的换算**——已改掉两处 `/1000`。

真窗口实测（`test-music/` 的 WAV，再手工把一首的 DB 行改成 MP3/320 走有损分支）：

| 分支 | `current.audio` | 徽章 | tooltip |
|---|---|---|---|
| 无损（container=`Wav`，codec 为 NULL） | `lossless: true` | `WAV`（Hi-Res 高亮） | `WAV · 48 kHz · 32 bit · 2 声道 · 无损 · 平均码率 3072 kbps` |
| 有损（codec=`MP3`, bitrate=320） | `lossless: false` | `MP3`（不高亮） | `MP3 · 44.1 kHz · 16 bit · 2 声道 · 有损 · 320 kbps` |

第二条同时证明码率换算已修（`320 kbps`，不是 `0 kbps`）。
测试侧加了三条：`infer_lossless_covers_both_write_paths`（两条写路径 + 三态）、
`audio_payload_carries_lossless`、`track_audio_wire_carries_three_state_lossless`
（第 3 条在 `tests/dto_wire.rs`，锁 wire 键集合——形状断言比修 bug 本身更重要，
这次正是因为**没有任何测试检查 payload 里有这个键**才藏了一个多月）。

### 批次 3 · 拆 `App.svelte`（**完成**，`App.svelte` 1070 → 127 行）

- [x] 视图路由改成 `views/`：`views/LibraryView.svelte`（顶栏 + 视图头 + 筛选/扫描/
      错误条 + 曲目表 + 空态），**不收 prop**，直接读 store
- [x] 快捷键从 `App.svelte` 移到 `lib/keyboard.ts`（判断/执行分两层，见下）
- [x] 拖拽/批量/右键菜单归位：批量条与菜单的**动作**进 store，`components/OverlayHost.svelte`
      收口所有浮层
- [x] 验收：`App.svelte` 127 行；真窗口 20 项交互全通（见 §8.5）

**拆法（关键：先搬状态，再搬视图）**

| 新文件 | 行数 | 承接什么 |
|---|---|---|
| `stores/list.svelte.ts` | 271 | 查询构造 + `TrackWindow` 分页 + `reload` + 播放当前列表 + 搜索防抖/排序/筛选 + 批量操作 |
| `stores/playlists.svelte.ts` | 170 | 歌单的**组合动作**（CRUD → 刷新侧栏 → 视情况重载 → toast） |
| `stores/scan.svelte.ts` | 67 | 选目录 → 扫描 → 统计/歌单/列表收尾（3 个入口共用） |
| `stores/history.svelte.ts` | 42 | 清空历史 + 连带刷新 |
| `stores/overlay.svelte.ts` | 67 | 右键菜单与对话框的开合状态（两份菜单**互斥**） |
| `stores/notify.svelte.ts` | 39 | 错误条（常驻）与 toast（2.6s）两条通道 |
| `lib/keyboard.ts` | 137 | 快捷键执行层（判断层仍是 `lib/list-keys.ts` 的纯函数） |
| `lib/boot.ts` | 68 | 启动引导：同步注册订阅 + 异步首屏 |
| `lib/focus-search.ts` | 24 | 搜索框聚焦的模块级注册表 |
| `views/LibraryView.svelte` | 174 | 主区全部标记 |
| `components/OverlayHost.svelte` | 199 | toast + 2 菜单 + 5 对话框 + 设置面板 |
| `components/AppShell.svelte` | 168 | 版面（**从 12 个 prop 降到 0 个**） |
| `App.svelte` | **127** | 装配 + 4 个 `document` 级副作用 |

**两条结构性决定（后面别回退）**

1. **`scrollTop` 从列表 store 搬到 `ui` store。** 它本来就是 `UiState` 的字段，
   而 `list → ui` 的依赖方向决定了 ui 不能 import list（会成环）。搬过去之后
   `ui.scheduleUiSave()` 不用再收一个 getter 参数，`togglePanel` 也不用——
   之前为了一个整数在每层传回调。
2. **`AppShell` 零 prop。** 批次 1 留了 12 个回调（理由是"要 App 本地状态"），
   批次 3 把状态搬进 store 后这些回调全部消失：切视图 `list.openBuiltinView`、
   面板 `ui.togglePanel`、批量 `list.batchPlay` / `playlists.addSelection`。
   判据升级为：**能在壳里用 store 实现的，一律不进 prop；壳只收 `children`。**

**编排层的归属（新增判据，批次 4 沿用）**

`stores/*` 之间可以有依赖，但**必须无环**。所以「跨 store 的组合动作」放在
**依赖别人**、而不是被依赖的那一侧：

- `library` 是最底层（只依赖 ipc）→ 扫描的编排不能放它，另建 `scan`
- `list` 依赖 ui/library/player/selection → 批量与「播放当前列表」放它
- `playlists` 依赖 list/ui → 歌单"加完歌要重载列表"放它
- `history` 依赖 list → 清空历史后重载放它

`ui` store 保持**不依赖任何其他 store**（只依赖 ipc/schema/loader）：它是被
最多 store 依赖的那个，一旦它反过来 import player 就成环。

### 批次 4 · 删旧壳（**完成**，`compat/` 与 `contracts/` 已零引用）
- [x] 删 `ui/src/compat/`（含 `mock.ts` —— 浏览器预览改用 `ipc` 的 mock 适配）
- [x] 删 `ui/contracts/`（类型全部来自 `lib/ipc/generated/`）
- [x] `ipc_contract.rs` 的双层扫描收敛成只扫 `lib/ipc/client.ts`
- [x] 验收：`gen:types:check` + `cargo test` + `svelte-check` 全绿

批次 1–3 完成前，`compat/` 与 `contracts/` **不动**——它们让旧界面继续可用，
是本轮重构的"安全网"。这也是为什么批次 4 排在最后。

#### 4.1 这一步的真实工作量：不是 `git rm`，是「把 contracts 消化掉」

`ui/contracts` 里混着三类东西，只有第一类能直接删：

| 类别 | 处理 |
|---|---|
| **与 Rust 生成物重复的类型**（`Track` / `Lyrics` / `PlaylistSummary` / `PlayerState` / `LibraryStats` / `BeetsStatus` / `Diagnostics` …） | 删，改引 `lib/ipc/generated`。`Track` 拆成 `TrackRow`（列表）与 `TrackDetail`（补 audio）/`TrackRef`（队列） |
| **纯前端逻辑**（`audio-format` / `lyrics` / `query` / `shortcuts` / `settings` / beets 事件形状） | 搬到 `lib/` 对应文件，只改入参类型 |
| **只服务旧壳的东西**（`AiyaApi` / `IpcResult`/`ok`/`fail` / `CH` 通道名 / `PlayerCommand` / `PersistedPlayer` / `trackMediaUrl`） | 删。`window.api` 与 `env.d.ts` 里的 `AiyaApi` 声明一起删 |

`vite.config.ts` / `tsconfig.json` 的 `@aiya/contracts` 别名同步移除；
`index.html` 里 `/src/compat/install.ts` 那个 `<script>` 也删掉（单一入口 `main.ts`）。

#### 4.2 mock 复活的两个设计点

1. **`transport` 注入，而不是 `if (preview)` 撒在业务里。**
   `lib/ipc/transport.ts` 是命令 + 事件的唯一出口，默认实现是 Tauri
   `invoke`/`listen`；预览时 `installMock()` 整体替换。好处是
   `client.ts` / `events.ts` / store / 组件**全都不知道**自己头顶是真宿主还是假数据，
   Tauri 包里也不留死分支。mock 只在 `main.ts` 里 `await import()`，
   构建产物里是独立 chunk（17 KB，真宿主永不加载）。
2. **派发表一对一。** 54 个命令名与 Rust 侧 snake_case 完全一致，漏一条的表现是
   "预览里某按钮报 internal"；反过来 `ipc_contract.rs` 继续守
   "注册了但 client 没入口"。两者合起来覆盖两个方向。
3. 假数据顺手补了四种音频规格（FLAC 16/44.1、MP3 320、FLAC 24/96、lossless
   未知的 M4A），让 §2.5 修好的 `lossless` + kbps 单位能在预览里肉眼验。

#### 4.3 顺手修掉的真缺陷：`spectrumEngine: 'pro'` 开出空白面板

默认设置是 `spectrumEngine: 'pro'`，而 audioMotion **必须挂真实音频节点**；
ADR-0006 之后音频在 Rust sidecar，`RemoteSpectrumSource.audioNode` 恒为
`undefined`。于是"开频谱"得到的是一个 `pro-host` 空 div —— 面板在、数据也有
（48 带 `spectrum:frame` 在推），就是**一个像素都不画**（2026-10-25 在预览与
真窗口都复现）。

修法不是改默认值（那只是把问题藏起来），而是把注释里早就写着、代码却没做的
"静默降级"落到实处：

```ts
// SpectrumPanel.svelte
const usePro = $derived(engine === 'pro' && source.audioNode !== undefined);
```

`usePro` 假 → 走 compact 的 canvas 绘制，头部把"形态切换"换成"实时"。
真机验证：默认 pro 设置下开面板，`litPixels` 从 0 → 14617（有数据在画）。
`audioNode` 真的存在时（将来的 WASAPI 回环）自动回到 pro，无需再改。

> **2026-10-25 收尾批更新**：这段降级已是历史。pro 的最终解法是**自研五形态
> canvas 渲染器**替代 audioMotion（见 §9 第 7 条），`audioNode` 判据随
> `SpectrumSource` 契约收紧一并删除，`usePro = engine === 'pro'`。
> "将来的 WASAPI 回环"也不会发生——loopback 数据仍落在 Rust 侧，
> 喂给 UI 引擎照样要传 PCM（ADR-0003）。

## 4. 关键设计决定

### 4.1 组件不取数
现在 `TrackList.svelte` 自己判 `result.ok`、自己 `setState`。改成
`<TrackList tracks={...} onplay={...} />`，取数与错误处理归 store。
好处：组件可以在 Storybook/测试里无后端渲染；store 的取数逻辑只有一份。

### 4.2 高频事件在 store 合帧，不在组件节流
`player:state` 10 Hz + `spectrum:frame` 30 fps。约定：
- `player` store 收到事件只写内部快照，用 `requestAnimationFrame` 合并成"每帧最多一次"通知；
- 频谱帧**不进 Svelte 响应式**（`SpectrumPanel` 直接读 store 暴露的 `Float32Array`/`Uint8Array`
  引用，自己 `requestAnimationFrame` 画 canvas）。
理由：把 30 fps 塞进 `$state` 会让整个组件树每秒 30 次脏检查，这是 v1 的老问题。

### 4.3 设置与 uiState 走 zod 边界
`settingsSchema` / `uiStateSchema` 已写好（`lib/ipc/schema.ts`）。store 里读设置时
`devValidate()` 一次；生产态直接信类型。**热路径不校验**（docs/17 §6）。

### 4.4 视图状态落库的时机
`ui` store 的 `scrollTop` / `sort` / `filter` 变更 → 300ms 防抖后 `ipc.app.setUiState`。
现在 `App.svelte` 每次滚动都发一次 IPC（`window.api.app.setUiState`），5 万首列表里
滚动会打出上百次调用。

### 4.5 错误呈现
`ApiError.retryable` 决定 toast 是否带"重试"。`notFound` 不弹 toast（列表项过期是正常现象，
刷新列表即可）。`busy` 提示"正在扫描，稍后自动刷新"。这套映射放 `lib/errors.ts`，
组件只调 `showError(e)`。

## 5. 验收口径

界面重构最难的是"没坏"，所以每批都要能证明：

1. `npm run check`（svelte-check）0 error / 0 warning
2. `cargo test --workspace` 全绿（契约测试含命令覆盖）
3. **逐视图手测清单**（每批跑一遍，记在 PR 描述里）：
   - 曲库：滚动 5 万首不卡；排序切三档；搜索；点行播放；右键菜单
   - 播放条：播放/暂停/上一首/下一首/拖动进度/音量/静音/变速/循环/随机
   - 队列：拖拽排序、删除当前曲、清空
   - 歌单：新建/改名/删除/拖拽排序/加歌/移除/去重计数
   - 历史：统计数字、清空、最近播放能播放
   - 设置：主题切换不闪、密度切换、输出模式切换后诊断页数字跟着变
   - 诊断：数值与设置页一致，beets 探测有结果
4. 启动日志无 ERROR/WARN，sidecar `protocol=5`
5. 窗口截图被安全策略拦（已知）→ 改用 `npm run probe:ui` 读真窗口的
   DOM、console、未捕获异常、4xx 资源（见 §5.1）；纯手感类仍需手测

### 5.1 UI 运行时探针 `probe:ui`

`tools/cdp-probe.mjs` 走 WebView2 的远程调试端口（CDP），在**真窗口**里取证，
不依赖截图权限、也不需要给应用加测试钩子：

```bash
# 真窗口
WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9222 npm run dev
npm run probe:ui                 # 结构 + console + 未捕获异常 + 4xx
npm run probe:ui -- --actions    # 追加面板开关交互序列
npm run probe:ui -- --play       # 追加"点第一行 → 读播放条"链路

# 浏览器预览（没有 Tauri 运行时，靠 lib/ipc/mock）
npm run dev:ui                                   # 另一个终端
msedge --headless=new --remote-debugging-port=9222 http://localhost:1420/
npm run probe:ui -- --play
```

判定：有未捕获异常 / 非 favicon 的 4xx / console.error → 退出码 1，可直接进 CI。

两条实现要点（都踩过）：
- 先 `Runtime.enable` 再 `Page.reload`——探针连上时页面早已加载完，不 reload
  就抓不到**挂载期**的异常，而拆壳与换数据层最容易出问题的正是挂载期。
- **选目标只认 `http(s)` 页面**：浏览器会带 `edge://`、扩展后台页等杂项，
  盲取 `targets[0]` 会连到同步确认弹窗上（实测）。多开时用 `--url=<子串>` 指定。

能证明的：DOM 形状、面板开关链路、主题/密度是否落到 `<html>`、主色是否提取、
错误条是否被误触发、有没有抛异常。
证明不了的：间距/配色等纯视觉细节（这一项由 §3.1 的"构建产物逐字节一致"兜住）。

## 6. 风险

| 风险 | 影响 | 对策 |
|---|---|---|
| `App.svelte` 里混着不成文的交互约定（拖拽、双击、键盘） | 拆完手感变了 | 批次 1 只搬不改；批次 3 每拆一块跑一次手测清单 |
| 现 `ui/contracts` 与生成类型字段名已分叉（时间字段两套） | 混用会静默拿错单位 | 新代码一律 `import from '$lib/ipc'`；批次 4 删 `contracts` 时一次性清理 |
| 30 fps 频谱进响应式 | 整体掉帧 | §4.4：canvas 自绘，不进 `$state` |
| 删 `compat/` 太早 | 半成品期间无法回退 | 批次 4 排最后，且先跑一轮完整手测 |

## 7. 不做

- 不引 router（桌面播放器不需要 URL，沿用 view 状态机）
- 不引 UI 组件库（现组件已够用，引入等于重画）
- 不做 SSR / 多窗口
- 不在这一轮做：gapless 可视化、快捷键自定义 UI（`ipc.app.setSettings` 已支持，页面后补）

## 8. 进度

| 批次 | 状态 | 验证 |
|---|---|---|
| 1 地基 | **完成** | `check` 0/0；CSS 构建产物 31845 字节逐字节一致；`probe:ui` 无异常、面板开关链路 4/4 |
| 2 数据层换血 | **完成** | `window.api` 业务代码归零；208 测试 / 0 警告；真窗口走通设置读写 + 事件链路（见 §8.3） |
| 2.5 后端补 `lossless` | **完成** | 211 测试 / 0 失败；真窗口两分支都对：无损 `3072 kbps`、有损 `320 kbps`（见 §2.5） |
| 3 拆 `App.svelte` | **完成** | `App.svelte` 1070 → **127 行**；真窗口 20 项交互全通，新增 2 个 bug 的回归验证（见 §8.5） |
| 4 删旧壳 | **完成** | `compat/` + `contracts/` 删除；211 测试 / `check` 0/0；真窗口与预览两条线全通（见 §8.6），顺手修 pro 频谱空白面板（见 §4.3） |
| 5 清理遗留 + 收尾 | **完成** | §9 十条：7 清 1 留（`complex` 参数按设计保持）；`PlayerState` 哨兵改 Option（docs/17 §9.5 偏差 1 偿还）；pro 频谱五形态自研渲染器（audioMotion 卸载，包体 202→174kB）；mock 歌单真按内容；E2E Playwright 4/4；cargo test 219 全绿 / `check` 0/0 / gen:types 无漂移 |

### 8.1 批次 1 实际改了什么

| 文件 | 变化 |
|---|---|
| `styles/tokens.css` 等 6 个 | 新增，1818 行规则从 `app.css` 迁出（见 §3.1） |
| `app.css` | 1825 → 17 行，只剩 `@import` |
| `lib/artwork.ts` | 新增 29 行；`artworkUrl` 的唯一实现，6 个组件 + `media-session.ts` 改从这里取 |
| `contracts/src/ipc.ts` | 删掉自带实现，改成 re-export（避免两份字符串实现静默漂移） |
| `components/AppShell.svelte` | 新增 157 行：`.app` 网格 + 侧栏 + 侧面板 + 批量条 + 播放条 |
| `App.svelte` | 1149 → 1071 行（-204/+126，主区整体缩进减 2 层） |
| `tools/cdp-probe.mjs` + `probe:ui` | 新增：截图被拦时的 UI 取证手段（§5.1） |

**为什么 `App.svelte` 只降到 1071 而不是计划里说的 ~120**：批次 1 只搬"壳"，
主区内容（顶栏 / 视图头 / 筛选 / 扫描条 / 错误条 / 曲目表 / 空态，约 180 行）
按计划留给批次 3 的 `views/`。现在它作为 `AppShell` 的 children snippet 存在，
批次 3 换成一个 `<LibraryView>` 即可，不用再动 `AppShell`。

### 8.2 壳与 App 的边界（批次 3 沿用这个判据）

`AppShell` 直接读 `ui` / `library` / `player` / `selection` 四个 store
（容器可以依赖 store，依赖方向仍是 `components → stores → ipc`），
只接收 12 个回调/数据 prop，且**只收那些依赖 App 本地状态的**：

- 侧栏：`onview` / `onplaylist` / `oncreate` / `onplaylistmenu` —— 切视图要重载列表
- 队列：`onqueueplay` —— 点队列行播放失败要写进 App 的错误条
- 批量条：`onbatchplay` / `onbatchenqueue` / `onbatchaddto` / `onbatchremove` ——
  选中集 → id 列表要当前查询上下文
- 面板：`ontogglepanel` —— 开关要顺带落库滚动位置，防抖器在 App
- 其它：`onsynced`（导入完要重载）、`spectrumSource`（建图只能在面板打开时）

反面判据：如果某个回调在壳里能只用 store 实现（比如播放条 15 个按钮、
队列的删除/拖拽/清空），就**不进 prop**，直接在壳里调 store。

### 8.3 批次 2 实际改了什么

| 文件 | 变化 |
|---|---|
| `stores/player.svelte.ts` | 重写：`ipc.player.*` + `events.playerState`（rAF 合帧）；删 `AudioSink`/`audioEl`/`alignVolume`/`setNormalization` |
| `stores/library.svelte.ts` | 重写：`ipc.library.*`；`scan:progress` 动态载荷按结构读（缺字段按 0） |
| `stores/ui.svelte.ts` | 重写：`ipc.app.*` / `ipc.playlist.*` / `ipc.lyrics.*`；`mergeSettings()` 用默认值兜老配置 |
| `stores/beets-session.svelte.ts` | 新增 `chooseAndPreview/apply/cancel/syncNow`，`events.beetsImport` |
| `components/ImportPanel.svelte` | 5 个命令调用全删，只剩「动作 → store」 |
| `lib/errors.ts` | 新增：`errorMessage/isNotFound/isBusy/isRetryable`（`ApiError` 不是 `Error` 实例） |
| `lib/spectrum-source.ts` | `AnalyserSource` → `RemoteSpectrumSource`（订阅 `spectrum:frame`，按需开 tap） |
| `lib/lyrics-loader.ts` | 取数签名 `Promise<IpcResult<Lyrics>>` → `Promise<Lyrics>` |
| `main.ts` | 去掉 `window.api.app.initialLook`（顺带修正注释：`__AIYA_LOOK__` 后端从来没注入过） |
| 删除 | `lib/spectrum.ts`、`lib/audio-sink.ts`、`compat/audio-sink.ts`、`compat/spectrum.ts`（死链） |
| 类型 | `PlayerState` / `Diagnostics` 改用生成类型；`AudioFormat.lossless` 改为可选（见 §2.4），**随后端补齐已回到必填**（§2.5） |

**真窗口验证证据**（`probe:ui` + 脚本点击，不是"看着对"）：

| 验证项 | 结果 |
|---|---|
| 设置面板 → `app_diagnostics` | 显示真值：`版本 0.1.0` / `Electron tauri` / `Schema v2` / `DB 144.1 KB` / 媒体流与系统集成块有数据 |
| `beets_status` | `beets 2.14.1 · Python 3.14.7`，库路径与配置目录都填上了 |
| 主题往返（`app_set_settings`） | 点「浅色」→ `<html data-theme=light>`；点「深色」→ `dark` |
| 设置落库 | `settings` 表 `app_settings` 行 `theme=dark`，且 `output` 等默认字段齐全（合并逻辑生效） |
| UI 状态落库（`app_set_ui_state`） | `ui_state` 行 `sidePanel=queue`、`view=library`、`sort=title` —— 面板开关的防抖写入通 |
| `player:state` → rAF → UI | 点「静音」：按钮 title `静音 → 取消静音 → 静音`（事件链路 + 合帧端到端） |
| console / 异常 | 0 error、0 warning、0 未捕获异常 |

### 8.4 浏览器预览：批次 2 起没数据，**批次 4 已恢复**

批次 2 把数据层换成 typed `invoke` 之后，`npm run dev:ui`（只跑 vite，不是
Tauri）渲染出空列表——这是有意接受的过渡状态。

批次 4 用 `transport` 注入补回来了：`main.ts` 检测到没有
`__TAURI_INTERNALS__` 就 `await import('./lib/ipc/mock')` 装假后端，
**命令与事件一起替换**（`player:state` / `scan:progress` / `spectrum:frame`
都能跑）。实测：曲库 13 行、首行 `薄荷变奏`、4 个歌单；点第一行 →
播放条 `薄荷变奏 / 林俊杰` + 徽章 `FLAC` + `96 kHz / 24 bit` +
tooltip `… 无损 · 平均码率 2300 kbps`；侧面板开关正常；频谱 canvas
有 17823 个不透明像素。

`?failCmd=<命令名>` 可以注入一条命令失败，用来手验错误条渲染。
日常验收仍以 `npm run dev`（真窗口）+ `npm run probe:ui` 为准——
mock 证明的是"UI 在无后端时也能成立"，不是"后端对了"。

### 8.5 批次 3 真窗口验证（20 项，全程 CDP 读真 DOM）

拆 1070 行不留回归，靠的是逐项点出来的证据，不是"看着一样"：

| 类别 | 验证项 | 结果 |
|---|---|---|
| 版面 | `.app` 直接子元素 | `aside.sidebar` / `main.main` / `footer.player`（+ 侧面板按需） |
| 列表 | 曲库行数 / 首行 / 顶栏计数 | 3 行；`1 ♪ 中音测试 440Hz 未知艺人 — 0:05`；`3 首` |
| 快捷键 | `Ctrl+A` 全选 | 批量条出现：`已选 3 首 | 播放 | 加入队列 | 加入播放列表 | 移出曲库` |
| 快捷键 | `Escape` | 清空选择 → 批量条消失；再按关侧面板 |
| 快捷键 | `↓` | `.row.focus` 出现（键盘选中移动） |
| 快捷键 | `空格` | 起播，播放条换曲（`player:state` → UI） |
| 浮层 | 曲目右键 | 8 项：播放 / 下一首播放 / 加入播放队列 / 添加到播放列表… / 按此艺人筛选 / 按此专辑筛选 / 在文件夹中显示 / 从曲库移除 |
| 浮层 | 歌单右键 | 6 项：播放 / 重命名 / 上移 / 下移 / 清空 / 删除 |
| 浮层 | 设置面板 | 打开正常（真值已在 §8.3 验过） |
| 浮层 | 新建歌单对话框 | `aria-label=新建播放列表`，提交后侧栏出现、视图跳到新歌单（`0 首 · 0 分钟`） |
| 歌单 | 批量加歌 | 下拉 → 点歌单 → toast `新增 3 首，跳过 0 首`；侧栏计数 `3`；选择集清空 |
| 歌单 | 加完歌切过去 | 3 行；视图头 `播放列表 | 加歌验证 | 3 首 · 0 分钟` |
| 歌单 | 歌单内右键移除 | 菜单第 8 项变成「从播放列表移除」；移除后 2 行、侧栏计数 `2` |
| 歌单 | 删除（当前正在看） | toast `已删除「加歌验证」`；**主区立刻回到 3 行**（见下面的 bug 1） |
| 搜索 | 输入 `440` | 3 行 → 1 行 |
| 排序 | 点「标题」两次 | `标题 ↑` → `标题 ↓`；表头 `.sort-head.on` 唯一 |
| 运行期 | console / 未捕获异常 | 0 error、0 未捕获异常 |

**这轮抓到的两个 bug（都不是重构引入的，是搬代码时照抄了旧写法）**

1. **删掉正在看的歌单后，主区不刷新。**
   `ui.refreshPlaylists()` 内部发现当前歌单不在新列表里时，会**自己**把
   `ui.view` 改成 `LIBRARY_VIEW`。之后代码再判断
   `ui.view.kind === 'playlist' && ui.view.id === playlist.id` 恒为假 →
   `reload()` 被跳过 → 侧栏已切回「全部曲目」，主区却停在空态，用户得
   手点一下侧栏才出数据。
   旧实现在 `App.svelte` 里是 `const view = $derived(ui.view)`，**读的同样是
   刷新后的值**，所以刷新前就存在。
   修法：进函数第一行先把"是否正在看它"存成布尔。
2. **两个右键菜单能同时挂在屏幕上。** 右键曲目行之后再右键侧栏歌单，
   `.context-menu` 会有两个（真窗口截图为证）。
   修法：`overlay.openTrackMenu` / `openPlaylistMenu` 互斥清空对方。

第 2 个是新增的 `overlay` store 顺手就能修的（旧代码里两个 `$state` 变量
也是各管各的，同样有这个毛病），这也是把浮层状态收口到一处的好处。

### 8.6 批次 4 验证（真窗口 + 浏览器预览两条线）

删壳这一步的风险集中在"契约被接错"，所以两条线都要过：真窗口证明后端还通，
预览证明 UI 在没有后端时也成立。

| 线 | 验证项 | 结果 |
|---|---|---|
| 门禁 | `cargo test --workspace` | **211 passed / 0 failed**（+1 ignored），含 `ipc_contract` 三条 |
| 门禁 | `svelte-check` | 0 error / 0 warning |
| 门禁 | `vite build` | 通过；mock 独立 chunk `17.09 kB`，主 bundle 不变 |
| 门禁 | `gen:types:check` | 41 类型与 Rust 一致 |
| 真窗口 | 结构 | `aside.sidebar` / `main.main` / `footer.player`；侧栏 4 项；3 行曲库 |
| 真窗口 | 播放链路 | 点第一行 → 标题 `高音测试 880Hz`；状态 `播放`→`暂停`（引擎真的在跑） |
| 真窗口 | 音频规格 | 徽章 `WAV`；`44.1 kHz / 32 bit`；tooltip `无损 · 平均码率 2822 kbps` |
| 真窗口 | 频谱（默认 pro 设置） | 面板 canvas `287×629`，`litPixels` 0（暂停）→ **14617（播放中）** |
| 真窗口 | 设置页 | 5 个 tab（外观/播放/系统/快捷键/诊断）；「频谱可视化」开关可切 |
| 真窗口 | console / 未捕获异常 | 0 error、0 未捕获异常（favicon 404 除外，dev server 既有） |
| 预览 | 结构 | 13 行曲库、首行 `薄荷变奏`、4 个歌单、8 个 `nav-item` |
| 预览 | 播放链路 | 点第一行 → `薄荷变奏 / 林俊杰`、`FLAC`、`96 kHz / 24 bit`、tooltip 带 `2300 kbps` |
| 预览 | 侧面板 | 队列开 → 歌词 → 回队列 → 关，`side-open` 与 DOM 同步 |
| 预览 | 频谱（compact） | canvas `287×317`，`litPixels: 17823` |

**探针本身在这一轮也修了两处**（否则结果会骗人）：

1. **选目标不能盲目取 `[0]`**。浏览器会带 `edge://`、扩展后台页等杂项，
   盲取会连到"同步确认弹窗"上。改成只认 `http(s)` 页面，并支持
   `--url=<子串>` 精确挑。
2. **`trackRows` 选择器过时**。批次 1 之后行是 `.main .list .row`
   （不是 `.track-row`），且要排除 `.failed` / `.skeleton` 两种占位行。
   另外新增 `--play`：点第一行后读播放条（标题/艺人/徽章/规格/tooltip/播放键），
   一条命令覆盖"点击 → 命令 → 事件 → store → 渲染"整条链。

## 9. 遗留（本轮明确不做，别忘）

> 2026-10-25 收尾批次逐条处理过一轮，台账见每条末尾的状态标注。
> 已清掉的保留原文+处理记录（为什么这么处理），没清的才是活债。

1. ~~**`TrackAudio.lossless` 功能缺失**~~ —— **已修**，见 §2.5。
   真正的根因是 Tauri 重写时整个 `lossless` 字段丢了（不是 §4.3 之前猜的
   Mp4 分支问题），补字段 + 出口派生后徽章恢复。
   残留子项（**仍开着**）：`Mp4` 容器（ALAC vs AAC）判不出来，返回 `null`。
   这是刻意的保守：容器下两种编码都有，猜错比显示「未知」更糟。要收窄得读
   MP4 box 结构（`alac` atom），或依赖 beets 的 `format` 填进 `codec`（beets
   路径已覆盖 ALAC）。**状态：已修主项，Mp4 残留维持设计不变。**
2. ~~**诊断页三个数组恒为空**~~ —— **已清**（快清 B）。
   `list_timings` / `recent_errors` 接了真数据（`AppState.timings` 环 20 条 +
   `ApiError` 构造点统一记账，DTO 强类型）；`shortcut_conflicts` **删除**——
   Rust 侧没有任何快捷键定义，后端算 = 抄一份 action 表 + 规范化算法，
   双源必漂移，语义本属前端，诊断页调与设置页同一个 `findConflicts`。
3. ~~**`PlayerState.index/cursor` 的 `-1` 哨兵**~~ —— **已清**（大件 A）。
   改 `Option<usize>`（wire `number|null`，空队列 = `null`）。实际改动面比
   预想小：Actor 内部 cursor 本来就是 usize，只有 `state()` 出口一处换算；
   DB `queue_state.current_index` 的 -1 是存储层表示，保留。顺带修了旧行为
   不一致（空队列 index=-1 而 cursor=0）。见 docs/17 §9.5 偏差 1 的偿还记录。
4. ~~**浅色主题启动闪一帧深色**~~ —— **已清**（快清 A）。
   根因有两层，只有一层在 `__AIYA_LOOK__` 注入缺失：主题真值在 SQLite，
   bootstrap 要一次 IPC；mirror（`aiya.look` localStorage）+ main.ts 早于
   mount 应用只解决了第一层。第二层是 `UiStore.settings` 初始值
   `DEFAULT_SETTINGS`（dark）——App 的主题 effect 首跑会用它把 mirror 写回
   深色（CDP 探针实测 `["light","dark","light"]` 跳变）。修法是 settings
   初始值也接镜像：`{...DEFAULT_SETTINGS, ...readLook()}`，修后
   `["light","light","light"]`。`__AIYA_LOOK__` 全局注入没做——mirror 够用，
   多一条全局通道多一个漂移点。
5. ~~**`player_report` 的形状未定**~~ —— **已清**（快清 A）：整条链删除。
   v1 `<audio>` 出口的诊断通道，Rust 化之后前端零调用、后端空转。
   诊断数据如今走 §9.2 的 timings/errors，53 条命令。
6. **`complex` 参数仍收 `Json`**：`query` / `settings` / `options` / `decisions`
   是刻意不收紧的（docs/17 §3.1），别"顺手"改成强类型。**仍开着（设计如此）。**
7. ~~**audioMotion（pro 引擎）形同虚设**~~ —— **已清**（大件 B），路线与原文
   设想不同：原文提的 WASAPI loopback 回灌 WebAudio **不可行**——loopback
   数据仍落在 Rust 侧，喂给 UI 里的 audioMotion 照样要传 PCM（ADR-0003）；
   独占模式下 loopback 还捕获不到自身输出（已知陷阱）。实际解法：自研
   canvas 渲染器实现五形态（渐变条/LED/镜像/径向/反射），同一份 48 带 tap，
   pro 的差异在渲染层；audiomotion-analyzer 依赖卸载，notices 清单减一项。
   设置页 hint 已更正（不再写"两种风格画出来一样"）。
8. ~~**mock 不建 `playlist_items` 表**~~ —— **已清**（快清 C）。
   mock 建 `playlistItems: Map<playlistId, trackId[]>`（对齐真实库
   playlist_items 形状），歌单视图真按内容过滤，计数/时长重算、去重与宿主
   同语义。CDP 实测：四个 mock 歌单 12/17/22/27 首各不相同（此前全是全库
   同一排序）。**现在可以拿预览验歌单语义了。**
9. ~~**`AppSettings` 未含 `output`**~~ —— **已清**（快清 C）。
   `AppSettings.output: DesiredOutput`（generated 类型），设置页播放 tab
   顶部四行（模式/缓冲/时钟/暂停释放设备），取值与 `aiya-audio` 的
   `ModePref::parse` 对齐。
10. ~~**快捷键设置页 / 主题密度逐项核对 / E2E**~~ —— **已清**（P4 + 收尾批）。
    快捷键页 P4 已完成（含冲突检测与设置页同源）；主题×密度四组合 CDP
    实测（dark/light × comfortable 52px / compact 42px，行高由设置驱动、
    无异常）；E2E 冒烟 Playwright 4 条全绿（`npm run test:e2e`，系统 Edge，
    不起 Tauri）。**仍开着的小项**：无。
