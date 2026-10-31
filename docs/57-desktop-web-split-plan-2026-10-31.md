# 57 · 分支方案：Windows 桌面 + Web 版（Docker）

- 日期：2026-10-31
- 性质：方向拍板 + 分支方案与降级评估
- 关联：docs/56（高分屏缩放支持，已落地）

## 0 · 拍板

1. **放弃 mac/Linux 桌面版**（兼容矩阵调研已完成、按矩阵逐节评估后成本过高；调研全文在 git 历史的 56/57 研究版中，不再维护）。
2. 产品线收敛为两条：
   - **Windows Tauri 桌面版**：现状架构不动（sidecar WASAPI 引擎 + SMTC + 托盘/单实例 + NSIS/便携）；
   - **Web 版**：同一套 Svelte UI，服务端 Rust 单二进制，Docker 打包。
3. Web 版播放不走原生驱动：浏览器 `<audio>` + HTTP Range。mac/Linux 调研中关于 `<audio>` 的能力事实与 Transport 设计**全部继承**到 Web 版（环境从 WebKitGTK/WKWebView 换成真实浏览器后，最硬的三个坑消失，见 §2）。

## 1 · 分支矩阵：共用什么、独有什么

| | Windows 桌面版（现状） | Web 版（新增） |
|---|---|---|
| UI | `ui/`（Svelte 5 + Tailwind 4 + three.js），**共用** | 同左 |
| 命令面契约 | `contracts/`（ts-rs 生成），**共用** | 同左 |
| 曲库/歌单/历史/封面/beets | `aiya-data`、`aiya-beets-*`（平台无关），**共用** | 同左（组装进 server） |
| 设置 | `app_settings` KV + `lib/ipc/transport` 可插拔，**共用** | 同左 |
| 播放 | sidecar `aiya-audio-host`（Symphonia + WASAPI，EQ/频谱/gapless 独占） | `MediaElementTransport`（`<audio>` + WebAudio probe 门） |
| 直播 | sidecar 内 ureq 拉流 | server 代理（hls.rs 逻辑搬入） |
| 媒体键 | SMTC（souvlaki） | `navigator.mediaSession`（三家桌面浏览器全支持） |
| 输出 | WASAPI 独占/共享、设备枚举 | `setSinkId`（仅 Chromium）；其余引导系统设置 |
| 分发 | NSIS 安装器 + 便携 zip | Docker 镜像（多阶段：node 构建 UI → cargo 构建 server → 运行时层 + beets） |

**aiya-audio / aiya-audio-host 不进 Web 版**（WASAPI 输出链桌面独占）。

## 2 · Web 版降级评估（浏览器 `<audio>` vs 桌面引擎）

### 2.1 编解码矩阵（`canPlayType` 运行时探测，缺失走 error code=4 标灰）

| 格式 | Chromium/Edge | Firefox | Safari (macOS) | 备注 |
|---|---|---|---|---|
| MP3 / AAC(M4A) / FLAC / WAV | ✅ | ✅ | ✅ | 三主力全绿 |
| ALAC (m4a) | ⚠️ 待验证（ffmpeg 内置，大概率可播） | ❓ 待验证 | ✅ | 上线前真机矩阵测一轮 |
| Vorbis (ogg) | ✅ | ✅ | ❌ | Safari 标灰 |
| Opus (ogg) | ✅ | ✅ | ⚠️ 仅 CAF 容器（Safari 17+） | |
| **AIFF** | ❌（大概率） | ❌（大概率） | ✅ | 桌面引擎独占格式，Web 标灰「请用桌面版」 |
| Opus/WMA/APE | 桌面引擎本就不支持 | 同左 | 同左 | 不变 |

### 2.2 能力对照

| 能力 | 桌面（不动） | Web 版 | 评估 |
|---|---|---|---|
| 无间隙 | 真 gapless | 双 `<audio>` 预载 swap | best-effort（几十 ms 缝隙），能力标志标注 |
| EQ | Rust DSP（10 段） | WebAudio BiquadFilter 链 | **probe 通过才开**（`createMediaElementSource` 在 Safari 有静音前科；对同一元素只能建一次，随元素生命周期管理） |
| 频谱 | Rust FFT → 30fps IPC | AnalyserNode + rAF | 与 EQ 同一 probe 门；**不要**让服务器解码只为算频谱 |
| 输出设备 | WASAPI 枚举 | `setSinkId`：Chromium ✅ / Firefox ❌ / Safari ❌ | Chromium 上比原评估（WebKit 全线无）更好 |
| ReplayGain | PCM gain | GainNode（probe 后）/ 元素 volume 衰减式 | macOS/Safari 用 volume 式 |
| 变速变调 | Rust | playbackRate + preservesPitch（webkit 前缀属性齐设） | 可用 |
| 进度 | 10Hz `player:state` | rAF 轮询 currentTime（timeupdate 250ms 兜底） | 状态权威在客户端（§3） |
| 防休眠 | SetThreadExecutionState | Wake Lock（仅 Chromium，防熄屏不防系统睡） | 降级接受，UI 不承诺 |
| 后台播放 | 独立进程 | 音频不被杀；rAF/定时器节流 | 进度驱动不依赖 setTimeout |

### 2.3 状态权威与接缝

- Transport 接缝沿用既定设计：`SidecarTransport`（桌面）/ `MediaElementTransport`（Web）。**Web 版让 MediaElementTransport 从"降级路径"转正为主路径**。
- 瞬态播放状态（position/paused/buffered）在客户端（audio 元素才有真值）；队列/收藏/历史/设置权威在服务端；播放记账走一条轻量上报接口。
- UI 零改动接法：IPC 层本来就是 transport 可插拔的（`lib/ipc/transport`，浏览器预览 mock 就装在这里）→ Web 版新增 **HTTP transport** 实现同一命令面。

## 3 · Web 版架构轮廓

```
[浏览器] ──HTTP──> [aiya-server（Rust 单二进制）]
                     ├─ 静态 UI（ui/dist）
                     ├─ 命令 API：library/playlist/settings/history/beets
                     │   （复用 aiya-data / aiya-beets-* ，平台无关 crates）
                     ├─ /audio/<id>：本地文件流式（Range/206）
                     ├─ /stream/<id>：直播代理（hls.rs 逻辑）
                     └─ SQLite（app_settings/ui_state/history…，同桌面 schema）
```

- **HLS 不依赖浏览器原生**（Safari 才有；Chromium/Firefox 要 hls.js+MSE，没必要）→ server 代理统一转发，CSP 收窄到同源。
- **CSP**：同源形态（`media-src 'self'`）；`aiya-artwork:` 自定义协议在 Web 版不存在，封面走普通 `/artwork/<id>` 路径——`artworkUrl()` 收敛为第三种前缀（同源相对路径）；mediaSession 的 blob 封面逻辑不变。
- **鉴权**：最小方案 = 单用户口令/token（环境变量注入或首启设置），默认绑定 `127.0.0.1`，Docker 部署时显式放开；TLS/反代交给部署方（文档说明，不内置）。
- **Docker**：多阶段构建（node 构建 UI → cargo 构建 server → 运行时层 debian-slim + python3 + beets venv）；卷：曲库、数据（SQLite）、封面缓存；单端口。
- 已知不做：跨端进度同步、Firefox/Safari 的输出设备选择、probe 失败浏览器的 EQ/频谱。

## 4 · 建议批次

| 批 | 内容 | 备注 |
|---|---|---|
| W1 | server 骨架：命令 API（HTTP transport 接入 `lib/ipc/transport`）+ 静态 UI 托管 + 鉴权 | mock 已把命令面当契约用，照抄 |
| W2 | 音频链路：`/audio` Range 服务 + `MediaElementTransport` + 能力探测/标灰矩阵 | 三主力格式先全绿 |
| W3 | 直播代理 + mediaSession + EQ/ReplayGain probe 门 | |
| W4 | Docker 化（多阶段 + beets 层 + 卷约定）+ 部署文档 | |
| W5 | 收尾：能力标志 UI 语义（「由浏览器解码播放」）、图标 px→em 扫描（docs/56 §8）、真机矩阵回归 | |

## 5 · 真机验证清单（W2–W5 出口条件）

- 浏览器 × 格式矩阵：Chrome/Edge/Firefox/Safari ×（MP3 / FLAC / M4A-AAC / ALAC / ogg-Vorbis / AIFF）。
- `/audio` Range：seek 随手（206）、VBR MP3 duration 偏差量级记录。
- Safari：MediaElementSource probe（有声/静音）、WebAudio 链路后 preservesPitch 行为。
- `127.0.0.1` trust 一致性（Chromium/Firefox/Safari 各验一次）；audio-only HLS 经代理在三家可播。
- 后台/最小化：进度不靠 setTimeout、回来不跳变。

## 6 · 与 docs/56 的关系

- 56 = 高分屏缩放支持的改动记录（已全部落地）；57 = 本分支方案。
- 被放弃的 mac/Linux 桌面兼容矩阵不再维护；其中仍有效的通用事实（`<audio>` 能力、WebAudio probe、Transport 设计、能力标志语义）已收进 §2。
