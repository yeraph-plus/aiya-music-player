# 65 · 网页版（Docker）落地前复核与批次计划（2026-11-05）

- 类型：调研（docs/57 方案在当前基线的落地复核 + 分支形态评估 + 批次框架，不含实现）
- 状态：closed（W0–W2 落地见 §10/§11，W3–W6 见 docs/66；推流线 W4 与
  beets Linux 套件接入继续延期，账目见 docs/66 §11）
- 关联：docs/57（方向拍板，本档复核其成立性并修订批次）；docs/20（远程容器调研，
  其 FM/推流线自本档 v2 起纳入范围，见 §3 拍板 3 与 §7）；docs/52 与
  LIVE_STATION_FIELDS.md（直播电台现状与站单字段契约）；docs/62（完工基线）
- 修订：v2（2026-11-05 二轮）——推流模块纳入范围；新增分支形态评估（§5）、
  后端迁移可行性清点（§6）、推流模块方案（§7）；批次表增 W0 实测批；
  多语言协调注记删除（界面双语已落库，f323438）。
  v3（2026-11-05 三轮）——W0 实测批执行完毕，记录与定案见 §10。
  v4（2026-11-06）——W1/W2 落地完毕（aiya-dto 抽取 + server 53 路由 + 浏览器
  引擎回报闭环），记录与实测见 §11。

## 0 · 复核缘起

docs/57（2026-10-31）已拍板两条产品线：Windows 桌面版架构不动；Web 版为同一套
Svelte UI + 服务端 Rust 单二进制（新 crate `aiya-server`）+ Docker 打包，播放走
浏览器 `<audio>` + HTTP Range。该拍板继续有效，本档不改方向。

拍板之后落库的批次：beets 协议 v2 冻结与单轨化收口（docs/58）、索引增强与
`aiya-beets-store`（docs/45/50/54）、直播电台桌面链路（docs/52）、数据库基线
归一化（docs/62 §1.4）、界面双语（docs/64，已落库）。本档逐条对照当前 HEAD
实测 docs/57 的假设，并修订批次框架。

## 1 · docs/57 假设逐条复核

| docs/57 假设 | 当前基线实测 | 判定 |
|---|---|---|
| transport 是唯一换气口，Web 版加第三实现零架构改动 | `ui/src/lib/ipc/transport.ts`：`setTransport()` 注入、tauriTransport 默认、mock 动态 import 独立 chunk；`client.ts` 归一化层不动 | ✅ 成立 |
| 命令面集中于 lib.rs（当时记 96 条） | 实测注册 95 条；`generate_handler` 唯一注册点；`tests/ipc_contract.rs` 与 `client.ts` 双向覆盖 | ✅ 成立（条数随批次浮动，以 ipc_contract 为准） |
| 事件 5 通道 | 实测 `events.ts` 订阅面 9 通道：`player:state`(10Hz) / `spectrum:frame`(30fps) / `scan:progress` / `beets:import` / `beets:install` / `beets:index` / `beets:enrich` / `beets:maintenance` / `audio:mode` | ⚠️ 修订：SSE 承 6 通道（scan + beets×5），player:state/spectrum:frame 客户端自产、audio:mode 裁掉——自产结论不变 |
| 共用 crates 平台无关，server 编译图不含音频侧 | 依赖面实测：aiya-player（serde/ts-rs/tempfile）、aiya-data（rusqlite bundled/walkdir/lofty/rayon）、aiya-beets-client（serde）、aiya-beets-store（rusqlite）——零 windows/tauri/miow；miow 只在 src-tauri 的管道客户端 | ✅ 成立（§6 迁移可行性的核心证据） |
| 直播 = server 字节代理（外部流） | 桌面侧已落地独立链路（`aiya-audio/src/hls.rs` + `ts.rs`，sidecar 内 ureq 拉流解码）；docs/52 定位是"音频直播推流**客户端**" | ✅ 成立；自建频道推流另见 §7 |
| beets 层容器内 debian-slim + venv + beets | 协议 v2 已冻结（schema 真源 + 字节镜像 + 运行时 schemaHash 比对）；宿主 spawn 由套件包 `manifest.json` 的 entry/args 驱动 | ✅ 成立；Linux 打包归 beets-tool 侧（§3 拍板 1） |
| 封面走普通 `/artwork/<id>` 路径 | C5 未变（`aiya-artwork` 协议仅桌面存在）；DTO 的 coverUrl 字段名不变、值换源 | ✅ 成立 |
| 契约位 = contracts/ 共用 | `contracts/README.md` 已自我标注为过期草案；契约真源是 Rust 类型 → `gen:types` → `ui/src/lib/ipc/generated/`，`dto_wire.rs` 锚定 | ⚠️ 修订：实为"generated/ 类型 + dto_wire 守卫共用" |

新增一条 docs/57 未覆盖的实测：UI 侧对 Tauri API 的直接引用面除 transport/events
外只有三处——`window-chrome.ts`（自绘窗框）、`dpi-diagnostics.ts`、
`dropzone.ts`。前端裁剪落点即此三处加能力位页面判断（§6.3）。

## 2 · 拍板之后新增的基线事实

- **事件面扩容**（§1 表）：SSE 通道清单按 6 通道设计；beets 长任务进度复用
  现有事件形状，web 端不新增协议。
- **aiya-beets-store 组装进 server**：C7（单一数据库写入者）在容器内的表述为
  "宿主 = aiya-server 进程"；store 是独立 SQLite 文件，与曲库的边界照搬。
- **数据库基线已归一化**（docs/62 §1.4）：server 直接用同一 `0001_init.sql`
  基线，`user_version` 语义一致；"发布后压平冻结、只走增量迁移"的纪律对新壳
  同样生效。
- **界面双语已落库**（f323438：i18n 域表 ~700 键、系统语言首启写库、后端报错
  英文化）：web 版前端直接继承双语能力，本档不再有任何协调事项。
- **文风门禁已在 verify**（`check:style:strict`）：aiya-server 与新 TS 文件的
  注释按 AGENTS §4 两段法起步，不欠新账。
- **SMTC 恒开 + mediaSession 恒全量注册**（docs/62 §1.6）：媒体键实现在 UI 侧，
  web 端直接复用，无宿主依赖。

## 3 · 拍板记录

v1（2026-11-05 一轮）：

1. **enrich 的 Linux 支持**：beets 套件是独立发布单元（venv 自带解释器），
   Linux 分支由 beets-tool 侧独立打包解决；宿主按 `manifest.json` 记录的
   entry/args spawn，Rust 侧预期零调整。W4/Docker 批只做接入验证，不开宿主
   工作项。
2. **多语言**：已随 f323438 落库，不再是事项。

v2（2026-11-05 二轮）：

3. **推流模块纳入 server 版范围**：docs/20 §7.1 的 Liquidsoap 频道推流线 +
   LIVE_STATION_FIELDS.md 的服务端站单同步，随本批落地（方案见 §7）。
4. **分支形态 = 单仓双壳、编译期分离**（§5 评估结论）：git 不开长驻分支，
   产品线由构建目标区分；对"启动时分支判断跳过 Windows 驱动"的回应是——
   Windows 代码不进 server 编译图，"跳过"在编译期完成，运行时无判断
   （与 docs/57 §2.4 一致，理由复核后仍成立）。

docs/57 §4 已拍板且继续有效、本档不再重议的：鉴权 = 单用户口令/token +
默认绑定 127.0.0.1 + TLS 交反代；同一镜像经环境变量切全栈/无头两形态；
常规播放 server 零解码（Symphonia/ffmpeg 不进 server 编译图）；不做运行时
双模式壳。

## 4 · 范围围栏（不入本线）

- **桌面版连远程库**（docs/20 §7.4：桌面 RemoteTransport + HTTP Range 流式
  解码 spike）：transport 三态设计时仅作为非目标记录，不为其预留接口债。
- **桌面版零改动**：`aiya-audio` / `aiya-audio-host` / `src-tauri` 不进
  server 编译图，桌面行为与发布物不受本线影响。
- ICY 元数据、封面上传（LIVE_STATION_FIELDS.md 非目标，照抄）。

## 5 · 分支形态评估

三个维度分开评估，结论各自独立。

### 5.1 代码组织：单仓双壳 vs git 分支 vs 独立仓库

| | 单仓双壳（判定 ✅） | git 长驻分支 | 独立仓库 fork |
|---|---|---|---|
| UI / i18n 词典 | 一份 | 两份，每次 UI 批次双投 | 同左 |
| 契约（generated/ + dto_wire + ipc_contract） | 一份 | 两份，漂移无门禁可拦 | 同左 |
| beets 协议镜像 | 一份（proto:check 守） | 两份 | 同左 |
| 共用 crates（player/data/beets-client/store） | 一份，两个壳各自组装 | 两份，修 bug 双投 | 同左 |
| 桌面回归 | verify 照常守 | 分支越走越远后合并即灾难 | 永不合并 |

C1/C4/C7 的边界投入（平台无关 crate、Rust 单源契约、协议字节镜像）的全部
收益就在"不 fork"。**判定：单仓双壳，不开 git 分支、不 fork。** 仓库现行
惯例是 main 直落小批、每批 verify 全绿，W 线批次照此办理（verify 会同时守
桌面线不破）。

### 5.2 二进制形态：编译期双壳 vs 运行时双模式

"启动时分支判断跳过 Windows 驱动"对应的是运行时双模式（同一二进制 `--serve`
切换），docs/57 §2.4 对此已有否决，理由复核后仍成立：

- server 镜像会拖进 Tauri/WebView2/SMTC/托盘的链接面（Linux 下是 webkitgtk
  全家），容器体积与攻击面全无收益；
- 95 条命令与 setup 每个子系统都要写运行时 if，分支成本渗进所有代码；
- Windows 件（sidecar/SMTC/托盘/单实例/对话插件）集中在壳层 setup，编译期
  分离后 server 壳的启动路径里**根本不存在这些代码**——"启动时跳过"的诉求
  由"编译期不编入"等价满足，且零运行时成本。

**判定：编译期双壳。** 共享编排下沉 `aiya-host-core`（traits：`EventSink` /
`DialogEnv` / `EngineSink`，docs/57 §2.2 原案），桌面壳与 server 壳各自在
setup 装配自己的实现；若未来出现"桌面版一键变服务器"的真实产品诉求，再议
运行时形态（那是另一条线的事）。

### 5.3 数据位置：C11 的容器增补

C11 规定数据位置唯一真源是 `paths.rs`（便携判定 `portable.txt` →
`%APPDATA%\dev.aiya.music`）。容器形态两判定都不适用：server 壳的数据根改由
环境变量（`AIYA_DATA_ROOT`）显式指定，不经 portable.txt 逻辑。这是对 C11 的
增补而非修改（桌面判定不动），实施时同批更新 AGENTS §3 C11 并按 §5.3 记录。

## 6 · 后端迁移可行性（"能不能迁移"的清点）

**结论：能迁移，无需重写。** 证据链四条：

1. 共用 4 crate 依赖面零平台 API（§1 实测）——域逻辑原样编进 server；
2. 命令体耦合极轻：`State<AppState>` + serde DTO 为主，`AppHandle` 仅少数
   命令触及（窗口/协议注册/dialog）；
3. Windows 件全部集中在壳层：setup 链 = 日志 → app_id 自愈 → DB 打开 →
   封面目录 → **sidecar launch** → 设置缓存 → … → **SMTC spawn** →
   **tray build** → 窗口；加 **单实例插件 / dialog 插件 / aiya-artwork 协议
   注册**。server 壳的 setup = 日志 → DB → store → 设置缓存 → 事件泵 →
   HTTP 服务，上述件一个都不出现；
4. `AIYA_DATA_ROOT` 注入数据根（§5.3）后，`aiya-data::open` 原样可用。

### 6.1 命令面三桶清点（实测 95 条注册命令，W0 全量矩阵见 §6.4）

| 桶 | 条数 | server 处置 |
|---|---|---|
| **A 原样/轻编排** | 34 | handler 直调 aiya-data / aiya-beets-client / store |
| **B 编排下沉** | 49 | 编排逻辑抽 `aiya-host-core`（扫描管线、设置 diff、历史裁剪、播放记账、事件泵五个下沉面），server 装配 web 实现；player 域接 `aiya-player` Actor（服务端队列预言机，docs/20 §7.2），beets 长任务走现有桥 + SSE |
| **C 桌面专属** | 12 | 命令不存在或返回 Unsupported（现有 ApiError 分类容纳），能力位接住 UI；web 替代路径见矩阵内标注 |

较 v1 估计（60/25/14）的偏移来自实测：player 域 18 条与 beets 长任务 22 条
全落在 B 桶（它们是编排本体），A 桶集中在纯数据读与 KV 写。

### 6.2 事件面

SSE 承 6 通道（`scan:progress` + beets×5）；`player:state`/`spectrum:frame`
客户端自产（rAF/AnalyserNode）；`audio:mode` 桌面专属裁掉。SSE 断线重连 =
连接建立即推当前快照，不逐帧补发（§9）。

### 6.3 前端裁剪面（"隐藏判断层"）

能力标志由 bootstrap 下发（`backend: 'sidecar' | 'web'` + codec 表 + beets/
enrich 三态），UI 按能力渲染——组件永远不写 `if (isWeb)`（docs/57 §2.1 原则）：

- **页面级**：9 个 view（Home/Library/Browse/Album/Artist/NowPlaying/Live/
  Beets/Settings）数据域全部在 server 侧，**无页面需要隐藏**；Live 页扩展
  频道/站单管理入口（§7），Beets 页按 pluginHealth 三态照常渲染；
- **分区级**：Settings 的输出分区（WASAPI 模式/设备/时钟/缓冲）、诊断页
  mediaKeys 组——能力位隐藏并标注原因；
- **壳级**：`window-chrome.ts`（自绘窗框）web 端不挂、`dpi-diagnostics.ts`
  裁掉、`dropzone.ts` 保留（浏览器 dnd 可用）；
- **新增**：站单/频道管理 UI（并入 Live 页或独立页，W4 定）。

### 6.4 迁移矩阵（95 条全量，W1 施工输入）

桶定义见 §6.1；"web 替代"指 UI 能力位承担的对应路径。

| 命令 | 桶 | server 处置 / 依据 |
|---|---|---|
| **app 域（11）** | | |
| app_info | A | 版本 + 数据目录 |
| app_get_ui_state / app_get_settings | A ×2 | KV 读 |
| app_set_ui_state | A | KV 写 |
| app_set_settings | B | diff 副作用：扫描面保留、引擎面裁剪 |
| app_diagnostics | A | 4 字段；mediaKeys 组 web 无义 |
| app_open_data_dir | C | 容器无"打开目录"语义 |
| app_open_url | C | web 端 window.open 客户端自理 |
| spectrum_set | C | 引擎 tap；web 走 AnalyserNode |
| player_set_eq / player_get_eq | C ×2 | Rust DSP EQ；web 走 WebAudio probe 门，回显由 settings 承担 |
| **beets / enrichment 域（26）** | | |
| beets_status | B | probe 套件包 |
| beets_preview / beets_apply | B ×2 | 长任务 + `beets:import` 事件 + 扫描联动 |
| beets_cancel | B | 取消闸 |
| beets_config_get / beets_inbox_imported_add | B ×2 | 配置读写 |
| beets_set_inbox / beets_set_target | B ×2 | 路径串校验逻辑复用；容器路径语义 W4 核 |
| beets_open_inbox / beets_reveal | C ×2 | reveal 文件管理器 |
| refresh_state | B | 扫描管线状态 |
| beets_install / beets_plugins_set / beets_uninstall | B ×3 | 长任务（安装/差量 pip/卸载安全闸）+ `beets:install` |
| beets_plugin_table | B | 三态插件表 |
| beets_adopt / beets_update / beets_duplicates / beets_move | B ×4 | 长任务 + 对应事件 |
| beets_query / beets_fields | B ×2 | store 读模型 |
| enrichment_worklist | B | store + 引擎 tap 信号（win 专属部分随能力降级） |
| beets_enrich_start | B | Linux 无 Essentia 时三态降级（§3 拍板 1） |
| beets_index_import | B | store 写入 |
| enrichment_query / enrichment_gc | A ×2 | store 直查 |
| **library 域（19）** | | |
| library_list / library_tracks / library_stats / library_albums / library_artists / library_genres | A ×6 | repo 直查 |
| library_scan | B | 扫描管线编排 |
| library_scan_cancel | B | 取消闸 |
| pick_folder / library_choose_root | C ×2 | 原生对话框；web 用输入或服务端目录浏览 |
| library_roots_get / library_roots_count | A ×2 | repo 直查 |
| library_roots_set | B | 写后触发重扫 |
| library_roots_prune / library_clear / library_remove_tracks | A ×3 | db 直写 + 事件 |
| library_reveal_track | C | reveal |
| library_ids_for_selection | A | 选择集解析 |
| tracks_resolve_paths | A | `/audio/{id}` 的取径来源 |
| **playlist 域（14）** | | |
| playlist_list / playlist_detail / playlist_create / playlist_set_rule / playlist_rename / playlist_delete / playlist_reorder / playlist_add_tracks / playlist_remove_at / playlist_move / playlist_clear | A ×11 | repo 直写 |
| playlist_refresh | B | 动态歌单重估（beets 查询联动） |
| playlist_import | B | web 上传字节替代宿主盘路径；解析逻辑复用 |
| playlist_export | B | 返回内容 / 下载 URL 替代写盘 |
| **player 域（18）** | | |
| player_state / player_queue / player_play_tracks / player_play_query / player_toggle / player_next / player_prev / player_seek / player_set_volume / player_toggle_mute / player_set_rate / player_set_repeat / player_set_shuffle / player_enqueue / player_insert_next / player_remove_at / player_move / player_clear | B ×18 | Actor 服务端化（队列预言机）；seek/volume/rate 真值在客户端、命令仍进权威记账（docs/57 §2.3） |
| **live / 其他（7）** | | |
| player_open_paths | B | 桌面开文件/URL；web 语义 = 返回代理 URL |
| live_open | B | URL 语义不变；引擎面 → 客户端播放 |
| live_pick_playlist | C | 对话框；web 用上传/拖放 |
| classify_paths | C | 拖放路径分类；web 前端分类后上传 |
| lyrics_get | A | repo 直查 |
| history_stats | A | repo 直查 |
| history_clear | A | 同事务清零（aiya_data::history::clear） |

矩阵以 `tests/ipc_contract.rs` 的双向覆盖为锚；W1 每接入一域，该域行的
路由即受 ipc_contract 守卫。

## 7 · 推流模块方案（自 v2 起纳入）

两条子链路，职责分立：

### 7.1 频道推流（曲库 → 实时 HLS）

沿 docs/20 §7.1 核实过的 Liquidsoap 方案（外部事实当时已核：`output.file.hls`
免 Icecast、音频-only 官方支持、`request.queue` 的 push/skip/flush_and_skip
控制命令、`persist_at` 重启续播、滚动窗口自动清理、官方多架构镜像内置
ffmpeg）：

```
aiya-server
  ├─ 频道生命周期：活跃频道 → spawn liquidsoap 子进程（模板脚本 + 频道 id）
  │    request.queue(interactive=true) → output.file.hls
  │    产出 /data/hls/{channelId}/live.m3u8 + 滚动段（segments=6, overhead=4, 2–4s/段）
  ├─ 控制面：Unix socket 发 push/skip/flush_and_skip；队列真值在 SQLite，
  │    频道（重）启时按 DB 种子回灌
  ├─ 分发面：托管 /hls/{channelId}/*（MIME=application/vnd.apple.mpegurl + CORS）
  └─ web 端：hls.js 挂 live 边缘；桌面版同样可吃此 URL（数据面 API 客户端无关）
```

镜像形态（W0 实测定）：同容器 `apt install liquidsoap`（部署最简、与 beets
同为 spawn 模式）vs compose 双容器挂 Liquidsoap 官方镜像（版本新、隔离重
依赖，但多一个服务编排）。倾向前者，若 debian 包版本过旧再切后者——这是
W0 实测批的判据之一。

### 7.2 站单同步（服务端 → 播放器）

字段契约已就位（LIVE_STATION_FIELDS.md）：服务端站单按 `id / url / title /
subtitle / description / cover` 下发，`source: 'service'`，合并规则照办
（用户已填字段赢、服务端只补空白、按 URL 去重、上限 20 条）；服务端有稳定
id 直接采用。server 侧新增站单存储表（或 KV）+ 数据面 API（`/stations` 下发
与站点 CRUD）；管理入口 = web 端 Live 页的管理面板（§6.3）。ICY 与封面上传
维持非目标。

### 7.3 与直播客户端链路的关系

外部电台（用户手填 http(s) m3u8）走字节代理（`/stream`，解决 CORS/MIME）；
自建频道（§7.1）源在同源，直接播 `/hls/...`，不经代理。桌面版继续用 sidecar
的 hls.rs 解码链，两条客户端实现互不复用（docs/57 §4 平行实现原则不变）。

## 8 · 批次框架

| 批 | 内容 | 出口判据 |
|---|---|---|
| **W0 实测批**（新增） | ① 共用 4 crate + 骨架在 `x86_64-unknown-linux-gnu` 的 `cargo check`（docker rust 镜像挂仓即可）；② axum 骨架：`POST /ipc` + SSE + 静态托管 + 鉴权；③ `/audio` Range/206 真浏览器播 + seek；④ HLS 字节代理拉真实 m3u8，三家浏览器播；⑤ Liquidsoap 最小频道（官方镜像，push 两首、skip、滚动段、hls.js 播、单频道 RSS）；⑥ 95 命令逐条三桶标注 → 迁移矩阵 | 实测记录落档修订本档；§7 镜像形态定案；迁移矩阵成为 W1 施工输入 |
| W1 | `crates/aiya-server` 骨架正式化：按域路由（A 桶先行）+ SSE 6 通道 + 静态托管 + 鉴权 + `AIYA_DATA_ROOT`；路由表与 ipc_contract 对齐的守卫测试 | e2e mock 子集语义在 http transport 下复现；curl 冒烟全命令面无缺路由 |
| W2 | 音频与播放链路：`/audio/<id>` Range 流式；`MediaElementTransport`（客户端合成 `PlayerState`：rAF positionMs、MediaSession 复用、`canPlayType` 探测表 + 缺失标灰）；player 域接 `aiya-player` Actor（generation/revision 丢弃规则客户端镜像） | Chrome/Edge/Firefox 播放/seek/连播；server 镜像无解码器依赖仍全功能 |
| W3 | 编排下沉与长任务：aiya-host-core 五个下沉面（扫描管线/设置 diff/历史裁剪/播放记账/事件泵）；beets 长任务域接通（6 个 beets 事件走 SSE）；扫描容器 `/music` 卷 | 扫描/导入/维护全链路在容器内走通；B 桶命令逐域转绿 |
| W4 | 直播与推流：外部流字节代理（`/stream`）；频道推流模块（Liquidsoap 生命周期/socket 控制/DB 种子回灌）；站单存储与 `/stations` API；Live 页管理面板；EQ/ReplayGain WebAudio probe 门 + 频谱 AnalyserNode 复刻（参数对齐 `aiya-audio/spectrum.rs`） | 自建频道三家浏览器连播、skip ≤1 段；站单下发合并规则符合 LIVE_STATION_FIELDS；probe 失败标灰有因 |
| W5 | Docker 化：多阶段构建（node 前端 → cargo server → 运行时层叠 beets venv + liquidsoap）；卷约定 `/music`（可只读）+ 数据卷；compose 样例与部署文档；beets Linux 套件包接入验证 | 容器内 beets status/apply 冒烟绿且 schemaHash 一致；单 compose 起全栈（含推流） |
| W6 | 收尾：能力位 UI 语义（"由浏览器解码播放"、输出分区隐藏、window-chrome/dpi 裁剪）；真机矩阵回归（docs/57 §6 清单 + 双语抽查 + AIFF/ALAC 标灰） | docs/57 §6 清单全绿；裁剪项均有能力位文案 |

W1 的 Rust 侧不触 `ui/src`；W2 起的 transport 第三实现与能力位是前端主要
施工面（§6.3 清单）。

## 9 · 实测与风险清单

继承 docs/57 §6 全部条目：浏览器 × 格式矩阵、Range seek 经反代、Safari
MediaElementSource probe、后台进度不靠 setTimeout、镜像零解码验证。

增补：

1. **SSE 断线重连与快照补发**：EventSource 自动重连 + 连接建立即推当前快照；
   generation/revision/seq 丢弃规则在客户端合成侧的镜像语义需要一份小契约
   说明（docs/20 §5.6 遗留设计点，落点 W2）。
2. **beets 长任务事件的重连语义**：五类事件断线期间不逐帧补发，重连后取
   最新快照（以协议 v2 现状为准）。
3. **容器卷上 SQLite WAL 的扫描基线**：5 万首口径复测（对齐 `bench:50k`）。
4. **Liquidsoap 单频道进程 RSS 与上下限**（docs/20 §7.1 遗留实测项，落点
   W0 ⑤/W4）：>50 并发频道时改"单进程多 output"脚本再生模型。
5. **推流段的磁盘 GC**：滚动窗口由 Liquidsoap 自清，但频道删除/异常退出的
   残留目录需要 server 侧清扫策略（W4 设计点）。
6. **鉴权面对推流 URL 的覆盖**：`/hls/*` 与 `/stream/*` 若需 token（查询参数
   或 cookie），hls.js 与 `<audio>` 的携带方式不同——W0 ④/W4 一并实测，
   未决前按"同源 + 默认绑内网"的最低面处理。

## 10 · W0 实测记录（2026-11-05，本机 Docker Desktop 29.7.2 + rustc 1.99.1）

### 10.1 逐项结果

| 项 | 结果 | 实测数据 |
|---|---|---|
| ① Linux 编译图 | ✅ | 共用 4 crate + aiya-server 骨架在 rust:1 容器（linux/amd64）`cargo check` 全绿；axum 0.8 / tower-http 0.6 / tokio / rusqlite bundled / lofty 全依赖 Linux 编译通过。"server 编译图不含音频侧"从推断变实证 |
| ② axum 骨架 | ✅ | `crates/aiya-server` 已建并入 workspace；`POST /ipc/{cmd}`（404 = ApiError 形状）、`GET /events`（SSE 快照首推 + 心跳）、静态托管、token 鉴权（401/Bearer/查询参数）全部按形状工作 |
| ③ Range 流式 | ✅ | `GET /audio/{file}` 带 `Range: bytes=0-99` → `206` + `content-range: bytes 0-99/1920044` + `accept-ranges: bytes` + `audio/wav`；浏览器 `<audio>` seek 3s 实测 **16ms** 返回（ServeFile 承担 Range，零手写） |
| ④ HLS 字节代理 | ✅ | `/stream?u=` 对 vod（ENDLIST）/ master（变体跳转）/ rolling（滑窗直播）三种清单重写 + 段透传，hls.js 全部播通；段 MIME=video/mp2t；重写时 token 透传进段 URL |
| ⑤ Liquidsoap 频道 | ✅ | 官方镜像 v2.4.6；`request.queue` push 两首（请求号 0/1）→ 10s 段滚动落盘 → master/variant 生成；`ch1.skip` = Done；hls.js 经 `/hls/ch1/live.m3u8`（Authorization 头路径）播放正常；**单频道 RSS 76.4 MiB / CPU ≈2%**（docs/20 §7.1 的"几十 MB 级"预估成立） |
| ⑥ 迁移矩阵 | ✅ | 95 条命令逐条三桶标注，落 §6.4（A 34 / B 49 / C 12） |

浏览器覆盖：本会话只有 Chromium 系（IAB 内嵌）。Firefox / Safari 真机矩阵
按 W6 清单执行，遗留项不变。

### 10.2 实测揭示的设计修正（进 W1/W4 输入）

1. **Json 提取器绕开 ApiError**：axum 的 `Json<T>` 提取器对非 JSON
   Content-Type 直接 415，绕开 ApiError 分类——C4 参数侧已知坑的 HTTP 形态。
   命令面统一手动解析请求体（spike 已按此实现）。
2. **token 的两条凭据路径都实测通过**（§9.6）：`/stream` 代理链 = 查询参数
   透传进重写清单（hls.js 段请求 URL 由清单给定，天然携带）；`/hls` 自建频道
   链 = hls.js `xhrSetup` 注入 `Authorization` 头。原生 HLS（Safari）只兼容
   前者——W4 鉴权定案时以此为准绳。
3. **ServeFile 覆盖 Range 语义**，`/audio/{trackId}` 的取径只需查库换路径，
   HTTP 侧零新轮子。
4. **Liquidsoap 2.4 语法基线**（进 W4 频道模块）：`output.file.hls` 位置参数 =
   `目录 + [(段名, 格式.{extname})] + 源`，清单名走 `?playlist` 命名参数；
   `set(...)` 全线弃用（改 `settings.path := value`）；段时长默认 10s，
   docs/20 的 2–4s 目标经 `segment_duration` 参数可调；telnet 控制面在容器内
   经 `/dev/tcp` 可达（bash 内建），控制面永不对外发布端口。
5. **网络面**：Docker Hub 直连不通，镜像经 daocloud（rust:1）与 1ms（不收
   白名单外镜像的 daocloud 拉不到的）双镜像源补齐；CI 与发布机的 registry
   可达性在 W4 构建链设计中要作为前提登记。

### 10.3 镜像形态定案（§7 遗留判据）

- **Liquidsoap 线：同容器 spawn 成立**，且基镜像候选从"debian + apt 安装
  liquidsoap"改为"**官方 liquidsoap 镜像叠加**（python3 + beets venv +
  aiya-server 二进制）"——apt 的 liquidsoap 版本不可控，官方镜像 2.4.6 已
  全链路验证，叠加层都是常规 apt/pip 内容。
- beets 线不变（venv 进同一容器，§3 拍板 1）。
- 最终 compose 形态（单服务 or server+liquidsoap 双服务）留给 W4 按频道
  生命周期实现方式定：spawn-per-channel 要求同容器，固定单频道可双服务。

### 10.4 遗留

- W0 全项出口判据达成，无阻断；W1 可开工（骨架 crate 已在 workspace，
  迁移矩阵 §6.4 是施工输入）。
- spike 工件在 `target/spike/`（不入库）：`aiya-server` 测试页随源码放
  `crates/aiya-server/spike/test.html`，hls.js 与容器数据不入仓。

## 11 · W1/W2 落地记录（2026-11-06）

### 11.1 W1 —— server 骨架正式化

- **`aiya-dto` 共享契约 crate**：src-tauri 的 `dto/`（1210 行，10 文件）整体迁出，
  gen-types 的导出 crate 列表加至四个；`tests/dto_wire.rs` 的 include 路径改指
  共享层。契约零漂移实证：`gen:types:check` 65 个类型全等，dto_wire 21 条全过。
  `drop.rs`（拖放分类）保留在桌面壳——它引用 `aiya_audio::decoder` 的扩展名
  单一源，跟音频侧走；`classify_paths` 本就是 C 桶桌面专属。
- **`aiya-server` 路由 53 条**：A 桶 35（app/enrichment/library/playlist/lyrics/
  history）+ player 域 18。参数侧手动解析（camelCase 键），错误统一 ApiError
  形状 + HTTP 分类码映射（invalidArg/unsupported→400、notFound→404、busy→409）。
- **`ServerState`**：`Mutex<DbHandle>` + `Mutex<PlayerActor>` + settings 内存
  镜像 + enrich store 懒打开，与桌面壳同构；`AIYA_DATA_ROOT` 即 C11 容器增补的
  首次实施（数据根 = `$AIYA_DATA_ROOT/library.sqlite` + `cache/artwork` +
  `beets-index/`）。
- **契约守卫** `tests/contract.rs`：ROUTED × `client.ts` 双向覆盖 + 桌面专属
  命令不泄漏；`player_report` 登记为 transport 内部通道白名单。
- SSE：连接建立即推快照 + 心跳（scan/beets 生产者 W3 接）；静态托管 `ui/dist`。

### 11.2 W2 —— 音频与播放链路

- **player_report 回报通道**（本批关键补件）：浏览器就是引擎，Loaded /
  Position / Ended / Failed 回报直接映射 `PlayerCommand` 同名变体走同一归约——
  队列推进、repeat、队尾停止、Loading→Playing 的状态机权威全部留在服务端
  Actor，桌面壳的引擎事件泵语义原样成立。
- `/audio/{trackId}`：id 查库取 path → ServeFile Range（206）；`/artwork/{id}`
  按 C5 口径只查缓存 id，thumbs 缺席回落原图（按需缩放留 W3）。
- 启动：`AIYA_MUSIC_DIR` 增量扫描入库（roots 写库 + scan_roots_ext）+ 队列
  hydrate 恢复（rememberPosition 口径）。
- 前端 `ui/src/lib/ipc/http.ts`：WebPlayerEngine（token 变化 = 装载
  `/audio/{id}`；loadedmetadata → loaded 回报 + 记忆位置落位；ended → Ended
  回报；位置 1s 节流回报；播放中 ~10Hz 合帧广播；MediaSession 元数据与
  媒体键）；宿主三态探测（tauri / http / mock，`?backend=` 可强制）；
  artwork 第三前缀（同源 `/artwork` + 查询参数凭据）。
- 门禁：svelte-check 0 error / 0 warning；clippy 新 crate 0；check-style 新
  crate 0 命中；aiya-dto / aiya-server Linux 容器 cargo check 通过。

### 11.3 实机可用性（Chromium 系，aiya-server 8787 + ui/dist）

- 实扫 `test-music`（3 轨 WAV + lrc）入库；曲库/正在播放/歌词（`lyrics_get`
  命中 .lrc）全量渲染。
- 点播 → 播放条推进（0:03→0:04 实测）；**5s 曲终 Ended → Actor 自动推进**
  连播跨两首；第三首播完队尾进 Idle（repeat off 语义正确）。
- 暂停 → 位置回报落库（positionMs=800）；**刷新页面跨会话恢复**：队列 3 首 /
  index 2 / paused / 记忆位置经 hydrate + 客户端装载落位（token=7 重装载）。
- 检测判据踩坑记录：首版探测以 `AppInfo.version` 字段为准——docs/62 压平后
  AppInfo 只剩 dataDir，探测恒 false 回落 mock；已改判 `content-type`（探测
  不耦合 DTO 字段集）。

### 11.4 门禁记分卡（npm run verify 分步复核）

| 步 | 结果 | 说明 |
|---|---|---|
| test:all（含 sidecar 构建 + workspace 测试 + svelte-check + 各门禁） | ✅ | e2e 得以运行即其通过的前置 |
| test:e2e | ✅ 27 passed | mock 路径（探测失败回落）不受三态改动影响 |
| check:fmt | ✗ | diff 全在 `aiya-audio/engine.rs` + `hls.rs`——i18n 批（f323438）落库时的手排残留，HEAD 既有，非本批范围 |
| check:clippy | ✅ | workspace 全量 |
| gen:types:check | ✅ | 65 类型（含迁出的 aiya-dto） |
| check:style:strict | ✗ | 3 条阻断全在 i18n 批已提交代码（lib.rs:4510 / App.svelte:83 / settings.ts:262 的 docs/64 引用与日期戳）；本批新代码 0 命中 |
| beets 九项冒烟 | ✅ 9/9 | apply/maint/proto/config/catalog/bins/uninstall/lock/enrich |

两处 ✗ 均为 i18n 批（f323438）的既有欠账，属该批次收尾动作，本批不代改。

### 11.5 遗留（W3 输入）

- `player_report` 的 Position 回报不触发队列持久化（acted 门控与桌面一致），
  长播放会话的 position 落库间隔由泵节流决定——W3 事件泵下沉时对齐。
- `app_set_settings` 的 diff 副作用（扫描根变更触发重排）未接；scan:progress /
  beets×5 的 SSE 生产者未接；频谱 AnalyserNode 未实现。均为 W3 既定范围。
