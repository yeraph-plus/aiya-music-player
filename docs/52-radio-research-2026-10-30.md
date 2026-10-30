# 网络电台调研（2026-10-30）

> 三个问题：① 音频流有没有通用的元数据（封面/标题/副标题）约定；② 电台功能怎么落
> （自有表、不进曲库、只和播放条 + 频谱打交道）；③ 工作区那个 GSAP 3D 卡片 demo，
> three.js 能不能做。
>
> **远程一侧（自建电台服务端）不在本轮范围**：早期 docs/20 里计划过，用户明确说先不做。
> 本文只谈「客户端收别人的流」。

---

## 0. 结论摘要

1. **HLS 实现是完整的，但现实中基本播不动真电台。** 代码只解 ADTS-AAC 段（能力与取舍
   见 `crates/aiya-audio/src/hls.rs` 头部注释），而实测抽样的 m3u8 电台 **全部是
   MPEG-TS 段**——那是代码里显式 `Unsupported` 拒掉的分支。另一半电台（占目录七成）
   是 icecast 直推流，连 m3u8 都没有，压根不是 HLS。
2. **不存在「一个通用协议拿到封面+标题+副标题」。** 行业事实标准是 ICY（只有一行
   `StreamTitle='艺人 - 曲名'` 字符串，没有封面、没有结构）、HLS timed metadata
   （规范完备但没人发）、以及各家私有 JSON。**封面只能拿台标**，这是所有播放器的做法。
3. **电台功能的最大障碍不是元数据，是三条外键。** `queue_items.track_id`、
   `history.track_id` 都 `REFERENCES tracks(id)` 且 `PRAGMA foreign_keys = ON`；电台
   id 进队列/历史会直接违反外键，整笔写事务失败。这是要不要动表结构的决策点。
4. **3D 卡片不需要 three.js，也不需要 GSAP。** 那个 demo 是纯 CSS 3D
   （`perspective` + `rotateY` + `translateX` + `scale` + class 交换 + CSS transition），
   JS 只在算鼠标倾角和串文本动画。用 Svelte + CSS 变量重写即可，还顺带守住
   docs/46 §7「同时只允许一个 rAF 循环」——GSAP 自带 ticker，引进来就是第二个循环。

---

## 1. 现状审计：流支持到哪一步了

### 1.1 有的（比预期多）

| 能力 | 位置 | 说明 |
| --- | --- | --- |
| HLS 播放列表解析 | `hls.rs::parse_media_playlist` | EXTINF / TARGETDURATION / MEDIA-SEQUENCE / ENDLIST / KEY(METHOD=NONE) |
| master playlist | `hls.rs::extract_master_variant` | 取第一个变体，只跟一层 |
| live 轮询 | `hls.rs::refresh` | 按 `MEDIA-SEQUENCE` 判新旧（URI 会被服务端滚动复用，只比 URI 会永久卡死） |
| 段预取 | `hls.rs::Prefetch` | 常驻线程 + 单槽 lookahead；独占模式的设备缓冲只有 10–50ms，段边界同步拉必断音 |
| 断网退避 / 停滞判死 | `OUTAGE_BUDGET` / `stall_budget` | 8s 墙钟预算（含拉取耗时），3×TARGETDURATION 没新段就报错 |
| 字节上限 | `PLAYLIST_MAX_BYTES` / `SEGMENT_MAX_BYTES` | 2MB / 8MB，防对端失控与解压炸弹 |
| 引擎入口分派 | `engine.rs::open_source` | `is_http_url` → `open_hls`，大小写不敏感 |
| 播放态标记 | `actor.rs:309` | path 前缀 http(s) → `AudioSource::Stream`；清空队列复归 File |
| 不可 seek | `decoder.rs` / `hls.rs::seekable` | 拖进度返回 `Unsupported("直播流不支持拖动进度")` |
| 频谱 | `engine.rs:889` | `tap.set_sample_rate(decoder.sample_rate())` 走同一条 WASAPI 路径，**流和文件一视同仁** |

单测覆盖解析矩阵 + 调度状态机（mock fetcher，不出网）：VOD 播完、live 追加、ENDLIST
转 VOD、断网放弃、断网恢复、live 跳缺段、VOD 缺段致命、跨段采样率变化、预取命中、
停滞判死、URI 复用 + MEDIA-SEQUENCE。这块质量是够的。

### 1.2 没有的（三处硬缺口）

**缺口一：只支持 ADTS 段，而真电台几乎全是 TS。**
`parse_media_playlist` 对 `.ts` 段直接 `Unsupported`，对 `EXT-X-MAP`（fMP4）也是。
实测抽样（radio-browser 按点击量排序的 hls=1 前 500 条里，64 条 URL 含 `.m3u8`，
从中抽 6 条解析 m3u8 正文）：

| 电台 | 段扩展名 | 判定 |
| --- | --- | --- |
| France Inter (radiofrance) | `.ts` | 拒绝 |
| BBC Radio 4 (akamaized) | `.ts` | 拒绝 |
| RTL (quortex → rtl.fr) | master → media → `.ts` | 拒绝 |
| Power POP (powerapp.com.tr) | `.ts` | 拒绝 |
| Ondacero (atres-live) | 嵌套 master | 需二跳以上 |
| Kral FM (radyotvonline) | 嵌套 master 带 session 参数 | 需二跳以上 |

6/6 都不走 ADTS。也就是说**现在这条能力在真实世界上没有可播放对象**，端到端验收一直
挂在「远程版 R2 FM soak / Liquidsoap 出真段」（我们自己出流才走 ADTS，见 docs/22 §3）。

**缺口二：icecast 直推流完全不认。**
目录里 287/500 条 URL 没有文件扩展名（`http://host:8000/stream`），127 条 `.mp3`、
22 条 `.aac`——codec 分布 MP3 352 / AAC 57 / AAC+ 47 / OGG 4 / UNKNOWN 40。
这些是 ICY 长连接（`Content-Type: audio/mpeg`，chunked，永不结束），不是 HLS。
现在 `open_source` 把所有 http(s) 都送 `open_hls`，结果是把 MP3 字节当 m3u8 文本
解析 → `playlist 里没有任何段`。**UI 上表现为「点了没反应/报格式错」。**

**缺口三：没有任何入口能把 URL 送进播放器。**
`player_play_tracks` / `player_play_query` 都按 `tracks.id` 查库（`fetch_track_refs`），
不存在「播这个地址」的命令。m3u 导入侧还**主动拒绝** HLS：`playlist_io` 见到
`#EXT-X-` 就返回 `ParseFailure::HlsManifest`（防的是把流清单当歌单导进来）。
`RadioView.svelte` 是 PLANNED 占位页（11 行，只有图标和「功能规划中」）。

### 1.3 一处不一致（审计发现）

- `engine.rs::is_http_url` 大小写不敏感（`HTTP://` 也算流），
  `actor.rs:309` 只认小写 `starts_with("http://")`。
- 后果：`HTTP://...` 路径引擎按流打开，播放态却标 `File` → 进度条可拖（一拖就报
  `Unsupported`）、时长位按 0 显示。属于「两处规则必须一致」的典型坑（hls.rs 注释里
  自己也写了两处必须一致）。修法：宿主侧抽一个共用的判定，或让 actor 也大小写不敏感。

### 1.4 UI 侧：`source` 字段零消费

`PlayerState.source` 在 wire 上（`ui/src/lib/ipc/generated/PlayerState.ts:33`），
**没有任何组件读它**。播放条现在的行为是纯副作用推出来的：

- `PlayerBar.svelte:260` `disabled={!hasTrack || durationMs === 0}` → 流自动禁拖（对）
- `PlayerBar.svelte:271` `formatTime(durationMs)` → 流显示 **`0:00`**（不对，应该「直播中」）

`actor.rs` 的注释、`hls.rs` 里 `duration_ms` 的注释（"UI 按 source=stream 显示直播中"）
都假设了这件事已经做了，实际没做。这是电台功能里最小、最该先补的一块。

---

## 2. 问题一：音频流的显示协议有没有通用约定

分三层看，答案是不同的。

### 2.1 曲目元数据（标题/副标题）

**(a) ICY —— 事实标准，但只有一行字符串。**
请求带 `Icy-MetaData: 1`，响应头给 `icy-metaint: N`；之后每 N 字节音频插一个
metadata block：1 字节长度 L（单位 16 字节），后跟 L×16 字节，内容形如
`StreamTitle='Artist - Title';StreamUrl='';`。空块 = L 为 0。

同一响应里还有电台自身信息（不是当前曲目）：`icy-name` / `icy-genre` / `icy-br` /
`icy-url` / `icy-description`。实测（`icecast.anon.fm/radio`）：

```
Content-Type: audio/mpeg
icy-br: 192
ice-audio-info: channels=2;samplerate=44100;bitrate=192
icy-description: Radio Anonymous (MP3)
icy-genre: trance,jazz,dnb,downtempo,experimental,psychedelic,rock,anime
icy-name: Anon.FM
icy-pub: 1
icy-url: https://anon.radio.fm
```

局限很硬：**一个字符串**。`Artist - Title` 的 `-` 是惯例不是规范，没有专辑、没有时长、
没有封面、没有时间戳。解析只能"按第一个 ` - ` 切，切不出来整段当标题"。

**(b) HLS timed metadata —— 规范完备，几乎没人用。**
Apple 规定的路径：TS 里挂独立的 ID3 PID（`TIT2`/`TPE1`/`TALB` + `PRIV` owner
`com.apple.streaming.transportStreamTimestamp`），或用 `#EXT-X-DATERANGE` 把
JSON/ID3 绑到时间区间；fMP4 里用 `emsg` box。Roku 另提过
`#EXT-X-IMAGE-STREAM-INF` 走一条独立的封面位图轨道。

实测：France Inter / BBC Radio 4 / RTL 的 playlist 里只有 `#EXT-X-PROGRAM-DATE-TIME`
和 `#EXT-X-START`，**一条 ID3 / DATERANGE 都没有**。广播台不发这个——它的成本在编码
链路，收益只在少数支持 timed metadata 的播放器。

**(c) 私有 JSON —— 现实里真正在用的。**
`/status-json.xsl`（Icecast 自带，很多台没关）实测可用：

```
{"icestats":{"source":[{"audio_info":"channels=2;samplerate=44100;bitrate=64",
 "genre":"trance,jazz,...","listenurl":"http://icecast.anon.fm:8000/radio-low.mp3",
 "server_name":"Anon.FM","server_type":"audio/mpeg", ... }]}}
```

有 `title` 字段时就是当前曲目。除此之外就是各家 JSON API（无标准）。

### 2.2 封面

**没有通用协议。** 曲目级封面在广播里不存在（电台不拥有曲目元数据的所有权）。
台标只有目录类数据源给：

- **Radio Browser**（社区目录，`de1.api.radio-browser.info`，免认证）：`name` /
  `url_resolved` / `favicon` / `tags` / `codec` / `bitrate` / `countrycode` /
  `stationuuid`。`favicon` 是台徽 URL，不是曲目封面。
- **RadioDNS / SPI**（Service Provider Information）：DNS 查 `_radiospi._tcp.<fqdn>`
  → HTTP 拉 XML，含 logo、名称、流派、多路流地址。欧洲广播联盟主推，**覆盖不到
  长尾电台**，且要按台域名逐个查，不是"一次查全部"。

结论：**"封面"这一栏的正确做法是拿台标当封面**，曲目级封面拿不到（TuneIn、
Apple Music 电台、foobar 的 radio 都是这么做的）。

### 2.3 落到我们的决策

| 信息 | 来源优先级 | 拿不到时 |
| --- | --- | --- |
| 标题（台名） | 用户录入 / 目录 API `name` | 必填，不允许空 |
| 副标题 | 用户录入的 tagline，或 `icy-genre` | 留空不占位 |
| 封面 | 台标：用户填 URL / 目录 `favicon` / 本地图片 → 下载进 artwork 缓存 | 用首字母/图标兜底（和专辑无封面同款） |
| 当前曲目 | ICY `StreamTitle`（有 ICY 支持之后）→ `/status-json.xsl` | 不显示，只显示「直播中」 |

**不做**：HLS timed metadata（规范完备但零覆盖）、RadioDNS SPI（要按台查 DNS，
长尾为零）。理由都是"投入产出比不成立"，不是"做不了"。

---

## 3. 问题二：电台功能怎么落地

### 3.1 表结构（自有表，不进曲库）

```sql
CREATE TABLE radio_stations (
  id          TEXT PRIMARY KEY,          -- st_<uuid>
  name        TEXT NOT NULL,
  subtitle    TEXT,                      -- 副标题/tagline（可空）
  stream_url  TEXT NOT NULL,             -- 直接喂给播放器的地址
  homepage    TEXT,                      -- 台站主页（可空）
  icon_url    TEXT,                      -- 台标来源（远程 URL，可空）
  artwork_id  TEXT REFERENCES artworks(id) ON DELETE SET NULL,  -- 落地后的台标
  codec       TEXT,                      -- 展示用：mp3/aac/aac+/hls
  bitrate     INTEGER,                   -- kbps，展示用
  tags        TEXT,                      -- 逗号分隔，检索/展示
  sort_order  INTEGER NOT NULL DEFAULT 0,
  created_at  INTEGER NOT NULL,
  last_played_at INTEGER,
  play_count  INTEGER NOT NULL DEFAULT 0
);
```

- **不进 `tracks`**：扫库不会碰它，`library_stats` 不含它，搜索/专辑/艺人/最近添加
  全都看不见。这是用户明确的"不进曲库"。
- **不要直接指向 `artworks`**：`artwork_id` 看着该指 artworks，但
  `artwork.rs::gc_unreferenced` 的对账口径是「tracks / albums / artists 三处
  `artwork_id` 任一命中才保留」，扫描收尾就跑一次。电台图标进了 artworks 而没被这
  三处引用 → **下一次扫描就被删掉**（行和 originals/ 文件一起）。要么把
  radio_stations 加进那个 `NOT EXISTS` 判定，要么台标单独存（`radio_icons/` 目录
  + 本地路径）。**建议后者**：改动面小，GC 逻辑一个字都不用动。
- `sort_order` 支撑卡片墙的手动排序（3D 切换要"下一张/上一张"，顺序必须稳定且用户可调）。

### 3.2 播放链路：三条外键是真正的坎

现在队列是这样持久化的：

```sql
CREATE TABLE queue_items (
  position INTEGER PRIMARY KEY,
  track_id TEXT NOT NULL REFERENCES tracks(id) ON DELETE CASCADE
);
```

`PRAGMA foreign_keys = ON`。电台 id 塞进去 → `FOREIGN KEY constraint failed` →
`save_queue_full` 整笔事务失败（上层只 `tracing::warn!`，静默）。同一问题在
`history.track_id` 上重演（`history::record` 插行 + 更新 tracks 统计，同事务）。

三条路线：

| 方案 | 做法 | 代价 |
| --- | --- | --- |
| **A. 队列多态**（推荐） | 迁移 0010 重建 `queue_items`：`kind TEXT NOT NULL DEFAULT 'track'` + `ref_id TEXT NOT NULL`，去掉对 tracks 的硬外键（改成应用层校验）；`history` 同样处理或直接跳过非本地曲目 | 一次表重建 + 持久化/恢复路径全过一遍；好处是"队列可以混曲目和电台"，next/prev/播放条/队列面板零分支 |
| B. 电台独立播放态 | actor 加 `Source::Station`，队列只存曲目 | actor 状态机分叉，next/prev/循环/随机/队列面板全要判断；省一次迁移 |
| C. 往 tracks 插影子行 | 最省事 | **违反"不进曲库"**，直接否 |

推荐 A。注意 `history` 更简单：**电台不该进播放历史**（历史列表是"曲目"语义，
`play_count` 也是曲目统计），所以 `record_history` 前面加一句 `if source == Stream
{ return }` 就够，不必改 history 表。

播放命令：新增 `player_play_station(stationId)`，直接用库里那行构造 `TrackRef`
（`id = st_...`、`path = stream_url`、`title = name`、`artist = subtitle`、
`duration_ms = None`、`artwork_id = 本地台标`），再 `QueueOp::ReplaceAll { tracks: vec![..] }`。
**不要复用 `fetch_track_refs`**——它查 tracks 表。

`TrackRef.id` 还有一个隐性消费者：UI 的"点歌手/专辑跳转"。电台的 `artist_id` /
`album_id` 一律 `None`，详情页那两行链接自然不渲染（需要确认 UI 的判断条件是
`artistId != null` 而不是无条件渲染）。

### 3.3 与播放条、频谱的交互

**播放条**（最小改动，三项）：

1. 显示「直播中」而不是 `0:00`：`durationMs` 为 null/0 且 `source === 'stream'` 时，
   时长位换成文字、进度条换成一条静态/微动条（不是 disabled 的滑块，disabled 滑块
   看着像"坏了"）。
2. 封面用台标（`current.artworkId` 已经能走 `aiya-artwork://`）。
3. 歌手位显示"副标题 或 当前曲目"，`artistId == null` 时不渲染成链接。

**频谱**：**零改动**。`open_source` 之后 `tap.set_sample_rate` 和 `push_mono` 走的是
同一条路径，HLS 段解出来的样本照常进分带。这点已在代码里成立，不需要为电台做任何事。

**暂停语义**：docs/46 那一轮定的"暂停收平"对流同样适用（tap 无输出 → 帧全 0）。
但**直播流没有"暂停后继续"的位置语义**：暂停 30 秒再播，回来是 30 秒之后的直播点
（HLS 会重新拉 playlist）。这不是 bug，但 UI 上"暂停一个电台"应该理解为"断开"，
需要产品上确认：暂停键对电台是暂停还是停止。

**电台页的 3D 卡片墙**：`RadioView.svelte`（现在 11 行占位：图标 + PLANNED + 一段说明）
整页替换。见 §4。

### 3.4 还需要动的点（容易漏）

- **CSP**：`img-src 'self' blob: data: aiya-artwork: http://aiya-artwork.localhost`——
  **不允许任意 https 图片**。所以台标不能直接在 `<img src="https://...">`，必须
  后端下载落地（或加一个 `station-icon` 代理）。同理 `connect-src` 不允许任意 https，
  UI 侧不能直接 fetch 目录 API → 目录检索也必须走后端命令。这是"看起来能过、实机
  白屏"的典型坑。
- **`is_http_url` 大小写不一致**（§1.3），顺手修。
- **IPC 类型生成**：新命令 + 新 DTO 走 `npm run gen:types`，`dto_wire` 加线格式快照。
- **mock**：`ui/src/lib/ipc/mock/backend.ts` 要加电台 CRUD + `player_play_station`，
  否则 dev/预览页全空；e2e 至少加"电台页能列出卡片、点播放后播放条显示直播中"。
- **迁移编号**：工作区里 0008/0009 是并行会话的 beets 改动（`0009_drop_track_enrichment`），
  电台的迁移必须是 **0010**，且要确认 0009 先落库（否则版本号撞车）。

### 3.5 分期建议

| 期 | 内容 | 为什么这个顺序 |
| --- | --- | --- |
| R0 | 修 `is_http_url` 不一致；UI 消费 `source`：播出「直播中」+ 静态条 | 半天量，且是后面所有验收的观测面（不然"能播了"看不出来） |
| R1 | `radio_stations` 表 + CRUD + 电台页列表（占位级排版，不做 3D）+ `player_play_station` + 队列持久化（方案 A 迁移） | 打通"能听"。此时只能听 ADTS HLS 台（几乎没有），所以 R2 是必需的 |
| R2 | **播放能力补齐**（决定电台功能有没有意义）：MPEG-TS 音频 demux 或 ICY 直推流，二选一或都做 | 见下 |
| R3 | 3D 卡片墙 + 台标落地 + ICY 当前曲目 | 体验层，可以慢慢磨 |

**R2 的两个选项，我倾向先做 ICY 直推流**：

- **MPEG-TS demux（覆盖 HLS 电台）**：Symphonia 0.5 没有 TS demuxer（docs/21 §4 已
  冻结在 0.5）。手写一个"只抽音频 PES"的最小 TS demuxer 是可行的：188 字节包 →
  PAT 找 PMT → 拿 AAC 的 PID → PES 头剥掉 → 得到 ADTS 字节 → 喂现有 `aac` 解码器。
  音频-only 的流没有 B 帧重排、没有多节目，量大约 300–500 行 + 单测（用 mock 字节
  拼 TS 包，不出网）。这是唯一能让 BBC / France Inter / RTL 这类主流 HLS 电台出声的路。
- **ICY 直推流（覆盖 70% 目录）**：需要一个流式 `MediaSource`（`is_seekable=false`，
  symphonia 的 mp3 demuxer 对不可 seek 源是支持的——`demuxer.rs:149` 有
  `if source.is_seekable()` 分支），加 ICY 头/元数据剥离（每 `metaint` 字节切一次）。
  覆盖广，但要自己管缓冲、断流重连、以及"元数据块必须从音频流里扣掉，否则解码器
  听到噪音"。

两者都做才是"能听绝大多数电台"。若只做一样，先做 ICY（覆盖面大一个数量级）；
若目标是"高音质欧美大台"，先做 TS。

---

## 4. 问题三：three.js 能不能做那个 GSAP 3D 卡片

### 4.1 先看清 demo 到底在做什么

`INDEX_基于GSAP的3D切换` 三个文件，机制完全是 **CSS 3D**，和 WebGL 无关：

- 三张卡常驻 DOM：`.current--card` / `.next--card` / `.previous--card`；
- 每张卡的位置由一个 CSS 变量组合算出：
  `transform: translate(-50%,-50%) translateX(var(--card-translateX-offset))
   translateY(var(--card-translateY-offset)) rotateY(var(--card-rotation-offset))
   scale(var(--card-scale-offset))`；
- current：`translateX 0 / rotateY 0 / scale 1.2`；previous：`-1.1×宽 / 25deg /
  0.9`；next：`+1.1×宽 / -25deg / 0.9`；容器 `perspective: 1000px`；
- 切换 = **换 class**，靠 `transition: transform 800ms` 自己补间；z-index 手动设
  保证层级正确；
- 鼠标倾角：`angle = atan2(e.pageX - centerX, 0) * (35/π)` → 写进
  `--current-card-rotation-offset`（GSAP 只负责 `gsap.set` 这个变量）；
- 文本：GSAP timeline 做 `stagger 0.1` 的 `translateY -120px / opacity 0` → 换 class
  → 反向入场；
- 背景：同一张图 180% 铺底、`blur(8px)`，用 `translateX ±25%` + opacity 做交叉淡入。

GSAP 在这里只干三件事：`gsap.set` 写 CSS 变量、timeline 串文本动画、首屏 stagger
入场。**没有一条是 GSAP 独有能力**——CSS transition + `transition:` 指令 + WAAPI 全能做。

### 4.2 three.js 能不能做

能，而且能做更多，但**不该为这个需求引入**：

| 维度 | CSS 3D（demo 的路子） | three.js |
| --- | --- | --- |
| 文字清晰度 | 浏览器排版，任意字号都锐 | 要么 3D 文字（贵），要么贴图烘焙（缩放糊） |
| 图片 | `<img>` 直接显示，自动解码/缓存 | 要 `TextureLoader` + 显存管理 + 释放 |
| 命中测试 | DOM 事件，天然 | `Raycaster` 手动做 |
| 可访问性/键盘/焦点 | 免费 | 全要手写 |
| 真 3D 效果 | 平面卡片 + 透视，够用 | 曲面 coverflow、景深、反射、与背景频谱同一个 bloom 后期 |
| 成本 | ~0（复用现有 CSS 变量与动效规范） | 第二个 WebGL 上下文 + 纹理/几何/后期一整套 |

关键约束：**项目已经有一个 WebGL 舞台**（`lib/render/` 七文件，背景频谱 + 山形 + 波纹，
docs/46）。加第二个 three.js canvas 意味着两个 GL 上下文抢 GPU、两套资源生命周期，
风扇和耗电都会体现在笔记本上。**要 three.js 的正确形态是把卡片并进现有舞台**（同一
renderer、同一 rAF、同一后期），而不是新开一个——那是"电台页做成 3D 场景"的大工程，
不是"做个卡片切换"。

### 4.3 推荐做法

**用 Svelte + CSS 变量重写 demo 的效果**，不引 GSAP、不引 three.js：

1. `RadioView.svelte` 里做 `stations: Station[]` + `activeIndex`，派生
   `prev/current/next` 三条（数组环回）。
2. 每张卡一个 `--offset` 变量（`-1 / 0 / 1`），CSS 用 `calc(var(--offset) * ...)` 算
   `translateX / rotateY / scale`——比 demo 的「三个 class 互相换」更直白，也不用
   手设 z-index。
3. 切换用 `transition: transform 640ms cubic-bezier(.22,.61,.36,1)`；文本用
   Svelte 的 `{#key activeIndex}` + `transition:fade/fly` 做 stagger（`delay` 按行号）。
4. 鼠标倾角沿用同一个公式（`atan2(dx,0) * 35/π`），只在 `pointermove` 里写一个变量，
   **不要引入独立的 rAF**；如果要平滑，就复用 `lib/render/scheduler.ts` 那个循环。
5. 背景大图交叉淡入用两个 `<div>` + opacity 过渡，`blur(8px)` 直接照搬——但注意
   `filter: blur` 在这种大尺寸下是 GPU 开销大户，要和背景频谱错开（频谱已经占了 GPU）。

顺手能拿到的两个提升（CSS 方案白送，three.js 要额外做）：台标 `object-fit: cover`
裁切、无台标时用首字母 + 渐变底色兜底（和专辑卡同款）。

**什么时候才值得动 three.js**：如果最终想要的不是"卡片切换"，而是"卡片漂浮在频谱
场景里、随低音震动、带 bloom"——那属于 docs/46 舞台的一部分，应该作为
`lib/render/layers/` 里的一个新层来做（和 ridge/ripple 并列），而不是电台页自己的画布。

---

## 5. 未决 / 需要拍板

1. **R2 先做哪个**：ICY 直推流（覆盖 70%，但自己管缓冲与重连）vs MPEG-TS demux
   （覆盖欧美大台，约 300–500 行手写 demuxer）。不补 R2 的话，R1 做完也只有极少数
   ADTS 台能播。
2. **队列是否多态**（方案 A：迁移 0010 重建 `queue_items`）还是电台走独立播放态（方案 B）。
   A 更干净，B 不动表。
3. **暂停键对电台的语义**：暂停（断开还是保持缓冲）还是停止。
4. **电台台标存哪**：独立 `radio_icons/` 目录（推荐，不碰 GC）还是进 `artworks`（要给
   `scanner.rs` 的孤儿清理加引用判定）。
5. **目录 API 要不要接**（Radio Browser）：接的话"加电台"可以是搜索式，不接就是纯手工
   填 URL。接的成本主要在 CSP + 后端代理 + 字段映射。
