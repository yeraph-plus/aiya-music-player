# 08 — Beets 集成：这是新版的第一等功能

> 目标不是“能调用 `beet import`”，而是把 beets 的**匹配、候选、差异、写标签、移动文件**变成一个可审阅、可撤销、可解释的 UI 流程。
> 这会决定新版和普通播放器的差异；音频引擎只要“稳定出声”，beets 才值得投入。

## 1. 先定边界

| 能力 | 归属 | 说明 |
|---|---|---|
| 曲库浏览/搜索/播放/队列 | 宿主 + 自有 DB | beets 不可用时也要能跑 |
| 标签匹配/候选/写标签/移动/复制 | beets sidecar | beets 是写操作权威 |
| 导入审阅 UI | Svelte | 新版核心界面 |
| beets 配置/版本/插件诊断 | 宿主 | 读 `beet config`/`--version` |
| beets library 读取 | sidecar 或只读 DB | 用于同步/统计，不直接让 UI 查 |

**不要把 beets 当播放器，也不要把播放器当 tagger。** 两者通过文件与事件解耦。

## 2. 三种集成方式，选哪一种

| 方式 | 能拿到候选/差异 | 稳定性 | 打包 | 结论 |
|---|---|---|---|---|
| 调 `beet` CLI | ❌ 只有文本/`--pretend` 文件列表 | 中（输出格式会变） | 简单 | 只适合“快速导入”，不够 UI |
| 直接读 beets `library.db` | 只有已入库结果 | 中（schema/插件字段） | 简单 | 适合同步/统计，不适合预览 |
| **Python sidecar + beets API** | ✅ `beets.autotag` / `beets.importer` | 中（需锁版本） | 需要 Python | **推荐** |

beets 官方文档明确说 importer 与 CLI 解耦，就是为了让 GUI 能接上；`beets.importer` 是 pipeline/coroutine，`beets.autotag` 提供候选。用 Python sidecar 是正路。

**降低 B1 风险**：预览可以先直接用 `autotag.tag_album()` / `tag_item()` 对分组后的 `Item` 做只读候选，不启动完整 importer；应用阶段再走 `importer`/CLI。这样第一版不必啃完整个 pipeline，也能拿到结构化 diff。

## 3. 推荐架构：`aiya-beets` sidecar

```
Tauri host (Rust)
  │  stdio / 长度前缀 JSON（低频；导入进度可 10Hz）
  ▼
aiya-beets (Python 3.10–3.14, beets 2.14.x)
  ├─ 读 beets config（beet config -p / BEETSDIR）
  ├─ Library / autotag / importer
  ├─ 写标签、移动/复制（按用户选择）
  └─ 输出候选、差异、进度、错误
```

为什么 Python：

- beets 本身就是 Python；直接 import 比解析 CLI 稳。
- 可以拿到 `Candidate`、`AlbumInfo`、`TrackInfo`、`Recommendation`、逐字段 diff。
- 可以用 `Library` 事务写库，避免“CLI 跑一半、状态不明”。
- GPLv3 项目可以分发 MIT 的 beets + PSF 的 Python；需要保留 notices。

### 3.1 运行方式

- **v1：依赖用户已安装的 beets**（`pipx install beets` / `uv tool install beets`）。启动时检测 `beet --version`；缺失就在设置页给安装指引。这样不捆绑 Python，工期最短。
- **v2：捆绑 `python-build-standalone` + beets**，零安装；体积 +40–80MB，需签名/更新。
- 开发/测试用固定 venv + `requirements.lock`（beets==2.14.x），避免 API 漂移。
- 永远不要让 sidecar 直接改宿主 DB；它只改文件/beets DB，宿主通过事件刷新读模型。

### 3.2 协议

```json
{ "id": 1, "cmd": "status" }
{ "id": 2, "cmd": "preview_import", "paths": ["D:\\Incoming"], "options": { "singleton": false, "autotag": true, "timid": true } }
{ "id": 3, "cmd": "apply_import", "session": "imp_01", "decisions": [ { "album": "alb_01", "action": "apply", "candidate": "cand_02" } ] }
{ "id": 4, "cmd": "sync_library" }
```

事件：

```json
{ "type": "progress", "stage": "matching", "current": 3, "total": 12, "path": "D:\\Incoming\\Album" }
{ "type": "album", "session": "imp_01", "album": { "id": "alb_01", "paths": ["..."], "current": { "artist": "...", "album": "...", "tracks": [] }, "candidates": [ { "id": "cand_02", "confidence": 0.93, "artist": "...", "album": "...", "tracks": [] } ] } }
{ "type": "done", "session": "imp_01", "imported": 10, "skipped": 2 }
```

规则：

- `session` 隔离一次导入；`apply` 必须引用预览过的 candidate，不能只传字符串。
- 所有文件操作默认 `no move / no copy / no write`，直到用户逐项确认。
- 进度事件可合并；错误必须带 `stage/path/beets_message`。
- sidecar 崩溃：宿主标记导入失败，不自动重试写操作（避免半写）。

## 4. 导入流程（UI 视角）

```
选择文件夹
  → 扫描音频文件（宿主/或 sidecar）
  → beets 分组/匹配（preview）
  → UI 显示专辑卡片 + 候选 + 逐字段 diff
  → 用户：应用某候选 / 保持原样 / 跳过 / 手动编辑
  → 应用：beets 写标签（可选移动/复制）
  → 宿主刷新曲库 + 封面 + 统计
  → 显示结果/失败项/可重试
```

### 4.1 差异 UI 必须显示什么

- 当前标签 vs 候选标签：`title/artist/album/albumartist/year/track/disc/genre`。
- 置信度与来源（MusicBrainz id/URL）；低置信度不要默认勾选。
- 文件动作：`只写标签` / `复制到 library` / `移动到 library`（默认只写标签）。
- 封面变化：当前/候选；是否替换。
- 逐曲目可展开；专辑级“全选/全不选”。
- 冲突：目标路径已存在、文件只读、beets 库已有同 item。

### 4.2 不要做的交互

- 不要 `beet import -q` 静默自动选择；这会把错误标签写进库。
- 不要在预览阶段移动/写文件；预览必须无副作用。
- 不要一次导入 1,000 张专辑不设上限；分页/批处理 + 可取消。
- 不要把 beets 的文本输出直接塞进 UI；用结构化字段。

## 5. 数据一致性

### 5.1 自有 DB 还是 beets DB 当主？

**推荐：自有 DB 是“读模型”，beets 是“写权威”。**

- 自有 DB 存：曲目/专辑/艺人、播放列表、队列、历史、设置、封面缓存。
- beets 存：标签匹配结果、写标签、移动/复制后的路径。
- 应用写操作后：
  1. sidecar 返回受影响的 `item_id/path`；
  2. 宿主重新扫描这些文件/路径；
  3. 更新自有 DB + 封面 + 队列中的路径引用；
  4. 发 `library_changed` 事件。
- 不在宿主里复刻 beets 的 autotag 逻辑；也不让 UI 直接读 beets DB。
- 可选字段 `beets_item_id` 存在 tracks 表，便于后续同步；没有也不影响播放。

### 5.2 为什么不直接读 beets DB 当主

- 插件会加字段/改 schema；耦合会把 UI 绑死在用户配置上。
- beets 不在时应用要能跑；自有 DB 是 fallback。
- 播放队列/历史/封面是播放器语义，不该塞进 beets。

### 5.3 同步触发

- 手动“从 beets 同步”按钮（v1）。
- 导入/写标签完成后自动刷新受影响项。
- watcher 只作为后续优化；不要用 watcher 保证正确性。

## 6. 安全与可逆

| 风险 | 措施 |
|---|---|
| 写错标签 | 预览 diff；默认不自动应用低置信度；保留 beets 的 `original` 字段（若启用） |
| 移动/复制破坏目录 | 默认只写标签；移动/复制需显式选择；目标冲突先报错 |
| 半途失败 | 按专辑/曲目粒度记录结果；失败项可重试；不自动回滚已写文件 |
| 插件执行任意代码 | 只加载用户已有 beets 配置；诊断页显示插件列表；不自动安装插件 |
| beets 版本漂移 | 锁定 2.14.x；启动检查版本；API 变更时给明确错误 |
| 大库卡死 | 分页、进度、取消；sidecar 独立进程，崩溃不拖垮播放 |

可选“撤销”：写标签前把原标签快照存到 `import_journal`（JSON），提供“恢复标签”（不保证文件移动可逆）。这是 P2，不要阻塞 v1。

## 7. 打包与许可

- beets：MIT；Python：PSF；两者都可与 GPLv3 应用一起分发。
- v1 建议**不捆绑**，只检测用户安装；安装指引放设置页。
- v2 捆绑时：
  - 用 `python-build-standalone` 或 `uv` 生成独立 runtime；
  - 只装 beets 必需依赖，避免把用户环境搞乱；
  - 保留 licenses/notices；提供 beets 源码链接；
  - 安装包签名覆盖 sidecar。
- 不要调用系统全局 Python 的 `pip install` 改用户环境。

## 8. 分阶段

| 阶段 | 交付 | 验收 |
|---|---|---|
| B0（2–3d） | `beet --version`/config 检测；sidecar `status`；设置页显示 | 没装 beets 时有清晰指引，不崩 |
| B1（3–5d） | `preview_import` + 专辑/候选/差异事件；只读 UI | 选一个文件夹，能看到候选与 diff，无文件改动 |
| B2（3–5d） | `apply_import`：写标签（默认不移动）；刷新曲库 | 应用后标签/封面更新；失败项可见 |
| B3（2–3d） | 移动/复制选项、冲突处理、批量/取消 | 目标冲突不覆盖；可取消 |
| B4（2–3d） | 同步/统计/诊断；可选撤销日志 | 大库不卡；错误可定位 |

**这是新版最值得投入的 2 周。** 音频引擎可以买/借，beets 审阅体验买不到。

## 9. 待确认

1. 你的 beets 是 pipx/uv/系统 Python 哪种安装？是否愿意 v1 依赖已安装？
2. 导入默认动作：只写标签，还是复制到 beets library？我建议**只写标签**，移动/复制显式开启。
3. 是否接受 Python sidecar（独立进程、stdio JSON）？这是拿到候选/diff 的唯一稳路。
4. 你是否用 beets 插件（如 `fetchart`、`lyrics`、`mbcollection`）？插件列表会影响 sidecar 启动参数与诊断。