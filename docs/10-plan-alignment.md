# 10 — 底盘架构规划对齐评估

> 输入：《本地 PC 音乐播放器：底盘架构规划》（2026-10-23，用户采纳为计划结论，下称"底盘规划"）。
> 对照物：本仓库 docs/00–09 + ADR-0001..0005。
> 结论：**方向一致度约八成，底盘规划采纳为基线补充**。四处冲突按本仓库实测证据裁决（§3）；引擎决策因"PCM 必要采样 tap + EQ/LRC/WASAPI 联合设计"的要求**改判为原生首发**（[ADR-0006](adr/0006-native-engine-first.md)）。

## 1. 完全一致（确认，无改动）

| 主题 | 底盘规划 | 本仓库文档 |
|---|---|---|
| 分层边界：UI/曲库/播放/任务各有生命周期，IPC 是通信边界 | 原则 1、§一 | 02 §3、03、ADR-0003 |
| SQLite 只由 Rust 访问；PCM/大二进制不进 DB | 原则 2、§三 | 02 §5.1、ADR-0002 |
| 播放引擎独立于 UI 生命周期 | 原则 4 | ADR-0001/0003、02 §10 |
| 本地播放 Rust 直读文件；WebView 资源走自定义协议 | §四 | ADR-0005（音频不走协议；封面 `aiya-artwork://`） |
| WebSocket 不做本地音频链路、不传 PCM | §四、坑 4 | 03 §7、ADR-0001 否决项 4 |
| 封面文件缓存 + DB 引用/hash/临时文件原子写 | §四.2 | 05 §4、ADR-0005（旧仓库 artwork-cache 实测经验一致） |
| 频谱：Rust 算 FFT，前端只收分带数组 | §五.2 | 02 §6.2、04 §5 |
| EQ 在 PCM 数据路径中、先于输出 | §五.3 | 04 §4 |
| LRC 与引擎解耦，引擎只提供位置 | §五.4 | 03 §5.3、02 §9 |
| 播放状态 Rust 权威，前端是投影 + 低频同步 + 本地外推 | §五.1 | ADR-0003、02 §6 |
| beets 独立 worker、不进主链路、写权分离 | §七.3 | 08、ADR-0001 |
| 长列表 = DB 分页 + IPC 窗口 + 虚拟列表协同 | §八.2 | 00 §2、07（已有实现并 5 万行实测） |
| IPC 按领域封装、不散落 invoke；DTO 与 schema 分离 | §九 | 02 §6、03 §6.6、ADR-0004 |
| WAL/外键/参数化 SQL/事务不包长 IO | §三.3 | 02 §5.1 |
| 远程服务（WebDAV/Liquidsoap）是适配器，不是核心域 | 原则 5、§六 | 02 非目标、README"现在不做什么" |
| 不把本地路径当曲目身份 | 坑 1 | 00 §1.19（标签指纹 ID，已满足） |

## 2. 采纳进基线（新增/补充）

1. **PCM"必要采样复制"tap（动画级频谱）**——本次引擎改判的触发点。设计落在 04 §5.1、ADR-0006：订阅制、旁路复制、关闭零开销、不破坏 bit-perfect。只做动画效果，不做专业分析（声学加权/标定都不做）。
2. **EQ、LRC、WASAPI 输出与 PCM 链路联合设计**——04 §5.2：同一 DSP 链、同一位置时钟，接口一次定形（EQ 实现仍后置）。
3. `tracing` + `tracing-subscriber` 结构化日志进第一版选型。
4. 任务队列语义（幂等、重试、租约、`queued→running→succeeded/failed/cancelled`）——v1 仍用内存调度（范围纪律），设计记录在案，jobs 落地时照此实现。
5. watcher 三原则（debounce 合并、文件稳定性检查、周期对账为最终依据）——与旧仓库 chokidar 实测一致（250ms 内建删=0 事件、改名只报 add）；v1 后置不变，对账为主的思想已吸收。
6. `track_analysis` 用 `(track_id, analyzer, version)` 复合身份记录分析来源/版本——分析功能后置，schema 形状已记录。
7. `playlist_items` 独立自增 ID + 排序列（同一曲目可重复加入歌单）——并入 schema v1 细节：`playlist_items(id INTEGER PRIMARY KEY, playlist_id INTEGER NOT NULL REFERENCES playlists(id) ON DELETE CASCADE, position INTEGER NOT NULL, track_id TEXT NOT NULL REFERENCES tracks(id) ON DELETE CASCADE)`。
8. 排序稳定性：排序键追加唯一 ID（如 `title_norm, id`）——与旧实现 `library_order` 物化位置表一致，确认。
9. 远程凭据入 OS 凭据库（Windows Credential Manager），不明文进 `config_json`——记录为 storage 适配器实现约束。
10. 时间戳统一 UTC Unix 秒——记入 schema 约定。
11. UI：App Shell 稳定挂载、设计 token、focus 样式、reduced-motion——作为 UI 二期重写指引（见 §3.2 裁决）。

## 3. 冲突与裁决（实测证据优先）

### 3.1 搜索：FTS5 vs LIKE —— 裁决：保持 LIKE

底盘规划建议"搜索优先考虑 FTS5"。本仓库前置实测（spike 验证报告 + 旧仓库 docs）：FTS5 `trigram` 分词器**查不到两字中文词**（"稻香"/"晴天"），`unicode61` 只支持整词/前缀；而 `title_norm + LIKE` 在 5 万行实测 3.7–4.2ms（00 §2.7、02 §5.3）。**中文曲库是主要使用场景，FTS5 是本项目已验证的坑。** 未来规模超 10 万首再评估 bigram 辅助列，不上 FTS5。

### 3.2 UI：Bits UI + Tailwind 全新界面 vs 原样搬 —— 裁决：v1 原样搬，规划作二期指引

底盘规划按"从零建 UI"给出 Bits UI/Tailwind/Lucide/路由选型与全新视觉。本仓库 07 号文档以 54 个调用点、95% 文件可原样复用、视觉已验证论证"现在重写是负收益"，且 UI-first 计划刚定（`5b4dcf2`）。**v1 复用不变**；底盘规划的 App Shell 稳定性、设计 token、无障碍清单并入 07 §6"什么时候重写 UI"的执行指引，Bits UI + Tailwind 列为二期候选栈。底盘规划自己的排序也是"基础布局与交互优先于装饰"。

### 3.3 引擎：Symphonia + Rodio/CPAL vs mpv sidecar —— 裁决：原生首发，但不采用 Rodio（ADR-0006）

- 用户新要求：频谱只要动画效果但**必须有**（mpv 无 PCM tap，做不到）；EQ、LRC、WASAPI 输出联合设计。底盘规划方向同为原生 Rust 组件。→ **v1 = Symphonia 解码 + `wasapi` crate 输出**，以 `aiya-audio-host` sidecar 进程交付（ADR-0002 独立进程决策不变）。
- **不采用 Rodio**：其 Sink/Source 模型要求把数据流所有权交给引擎，与自有 DSP 链、位置时钟、独占模式直接冲突，且不支持 WASAPI 独占。CPAL 仅作未来跨平台共享输出占位（02 非目标已声明 Linux/macOS 占位）。
- mpv 降级为 spike 对照与逃生实现（若原生 spike 暴露硬伤，回退 mpv = 明确放弃频谱、EQ 受限）。
- 代价：P2 由 1 周 → 2 周，总工期 4–6 周 → 5–7 周。换来：v1 即有链内频谱、原生 EQ 路径、bit-perfect 可证明、独占分期可控、安装包不背 ~50MB mpv。

### 3.4 多来源数据模型从 v1 建 schema vs 不预支 —— 裁决：v1 不建，前提已确认

底盘规划建议 `media_files`/`storage_locations`/`sync_state` 第一版留 schema。本仓库决策（README 决策 6、05 §10）：demo 无多来源，不预支复杂度。**维持**，且两个前提已满足：① track 身份 = 标签指纹而非路径（00 §1.19），未来加多位置是增量迁移、不是身份重写；② `StorageProvider` 抽象（底盘规划 §六）记为预留扩展点。底盘规划自己也写明"第一版可以先支持本地目录"。

### 3.5 watcher + 持久任务队列进第一期 vs 后置 —— 裁决：维持后置

范围纪律（05 风险 #8）：v1 = 启动扫描 + 手动扫描 + 内存调度；beets（P3）优先于 watcher/jobs。设计要点已记录（§2.4/§2.5），落地时不再重新设计。

### 3.6 其他小项

| 项 | 底盘规划 | 裁决 |
|---|---|---|
| 路由 `svelte-spa-router` | 桌面 SPA 页面切换 | 单主窗口应用，v1 无路由需求；UI 二期再议 |
| TanStack Virtual | 长列表库 | 已有自研虚拟列表 + 稀疏分页窗口（有测试、5 万行实测），不引入 |
| rusqlite vs sqlx | 二选一 | 已定 rusqlite + 专用 DB 线程（02 §5.1） |
| refinery vs 自建迁移 | 二选一 | 沿用 `0001_init.sql` + `user_version` runner（旧仓库经验），不引入 refinery |
| Playwright | UI 回归 | E2E 用 tauri-driver/WebDriver（Tauri 官方路径）；Playwright 可作 dev 辅助 |
| `music://` 协议名 | 示例命名 | 已定 `aiya-artwork://`（ADR-0005），且仅用于封面 |
| `track_artists`/`album_artists` 多对多 | v1 关联表 | v1 沿用 `tracks.artist/album_artist` 字符串字段（旧 UI 与查询已按此工作）；多艺人多对多等 beets 深度整合时再迁移 |

## 4. 对其他文档的影响

- 04：由"B 方案细节"升为 **v1 主方案**，新增 §5.1 tap / §5.2 EQ-LRC-WASAPI 联合设计、Rodio 选型说明。
- 05：P2 改原生引擎 2 周；总工期 5–7 周；P4 频谱 compact 随引擎交付；风险表更新。
- 06：#1 已拍板（ADR-0006）；#8 已关闭。
- 09：§3.1 修订记录（原分析保留作依据）。
- 02：§2 sidecar 实现说明更新。
- 07：§2.5 频谱不再有"mpv 无数据降级"问题。
- README：决策 1/3 更新。
