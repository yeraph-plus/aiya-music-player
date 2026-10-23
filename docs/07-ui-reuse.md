# 07 — UI 复用：先原样搬，再逐步重写

> 结论：**约 95% 的 Svelte 文件可以原样复制，不改。** 需要写的是 4 个适配器，不是重写 UI。
> 当前 UI 是今天刚写的 demo，视觉/交互已经验证过；现在重写是负收益。先用兼容层跑起来，把投资留给 beets 和新体验。

## 1. 盘点：UI 到底依赖什么

| 依赖 | 数量/位置 | 是否能原样保留 |
|---|---|---|
| `window.api.*` 调用点 | **54 处**，分布在 stores/components | 需要兼容层，组件不改 |
| 用到的 API 方法 | **48 个**（完整 `AiyaApi` 约 53 个） | 兼容层实现 |
| `artworkUrl()` 引用 | 7 个组件 + `media-session` | 改共享函数，组件不改 |
| 音频出口 | `lib/audio-sink.ts`（1 个类） | 换实现，保留类 API |
| 频谱源 | `lib/spectrum-source.ts`（1 个接口） | 换实现；pro 模式暂不可用 |
| 首屏主题 | `main.ts` 读 `window.api.app.initialLook` | 改 3 行启动代码 |
| MediaSession | `lib/media-session.ts` | 先保留；它不会自动变成 SMTC，但也不阻塞 |
| Svelte 组件/样式/纯函数 | `components/`、`lib/`、`app.css` | **原样复制** |

`App.svelte` 约 1,100 行，确实有债务，但它不是这次的投资重点；先让它跑，后面按 feature 拆。

## 2. 五个必须处理的接缝

### 2.1 `window.api` → Tauri invoke

写一个 `ui/src/compat/api.ts`，实现旧 `AiyaApi`：

```ts
// 伪代码
export const api: AiyaApi = {
  library: {
    list: (q) => invoke('library_list', { query: q }),
    onScanProgress: (cb) => listen('scan_progress', (e) => cb(e.payload)),
    artworkUrl: (id, size) => artworkUrl(id, size), // 见 2.3
    // ...
  },
  player: {
    state: () => invoke('player_state'),
    onState: (cb) => listen('player_state', (e) => cb(e.payload)),
    onCommand: () => () => {}, // 后端出声，UI 不再消费命令
    // ...
  },
};
```

要点：

- 保留旧 `IpcResult<T>` 信封（`{ ok, data }` / `{ ok:false, code, message }`），UI 的 `result.ok` 分支不用改。
- `listen()` 返回的 Promise 要包成同步 unsubscribe 函数（旧 API 约定）；内部先 `let un: UnlistenFn | null`，返回 `() => un?.()`。
- 错误码/文案映射在 compat 层做，UI 不改。
- 不需要把 Tauri 的 generated bindings 直接暴露给 UI；compat 是唯一适配层。
- 旧 `contracts` 包可以原样复制，作为“UI 兼容契约”；这与“不要历史数据库兼容”不冲突——省的是 UI 改造，不是背旧数据。

### 2.2 `AudioSink` → 远程出口

保留 `AudioSink` 的公开方法，替换内部：

```ts
export class AudioSink {
  constructor(_audio: HTMLAudioElement, _opts: { report: (r: PlayerReport) => void }) {}
  apply(_cmd: PlayerCommand): void { /* 后端已执行；no-op */ }
  setNormalization(_db: number | null): void { /* 设置已在后端 */ }
  setVolume(_v: number, _m: boolean): void { /* 状态已在后端 */ }
  getSpectrumSource(n = 48): SpectrumSource { return new RemoteSpectrumSource(n); }
  dispose(): void {}
}
```

- `player.svelte.ts`、`App.svelte` 的 `init(audioEl)` 完全不用改；`<audio>` 元素留着但**永不加载 `src`、不接 Web Audio**——它是死挂载点，不是第二条音频路径。ADR-0001 禁止的是“用于出声/取 PCM 的音频元素与音频图”，与保留空元素不冲突；后续 UI 重写时删除。
- `apply` 不转发 `load`，否则会出现第二条音频路径；后端是唯一出口。
- `report` 不再需要；后端直接产生状态/位置事件。
- 这样 UI 的播放按钮、队列、进度条都照旧工作。

### 2.3 `media://` → Tauri 封面协议

只改 `contracts/ipc.ts` 的 `artworkUrl()`。注意 Tauri 自定义协议在 Windows 上通常通过 `http://<scheme>.localhost` 暴露，不能想当然写 `aiya-artwork://`：

```ts
// 伪代码：以实际 Tauri 版本/平台为准
const base = isWindows
  ? `http://aiya-artwork.localhost/artwork/`
  : `aiya-artwork://localhost/artwork/`;
return `${base}${encodeURIComponent(artworkId)}?size=${safe}`;
```

- Tauri 注册 `aiya-artwork` scheme，按 artworkId 查缓存；带 `ACAO: *`，`artwork-blob.ts` 和 canvas 取色继续工作。
- CSP 要允许该 origin（`img-src` / `connect-src`）；否则图裂/取色失败。
- 先用一个 spike 验证 `fetch()` 能读该 URL；如果不行，改用 Tauri `convertFileSrc` 或直接用 `asset` 协议映射封面文件。
- 7 个组件的 `artworkUrl()` 调用不用改。
- 音频不再需要 `media://track/*`；`trackMediaUrl()` 删除或改成抛错（确保没有遗漏调用）。

### 2.4 首屏主题

旧 UI 在 mount 前读 `window.api.app.initialLook`。Tauri 用 `initialization_script` 注入：

```ts
window.__AIYA_LOOK__ = { theme: 'dark', density: 'comfortable' };
```

`main.ts` 改 3 行：

```ts
const look = window.__AIYA_LOOK__ ?? { theme: 'dark', density: 'comfortable' };
```

不需要为了首帧去同步 IPC。

### 2.5 频谱：compact 可复用，pro 先停用

- v1 原生引擎自带链内 tap（04 §5.1，ADR-0006）：compact 频谱随引擎就位，不再有“mpv 无数据降级”问题。
- 写 `RemoteSpectrumSource implements SpectrumSource`：从 Tauri event/Channel 收 48 个 `u8`，维护 `bandCount/live/start/stop/frame`；`audioNode` 为 `undefined`。
- `SpectrumPanel` 的 compact 分支只读 `frame()`，可以直接用。
- pro 分支（audioMotion）要求 `AudioNode`；后端没有。最小改动：`spectrumEngine === 'pro' && source.audioNode` 才走 pro，否则回落 compact；或在设置里暂时把 pro 标为“即将支持”。
- 这是唯一需要碰 Svelte 组件的地方；一行条件，不是重写。
- 不要为了 pro 把音频图搬回 WebView；那会重新引入 CORS/静音/生命周期问题。

## 3. 可以原样复制的目录

```text
apps/desktop/src/renderer/src/
├── App.svelte
├── app.css
├── main.ts                  # 改 3 行 initialLook
├── components/              # 全部复制；SpectrumPanel 一行条件
├── stores/                  # 全部复制
└── lib/                     # 除 audio-sink.ts / spectrum-source.ts 外全部复制
```

需要新增的 compat 文件：

```text
ui/src/compat/
├── api.ts                   # window.api
├── audio-sink.ts            # 替换原 lib/audio-sink.ts
├── spectrum.ts              # RemoteSpectrumSource
├── artwork.ts               # artworkUrl / scheme
├── boot.ts                  # initialLook + mount
└── types.d.ts               # window.api 声明
```

> 复制后把 import 路径 `@aiya/contracts` 指到本地 `contracts` 副本或 workspace 别名即可；不要改组件内部的 import。

## 4. 功能优先级：只做值得做的

旧功能表 125 项，不代表新版都要 parity。按“UI 体验 + beets”重排：

| 优先级 | 功能 | 说明 |
|---|---|---|
| **P0** | 应用壳、侧栏、曲库列表、搜索/排序/筛选、播放列表侧栏、设置/主题、播放条、队列面板 | 复制 UI + compat API；先只读/本地扫描 |
| **P0** | 本地扫描、封面、曲目元数据 | 新库从零建，无旧数据迁移 |
| **P1** | 播放/暂停/上下首/seek/音量/速率、队列编辑、恢复 | 引擎二选一（见 `09`） |
| **P1** | 歌词、托盘、关窗到托盘、SMTC、防休眠 | 系统集成 |
| **P1.5** | **beets 导入预览/差异/应用** | 项目重点；见 `08` |
| **P2** | 频谱 compact、播放历史、快捷键、批量操作、诊断 | 体验增强 |
| **P3** | 频谱 pro、watcher、jobs、EQ/DSD/crossfade、多位置、在线 | 明确后置 |

**明确不做 v1**：旧库迁移、旧 `media://` 音频、旧 preload/IPC、Electron 主进程、audioMotion 依赖、跨平台音频。

## 5. 落地步骤（1–2 天可跑起来）

1. 复制 renderer → `ui/src`，配置 Vite/Svelte；先不接 Tauri，用 mock `window.api` 渲染静态列表，确认样式/字体/布局。
2. 写 `compat/api.ts`，用 Tauri command mock 返回 20 条假数据；列表/搜索/选择先通。
3. 接真实 `library_list`/`library_tracks`/`artwork`，验证 5,000 行滚动与封面。
4. 换 `AudioSink` 为 remote sink；播放条先显示状态，不出声；再接引擎。
5. 换 `RemoteSpectrumSource`；默认 compact，pro 隐藏。
6. 用 Playwright/tauri-driver 做 3 条 smoke：启动→列表→打开设置；播放→暂停；搜索→选择。

**验收**：UI 文件 diff 中，`.svelte` 改动不超过 1 个文件（SpectrumPanel 一行）；其余改动都在 `compat/` 和 `main.ts`。

## 6. 什么时候再重写 UI

等下面任意一条成立再拆：

- 要加 beets 审阅/差异/批量编辑这类新交互，旧 `App.svelte` 放不下。
- 要做多窗口/多面板/自定义布局，旧 store 结构成为瓶颈。
- 要换设计系统/组件库，逐 feature 替换比原地补更便宜。

重写顺序：`App.svelte` → library feature → player bar → queue → settings → beets。后端契约不变，所以 UI 可以独立迭代。

> 现在把 UI 推倒，只会得到“同样的界面 + 新的 bug + 两周工期”。先把兼容层做薄，把时间花在 beets 和体验上。