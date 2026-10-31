# 57 · 分支方案：Windows 桌面 + Web 版（Docker）

- 日期：2026-10-31
- 性质：方向拍板 + 分支方案与降级评估（含执行前验证结论）
- 关联：docs/56（高分屏缩放支持，已落地）

## 0 · 拍板

产品线两条：

1. **Windows Tauri 桌面版**：现状架构不动（sidecar WASAPI 引擎 + SMTC + 托盘/单实例 + NSIS/便携）。
2. **Web 版**：同一套 Svelte UI，服务端 Rust 单二进制（新 crate `aiya-server`），Docker 打包；播放走浏览器 `<audio>` + HTTP Range，不做原生音频驱动。

## 1 · 分支矩阵：共用什么、独有什么

| | Windows 桌面版 | Web 版 |
|---|---|---|
| UI | `ui/`（Svelte 5 + Tailwind 4 + three.js），**共用** | 同左 |
| 命令面契约 | `contracts/`（ts-rs 生成），**共用** | 同左 |
| 曲库/歌单/历史/封面/beets | `aiya-data`、`aiya-beets-*`（平台无关、零 tauri/windows 依赖），**共用** | 同左（组装进 server） |
| 播放状态机 | `aiya-player`（**纯逻辑 crate**，依赖仅 serde + ts-rs），**共用** | 同左——两端队列/模式/状态归约行为一致 |
| 设置 | `app_settings` KV + `lib/ipc/transport` 可插拔，**共用** | 同左 |
| 音频输出 | sidecar `aiya-audio-host`（Symphonia + WASAPI），**桌面独占** | `<audio>` + WebAudio probe 门 |
| 直播 | sidecar 内 ureq 拉流 + 解码（hls.rs） | server 字节代理（**新模块**，见 §4） |
| 媒体键 | SMTC（souvlaki） | `navigator.mediaSession`（三家桌面浏览器全支持） |
| 输出 | WASAPI 独占/共享、设备枚举 | `setSinkId`（仅 Chromium）；其余引导系统设置 |
| 构建 | 全 workspace（Windows dev 照旧） | `cargo build -p aiya-server`——aiya-audio / aiya-audio-host / src-tauri **不进编译图** |
| 分发 | NSIS 安装器 + 便携 zip | Docker 镜像（多阶段构建） |

## 2 · 接触面与代码组织（不做 lib.rs 重写）

### 2.1 前端：第三个 Transport 实现，零架构改动

换气口 `ui/src/lib/ipc/transport.ts` 本来就是注入式的：`Transport = { invoke, listen }` + `setTransport()`，mock 是第二个实现（97 个 case，e2e 全量跑在它上面）——命令面脱离 Tauri 可实现已被证明。Web 版加第三个实现：

- `invoke → POST /ipc/<cmd>`，`listen → SSE`。命令名、参数形状、事件名原样复用。
- `main.ts` 的宿主探测（`'__TAURI_INTERNALS__' in window`）从二态收敛为三态（tauri / http / mock），mock 保留为预览工具。
- **分支只许存在于 transport 这一层**：UI 组件永远不写 `if (isWeb)`，平台差异走 bootstrap 下发的能力标志（`backend: 'sidecar' | 'web'` + codec 表），UI 按能力渲染、禁用项标注原因。

### 2.2 后端：按域路由到既有 crates，渐进下沉

命令面 96 条全部集中在 `src-tauri/src/lib.rs`，但耦合极轻：显式 `AppHandle` 仅 7 处，其余是 `State<AppState>` + serde DTO。server 端路由**调 crate，不抄 lib.rs 逻辑**：

- `library_*` → aiya-data；`beets_*` → aiya-beets-client；`settings/ui_state/history` → 同一个 SQLite KV。
- 少量真正共享的编排（refresh.rs 扫描管线、历史裁剪、设置 diff 副作用）按需下沉到新 crate `aiya-host-core`（traits：`EventSink` / `DialogEnv` / `EngineSink`），桌面壳与 server 壳同步变薄——渐进做，不 big-bang。
- Tauri 专属域（`pick_folder`、窗口/SMTC/托盘/artwork 协议、`spectrum_set`、sidecar 控制）不进 server：命令不存在或返回 Unsupported，能力标志接住 UI。

### 2.3 事件面与状态权威

- 事件只有 5 个 channel：`player:state`(10Hz)、`spectrum:frame`(30fps)、`scan:progress`、`beets:import`、`beets:install`。Web 端 `player:state`/`spectrum:frame` **客户端自产**（rAF / AnalyserNode），SSE 只承载 `scan:progress` + `beets:*`。
- 瞬态播放状态（position/paused/buffered）在客户端（audio 元素才有真值）；队列/收藏/历史/设置权威在服务端（`aiya-player` 状态机直接复用）；播放记账走一条轻量上报接口。

### 2.4 两个壳，编译期分离

不做单二进制运行时双模式（`--serve` 之类）：Tauri 壳拖着 webview 全家，容器里就是 webkit 链接面；每个子系统都得写运行时 if。"哪个壳"由构建目标（crate）决定：`src-tauri`（桌面）与 `src-server`（web）都是薄壳，共享逻辑在 crates。

## 3 · Web 版降级评估（浏览器 `<audio>` vs 桌面引擎）

### 3.1 编解码矩阵（`canPlayType` 运行时探测，缺失走 error code=4 标灰）

| 格式 | Chromium/Edge | Firefox | Safari (macOS) | 备注 |
|---|---|---|---|---|
| MP3 / AAC(M4A) / FLAC / WAV | ✅ | ✅ | ✅ | 三主力全绿 |
| ALAC (m4a) | ⚠️ 待验证（ffmpeg 内置，大概率可播） | ❓ 待验证 | ✅ | 上线前真机矩阵测一轮 |
| Vorbis (ogg) | ✅ | ✅ | ❌ | Safari 标灰 |
| Opus (ogg) | ✅ | ✅ | ⚠️ 仅 CAF 容器（Safari 17+） | |
| **AIFF** | ❌（大概率） | ❌（大概率） | ✅ | 桌面引擎独占格式，Web 标灰「请用桌面版」 |
| Opus/WMA/APE | 桌面引擎本就不支持 | 同左 | 同左 | 不变 |

### 3.2 能力对照

| 能力 | 桌面 | Web 版 | 评估 |
|---|---|---|---|
| 解码 | Symphonia + WASAPI | 浏览器解码 | 三主力格式无损；**server 不碰解码**（§4） |
| 无间隙 | 真 gapless | 双 `<audio>` 预载 swap | best-effort（几十 ms 缝隙），能力标志标注 |
| EQ | Rust DSP（10 段） | WebAudio BiquadFilter 链 | **probe 通过才开**（`createMediaElementSource` 在 Safari 有静音前科；对同一元素只能建一次，随元素生命周期管理） |
| 频谱 | Rust FFT → 30fps IPC | AnalyserNode + rAF | 与 EQ 同一 probe 门；不让服务器解码只为算频谱 |
| 输出设备 | WASAPI 枚举 | `setSinkId`：Chromium ✅ / Firefox ❌ / Safari ❌ | Chromium 上可用；Firefox/Safari 引导系统设置 |
| ReplayGain | PCM gain | GainNode（probe 后）/ 元素 volume 衰减式 | Safari 用 volume 式 |
| 变速变调 | Rust | playbackRate + preservesPitch（webkit 前缀属性齐设） | 可用 |
| 进度 | 10Hz `player:state` | rAF 轮询 currentTime（timeupdate 250ms 兜底） | 状态权威在客户端（§2.3） |
| 防休眠 | SetThreadExecutionState | Wake Lock（仅 Chromium，防熄屏不防系统睡） | 降级接受，UI 不承诺 |
| 后台播放 | 独立进程 | 音频不被杀；rAF/定时器节流 | 进度驱动不依赖 setTimeout |

## 4 · Web 版架构轮廓

```
[浏览器] ──HTTP──> [aiya-server（Rust 单二进制）]
                     ├─ 静态 UI（ui/dist）
                     ├─ POST /ipc/<cmd>：命令 API（按域路由，§2.2）
                     ├─ SSE /events：scan:progress + beets:*
                     ├─ /audio/<id>：本地文件流式（Range/206）
                     ├─ /stream/<id>：直播字节代理（playlist/segment 转发 + 正确 MIME）
                     └─ SQLite（app_settings/ui_state/history…，同桌面 schema）
```

- **server 零解码**：音频服务是字节级 IO（Range/206 读文件转发），解码全在浏览器——Symphonia/ffmpeg/gstreamer 都不进容器。hls.rs（Symphonia probe + TS 剥包的解码源）**留桌面**，与直播代理是两条平行实现，不强复用。
- **beets 层（容器内唯一要设计的）**：Windows 便携包 embed 管线（build_bundle.py）不迁移——镜像本身就是"免配置"分发，直接 debian-slim + `apt python3 python3-venv` + venv + beets；`beets.rs` spawn 路径补 POSIX 布局（`bin/python` vs `Scripts/python.exe`）。
- **CSP**：同源形态（`media-src 'self'`）；`aiya-artwork:` 自定义协议在 Web 版不存在，封面走普通 `/artwork/<id>` 路径——`artworkUrl()` 收敛为第三种前缀（同源相对路径）；mediaSession 的 blob 封面逻辑不变。
- **鉴权**：最小方案 = 单用户口令/token（环境变量注入或首启设置），默认绑定 `127.0.0.1`，Docker 部署时显式放开；TLS/反代交给部署方（文档说明，不内置）。
- **Docker**：多阶段构建（node 构建 UI → cargo 构建 server，builder 层装 build-essential 供 bundled sqlite → 运行时 debian-slim + python 层）；卷：曲库（可只读）、数据（SQLite）、封面缓存；单端口。glibc 路线低风险，musl 静态无必要。
- 已知不做：跨端进度同步、Firefox/Safari 的输出设备选择、probe 失败浏览器的 EQ/频谱。

## 5 · 建议批次

| 批 | 内容 | 备注 |
|---|---|---|
| W1 | `aiya-server` 骨架：HTTP transport（第三实现）+ `POST /ipc` 按域路由 + SSE + 静态 UI 托管 + 鉴权 | mock 的命令面就是契约；先 library/settings/playlist/history 域 |
| W2 | 音频链路：`/audio` Range 服务 + `MediaElementTransport` + 能力探测/标灰矩阵 | 三主力格式先全绿 |
| W3 | 直播字节代理 + mediaSession + EQ/ReplayGain probe 门 | |
| W4 | Docker 化（多阶段 + beets 层 + 卷约定）+ 部署文档；beets.rs POSIX spawn 路径 | |
| W5 | 收尾：能力标志 UI 语义（「由浏览器解码播放」）、图标 px→em 扫描（docs/56 §8）、真机矩阵回归 | |

## 6 · 真机验证清单（W2–W5 出口条件）

- 浏览器 × 格式矩阵：Chrome/Edge/Firefox/Safari ×（MP3 / FLAC / M4A-AAC / ALAC / ogg-Vorbis / AIFF）。
- `/audio` Range：seek 随手（206）、VBR MP3 duration 偏差量级记录。
- Safari：MediaElementSource probe（有声/静音）、WebAudio 链路后 preservesPitch 行为。
- `127.0.0.1` trust 一致性（Chromium/Firefox/Safari 各验一次）；直播经代理在三家可播。
- 后台/最小化：进度不靠 setTimeout、回来不跳变。
- Docker：镜像内无任何音频解码器依赖也能全功能（验证"零解码"边界成立）。
