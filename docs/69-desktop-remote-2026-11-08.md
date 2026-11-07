# docs/69 · 桌面直连远程版调研与方案（N3 前置）

- 状态：**调研完成 → N3.0 实测批待开工**
- 前置：docs/68（web 版全量——http 传输/登录闸/能力面/站单契约都已落）；
  docs/03 §边界（PCM 不跨网；网络音频单独设计，不污染本地链路）；
  docs/07 §2.2（AudioSink → remote sink 的壳复用设计，本方案部分采纳）；
  docs/10 §9（远程凭据入 OS 凭据库原则）；
  docs/65 §3 拍板（**web 版播放走浏览器，server 零解码**——桌面直连复用同款）。
- 范围拍板（本档 §3）：**v1 纯瘦客户端**——桌面连远程 = 整库走远程（本地
  曲库/拖放/输出设备等桌面专属面在该形态下隐身）；**不做**混合双库（v2 评估）、
  **不做** PCM 跨网、**不做**热切换（切库重启生效）。

## 1 · 现状盘点（代码级证据）

桌面直连远程版的本质 = **把 web 版的传输形态搬进桌面壳**。web 版已经把
最难的部分做完了，桌面侧缺的只是"装配点的第四态"和几个地址假设：

| # | 事实 | 证据 | 对远程化的意义 |
|---|---|---|---|
| 1 | boot 三态装配点：`__TAURI_INTERNALS__` 有 → tauriTransport；无 → `__AIYA_BOOT__==='http'` 注入时装 http 传输（先于 mount，含 probeAuthGate 登录闸） | `ui/src/main.ts` | 第四态 = **tauri 存在 + 本地配置里有远程地址** → 装 http 传输。判定点唯一，改动集中 |
| 2 | 传输单例可注入：`transport.ts` 的 `setTransport()` 整体替换命令+事件 | `lib/ipc/transport.ts` | http 传输即一个 Transport 实现——桌面壳装它**零结构改动** |
| 3 | http 传输全部**同源相对路径**：`/auth/state`、`/ipc/{cmd}`、`/audio/{id}`、`/events`（`withToken` 拼 token） | `lib/ipc/http.ts:285/318/168/312` | **baseUrl 参数化**（`const base = remoteUrl ?? ''`）即可指向远程——改动集中在 http.ts 一处 |
| 4 | 封面已有 web 分支：`/artwork/{id}?size&token`（元素级 GET 走查询参数） | `lib/artwork.ts:36-40` | 同款 base 前缀；桌面态的 `aiya-artwork://` 本地协议分支不冲突（按形态互斥） |
| 5 | server CORS 白名单已有：`AIYA_CORS_ORIGIN` → CorsLayer 挂**全局**（auth 外层，GET/POST）——所有路由（命令/事件/audio/hls/artwork）一致出 CORS 头 | `server main.rs:143-165` | 白名单加桌面 origin 后，命令面与**媒体面（hls.js 的 fetch）**同时打通 |
| 6 | **CSP 是硬门槛**：桌面壳 `connect-src 'self' ipc: http://ipc.localhost …`、`media-src 'self' blob:`——远程 fetch/EventSource/audio 全被拦 | `tauri.conf.json:26` | 必须放宽（§3 拍板点 A）；这是本方案最大的桌面侧改动 |
| 7 | 登录闸/会话/公钥三链与 token 查询参数（元素级 GET）都已按 web 版工作 | docs/65 §10/§11 | 远程登录闸**原样复用**（probeAuthGate 的 URL 加 base 即可） |
| 8 | 媒体元素跨域无 CORS：`<audio src="https://…">` 不读 CORS 头 | 标准行为 | audio 直连远程 /audio 无障碍——只有 **hls.js 的 fetch**（m3u8/段）需要 CORS 头 |
| 9 | 拖放/本地文件选择是桌面专属面（drop.rs/pick_folder），caps 已分档 | `backend-caps.ts` | 远程态 caps 自动作 web 形态——桌面专属面**自动隐身**，无需新写分支 |
| 10 | token 存 localStorage（`readStoredToken`）——按 webview origin 持久化 | `http.ts` | 桌面远程态沿用 localStorage（v1）；OS 凭据库（docs/10）列 v2 |
| 11 | 站单/频道契约已就位：ws 接触点、`station_list` 站单、visibility 二档 | docs/68 §12/§14 | 远程收台与"正在播"元数据**协议零新增** |

## 2 · 方案骨架（v1 纯瘦客户端）

### 2.1 boot 第四态

```
main.ts 装配（mount 前）：
  tauri 存在？
    ├─ 否 → 现状（http 注入 / mock）
    └─ 是 → invoke app_get_settings（tauriTransport 可用）
        ├─ settings.remote.url 非空 → 装 http 传输（baseUrl=远程地址）
        │   → probeAuthGate() → mount（登录闸/主界面照常）
        └─ 空 → 现状 tauriTransport（本地库）
```

- `http.ts` 参数化：`installHttpTransport({ baseUrl?: string })`——base 进
  `withToken`/`fetch`/`EventSource` 的 URL 拼接（约 6 处模板串）；
  `artworkUrl` 的 web 分支同款（`__AIYA_WEB__` 增加 `base` 字段）。
- 设置页新增「远程服务器」段：地址（http(s)://host:port）/ 连接测试按钮 /
  状态显示；写 `settings.remote = { url, visibilityNote }`（本地 DB——
  **桌面本地库仍承担设置存储**，与"曲库走远程"不冲突）。
- **切库重启生效**（boot 时读一次；设置页改完提示重启）。热切换涉及
  transport/播放状态/事件订阅的整体重建，v2 再议。

### 2.2 网络面（三类通道 × 远程化形态）

| 通道 | 机制 | 远程化要求 |
|---|---|---|
| 命令（POST /ipc） | fetch + Bearer 头 | CSP connect-src 放开；server CORS 白名单含桌面 origin |
| 事件（GET /events SSE） | EventSource + token 查询参数 | 同上（EventSource 受 connect-src 管） |
| 媒体 | `<audio>`（audio/{id}、直播流）+ **hls.js**（fetch m3u8/段）+ /hls 频道 | audio 不受 CORS 但受 CSP media-src；**hls.js 受 CORS + connect-src**——server CORS 白名单是硬前提 |
| 封面 | `<img>` /artwork?token | img-src 放开；CORS 不需要（元素级 GET） |

### 2.3 登录与会话

- 远程登录闸原样复用：boot 探测远程 `/auth/state` → `__AIYA_GATE__` →
  LoginView（POST 远程 /auth/login）→ token 进 localStorage。
- `AIYA_TOKEN`（静态第二凭据）同样适用（脚本/桌面混合场景）。

## 3 · 拍板点

| # | 议题 | 选项 | 建议 |
|---|---|---|---|
| A | **CSP 策略** | ① CSP 整体置 null；② connect-src/media-src/img-src 放开 `http: https: ws: wss:`（保留 script-src 等）；③ tauri http plugin 绕过 | **②**：保留 script-src 'self' 的注入防线，只放开网络出口（远程地址用户可配，无法白名单枚举；家庭自用场景风险可接受，docs/65 的公网警告条款同步适用） |
| B | token 存储 | ① localStorage；② OS 凭据库（tauri keyring 插件） | **v1 = ①**（与 web 版同机制）；v2 升 keyring（docs/10 原则不违反——v1 明示） |
| C | 本地库可见性 | ① 纯瘦客户端（本地面全隐）；② 双库并存（库源切换 UI） | **v1 = ①**（caps 流转即可，零新增分支；双库涉及合并/去重/播放记账归属，工作量一个量级） |
| D | 拖放/文件选择 | 远程态禁用 | ①（caps.streaming/pickFolder=false 自动隐身；**上传通道**是另一个功能，不在本档） |

## 4 · 风险

1. **桌面 webview 的 Origin 值**需实测（Windows = `http://tauri.localhost`，
   CORS 白名单要精确到它；协议差异文档化进 compose 注释）。
2. **CSP 放宽范围**的复核：②方案下 `connect-src http:` 允许明文——
   与 docs/65"公网务必 HTTPS 反代"同一警示口径。
3. **hls.js 跨域**：hls.js 默认 XHR 带 credentials=false——凭据走 URL token
   （既有 withToken 方案）不受影响；`xhrSetup` 若有 credentials 需核查。
4. **本地设置与远程会话的生命周期**：远程 server 重启后本地 localStorage 里
   的旧 token 失效 → 登录闸自然重现（401 → gate），无需额外逻辑（已有链）。
5. **桌面专属面误开**：远程态下 `beets`（本地整理工具）等 server 面的
   caps 判定要复核——beets 面在 web 形态的显隐现状（web 版已工作）即答案。

## 5 · N3 批次拆解

| 批 | 内容 | 验收 |
|---|---|---|
| **N3.0 实测批** | 桌面 webview 连远程的最小验证：临时放 CSP → 桌面壳 fetch 远程 /auth/state → Origin 值取证 → hls.js 跨域播一段 → audio 直连 /audio。产出：Origin 精确值 + CSP 放开范围的最小集 + 三类通道的连通证据 | 实测记录落本档 §6 |
| **N3.1 装配批** | boot 四态 + http.ts/artwork.ts baseUrl 参数化 + 设置页「远程服务器」段（地址/测试/重启提示）+ caps 流转复核 | 桌面连远程：登录闸 → 曲库/播放/直播全量可用；本地态零回归（mock/tauri e2e 绿） |
| **N3.2 收口批** | CSP 拍板落定（tauri.conf）+ compose/README 的 CORS 文档（桌面 origin 白名单）+ v2 议题记账（keyring/双库/热切换） | 桌面远程态全门禁绿；docs/65 部署文档补桌面直连段 |

## 6 · 明确不做（本档范围外）

- **PCM 跨网**：docs/03 边界——桌面远程态的播放引擎 = 浏览器 `<audio>`/
  hls.js（web 版同款），本地 Rust 引擎/输出设备/频谱 DSP 不参与（caps
  隐身清单自动生效）。
- **混合双库**（v1）：单库语义 = "当前连的是谁"，切换即重启。
- **热切换**：transport/播放器/事件订阅的整体重建，v2 评估。
- **上传通道**（本地文件推给远程库）：beets 整理链路的 web 化，另一批。
