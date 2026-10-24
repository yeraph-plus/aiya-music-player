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
  main.ts                 只做 mount（现在 17 行，保持）
  App.svelte              ~120 行：只负责布局骨架 + 视图切换
  styles/
    tokens.css            从 app.css 抽：:root / [data-theme=light] 变量
    base.css              reset + 排版 + 滚动条
    utilities.css         .panel / .btn / .row 这类原子类
  lib/
    ipc/                  ✓ 已完成（client/schema/events/generated）
    format.ts  virtual.ts  selection.ts  view.ts  ...（纯函数，原样保留）
    artwork.ts            artworkUrl()（从 contracts 搬来，唯一源）
    keyboard/             快捷键注册表 + 冲突检测（从 App.svelte 抽出）
  stores/
    player.svelte.ts      直连 ipc.player.* + events.playerState（合帧）
    library.svelte.ts     直连 ipc.library.*（游标分页）
    playlists.svelte.ts   直连 ipc.playlist.*
    ui.svelte.ts          view / 侧栏 / 面板开关 / 主题（zod 校验边界）
    selection.svelte.ts   纯内存，原样保留
    beets.svelte.ts       直连 ipc.beets.* + events.beetsImport
    history.svelte.ts     直连 ipc.history.*
  views/
    LibraryView.svelte    曲目表（TrackList 的宿主）
    PlaylistView.svelte
    HistoryView.svelte
    SettingsView.svelte
    DiagnosticsView.svelte
    BeetsView.svelte
  components/             保留，但改成 props in / events out（不取数）
```

**依赖方向铁律**：`components → stores → ipc`。组件里出现 `ipc.` 或 `window.api`
就是错的，`grep` 可查。

## 3. 分批落地（每批可独立验证）

### 批次 1 · 地基（无可见变化，但后面全部依赖它）
- [ ] `styles/tokens.css` / `base.css` / `utilities.css` 三层拆开，`app.css` 只剩 `@import`
- [ ] `lib/artwork.ts` 承接 `artworkUrl()`，`contracts/` 不再被新代码引用
- [ ] `App.svelte` 拆壳：布局（侧栏 / 主区 / 播放条 / 队列）移入 `AppShell.svelte`，
      `App.svelte` 只留视图切换 + 全局副作用
- [ ] 验收：`svelte-check` 0/0；界面与拆之前**逐像素一致**（截图对比，见 §5）

### 批次 2 · 数据层换血（关键一步）
- [ ] `player` store 改直连 `ipc.player.*` + `events.playerState`（10 Hz 合帧）
- [ ] `library` store 改直连 `ipc.library.list`（游标分页，`TrackQueryInput`）
- [ ] `ui` store 的 `getSettings/setSettings` 走 `ipc.app.*` + `settingsSchema` 校验
- [ ] `playlists` / `history` / `beets` store 从组件里抽出来（现在散在面板里）
- [ ] 验收：`grep -r "window.api" ui/src` 只剩 `compat/` 自身；功能无回归

### 批次 3 · 拆 `App.svelte`
- [ ] 视图路由改成 `views/`（`LibraryView` 先行：曲目表 + 表头 + 批量栏）
- [ ] 快捷键从 `App.svelte` 移到 `lib/keyboard/`（含 `E_SHORTCUT_CONFLICT` 检测）
- [ ] 拖拽/批量/右键菜单归位（`lib/drag.ts`、`components/BatchBar`、`ContextMenu`）
- [ ] 验收：`App.svelte` < 150 行；六视图切换 + 快捷键全通

### 批次 4 · 删旧壳（等新 UI 自给自足才做）
- [ ] 删 `ui/src/compat/`（含 `mock.ts` —— 浏览器预览改用 `ipc` 的 mock 适配）
- [ ] 删 `ui/contracts/`（类型全部来自 `lib/ipc/generated/`）
- [ ] `ipc_contract.rs` 的双层扫描收敛成只扫 `lib/ipc/client.ts`
- [ ] 验收：`gen:types:check` + `cargo test` + `svelte-check` 全绿

批次 1–3 完成前，`compat/` 与 `contracts/` **不动**——它们让旧界面继续可用，
是本轮重构的"安全网"。这也是为什么批次 4 排在最后。

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
5. 窗口截图被安全策略拦（已知），视觉回归用**同一窗口尺寸下的手测**代替

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
