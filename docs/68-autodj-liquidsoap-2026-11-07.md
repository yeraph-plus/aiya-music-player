# docs/68 · Web 版自动 DJ 推流模块（Liquidsoap）执行方案

- 状态：**v2 设计修订**（N1.0 单容器闭环后，按镜像体积实测与四个设计
  点改判双容器 + Live 页扩展；§0 定案表标 v2 的行以新版为准）
- 前置账：docs/65 §7（推流方案 v2 起纳入）/ §9（风险清单）/ §10.1-⑤（W0
  官方镜像 v2.4.6 最小频道实测：request.queue push 两首 → 10s 滚动段 →
  skip = Done → hls.js 连播，**单频道 RSS 76.4 MiB / CPU ≈2%**）；
  docs/65 W4 批次定义；docs/66 §7（W5 镜像化，liquidsoap 层按拍板暂缺）；
  LIVE_STATION_FIELDS.md（站单字段契约 + `source: 'service'` 预留 +
  服务端对接草案）
- 修订：v1（2026-11-07）——对标 AzuraCast **AutoDJ 基础面**重定范围：
  节目表式歌单编排 + 切歌；**不做**录音入口、IceCast 流、ICY、Web 面板、
  多用户。架构定调：**Liquidsoap 独立工作，应用层只负责给引擎提供配置**。

## 0 · 定案（拍板记录）

| # | 决定 | 来源 |
|---|---|---|
| 1 | ~~单容器 apt~~ → **v2 修订：compose 双容器**（官方 `savonet/liquidsoap` 镜像外置 + 应用内 worker 管线经**共享配置卷**交配置）——单容器实测 runtime apt 层 562MB（liquidsoap 全家依赖 ~450MB），镜像 1.32GB 过重；双容器应用镜像回到 ~870MB，且 liq 升 2.4.6（W0 已实测、aac 回归） | 用户 v2 拍板 2026-11-07（§8/§12.0） |
| 2 | 控制层**合入 aiya-server 内嵌模块**，不新建独立 crate、不进 host-core | 本档 §4 论证（裁量项，用户放行） |
| 3 | ~~版本决策树~~ → **v2 修订：liq 容器 = 官方镜像 v2.4.6**（W0 ⑤ 实测通过口径），2.1.3 差异清单（§11.1）转历史档案——渲染器按 2.4 语法回归（aac/directory 标签/cross(2.)） | 随定案 1 |
| 4 | ~~开播管理器新页面~~ → **v2 修订：收台页（LiveView）双 tab 扩展**——"收听"tab 照旧 + "我的频道"tab（caps.streaming 才渲染）承载管理器；开播频道按站单进收听 tab | 用户 v2 拍板"直接拓展当前页面更省事"（§12.3） |
| 5 | 选歌智能在**应用层**（server 按规则选歌 + DB 记账防重复 → push），Liquidsoap 只管编排/转场/调度 | 对齐 AzuraCast 原版分工（其 AutoDJ = PHP 选歌，Liquidsoap = 流引擎） |
| 6 | 桌面（tauri 宿主）不做推流，开播管理器入口按能力位隐身 | docs/65 "linux 侧 web 版" 定调延续 |
| 7 | 站单**静态字段**走 `station_list` 命令下发（LIVE_STATION_FIELDS 契约：服务端权威 id、用户已填字段赢）；**动态元数据**走 `/hls/{ch}/ws` 接触点（对外部/远程收听端的公开协议面；自家 UI 继续走 /events SSE） | 用户 v2 提议 ws 同路径接触点（§12.1） |
| 8 | 节目表 UI = 一周×24h 网格展示 + 时段绑定歌单（station_schedules 数据模型不变：start_hm/end_hm/days_mask/priority）；频道级开播/停播开关 | 用户 v2 拍板（§12.2） |
| 9 | 流鉴权二档：stations.visibility `private`（默认，过 auth 层）/ `public`（/hls/{ch}/* 与 ws 免鉴权，路径豁免集随频道变更重建） | 用户 v2 拍板（§12.4） |

## 1 · 引擎选型结论（ffmpeg 判否）

三条硬边界（详细论证见方案讨论记录，此处存结论）：

| 能力 | Liquidsoap | ffmpeg |
|---|---|---|
| 运行时编排（插歌/点歌/换歌单） | `request.queue` + `fallback` 链 | **结构性缺席**：filtergraph 编译期固定，改歌单 = 杀进程重拉（断流 + 缝隙） |
| 壁钟节目表（整点切节目） | `switch()` 时钟驱动 + transition + 时区 | 无任何调度概念，只能外部 cron kill/restart |
| 相邻轨动态 crossfade | `cross()` 运行时作用于任意相邻两轨 | `acrossfade` 编译期两两固定 |

ffmpeg 路线的实质 = 自研 mini-Liquidsoap（调度器/选歌器/队列/转场全上
应用层），ffmpeg 退化成 HLS 段编码器——工作量倒挂且转场生硬。**判据不是
"能不能流成 HLS"，是"运行时编排"这一条。** 且 AzuraCast 原版即此分工：
选歌在应用层、流引擎在 Liquidsoap——"应用层只给配置"是对标产品的原版
形态，不是折衷。

## 2 · 架构形态

```
server（crates/aiya-server，应用层）          liquidsoap（独立子进程，每频道一个）
├─ 数据模型：stations / schedules /          ├─ 吃渲染好的 .liq 配置
│  station_playlists / station_history       ├─ fallback(request.queue, switch(节目表), 垫底)
├─ 配置渲染器 liqgen：DB → .liq 模板          ├─ cross() 转场
│  → liquidsoap --check 校验闸               ├─ request.queue 接应用层 push（插播）
│  （失败 = ApiError 带 stderr 摘录，         ├─ output.file.hls 滚动段 → /data/hls/<ch>/
│   beets E_CONFIG 同款理念）                 │    （既有 /hls ServeDir 直接吐给 hls.js）
├─ 生命周期：spawn / 崩溃重启退避 /           └─ on_track 钩子 → 追写 JSONL
│  server 退出收尸（复用 beets spawn 手法      /data/hls/<ch>/now.jsonl
│  + proc.rs no_window；telnet 协议薄，
│  不配独立 crate）
├─ 控制面：telnet 127.0.0.1:<port> push/skip/queue/stop
└─ 状态回灌：轮询 now.jsonl（1s）→ SSE "stations" 通道 → UI "正在播"
```

接触面**只有两个**：配置文件（渲染器产出）+ telnet 控制与 JSONL 状态
（运行时）。与 beets 的差异要点明：beets 的"独立"是自家代码独立包（NDJSON
状态机协议、版本窗口、独立发布线，值一个独立 crate）；liquidsoap 的"独立"
是第三方引擎整进程隔离——协议是几条文本命令 + 一个文件，包 crate 是过度
抽象，进 host-core 则把桌面编译图带上死代码（推流 server 专属，违背
"桌面不背 server 专属面"的边界精神）。

## 3 · 频道与节目模型

表（aiya-data repo 增量，schema 版本随桌面同库不分叉——docs/66 §"DB
schema 不做客户端分支"原则）：

```
stations            id, name, subtitle, description, cover_url,
                    hls_dir(段目录名,默认 ch<id>), active(bool),
                    created_at_ms, updated_at_ms
station_playlists   id, station_id, name,
                    kind('manual'|'random_all'),          -- MVP 两档
                    track_ids(JSON, manual 用),
                    rule(JSON, 预留规则引擎：genre/artist/年份过滤)
station_schedules   id, station_id, playlist_id,
                    start_hm('08:00'), end_hm('10:00'),   -- end<=start 表跨零点
                    days_mask(int, bit0=周一), priority(int, 重叠时高者赢)
station_history     id, station_id, track_id, played_at_ms
                    -- 频道播放记账独立成表，不进曲库 tracks.last_played
                    --（污染用户手动播放统计）；防重复窗口按此表查
```

- MVP 歌单两档：`manual`（UI 挑歌集合）+ `random_all`（全库随机，按
  station_history 窗口防重复，如"近 24h 不重歌"）。规则引擎（genre/权重）
  留 `rule` JSON 位，不在 MVP。
- 站单下发：`station_list` 附 HLS 地址 → 前端按 LIVE_STATION_FIELDS 草案
  写 `liveRecent`（`source: 'service'`，服务端权威 id；**用户已填字段赢，
  服务端只补空白**——合并管线已有，`normalizeLiveRecent` 归一路径不动）。
- HLS 地址形态：`/hls/<hls_dir>/live.m3u8`（相对同源；反代场景与
  `/stream` 同规则——见 §10 鉴权项）。

## 4 · 控制层（模块落点与协议面）

- **落点**：`crates/aiya-server/src/station.rs`（编排/守护/telnet/回灌）+
  `liqgen.rs`（渲染器，纯函数 DB 行 → String，单测友好）。不进 host-core、
  不建 crate（论证见 §2）。
- **spawn**：每频道一进程 `liquidsoap /data/hls/<ch>/station.liq`。
  渲染 → 写临时文件 → `--check` 通过后落正式名 → spawn。复用 beets.rs 的
  spawn 手法（子进程组、日志导向）与 proc.rs no_window（Windows 编译面
  不进——station.rs 全模块 `#[cfg]` 不需要：server 本就只在 linux 编译图
  需要，但代码保持跨平台可编译，spawn 细节用 host-core 现成助手）。
- **守护**：spawn 后 tokio 任务 `waitpid`，异常退出按退避重启（1s/2s/5s/
  上限 30s），重启计数进 `station_status`；server 关停时 kill 全部频道
  进程（收尸；孤儿防御：liq 侧 `set("frame.dpi")` 无关，靠进程组 kill）。
- **telnet 控制**：端口分配 `12700 + (station_id % 2000)` 冲突时线性探测；
  命令面 MVP 四条：`request.push <uri>`、`<ch>.skip`、`queue`、`stop`。
  连接短连（每命令一连接），断连重试天然安全。
- **状态回灌**：`.liq` 里 `on_track` 追写 JSONL（title/artist/uri/
  started_at）到 `now.jsonl`；server 1s 轮询读尾部 → 变化才广播 SSE
  `stations` 通道（events.rs broadcast 现有频道机制，加一个 channel 名）。
  选文件不选 http 钩子：**2.1 就有**、断线无状态、无新端口。若复测发现
  2.1 支持出站 http 推送再升级（风险清单 §10.1）。
- **配置生效语义**：频道增/改/删 = 重渲染 + 重启该频道进程（liq 无热
  重载）。切换瞬间 hls.js 自动重连，滚动窗口（默认 ~30s 段留存）内无感。
  写进 UI 文案："编辑后频道重启一次"。

## 5 · 配置渲染（.liq 模板骨架）

```liquid
set("log.file.path", "/data/hls/ch1/liq.log")
set("log.stdout", false)
set("server.telnet", true)
set("server.telnet.bind_addr", "127.0.0.1")
set("server.telnet.port", 12701)

# 插播优先（fallback 链顶）；request.queue 空时落节目表
request = request.queue(id="request")
tbl = switch(id="schedule", track_sensitive=false, [
  ({ 8h-10h and 1w-5w }, pl_morning),   # 渲染器按 station_schedules 展开
  ({ 10h-12h and 1w-5w }, pl_daytime),
])
base = playlist(id="default", mode="randomize", "/data/hls/ch1/default.m3u")
safe = blank(id="blank", duration=2.)   # 全空兜底：宁可 2s 静音不静默断流

src = fallback(track_sensitive=false,
              [request, cross(2., tbl), base, safe])
output.file.hls(playlist="live.m3u8", segment_duration=6.0,
                segments=6, segments_overhead=6,
                directory="/data/hls/ch1", fallible=true,
                on_error=function(_) { 13. }, src)
# on_track: 追写 /data/hls/ch1/now.jsonl（渲染器展开成 liq 函数）
```

- 节目歌单文件（`default.m3u` 等）由 server 生成（绝对 file:// uri 清单），
  节目表切换时按需重写 + `request.push` 免插播语义区分。
- 渲染器是纯函数：输入 station/schedules/playlists 行，输出 .liq 文本 +
  m3u 文件集合。单测锁"给定 DB 行 → 关键指令行存在"。
- `--check` 闸：`liquidsoap --check <file>` 退出码非 0 → ApiError 带stderr
  末 20 行（用户能在 UI 看到哪行配置写坏了——beets 配置门同款体验）。

## 6 · API 面（/ipc 命令族）

走既有 `/ipc/{cmd}` 单面模式（不另开 REST 挂载，contract.rs 反向闸自动
覆盖登记）：

```
station_list            频道 + 状态（running/crashed/stopped + now playing）
station_create/update   基本信息（name/subtitle/description/cover_url）
station_delete          停进程 + 清段目录（GC 策略 §8）+ 删站单记录
station_set_playlists   歌单 CRUD（manual 挑歌 / random_all）
station_set_schedules   节目表 CRUD（时段/星期/优先级）
station_activate        启动/停止频道（active + 立即生效）
station_skip            手动切歌（telnet）
station_push            插播指定 track（点歌）
```

SSE：新通道 `stations`（频道状态与 now-playing 变化广播）。事件 payload
与 `station_list` 同形——首连快照补发（与 ipc/events 现有语义一致）。

## 7 · UI 面（开播管理器 ≠ 收台页）

- **能力位**：`backend-caps.ts` 增 `streaming: boolean`——web 宿主 `true`、
  tauri/mock 恒 `false`（mock 恒桌面形态的既有口径）。server 侧预留
  `AIYA_STATIONS=0` 死开关（灾难恢复隐藏入口，MVP 可后补）。
- **新 page**：`page: 'station'`（Sidebar 新条目，图标 RadioTower，仅
  caps.streaming 渲染）→ `views/StationView.svelte`。MVP 三块：
  1. 频道列表卡（状态点 / 正在播 / 启停 / skip）
  2. 频道详情：基本信息 + 歌单编排（挑歌走曲库复选，与既有选择器同款）
     + 节目表列表编辑器（时段/星期行编辑，**不画时间轴**——MVP 列表就够，
     时间轴是装饰）
  3. "正在播"条随 SSE 走
- **收台页（LiveView）不动**；开播即自动出现在收台列表（`liveRecent`
  站单下发，§3）。i18n 走 domains/ 新增 `station.ts`（zh-Hans + en）。

## 8 · 镜像与部署（v2：双容器）

**体积实测账（2026-11-07，docker history）**：

| 项 | 单容器（v1） | 双容器（v2） |
|---|---|---|
| base bookworm-slim | 85MB | 85MB（应用）/ 官方镜像自带 |
| runtime apt 层 | **562MB**（liquidsoap 全家依赖 ~450MB + Qt5/libdrm/curl） | ~112MB（仅 Qt5/libdrm/curl） |
| beets 套件包 zip | 218MB | 218MB |
| server + ui | 11MB | 11MB |
| **应用镜像合计** | **1.32GB** | **~870MB** |
| liq 容器 | （内置） | 官方 `savonet/liquidsoap:v2.4.6` 881MB（pull 不构建，不占自家 ghcr） |
| 部署总 pull | 1.32GB | ~1.75GB |

- 结论：应用镜像 -450MB；总下载量略增（881MB 官方镜像换走 450MB apt 依赖
  + 版本红利）；若后续再压，下一刀是 beets zip 218MB（口径单议）。
- **liq 版本红利**：官方 v2.4.6 = W0 ⑤ 实测口径——aac 编码回归、
  `cross(2., src)`/`directory=` 标签回归，渲染器比 2.1.3 形态更简；
  §11.1 差异清单转历史档案（保留：若用户自建精简 liq 镜像走 apt 2.1.3 时
  仍适用）。
- **compose 形态**：

  ```yaml
  services:
    aiya-server:        # 应用镜像（ghcr.io/yeraph-plus/aiya-music）
      volumes:
        - aiya-data:/data
        - aiya-hls:/hls             # 共享卷：段 + 配置 + 回灌
      environment:
        - AIYA_LIQ_HOST=liquidsoap  # telnet 控制面（默认 127.0.0.1 = 单容器兼容）
    liquidsoap:
      image: savonet/liquidsoap:v2.4.6
      volumes:
        - aiya-hls:/hls
        - ./docker/liq-keeper.sh:/keeper.sh:ro   # 管家脚本挂载，不构建镜像
      command: ["sh", "/keeper.sh"]
  ```

- **worker 管线（应用侧）**：station.rs 渲染 .liq → 写共享卷
  `/hls/conf/{ch}/station.liq` + `manifest.json`（期望状态：启停/端口）；
  **telnet 控制面** host 从 127.0.0.1 改为 `AIYA_LIQ_HOST`（单容器不变）。
  spawn/守护任务替换为"配置写入 + 对账读"——station.rs 主体与命令面不动。
- **管家（liq 容器内，keeper.sh ~40 行 sh）**：2s 轮询对账——manifest 有
  而进程无 → spawn；manifest 变更（liq 文件 mtime/hash）→ kill+respawn；
  manifest 无 → kill+清运行位。进程态记 `/hls/conf/{ch}/.pid`。
  官方镜像无 inotify-tools，轮询足够；自家代码只有这份 sh 挂载进官方镜像。
- **回灌/HLS 段**：同一共享卷，server 侧轮询路径与 /hls ServeDir 根不变
  （卷内同路径）——N1.0 代码零改动。
- env/卷：`AIYA_LIQ_HOST`（新增，可选）；`TZ` 交给部署（节目表按容器本地
  时区解释）。
- **磁盘 GC**：段滚动窗口 liq 自清；server 清扫频道删除残留与孤儿目录
  （docs/65 §9.5 落案）。

## 9 · 批次拆解（N1）

| 批 | 内容 | 验收 |
|---|---|---|
| **N1.0 复测批 ✅** | 单容器 2.1.3 全链闭环（§11 施工记录：boot/HLS/回灌/SSE/skip/push 七项全绿）+ 差异清单 §11.1；**随 v2 双容器定案，2.1.3 适配转历史档案** | 已达成 |
| **N1.1 数据面** | 四表 + **stations.visibility 字段**（§12.4）+ repo 增量 + dto（StationRow 等，ts-rs 四 crate 生成链）+ contract 登记命令族（SERVER_ONLY 豁免续用） | dto_wire / contract / gen:types 全绿 |
| **N1.2 引擎面 v2** | 渲染器按 **2.4.6 语法回归**（aac/directory 标签/cross(2.)，§11.1 反推）+ docker/liq-keeper.sh 管家 + 配置卷写入（spawn/守护任务退役）+ `AIYA_LIQ_HOST` telnet 面 + now.jsonl 回灌 → SSE | compose 双容器起：建频道 → 段落盘 → skip 生效 → SSE 收到正在播；server 重启推流不断 |
| **N1.3 命令面** | §6 全命令 + visibility 开关 + 防重复选歌（station_history 窗口）+ m3u 生成/重写（**annotate:title/artist 注入**）+ 公开/私人路径豁免集 | curl 全命令冒烟；public 免 token 可拉段、private 无 token 401 |
| **N1.4 UI 面** | backend-caps.streaming + **LiveView 双 tab**（收听 / 我的频道）+ 频道管理三块（基本信息/歌单编排/**周×24h 节目表网格**）+ 站单下发 liveRecent（source='service'）+ i18n | mock e2e（caps.streaming=false 无 tab）+ http e2e 新用例（建频道→开播→收听 tab 出现该台卡片→点播）|
| **N1.5 收尾** | GC + **ws 接触点 `/hls/{ch}/ws`**（§12.1：hello+track 帧）+ 验收矩阵 + README/compose 注释 + 本档转 closed | **三家浏览器连播、skip ≤1 段、整点节目表切换、编辑频道重启后 hls.js 自愈**（docs/65 W4 验收原话） |

## 10 · 风险与实测清单

1. **2.1.3 行为差异复测清单**（N1.0 逐条）：telnet 命令名与 `request.push`
   语法、`annotate:` 标签语法（2.0 变过）、`output.file.hls` 参数面
   （segment_duration/segments/segments_overhead）、`cross()` 与
   `switch()` 的 transition 组合、`on_track` 钩子签名。
2. **telnet 断连**：短连模式天然规避长连状态；push 失败重试一次后报
   station_status=unreachable（守护任务负责重启进程而非重试 telnet）。
3. **/hls 鉴权面**：`/hls` 现为 nest ServeDir，其在 auth 层内外的位置
   N1.3 核实收口；段 URL 若需 token，hls.js（xhrSetup）与 `<audio>`（查询
   参数）携带方式不同——W0 ④ 已实测透传可行，落地按 m3u8 重写同款。
4. **跨零点节目**：`end_hm <= start_hm` 渲染为跨夜区间（liq 时间谓词
   `or` 拼接），单测锁。
5. **时区**：节目表语义 = 容器本地时区，compose 注释 `TZ`；UI 编辑器
   标注"按服务器时区"。
6. **容器卷 SQLite WAL 5 万首扫描基线**（docs/65 §9.3）：频道选歌查询
   与扫描同库，N1.3 顺带跑一次 `bench:50k` 口径复测。
7. **>50 并发频道**（docs/65 §9.4）：单频道单进程 RSS 76MB，10 频道内
   无虞；多 output 单进程模型远期再议。
8. **收尸边界**：server 被 kill -9 时频道进程成孤儿——容器 ENTRYPOINT
   语义下 server 即 PID 1 子进程树根，容器停止整体回收；docker exec 手工
   液态 soap 不在支持面。

## 11 · N1.0 施工记录（2026-11-07，构建 + 基本控制层闭环）

### 11.1 bookworm apt 2.1.3 复测差异（对照 2.4.6 / 方案假设，逐条实证）

| # | 差异 | 2.4.6 写法（W0） | 2.1.3 写法（实证） |
|---|---|---|---|
| 1 | crossfade | `cross(2., src)`（duration 位置参） | **原生 cross 要 transition 函数**；用 stdlib 高级封装 `crossfade(~duration=2., src)`（libs/fades.liq:365） |
| 2 | output.file.hls 目录 | `directory="/x"` 标签 | **纯位置参数**（无 tmpdir/directory 标签），且编码格式同为位置参：`output.file.hls(..., fallible=true, "/dir", [("stream", %mp3)], src)` |
| 3 | 编码格式 | `%audio/aac` | **Debian 主包无 ffmpeg/aac 绑定**（无插件拆包可补）；`%mp3`（lame）在主包——**HLS 走 mp3 段**，hls.js 可播（CODECS="mp4a.40.34"） |
| 4 | track 钩子 | `on_track(f, src)`（2.2+） | **方法式** `source.on_track(src, f)`；顶层 `on_track`/`on_metadata` 均未定义 |
| 5 | null 字面量 | `null` | **`null()`**（裸 null 被当函数，类型错 `(?_)->_`） |
| 6 | root 运行 | —（W0 官方镜像非 root？未遇） | **必须 `settings.init.allow_root.set(true)`**，否则静默退出（security exit） |
| 7 | 元数据缺键 | — | `m["title"]` 缺失给**空串**（`??` 只救 null）——空串兜底要显式 if |
| 8 | telnet 命名空间 | `ch1.skip`（源域） | **全局 `request.push <uri>` / `request.skip` / `request.queue`**；源域 skip 以 output id 命名（`/t/hls/live.m3u8.skip`） |

决策树（§8）落点：**bookworm 2.1.3 成立**（%mp3 路线），trixie/savonet 不触发。

### 11.2 最小控制层（station.rs）形态与实测

- **落点**：`crates/aiya-server/src/station.rs`（渲染 `render_liq` 纯函数 +
  boot + 守护 + telnet + 回灌轮询）——本轮单文件，N1.2 拆 liqgen.rs。
- **频道定义**：`AIYA_STATION_BOOT=<段目录名>`（env 测试频道，N1.1 换 DB
  行）；`AIYA_STATION_PORT` 可覆盖 telnet 端口（默认 12701）。歌单 = 曲库
  在线曲目随机 100。
- **挂载语义**：`/hls` 挂载改 **station 感知**——boot 的段根（
  `<data_root>/hls`）优先，`AIYA_HLS_DIR` 试验位兜底（W0 spike 通道保留）。
  boot 在 router 构建前执行（main.rs 顺序），挂载无需重载。
- **回灌通道定案**：`on_track → file.write 覆盖写 now.jsonl`（docs/68 §4
  原方案）——stdout print 被**管道块缓冲**攒行不刷（实测 nowPlaying 迟迟
  为空的破案点）；覆盖写"当前轨"语义免轮转 GC；server 1s 轮询读尾部，
  **值变化才广播**（SSE `stations` 通道，events.rs broadcast 现有机制）。
- **日志**：liq 日志落 `<dir>/liq.log`（log.stdout=false，spawn stdout/
  stderr 均弃）——诊断面不再依赖宿主抓流。
- **契约面**：`station_status/skip/push` 三命令进 ROUTED；contract.rs 新增
  **SERVER_ONLY 豁免集**（server 路由、UI 未接线——N1.4 StationView 上线
  后逐条清空，豁免 ≠ DESKTOP_ONLY：它们在 server 是真路由）。
- **容器端到端（全绿）**：env 起频道（渲染 → --check → spawn）→ mp3 段
  滚动落盘 → `/hls/ch1/live.m3u8` master/variant/段三拉全 200 →
  nowPlaying 经 now.jsonl 轮询进 status → SSE `stations` 帧 0.4s 内到达 →
  `station_skip` telnet 返回 `Done.` → `station_push`（uuid v5 id → SQL
  path → telnet `request.push file://…`）返回请求号 28 入队。
- **实测坑（本轮新增）**：
  - docker build 链上 `grep -c` 计数 0 退出码非零，`&&` 短路导致**镜像没
    重建**而容器行为"回退"——连续两轮假线索；build 必须独立跑并打印
    exit code；
  - `docker exec` 的容器内路径在 MSYS 下被毒化（`/data/...` →
    `C:/Program Files/Git/data/...`）——`MSYS_NO_PATHCONV=1` 一律前置；
  - SSE 值变化闸是"同值不发射"——两次写同一探测值第二次必无帧，别误判
    通道断连（先查 status.nowPlaying 确认值真的变了）。

### 11.3 遗留与下一步

- station_push 的 trackId 获取在 UI 侧走曲库选择器（N1.4），命令面本身
  已验；uuid v5 id 可从 path 复算（NS_TRACK 常量，ids.rs:12）。
- m3u 条目尚未带 `annotate:title=` ——4 首 fixture wav 无标签时回灌恒
  "unknown"；N1.3 选歌注入 title/artist 后"正在播"才有真实可读文案。
- now.jsonl 覆盖写无并发锁：liq 单线程 on_track 顺序写，宿主轮询读半行
  以 trim+跳过容错——两条路径都是单写单读，不加锁。
- N1.1（四表 + dto + repo）按 §9 批次表推进。


## 12 · v2 设计修订详设（四个设计点）

### 12.1 站单下发与 ws 接触点（静态/动态分离）

- **静态字段**（LIVE_STATION_FIELDS 契约：id/title/subtitle/description/
  cover/addedAtMs）走 `station_list` 命令下发——前端 merge 进 `liveRecent`
  （`source: 'service'`，**用户已填字段赢、服务端只补空白**，合并管线既有）。
  静态字段不需要长连接，塞进 ws 是重复面。
- **动态元数据**走 **`/hls/{ch}/ws`**（与 m3u8 同路径空间，一个前缀管一个
  频道；axum ws feature）：
  - `hello` 帧（连接即发）：站单静态字段 + `hlsUrl` + `program`（当前节目
    名，来自节目表命中行）+ `visibility`；
  - `track` 帧（on_track 实时）：`title/artist/artworkUrl`（歌曲封面，URL
    按可见性决定是否携带 `?token=`——§12.4）；
  - 心跳 15s；私人频道 ws 要求 `?token=` 与流同闸。
- **两条通道的分工**：自家 UI 收实时信息继续走 `/events` SSE `stations`
  通道（N1.0 已通）——ws 是给**外部/远程收听端**的公开协议面（手机浏览器
  直开 URL、将来桌面瘦客户端远程连接"和普通播放一样映射地址"）。两通道
  并行不互斥，ws 帧载荷与 SSE 同构，同一处状态源（now.jsonl 轮询）广播。
- 反代注意：ws upgrade 头透传（compose 部署注记）。

### 12.2 节目表 UI（一周 × 24h）

- 数据模型不动（station_schedules：`start_hm/end_hm/days_mask/priority/
  playlist_id`）——网格是它的可视化。
- **形态**：频道详情内"节目表"块 = 7 列（周一～日）× 24 行小时格：
  - 已排时段着色（每歌单一色），跨零点时段跨行显示（渲染语义已有：
    end<=start 跨夜展开）；
  - **点击空格 → 时段编辑对话框**（起止/星期/歌单选择）——拖选多格 MVP
    不做（工作量一档，后补）；
  - 重叠时段按 priority 高者赢，网格上重叠区显示最高优先级并加角标；
  - "未排时段"全局默认歌单（频道级 fallback，渲染器 base playlist）。
- **开播/停播**：频道卡头部开关（`station_activate` 命令已有语义）——停播
  = 管家 kill 进程 + 段目录保留（再开播热恢复）。

### 12.3 收台页扩展（双 tab）与存储契约

- **信息架构**：LiveView 双 tab——
  - 「收听」：现状照旧（liveRecent 卡片 + 播放 + 悬浮信息层）；
  - 「我的频道」（`caps.streaming` 才渲染 tab）：频道列表卡（状态点/正在
    播/启停/skip）→ 频道详情（基本信息 + 歌单编排 + §12.2 节目表网格）。
  - 开播频道自动出现在「收听」tab（站单下发）——**听与管同页跳转、UI 不
    分家**；独立 StationView page 取消（省 Sidebar 入口与导航面）。
- **存储契约**：收听列表（`liveRecent`，客户端设置 JSON）与服务端频道
  （`stations` 表）**各自独立存储**；**字段契约复用**——StationRow 下发时
  映射 LiveRecentEntry 形状（title/subtitle/description/cover），约定字段
  同名同义；`id` 服务端权威、`url` 可随部署变、用户在收听卡片上补填的字段
  赢——与 LIVE_STATION_FIELDS 草案逐字对齐。
- 站单下发的卡片带 `source: 'service'` 标记：UI 对 service 卡片不做字段
  覆盖写（改动走服务端命令），manual 卡片不受影响。

### 12.4 流鉴权二档（public/private）

- `stations.visibility`：`private`（默认）/ `public`，频道编辑可切。
- **private**：`/hls/{ch}/*` 与 `/hls/{ch}/ws` 过 auth 层（N1.0 实证：无
  凭据 401）；收听端 token 携带 = `?token=`（m3u8 与段 URL 透传，W0 ④ 已
  实测；hls.js xhrSetup / `<audio>` 查询参数两种形态都有先例）。
- **public**：两前缀免鉴权——auth 层维护**路径豁免集**
  （`Arc<RwLock<HashSet>>`，启动 + station_create/update/delete 时重建；
  中间件按前缀匹配，不逐请求查 DB）；站单 URL 不带 token。
- **封面**：MVP 走站单 `cover_url`（频道封面，公开频道可直链）；正在播
  **歌曲**封面的 artworkId → `/artwork/{id}` 在 public 频道下暂不豁免
  （避免整库封面目录泄露），ws track 帧的 artworkUrl 携带 `?token=`
  （server 侧生成，有效期跟会话）——N2 再议公开频道的封面策略（短期签名
  或 artwork 白名单）。
- 部署警告（对齐 docs/65 口令明文条款）：public = 局域网/公网无凭据可播
  ——分享场景开、默认收着。


## 13 · N1.1–N1.3 施工记录（2026-11-07，双容器闭环）

### 13.1 落地面

- **数据面（N1.1）**：四表直接压平进 `migrations/0001`（项目未发布无存量库
  的既定策略；0004 独立文件作废）；`aiya-data/src/station.rs`（StationRow/
  Playlist/Schedule 行 + create/update/delete/set_active/replace_*/history
  防重复窗口，写路径全事务全量替换语义）+ `ids.rs` 三个 NS_* v5 派生；
  `aiya-dto/src/station.rs`（StationSummary/Detail/Playlist/Schedule + Edit
  载荷，camelCase/毫秒/track_ids 已解析）；gen:types 72 类型链通。
- **引擎面（N1.2 v2）**：station.rs 多频道重构（HashMap 频道表、端口线性
  探测分配、stop 走 mpsc 信号/摘 ON 位）；**InProcess/Sidecar 双形态**由
  `AIYA_LIQ_HOST` 有无决定；Sidecar 写 `hls_root/conf/{ch}/station.liq +
  ON + manifest.json`（**顶层 conf/**——与频道段目录 /hls/{ch}/ 平行，keeper
  契约）；`docker/liq-keeper.sh`（~40 行 sh：2s 对账，md5 变更重启/崩溃
  respawn/停播 kill，.pid/.md5 记账）；`AIYA_HLS_ROOT` 定段根（双容器
  /hls、单容器 data_root/hls）；runtime_of 的 running 判据按形态分流
  （Sidecar = ON+.pid 文件位，InProcess = 守护原子位）。
- **命令面（N1.3）**：10 命令族（list/get/create/update/delete/
  set_playlists/set_schedules/activate/skip/push）接 DB；auth 中间件加
  **public 频道路径豁免**（/hls/{dir}/ 前缀按内存频道表判，不查 DB）；
  contract SERVER_ONLY 豁免 10 条（N1.4 接线后清）。
- **镜像**：Dockerfile 摘 liquidsoap——**1.32GB → 661MB**（apt 全家
  ~450MB + 历轮层混账一起出清）；liq 容器 = 官方 savonet/liquidsoap:v2.4.6
  （pull 不构建）；compose.yaml v2 双服务样例（曲库**同款只读挂给 liq
  容器**——m3u 里的文件路径两容器一致）。

### 13.2 2.4.6 实测差异（对照 §11.1 的推断——三条被推翻）

| 项 | §11.1 推断 | 2.4.6 实测 |
|---|---|---|
| output.file.hls 目录 | directory 标签回归 | **标签不存在**（Error 6，与 2.1 同构）——位置参一 = `dir`（hls.liq：`dir, streams, s`） |
| crossfade | cross(2., src) 位置参 | **stdlib crossfade(duration=2., src)**——原生 crossfade/cross 首参均非 float |
| track 钩子 | 顶层 on_track(f, src) | **方法式 `src.on_track(synchronous=true, f)`**（副作用方法返回 unit，独立调用不重赋值） |
| 编码器 | %audio/aac 回归 | **官方镜像同样缺 %audio/aac 老语法**；%ffmpeg 复合编码器可用但 **ts 段卡视频流不出段**（无视频输入）——**退 %mp3（lame，滚段正常）**；段名 stream 名避开 `#`（URL fragment 语义冲突，浏览器/客户端请求会剥） |
| keeper 起进程的前提 | — | 官方镜像默认用户 uid 100(liquidsoap) **写不动 server(root) 建的卷目录**——liq 容器 `--user root` 跑（模板已有 allow_root 闸）；**曲库只读卷必须同挂 liq 容器**（m3u 的文件路径在 liq 侧解析） |

### 13.3 实测坑（本轮新增）

- **管道吃退出码三度咬人**：`cmd | head -3; echo rc=$?` 的 rc 是 head 的——
  2.4.6 "有 directory 标签"的错误结论即由此而来（§11.1 第 2 条作废）；
  check 判定必须落盘真实退出码或直接看错误行。
- **PathBuf::join 绝对路径陷阱**：`root.join("conf").join(&hls_dir)` 中
  hls_dir 带 `/` 前缀时**整体替换根**——conf 判据落空（running 恒 false）；
  取 `file_name()` 相对名再拼。
- **docker build 链上 `grep -c` 计数 0 退出码非零**短路 build（§11.2 已记，
  本轮再次复现——build 永远独立跑并打印 exit）。
- **e2e 容器重建顺序**：卷里有旧 conf 目录时 keeper 会按旧配置对账——
  全清（两容器+两卷+network）后再跑，别留半旧状态。
- **镜像拉取旁路**：daemon 无代理配置时，`curl -x http://127.0.0.1:10808
  --ssl-no-revoke -L` 手工走 registry API（token→index→amd64 manifest→
  blobs→OCI layout tar→docker load）——**-L 必带**（blob 307 跳 CDN）；
  Windows curl 的 schannel 吊销检查在代理后必挂（--ssl-no-revoke）；
  daocloud 白名单不含 savonet、1ms 对该镜像挂起。

### 13.4 验收（compose 双容器，全绿）

create(public) → activate on（keeper 2s 内 spawn）→ **mp3 段滚动**
（stream_1..N.mp3 + stream.m3u8 + live.m3u8）→ **public 免 token 三连 200**
（master/variant/段）→ skip `Done.`（telnet 经 AIYA_LIQ_HOST:port）→
nowPlaying 回灌（now.jsonl → 1s 轮询 → SSE `stations` 帧）→ activate off
（ON 摘除 → keeper kill → running 翻转）。server 重启推流不断（keeper 与
server 生命周期解耦——双容器形态的核心红利）。

### 13.5 遗留与下一步

- **aac 段**：%ffmpeg ts 段卡视频流的问题待解（音频-only 声明的正确姿势，
  或 `frame.video.enabled := false`）——mp3 段功能等价，不阻塞。
- **N1.4 UI 面**：LiveView 双 tab + 节目表网格 + 站单下发（dto 已就位）。
- **N1.5**：ws 接触点、磁盘 GC、`annotate:` 注入真实曲目名。
- m3u 随机 100 首的"整表重写触发段缓存刷新"策略（playlist reload）在
  N1.3 的选歌深化里一起收。

## 14 · N1.4–N1.5 施工记录（2026-11-08，UI 面 + 接触点收口）

### 14.1 N1.4 UI 面

- **能力位**：`backend-caps.streaming`（web 恒有、tauri/mock 恒无）。
- **client.ts station 段**：10 命令全接线（参数侧结构类型，对齐"生成物只
  覆盖返回值"的文件约定）；**contract SERVER_ONLY 豁免清空**——UI 调用集
  已覆盖全部 10 条，正向断言恢复全量守卫。
- **i18n**：domains/station.ts（zh/en 键位奇偶，~70 键）。
- **StationPanel**（components/station/）：频道列表卡（状态点/正在播/启停/
  skip/复制地址/编辑/删）+ 编辑表单（含 visibility 二档与分享警示）+ 歌单
  编排（manual 走 library_list 搜索挑歌 / random_all）+ **周×24h 节目表
  网格**（schedules 展开 day×hour 覆盖集着色、点击空格建时段、重叠
  priority 高者显示、跨零点时段展开）——歌单/节目表整表提交。
- **LiveView 双 tab**（TextTabs 复用）：「收听」照旧 + 「我的频道」（
  caps.streaming 才渲染）；站单下发 `mergeServiceStations`（**用户字段赢、
  服务端只补空白**、id 服务端权威、url 随部署更新）——挂载即拉 + 管理器
  onchanged 再拉。
- **e2e**：http 套件新增用例（建频道→详情→列表→站单卡片进收听 tab→改
  visibility→删除），**双语兜底**（headless Edge 首启写库英文的既知坑）；
  http-e2e-server.mjs **补 UI 构建步**（STATIC_DIR 托管 dist——UI 源码
  改动不进 dist，e2e 跑的就是上一轮前端；win 下 spawnSync npm 要 shell）。
  http 6/6 + mock 44/44 + svelte-check 0/0。

### 14.2 N1.5 接触点收口

- **annotate 注入**：m3u 行 `annotate:title="…":{path}`（title 内嵌引号
  换写）——回灌 nowPlaying 出**真实曲目名**（容器实测
  nowPlaying="station-demo-02"，不再是 unknown）。选歌深化（按
  station_playlists/rules/防重复窗口生成 m3u）留下批——表与命令面已就位。
- **GC**：boot 时清扫 /hls 下无 station 行对应的孤儿频道目录（单点执行）。
- **ws 接触点**：`GET /hls/{dir}/ws`（axum ws feature）——连接即 hello 帧
  （站单静态字段 + 当前正在播），之后原样转发 events 总线 stations 通道
  （与 SSE 同一状态源）；鉴权同流（public 豁免 / private `?token=`）。
  **挂载坑**：`/hls/{dir}/ws` 顶层路由与 `/hls` nest_service（内部
  `{*tail}`）路由冲突 panic——**nest 内组合**（route + fallback_service
  ServeDir）是唯一形态；ServeDir 的目录根在 nest 内 fallback 提供。

### 14.3 门禁终态

contract 3/3（SERVER_ONLY 清空）、clippy 0、style:strict 阻断 0（station
新注释的 docs 引用全部改写/转 L2——"代码内不设指向 docs 的路标"语域规则）、
svelte-check 0/0、http e2e 6/6、mock 44/44、station 单测绿、镜像 661MB。
