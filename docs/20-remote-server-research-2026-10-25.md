# docs/20 — 远程存储 / 容器部署版调研报告

日期：2026-10-25
性质：**调研**（不含实现）。结论供拍板，拍板后另立 ADR 与批次计划。

## 0. 需求原文拆解

> 项目支持远程存储；线上容器部署；使用和本地端同结构的 DB 和基础功能（不超出浏览器壳范围）；单独支持 FM 式歌单推流播放 m3u8。

拆成四条硬需求：

1. **容器部署**：服务端跑在线上（Docker），音乐文件存服务端（"远程存储"的真实含义 = 库不在本机）。
2. **同结构 DB**：服务端使用与本地版相同的 SQLite schema（`aiya-data/migrations/` 全套），基础功能（曲库浏览/歌单/队列/历史/歌词/诊断）不缩水。
3. **浏览器壳**：客户端是浏览器页面，不是 Tauri 窗口。"不超出浏览器壳范围"= 凡浏览器 API 给不了的能力（WASAPI 独占、本地文件系统、sidecar）直接放弃或找替代，不做任何"让浏览器假装有"的尝试。
4. **FM 式 m3u8**：一条**独立于常规播放**的链路——歌单以 HLS（m3u8）形式推流播放。

## 1. 已核实事实

### 1.1 仓库侧（本次实测）

| 事实 | 证据 | 对本调研的意义 |
|---|---|---|
| IPC 有唯一换气口 | `ui/src/lib/ipc/transport.ts`：`setTransport()` 注入，浏览器 mock 已整体替换并跑通 E2E 4/4 | 远程传输是**第三个 Transport 实现**，不是改 53 处调用 |
| 53 命令的参数收 `Json` 复合对象 | docs/17 §3.2 | HTTP 化时形状不变：`POST /ipc/{cmd}` + JSON body 即可 1:1 映射 |
| 返回值类型由 ts-rs 生成（42 类型） | `npm run gen:types:check` 无漂移 | 服务端用同样的 serde 结构序列化，前端生成物**零改动** |
| DB 是 rusqlite + 同步连接 | `aiya-data/Cargo.toml`；schema 在 `migrations/0001_init.sql`（artists/albums/tracks/artworks/playlists/playlist_items/queue_state/queue_items/history/settings） | 同结构 DB = 直接复用 `aiya-data` crate，重活已经在里面 |
| `tracks.path` 是 `TEXT NOT NULL UNIQUE`，且 `ids::track_id` 派生自路径 | 0001_init.sql L57-60；文档头注明确"ids::track_id 的派生逻辑不动" | **路径模型是远程化最大的改造点**，见 §4.3 |
| 音频所有权在 Rust（ADR-0006），PCM 禁止进 WebView（ADR-0003） | ADR | 与浏览器壳**物理冲突**，见 §2——这是全篇最重要的结论 |
| 频谱渲染器已与数据源解耦 | 大件 B（fe67a65）：自研五形态吃 48 带数组，`SpectrumSource` 是接口 | 浏览器版加一个 `AnalyserSpectrumSource`（Web Audio）即可，渲染器复用 |
| UI 已验证纯浏览器可跑 | mock 模式 + E2E 全绿 | 浏览器壳不是假设，是已验证事实 |

### 1.2 外部侧（2026-10-25 检索核实）

| 事实 | 证据 |
|---|---|
| hls.js 当前稳定 v1.7.3（2026-09-11），Apache-2.0，~1030 万周下载 | forasoft 2026-10-19 深度指南 + GitHub repo |
| hls.js 官方支持 **audio-only**：Packed Audio 段（AAC/MP3/AC-3）、TS 里 AAC/MP3、fMP4、ALAC | hls.js GitHub Features 列表 |
| Chrome 142+ 原生 HLS，但官方建议仍 `Hls.isSupported()` 先行（原生分支拿不到 rendition 控制，且 Chrome 147 原生路径有已知失败案例） | hls.js README + FastPix 2026 指南 |
| 所有带 MSE 的浏览器（Chrome/Edge/Firefox/桌面 Safari）都能跑 hls.js；iOS 17.1+ 走 Managed Media Source | 同上 |
| m3u8 服务需要 CORS + 正确 MIME（`application/vnd.apple.mpegurl`）+ 段的 Range 支持，"本地能播线上不能播"基本都是 CORS/MIME 配错 | PiStack 2026-08 对比文 |
| axum 当前 0.8.9（2026-04-14，tokio 团队，MIT，MSRV 1.80），crates.io 本身用它；tower-http 提供 Range/ServeDir/SSE | lib.rs + 多源交叉 |
| hls.js 内置 AES-128 解密 | GitHub Features 列表（FM 频道若要防盗链可用） |

## 2. 核心结论：ADR-0006 在浏览器壳里必须反转

本地版的天条是"音频所有权归 Rust 后端"（ADR-0006），理由是 WASAPI 独占、进程级隔离、PCM tap。**浏览器壳里这条物理上不可能**：页面拿不到服务器的声卡；把 PCM 推给浏览器又直接违反 ADR-0003 且毫无收益（浏览器自己就有解码器）。

所以远程版的音频所有权**回到浏览器的媒体元素**：

- 常规曲目：`<audio src="/audio/{trackId}">` + HTTP Range。
- FM 频道：`<audio>` + hls.js 喂 m3u8。
- 音量/暂停/进度：元素属性 + Web Audio GainNode（音量 ramp 用它做，`element.volume` 无平滑）。
- 频谱：`MediaElementAudioSourceNode` → `AnalyserNode` → 48 分带数组 → 现有五形态渲染器。**远程版反而救活了当初被架构杀死的"真 WebAudio 节点"路线**（docs/18 §9 第 7 条的镜像情形）。

这不是妥协，是边界内的唯一解；关键在于**服务端还剩什么职责**：

| 职责 | 本地版 | 远程版 |
|---|---|---|
| 队列/顺序/随机/repeat 状态机（aiya-player Actor） | Rust | **服务端保留**（queue_state/queue_items 表本来就在服务端 DB 里） |
| 解码/输出 | sidecar | 浏览器 |
| 播放位置（position/进度条） | Rust 产生，10Hz 广播 | **客户端产生**，节流上报（1–2s 一次）回填 history/last_played |
| 频谱 | Rust tap | 客户端 AnalyserNode |

由此推出远程版的新契约形状：服务端是**"队列预言机 + 曲库 API + 流媒体源"**，客户端是"渲染 UI + 真出声"。`player:state` 10Hz 广播在远程版消失（进度本来就生在客户端），换成：客户端操作（next/prev/playAt/queue 变更）→ 服务端返回新快照 + 要播的 trackId/URL。

## 3. 方案总览

```
┌─ 浏览器 ──────────────────────────────┐
│  现有 UI（Svelte，零改动或近零改动）      │
│  Transport#3: RemoteTransport           │
│   ├ 命令: POST /ipc/{cmd}  (JSON)       │
│   ├ 事件: SSE  /events                  │
│   ├ 音频: <audio> + Web Audio Graph     │
│   └ FM:   hls.js 1.7.3 → <audio>       │
└────────────┬────────────────────────────┘
             │ HTTPS（反代终结 TLS）
┌─ 容器（单镜像）────────────────────────┐
│  aiya-server (axum 0.8.9, 新 crate)     │
│   ├ /ipc/*      53 命令映射（复用 lib.rs │
│   │              的命令体，去掉 Tauri）  │
│   ├ /events     SSE                     │
│   ├ /audio/:id  Range 流（path→流）      │
│   ├ /art/:id    封面（替代 aiya-artwork 协议）│
│   ├ /fm/:ch/*.m3u8  FM 频道播放列表      │
│   └ /segments/  HLS 段静态服务           │
│  aiya-data（原样复用，SQLite WAL）        │
│  ffmpeg（FM 切段/转码子进程）             │
│  卷: /data/aiya.sqlite + /music          │
└─────────────────────────────────────────┘
```

### 3.1 传输层（改动最小的关键）

`transport.ts` 的 `setTransport()` 就是为此预留的缝。新增 `RemoteTransport`：

- `invoke` → `fetch(POST /ipc/{cmd}, JSON args)`；服务端把 `ApiError` 序列化进响应体 + HTTP 409/404/500 分类码，客户端按现有归一化逻辑收——**client.ts 一行不改**。
- `listen` → `EventSource('/events')`，事件名对齐现有通道（`scan:progress`、`beets:import` 等服务端事件）。`spectrum:frame` 在远程版不存在（频谱改客户端产出）。
- mock Transport 已经证明"换 Transport = 全 UI 工作"，这条是**最低风险**的一环。

### 3.2 服务端 crate

- 新建 `crates/aiya-server`（或顶层 `server/`）：axum 0.8.9 + tokio（full）+ 复用 `aiya-data`、`aiya-player`（队列 Actor 可以原样跑在服务端——它只管顺序不碰音频，副作用是返回 trackId）。
- `src-tauri/lib.rs` 里 53 个命令体应尽量抽成与 Tauri 无关的函数（当前耦合在 `State<AppState>` 上，抽一层 trait 或直接挪 AppState 到 server crate）。
- aiya-player 的 Actor 在服务端跑的注意点：它是同步 actor（线程），axum handler 经 channel 通信即可，与 sidecar 模式同构。

### 3.3 同结构 DB 与路径模型（最大改造点，三选一）

| 方案 | 做法 | 代价 | 判定 |
|---|---|---|---|
| **A. 容器内路径**（推荐起步） | 扫描容器内 `/music` 挂载卷，`tracks.path` 存容器路径，schema **零改动**，scanner/repo 原样跑 | 无 schema 改动；缺点：曲库必须先落到容器文件系统 | ✅ R1 采用 |
| B. 对象键（S3/MinIO） | `path` 存对象键，scanner 改为列对象，流式播放经预签名 URL | scanner/ids/path_lower 语义全要动，且"同结构 DB"名存实亡 | ❌ 与需求 2 冲突，列为远期 |
| C. 本地卷 + WebDAV/网盘挂载 | rclone 挂载远程盘进容器 `/music` | 零代码改动但运维面脏（挂载抖动=扫描假删） | 备选，作为用户自选部署形态记录 |

两个必须写明的坑（来自 schema 头注）：

1. `ids::track_id` 从 path 派生且**约定不改**——方案 A 下天然成立；任何把 path 换成语义键的方案都会连锁破坏 id 稳定性，这是 B 被否的根因之一。
2. `path_lower` 内联了 Windows 大小写不敏感语义——Linux 容器里它退化为 `lower(path)` 的普通 UNIQUE 列，不影响正确性，不用改。

### 3.4 常规曲目播放链路

- `GET /audio/{trackId}`：服务端按 id 查 path → 流式返回 + `Accept-Ranges`（tower-http 或手写 Range；浏览器 `<audio>` seek 依赖它）。
- 内容类型按扩展名/探测给；封面 `GET /art/{albumId}` 替代 `aiya-artwork` 协议（DTO 里 coverUrl 字段在远程版由服务端填 HTTP URL，本地版填 `http://aiya-artwork.localhost/...`——DTO 字段名不变，值换源）。
- CORS：m3u8/段/音频都要 `Access-Control-Allow-Origin`（同域部署时天然满足；若 UI 与 API 分域则反代统一）。

### 3.5 FM 式 m3u8（需求 4，独立链路）——三方案对比

| | A. 真·直播电台（服务端 ffmpeg 实时转码 → live HLS） | **B. 个人频道（服务端按 FM 队列生成 m3u8，hls.js 顺序播）✅推荐** | C. 假推流（前端顺序 `<audio>` + auto-advance） |
|---|---|---|---|
| 形态 | 所有人同一条流，真"推流" | 每人一条自己的频道（VOD/EVENT 型 m3u8） | 无 m3u8，普通播放 |
| 延迟 | 6–30s（live 边缘） | 首段 1–3s | 0 |
| 逐用户控制（切歌/跳过） | ❌（会打断所有人） | ✅（重新生成/换 playlist URL） | ✅ |
| 服务端成本 | ffmpeg 长驻进程 ×1 | ffmpeg 切段（可预切 + 懒切） | 无 |
| "m3u8" 成色 | 真 live | 真 HLS（hls.js 官方支持 audio-only packed audio） | ❌ 不满足需求原文 |
| 复杂度 | 高（进程管理、断流恢复、ABR 无意义） | 中 | 低 |

**推荐 B**：满足"m3u8 推流"字面与体验（FM = 打开就放、自动连播、随时跳下一首），又是单人可控行为。A 作为远期"公共频道"扩展（同架构加一个 ffmpeg 长进程 + EVENT playlist 即可，不推翻 B 的任何组件）。

B 的实施要点：

1. **切段**：扫描时对 FM 候选曲目用 ffmpeg 转 AAC + 按 6–10s 切 segment（`-f hls -hls_time 6 -hls_segment_type audio`，packed audio 段 hls.js 官方支持）。首次进入频道时懒切 + 缓存（`/segments/{trackId}/seg-N.aac`），避免全库预转。
2. **playlist 生成**：`GET /fm/{channelId}/index.m3u8`——`channelId` 对应服务端 FM 队列（可复用 queue_items 形状或独立 fm_state）；返回 `#EXTM3U + #EXTINF` 序列指向 `/segments/...`。**切歌/加歌 = 服务端改队列 + 客户端换 playlist URL 重载**（hls.js `loadSource` 幂等，成本 ~首段一个 RTT）。
3. **无缝连播**：HLS 天然跨曲目连续（一个 m3u8 列 N 首），比 `<audio>` 逐曲换源更接近"FM 不间断"的听感；跨曲目 gapless 受段边界限制，FM 场景可接受（真电台本来就有微间隙）。
4. **hls.js 接入**：`Hls.isSupported()` 优先（Chrome/Edge/Firefox 全覆盖），Safari 原生兜底分支照官方模式写；错误处理用 `Hls.Events.ERROR` 的 fatal 分支重建实例。
5. MIME：m3u8 = `application/vnd.apple.mpegurl`；AAC 段 = `audio/aac`（或 `audio/mp4` 若 fMP4 段）。CORS 头必备。

### 3.6 浏览器壳功能矩阵（诚实清单）

| 功能 | 远程版状态 | 替代 |
|---|---|---|
| 曲库/歌单/队列/历史/歌词/诊断 | ✅ 原样（纯 DB + IPC 映射） | — |
| 封面 | ✅（HTTP 端点替代 aiya-artwork 协议） | — |
| 频谱（compact+pro 五形态） | ✅ **更真**（AnalyserNode 真数据） | 新 `AnalyserSpectrumSource`，48 分带逻辑客户端复刻（FFT 2048 + 对数分带，照 aiya-audio/spectrum.rs 抄参数） |
| 音量/静音 | ✅ | GainNode（要 ramp） |
| EQ | ⚠️ 可做 | Web Audio BiquadFilterNode 链（二期，与本地版参数模型对齐即可） |
| WASAPI 独占/时钟/缓冲设置 | ❌ 不可能 | 设置页输出分区在远程版隐藏（`isTauriRuntime()` 已有，加一个能力位） |
| 防休眠、托盘、SMTC、媒体键 | ❌ | MediaSession API 兜媒体键（一行 metadata），其余放弃 |
| 快捷键 | ✅（页面级 keydown，本来就是前端实现） | — |
| beets 集成 | ⚠️ | 容器内装 beets+Python 可行但重；远程版首期隐藏入口 |
| 扫描 | ✅ 服务端扫 `/music` 卷，进度走 SSE | — |

## 4. 分期计划（拍板后细化）

| 期 | 内容 | 验收 |
|---|---|---|
| R0 spike（1–2 天量级） | aiya-server 骨架：只读 `/api/tracks`（游标分页）+ `/audio/:id` Range + 静态页 + 浏览器 `<audio>` 播一首含 seek | 真容器里 curl Range + 浏览器播/seek/封面，记录实测延迟 |
| R1 基础功能 | RemoteTransport（53 命令 + SSE）+ aiya-player Actor 服务端化 + 同结构库扫描入容器 | 现有 UI 连远程端全功能可用；E2E 加"远程模式"跑现有 4 条 |
| R2 FM 频道 | ffmpeg 懒切管线 + 动态 m3u8 + hls.js + FM 队列 API/UI | 真机连播 10 首不断流；切歌 ≤1s 生效；Chrome/Edge/Safari 三端实测 |
| R3 打磨 | 鉴权（token/反向代理示例）、诊断页远程真值、设置项能力位裁剪、Dockerfile + compose | 镜像一键起；文档给部署样例 |

## 5. 风险与待实测（Spike 清单）

按项目惯例，以下每条"不接受应该可以"，R0/R2 必须实测：

1. **Range 流经反代的 seek 表现**（Nginx/Caddy 缓冲策略会影响拖动体验）——R0 实测两种代理。
2. **ffmpeg 懒切的首段时延**：点击进入 FM 频道到出声，目标 <3s；若慢，切两档（进入时先切前 3 段，后台补齐）。
3. **容器里 SQLite WAL 的落盘性能**：单写者（aiya-data 现有串行用法）问题不大，但要测扫描 5 万首容器内 IO 基线，对齐旧基线 366–403ms 冷启动口径。
4. **iOS Safari 原生 HLS 分支与 MediaElementSource 的兼容**（原生分支绕过 hls.js，频谱要确认 AnalyserNode 拿得到数据）——不在桌面三端验收内，列为已知限制。
5. **53 命令里混着本地专属命令**（reveal_in_explorer、盘符扫描等）：远程版返回 `notSupported` 类 ApiError（现有错误模型直接容纳，`AudioErrorCode` 同款分层），前端按能力位隐藏入口，而不是留死按钮。
6. **aiya-player Actor 的线程模型搬进 axum**：actor 是专用线程 + channel，理论上与 sidecar 同构；但 pending_stop/LoadAt 等时序语义（任务 6 修过的）在"客户端出声"下需要重新定义哪些由服务端状态机管、哪些退化成客户端行为——这是 R1 的设计重点，需要一份小契约文档。

## 6. 第二轮补充调研（2026-10-25 10:40，按用户四点反馈）

### 7.1 Liquidsoap 做 HLS 切片（不用 Icecast）——核查结果：可行，且比 ffmpeg 自管更稳

外部事实（liquidsoap.info 官方文档 + GitHub 讨论 + repo README，2026-10-25 核实）：

| 事实 | 证据 |
|---|---|
| `output.file.hls` 自 1.4.x 起支持，直接落盘 m3u8 + 段，**完全不需要 Icecast**（官方讨论原话："If you don't need to use Icecast - you can remove output.icecast"） | liquidsoap.info/doc hls_output + savonet discussion #3826 |
| 音频-only 官方支持：`%ffmpeg(format="mpegts", %audio(codec="aac"...))` 单流即可；fMP4 段 2.4.3 起完整支持 | 同上 |
| `persist_at` 重启续播：关停时保存状态，重启接着既有段/播放列表继续（频道进程升级不炸听众） | hls_output 文档 |
| 滚动窗口 `segments=N, segments_overhead=M`：播放列表只留 N 段，多留 M 段给落后听众；过期段自动清理（不用自己写 GC） | 同上 |
| 频道控制命令现成：`request.queue(interactive=true)` 注册 `push / queue / skip / flush_and_skip`；控制面走 TCP telnet 或 **Unix socket**（容器内最合适，官方注明无鉴权——所以绝不能对外暴露 socket） | 官方 server 文档 + request.liq |
| 版本与镜像：稳定 2.4.2 / rolling 2.5.x，官方 Docker `savonet/liquidsoap`（amd64+arm64），内置 FFmpeg 7/8（FLAC/ALAC/AAC/MP3 解码全有，FM 链路不需要预转码） | repo README 版本表 |
| `on_file_change` 钩子可把段同步到 S3 等远端存储（将来"存储也在容器外"的扩展点） | hls_output 文档 |

**FM 方案修订**：第一轮推荐的"方案 B（ffmpeg 懒切 + 动态 VOD m3u8）"改判为**备选**，主方案改为 **Liquidsoap 每频道一个实时流**：

```
aiya-server (axum)
  ├─ 频道生命周期：活跃频道 → spawn savonet/liquidsoap 子进程（模板脚本 + 频道 id）
  │    脚本：request.queue(interactive=true) → cross/skip 防护 → output.file.hls
  │    产出：/data/hls/{channelId}/live.m3u8 + 滚动段（segments=6, overhead=4, 2–4s/段）
  ├─ 控制面：Unix socket 发 push/skip/flush_and_skip（下一首/加歌/清空跳播）
  │    队列真值仍在 SQLite（fm_queue 表），socket 只是运行时镜像；
  │    频道（重）启时按 DB 种子回灌
  └─ 分发面：axum 静态托管 /hls/{channelId}/*（m3u8 MIME=application/vnd.apple.mpegurl，段带 CORS）
       浏览器端 hls.js 挂到 live 边缘；桌面客户端同样吃这个 URL
```

两种 FM 形态的真实差别（写清楚为什么值得换）：

| | Liquidsoap 实时流（新主方案） | ffmpeg 懒切 VOD（第一轮 B） |
|---|---|---|
| "FM 感" | 真·电台：连播时间轴连续、join 即在播、skip 生效 ≤1 段（2–4s） | 顺序播放一个静态列表，更像"排队放歌" |
| 逐用户控制 | 每人一条自己的频道 = 每活跃频道一个进程（隔离干净）；同频道多人共享进度 | 天然逐用户（每请求一份 playlist） |
| 服务端状态 | 进程 + socket，liquidsoap 负责流韧性（mksafe/persist），我们只管队列 | 全部自己写（切段、playlist 生成、续切状态机） |
| seek/回听 | ❌ 不可回听（live 窗口外段已删） | ✅ 可拖 |
| 成本 | 每活跃频道一个 OCaml+ffmpeg 进程（估几十 MB 级 RSS，需实测） | 无常驻，按请求切 |

判定：用户明确"FM 式"且点名 Liquidsoap——**直播形态就是 FM 的本义**，可回听交给常规播放链路（`/audio/{id}`）承担，两者职责分立。需实测项：单频道进程 RSS 与上下限（>50 并发频道时改"单进程多 output"脚本再生模型，`persist_at` 保证重启不丢段）。

### 7.2 浏览器 `<audio>` 与当前前端框架的兼容性——实证结论：零耦合，直接插

实测仓库：**UI 全部组件没有任何 `<audio>` 元素或 `new Audio` 调用**（grep 零命中）。播放状态 100% 来自 Rust 的 `PlayerState` 快照（`positionMs/durationMs/volume/rate/index/...`），`PlayerBar` 只负责渲染。这就是说前端框架**天生引擎无关**，浏览器音频引擎是一个新生产者，不是一次改造：

```
BrowserAudioEngine（新模块，仅远程模式装配）
  ├ <audio> 元素：src=/audio/{id}（常规曲目）或 hls.js attach（FM 频道）
  ├ 本地时钟：timeupdate/rAF 合成 positionMs（10Hz 对齐现有 tick 语义），
  │   durationMs=element.duration，volume=GAIN 节点（要 ramp，element.volume 无平滑），
  │   rate=element.playbackRate
  ├ 队列命令（next/prev/playAt/queue 变更）→ 服务端队列 API → 返回应播的 trackId
  ├ 本地合成完整 PlayerState 快照（形状=generated/PlayerState，字段级一致）
  └ 不支持的命令（独占/时钟/缓冲/audioMode）→ 回 ApiError.notSupported（现有错误模型容纳）
```

设置页的输出分区（模式/缓冲/时钟/暂停释放）在远程模式整体隐藏——`isTauriRuntime()` 已有，补一个能力位即可。频谱照 §2 结论走 AnalyserNode。**风险点**：`PlayerState` 里 `positionMs` 的推进时钟从 Rust 换到 JS，进度条平滑度依赖 rAF 合帧，与现有"事件合帧"约定一致，无新债。

### 7.3 beets worker 容器化——判定：支持，沿用现有 sidecar 协议

现状：beets 集成是 Python sidecar + stdio JSON（B0–B3），`aiya-server` 以同构方式 spawn 它即可。容器内装 `python3 + beets 2.14.x`（pip，支持 3.10–3.14），beets 库与音乐同挂 `/music` 卷；"自有 DB 是读模型、beets 是写权威"的架构在远程版原样成立。两个容器化特有注意点写进实施清单：① beets config 的 `directory` 必须显式指向容器路径（旧坑：不显式传会落到 platformdirs 默认目录）；② 首写迁移备份提示会污染 stdout（协议层已容错，容器内复测一次）。

### 7.4 部署形态：全栈（带 Web）与无头（仅接口）——API 必须分面

用户期望两种容器形态都要成立，这改变 API 设计的分层方式：

```
数据面 API（客户端无关，两种形态都暴露）
  /ipc/*（53 命令映射）、/events（SSE）、/art/*、/hls/*（FM 流）、
  歌单/历史/诊断/beets 触发 —— 消费者：浏览器 UI、Tauri 桌面版、第三方客户端
音频面（按客户端能力分叉，不是 API 分叉）
  浏览器：/audio/{id}（Range）+ /hls（FM）→ <audio>/hls.js
  桌面版：连远程库（数据面）+ 本地 WASAPI 出声 —— "远程存储"对本地播放器的真实含义
```

**桌面版连远程库**是第 4 点最有价值的推论：`RemoteTransport` 在桌面端同样适用（transport 缝不挑宿主），数据面全走容器，音频面保持 ADR-0006 本地原生。新增一个真实工作项：**aiya-audio 需要 HTTP 流式解码输入**——现在 Symphonia 从文件读，远程曲库要一个 Range Range-reader（reqwest 实现 `Read+Seek`，seek 用 Range 请求回退）喂解码器。这是 R1 桌面连远程的前置项，工作量中等，先做 spike 实测拖动延迟。

镜像形态：

| | 全栈镜像 | 无头镜像 |
|---|---|---|
| 内容 | axum + ui/dist 静态托管 + beets sidecar + liquidsoap | 同左，去掉静态托管（或仅留最小引导页） |
| 客户端 | 浏览器直接用 | Tauri 桌面版 / 第三方 / 独立托管的前端 |
| 配置开关 | `AIYA_SERVE_UI=true/false` 一项环境变量即可，同一镜像两用 | — |

判定：**同一镜像、一个环境变量切两种形态**，不做两个镜像（构建/版本漂移成本高于收益）。

### 7.5 修订后的决策点（替代 §6 尾部三条）

1. ~~路径模型~~ → **容器内路径**（§4.3 方案 A）默认采纳，无异议即不再议。
2. ~~FM 方案~~ → **Liquidsoap 每频道实时流**为主（§7.1），ffmpeg 懒切 VOD 为备选（若实测发现频道进程成本不可接受再回退）。
3. **EQ 不做**（原生音频项，远程版无此概念）——已定。
4. **beets 做**（worker 模型容器内照跑）——已定。
5. 桌面版"连远程库"是否纳入第一批（牵出 HTTP 流式解码前置项）：建议作为 R1.5 独立小批，不阻塞浏览器线。

### 7.6 对分期计划的修订

- R1 增加：`AudioEngine` 双实现接口抽取（本地 NativeEngine 包现有路径，BrowserEngine 新写）+ 设置页能力位。
- R1.5（新，可与 R2 并行）：桌面版 RemoteTransport + HTTP Range 流式解码 spike（Symphonia 喂流 + seek 实测）。
- R2 改为：Liquidsoap 频道管理器（spawn/socket/DB 种子回灌）+ axum 静态分发 + hls.js 播放器页；验收加"单频道进程 RSS/上下限"与"skip 生效时延 ≤1 段"两条实测。
- R3 不变（鉴权/诊断/裁剪/compose），加一条：beets 容器内端到端走一遍 B1–B3。

## 7. 结论（第二轮修订）

- **Liquidsoap 方案成立**：`output.file.hls` 免 Icecast、音频-only 官方支持、`request.queue` 控制命令现成、`persist_at` 重启续播、官方多架构 Docker 镜像。FM 主方案改判为"每频道一个实时流"，与"可回听"的常规播放链路职责分立。
- **浏览器 audio 兼容性不是风险而是既成事实**：UI 零 audio 元素、全靠 PlayerState 快照，`BrowserAudioEngine` 作为新生产者插入 store 即可，UI 零改动。
- **beets 容器内照跑**（sidecar 协议原样），**EQ 出局**（已定）。
- **两种部署形态收敛为一个镜像 + 环境变量**，API 按"数据面（客户端无关）/音频面（按能力分叉）"分层；由此解锁桌面版连远程库（真·远程存储），代价是 aiya-audio 的 HTTP 流式解码前置项。
