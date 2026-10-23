# 05 — 重建计划：UI 先跑，音频买/借，beets 做主

> 前提：这是今天的 demo，**没有任何历史数据/旧库/旧协议需要兼容**。
> 策略：**UI 原样搬 + 薄兼容层；音频用成熟 sidecar；把时间花在 beets 和界面体验。**
> 估时按 1 名全职开发者；UI 复用后，音频不再是最大变量，beets 才是。

## 1. 和上一版计划的变化

| 旧计划 | 新计划 | 原因 |
|---|---|---|
| 原计划的 core crate | **`aiya-player`** | 硬盘上有同名仓库，避免搜索/符号漂移 |
| 逐 feature 重写 UI | **先原样复制 + compat 适配** | UI 是今天刚写的，重写负收益 |
| 保留旧库 v1–v5 迁移 | **全新 schema v1，不迁移** | demo 无历史包袱 |
| 自研 wasapi-rs 首发 | **mpv sidecar 首发**（wasapi-rs 为 B 方案） | 重点 UI/beets，音频买/借 |
| mpv sidecar 首发（本文件早版） | **原生引擎首发**（Symphonia + wasapi，ADR-0006） | 频谱动画需要链内 PCM tap，mpv 无 tap；EQ/LRC/WASAPI 联合设计 |
| beets 放 P6 | **beets 提前到 P3，独立 sidecar** | 项目差异点 |
| 7–10 周 | **4–6 周**（全职） | UI/音频都省了 |

## 2. 阶段总览

| 阶段 | 目标 | 主要产物 | 退出条件 | 估时 |
|---|---|---|---|---|
| P0 | UI 兼容 + 引擎/beets spike | 复制 UI、`compat/`、原生引擎 spike、beets `status` | 旧 UI 在 Tauri 里显示；原生引擎能出声；`beet` 可检测 | 2–3d |
| P1 | 数据 + 只读 UI | 全新 schema、扫描、列表/搜索/封面、播放列表 | 5 千–5 万首可浏览；UI 组件基本不改 | 1w |
| P2 | 播放纵切 | `AudioPort` + 原生引擎（Symphonia+wasapi）、PlayerActor、恢复 | 点行出声、seek、暂停、关窗继续 | 2w |
| P3 | **beets 预览/应用** | Python sidecar、diff UI、写标签 | 选文件夹→候选→diff→应用→曲库刷新 | 1–1.5w |
| P4 | 体验/系统 | 歌词、托盘、SMTC、频谱 compact、快捷键、设置 | 日常可用；E2E 通过 | 1–1.5w |
| P5 | 打包/收尾 | 安装包、签名、GPL notices、soak | 干净 Win11 安装/卸载；4h 长播 | 3–5d |

**总计约 5–7 周**（ADR-0006 原生引擎后）。若原生 spike 暴露硬伤可回退 mpv（P2 省回 1 周，明确代价：无频谱、EQ 受限）；如果 beets API 不顺，P3 +1 周。

## 3. P0 — UI 兼容 + 两个 spike（2–3 天）

### 3.1 复制 UI

```bash
# 伪命令：把 demo renderer 原样复制到新仓库
cp -r ../AIYA_MUSIC/apps/desktop/src/renderer/src ui/src
cp -r ../AIYA_MUSIC/packages/contracts ui/contracts   # 先当“UI 兼容契约”
```

- 组件/样式/`lib` 不改；只加 `compat/`。
- `main.ts` 改 3 行首屏主题。
- `artworkUrl` 改一处 scheme。
- `audio-sink.ts` / `spectrum-source.ts` 换实现。
- 详细步骤见 [`07-ui-reuse.md`](07-ui-reuse.md)。

### 3.2 引擎 spike（2 天，二选一）

- A：`mpv.exe` + `--input-ipc-server`，测独占/seek/device/gapless/1h。
- B：`wasapi` 0.26 + `symphonia`，测 Shared/Exclusive/对齐/时钟。
- 用同一批 fixtures，按 [`09-audio-engine-choice.md`](09-audio-engine-choice.md) §4 的门槛打分。
- **默认 B（原生）**；A（mpv）仅作对照数据，不进发布路径（ADR-0006）。

### 3.3 beets spike（半天）

- 检测 `beet --version` / `beet config -p`。
- Python sidecar 能 `import beets`、打开 `Library`、返回 10 条 items。
- 不写文件、不移动；只证明“能拿到结构化数据”。
- 详细设计见 [`08-beets.md`](08-beets.md)。

### 验收

- [ ] 旧 UI 在 Tauri 窗口里显示，列表/播放条/设置样式不崩。
- [ ] `window.api` compat 能返回假数据；至少列表/搜索/选择可用。
- [ ] mpv 或 wasapi-rs 任一能出声，且能报出设备/格式。
- [ ] `beet` 检测成功/失败都有清晰提示。

## 4. P1 — 数据 + 只读 UI（1 周）

### 交付

- 全新 `aiya-data`：schema v1（tracks/albums/artists/playlists/queue/history/settings），无旧库迁移链。
- 扫描：walkdir + lofty + image；封面 96/256/512；失败隔离。
- 查询：cursor + `LIMIT/OFFSET`；5 万首变慢再加物化 offset；`title_norm` + LIKE。
- `library_list/tracks/stats/playlists_list` 等命令。
- UI compat 接真实数据；列表/搜索/排序/筛选/封面/播放列表侧栏。
- 封面自定义协议 `aiya-artwork`（Windows 实际 `http://aiya-artwork.localhost`）。

### 验收

- [ ] 5 万首冷启动 ≤ 450ms；深分页 ≤ 5ms；搜索 ≤ 20ms。
- [ ] 列表首屏 ≤ 100ms；滚动稳定。
- [ ] 封面缓存命中；UI 看不到文件路径。
- [ ] UI `.svelte` 改动 ≤ 1 个文件（频谱 pro 条件）。

## 5. P2 — 播放纵切（1 周）

### 交付

- `aiya-player`：队列/随机/重复/状态归约；`PlayerActor`。
- `AudioPort` trait + `NativeAudio` 适配器：Symphonia 解码、ring buffer、事件驱动 WASAPI Shared 渲染、位置时钟（04 §3）。
- DSP 链一次定形：`decode → [resample(共享)] → [ReplayGain] → [EQ 预留] → [tap 预留] → [volume]`（ADR-0006）；`SetEq`/`SetSpectrum` 协议进 golden fixtures，tap 数据链 P2 就位、频谱 UI P4 接。
- 命令：play/pause/next/prev/seek/volume/mute/rate；队列编辑；恢复。
- 关窗到托盘；UI 重载不断声。

### 验收

- [ ] 点列表任意行，按当前视图顺序播放；播完最后一首停。
- [ ] 连点 200 次下一首不越界；旧 generation 事件不覆盖新状态。
- [ ] seek 容差 ±100ms（共享）/±50ms（独占，实测记录）。
- [ ] UI 重载/关窗到托盘，音乐继续；重启恢复队列/进度/音量。
- [ ] 引擎 sidecar 崩溃：宿主重启，从最后位置恢复/暂停；UI 有提示。

## 6. P3 — beets（1–1.5 周，核心）

### 交付

- `aiya-beets` Python sidecar：`status/preview_import/apply_import/sync_library`。
- 预览：专辑分组、候选、逐字段 diff、置信度、封面。
- 应用：默认只写标签；复制/移动显式选择；冲突/失败可见。
- 导入 UI：文件夹选择、专辑卡片、逐项接受/跳过、进度、结果。
- 同步：写完后刷新自有 DB/封面/队列；`beets_item_id` 可选。

### 验收

- [ ] 没装 beets：设置页有安装指引，不崩。
- [ ] 选一个文件夹：预览无文件改动；能看到候选/diff。
- [ ] 应用后：标签/封面更新；失败项有原因；可重试。
- [ ] 目标路径冲突不覆盖；移动/复制默认关闭。
- [ ] 1,000 张专辑导入可取消、不卡 UI。

## 7. P4 — 体验/系统（1–1.5 周）

### 交付

- 歌词：读文件/编码/解析；前端时钟/滚动/点击。
- 托盘、关窗到托盘、单实例、防休眠、SMTC（隐藏顶层窗口）。
- 频谱 compact：原生 tap 已随 P2 就位（04 §5.1 订阅制必要采样复制），接 `RemoteSpectrumSource` 即可；pro 后置。
- 快捷键、批量选择/右键、Toast/对话框、主题/密度/主色。
- 播放历史/最近/常听；诊断页音频/beets 状态。

### 验收

- [ ] SMTC 在真实 Windows 会话可见，按钮/进度双向。
- [ ] 关窗到托盘音乐继续；二次启动唤起窗口。
- [ ] 歌词/封面/主题在深浅色下正常。
- [ ] 诊断页能显示：设备/格式/独占结果、mpv 版本、beets 版本/库路径。

> 频谱注意（已随 ADR-0006 解决）：tap 在引擎链内（04 §5.1），订阅制必要采样复制、关闭零开销；仍然不要为了任何频谱形态把音频搬回 WebView。

## 8. P5 — 打包/收尾（3–5 天）

- 安装包：Tauri + mpv sidecar + beets sidecar（若捆绑）；WebView2 检测。
- 签名：host + sidecars；杀软误报测试。
- GPLv3 notices：mpv/FFmpeg/beets/Python/Tauri；源码获取方式。
- soak：4h 播放、1 万文件扫描、导入 100 张专辑、设备切换。
- 文档：安装、beets 配置、已知限制。

## 9. 复用 / 重写 / 删除

| 资产 | 处理 | 说明 |
|---|---|---|
| renderer 组件/样式/纯函数 | **原样复制** | 只加 compat；不重写 |
| `packages/contracts` | **复制为 UI 兼容契约** | 保持 `AiyaApi` 形状；Rust 后端按此返回 |
| `packages/playback` | **重写** | Rust `aiya-player`；语义保留 |
| `packages/database` | **重写** | 新 schema v1；SQL 思路复用，不迁旧库 |
| `packages/library` | **重写** | lofty/walkdir/image |
| `packages/media` 音频协议 | **删除** | 封面 cache 思路保留 |
| `packages/storage` | **删除/合并** | 引擎直接读文件 |
| `packages/jobs` | **后置** | v1 用内存任务 + 扫描调度即可 |
| `apps/desktop/src/main` | **重写** | Tauri host + 系统集成 |
| `preload` | **删除** | compat 直接定义 `window.api` |
| `audio-sink.ts` / Web Audio 图 | **替换** | `RemoteAudioSink`；不保留第二条音频路径 |
| `audiomotion.ts` | **后置** | 依赖 AnalyserNode；pro 暂不支持 |
| 旧 DB/迁移 v1–v5 | **删除** | 无历史兼容 |
| 旧 `media://track/*` | **删除** | 音频走引擎；封面用 `aiya-artwork://` |

## 10. 新 schema（v1，不再有旧版本）

```sql
-- 只列核心；完整迁移在 aiya-data/migrations/0001_init.sql
CREATE TABLE tracks (
  id TEXT PRIMARY KEY,
  path TEXT NOT NULL UNIQUE,
  title TEXT NOT NULL,
  title_norm TEXT NOT NULL,
  artist TEXT,
  album TEXT,
  album_artist TEXT,
  track_no INTEGER,
  disc_no INTEGER,
  year INTEGER,
  duration_ms INTEGER,
  codec TEXT, sample_rate INTEGER, bits INTEGER, channels INTEGER, bitrate INTEGER,
  gain_db REAL, peak REAL,
  artwork_id TEXT,
  beets_item_id INTEGER,
  added_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);
CREATE INDEX idx_tracks_title_norm ON tracks(title_norm);
CREATE INDEX idx_tracks_artist ON tracks(artist);
CREATE INDEX idx_tracks_album ON tracks(album);

CREATE TABLE playlists (...);
CREATE TABLE playlist_items (...);
CREATE TABLE queue_state (id INTEGER PRIMARY KEY CHECK (id=1), current_index INTEGER, position_ms INTEGER, repeat TEXT, shuffle INTEGER, volume REAL, muted INTEGER, rate REAL);
CREATE TABLE queue_items (position INTEGER PRIMARY KEY, track_id TEXT NOT NULL REFERENCES tracks(id) ON DELETE CASCADE);
CREATE TABLE history (...);
CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);
```

- 不需要 `library_order` 物化表也能先跑；5 万首后再加。
- 不需要 `media_assets/media_locations` 三分离（demo 无多来源）；如果以后要多位置，再迁移。
- 队列仍规范化，不写 JSON 整行。
- 所有迁移从 v1 开始；`user_version` 只用于未来增量。

## 11. 风险

| # | 风险 | 影响 | 缓解 |
|---|---|---|---|
| 1 | 原生引擎 Shared/独占/时钟工期超预期 | 播放延期 | P0 spike 按 09 §4 门槛先行；mpv 作逃生（代价：无频谱、EQ 受限） |
| 2 | WMA/DFF 等 Symphonia 格式缺口 | 部分曲播不了 | 先明示"不支持"；FFmpeg LGPL 动态链接后补（04 §2.2） |
| 3 | beets API/版本漂移 | 导入功能坏 | 锁 2.14.x；sidecar 隔离；错误可见；CLI fallback |
| 4 | UI compat 漏方法 | 页面局部坏 | 54 个调用点清单；smoke 覆盖；`window.api` 全量实现 |
| 5 | 频谱 pro 不可用 | 体验缺失 | compact 随原生 tap 交付（ADR-0006）；pro 后置；不把音频搬回 WebView |
| 6 | 导入写坏文件 | 数据风险 | 默认只写标签；预览无副作用；移动/复制显式；失败可重试 |
| 7 | 打包/签名/杀软 | 安装失败 | 一起签名；干净机测试；日志 |
| 8 | 范围蔓延 | 延期 | v1 不做 EQ/DSD/watcher/jobs/在线 |

## 12. 前 10 个任务

1. 建 Cargo workspace + `ui` Vite；复制 renderer；跑静态页面。
2. 写 `compat/api.ts`，用 mock 数据接通列表/搜索/选择。
3. 跑原生引擎 spike：wasapi-rs Shared/Exclusive（44.1/16、48/24、帧对齐）、Symphonia 格式矩阵、seek、设备、1h（09 §4 门槛）；mpv 对照可选。
4. 跑 beets spike：`beet --version`、`Library` 读取 10 条。
5. 写 `aiya-data` schema v1 + 扫描 + 列表查询。
6. 接 `aiya-artwork://` + 封面；验证 canvas 主色。
7. 写 `aiya-player` 队列 + 状态归约；翻译旧测试。
8. 写 `MpvAudio` + `PlayerActor`；UI 播放条接通。
9. 写 beets preview/apply + 导入 UI。
10. 打包 + GPL notices + 4h soak。

## 13. 完成定义（v1）

- UI：旧界面基本原样，列表/搜索/播放/队列/设置/歌词可用。
- 音频：共享稳定；独占可选且失败可解释；设备切换不崩；关窗继续。
- beets：能预览候选/diff、应用标签、刷新曲库；默认不移动文件。
- 工程：全新库可重建；无旧数据依赖；核心测试/E2E/性能基准有记录。
- 许可：GPLv3 + 所有第三方 notices/源码提供方式齐全。

> 如果只能先做一件事：**先把 UI 原样跑起来，再把 beets 预览做出来。** 音频用 mpv 顶着；不要让“自研播放器”吃掉这个项目真正的差异点。