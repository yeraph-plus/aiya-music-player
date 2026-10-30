# 直播流（HLS / MPEG-TS）支持与「直播」页 — 调研与实施计划（2026-10-30）

> **本轮范围（按用户 2026-10-30 口径重写）**
>
> - **不做 ICY / icecast 接入**。上一版那一整套（`Icy-MetaData`、`StreamTitle`、
>   `status-json.xsl`、Radio Browser 目录、RadioDNS）全部作废，不再作为方案存在。
> - 只补 **m3u8 → MPEG-TS** 这条主流链路。
> - 目标不是「网络电台功能」，而是 **音频直播推流客户端**：给一个 HLS 地址或文件，
>   能打开、能播、能看到直播态。**不做目录、不做台站收藏管理、不做元数据协议协商。**
> - 页面 `radio` 改名 **`live`（直播）**，避免"电台"被理解成"要有个电台库"。
> - 打开入口不是页面里的地址框，而是 **整个应用壳支持拖入文件 → 识别 → 进播放列表**。
> - m3u8 / 拖入的音频 **可以播，但不进曲库**。

---

## 0. 一页结论

| 问题 | 结论 |
| --- | --- |
| HLS 现在能播吗 | 代码完整，但只认 **ADTS-AAC 段**；实测 6 个真实 m3u8 电台 **6/6 是 `.ts` 段**，全被显式拒绝 → **在真实世界上无可播对象** |
| 补 TS 要写完整 demuxer 吗 | **不用**。HLS 的 TS 段里 AAC 是 ADTS 封在 PES 里，剥掉 TS 包层和 PES 头，剩下的字节直接喂现有 `probe(hint=aac)`。**唯一施工点：`hls.rs::open_segment`** |
| Opus 要不要现在做 | **不做**。symphonia 0.5.5 和 0.6.1 都**没有** opus 解码器（`all-codecs` 里没有）；能用的第三方要么拖 C 工具链、要么绕开 symphonia 的解码器抽象。零使用场景，记账即可 |
| 拖入能播但不进库，最大障碍 | **不是播放器，是队列**。`queue_items.track_id` 是 `NOT NULL REFERENCES tracks(id)` 且 `PRAGMA foreign_keys = ON`，非库条目一进队列就 FK 失败，写库事务静默回滚 → 必须迁移 0010 |
| 播放条要动多少 | 三处：时长位（`0:00` → 直播态）、进度条（禁用滑块 → 直播条）、标题/封面来源。`PlayerState.source` 已在 wire 上，**但 UI 零消费** |

---

## 1. 现状（用来定施工点）

### 1.1 已经有的

`crates/aiya-audio/src/hls.rs` 是一份完整的 HLS 客户端，缺的只有段格式：

- 解析 `EXTINF` / `TARGETDURATION` / `MEDIA-SEQUENCE` / `ENDLIST` / `KEY(METHOD=NONE)`；
  master playlist 只跟第一个变体（`extract_master_variant`）。
- live 轮询按 `MEDIA-SEQUENCE` 判新旧（**不能只比 URI**——服务端会滚动复用 URI）。
- 段预取线程（`Prefetch`，单槽 lookahead）——独占模式设备缓冲只有 10–50ms，
  段边界同步拉必断音。
- 断网退避 8s 墙钟预算、3×TARGETDURATION 无新段判死、2MB/8MB 字节上限。
- `position_ms()` 是**累加已播帧数**（`frames_played * 1000 / sample_rate`），
  **跨段不回跳**——直播"已收听时长"可以直接用。
- 引擎入口 `engine.rs::open_source`：http(s) → `open_hls`，否则本地文件。
- `actor.rs:309` 按 path 前缀 → `AudioSource::Stream`，清空队列复归 `File`。
- `seekable() = false`，`seek_ms` 报「直播流不支持拖动进度」。
- 频谱零改动即可用：`tap.set_sample_rate(decoder.sample_rate())` 与 `push_mono`
  走的就是同一条 WASAPI 路径。

### 1.2 缺口

**缺口一：`parse_media_playlist` 见 `.ts` 直接拒。**

```
"MPEG-TS 段不支持：本播放器只解 ADTS AAC 段（.aac / 无扩展名）"
```

**顺带一个真 bug**：判定是 `line.to_ascii_lowercase().ends_with(".ts")`。
带查询串的段名（实测 Power POP：`l_53110566_..._6570.ts?nimblesessionid=1129737273`）
**不匹配**，于是被当 ADTS 段收下来，到 probe 才炸，报的是「HLS 段解不开」——
用户看到的是"文件坏了"，不是"格式不支持"。修法见 §3.2。

**缺口二：本地 `.m3u8` 打不开。** `open_source` 只按 http 前缀分流，本地 m3u8 走
`open_file` → symphonia probe 一个文本文件 → `Decode`（"文件坏了"）。

**缺口三：没有任何入口。** `player_play_tracks` / `player_play_query` 都按 `tracks.id`
查库（`fetch_track_refs`）；m3u 导入侧还主动拒绝 HLS（`playlist_io` 见 `#EXT-X-` 就
返回 `ParseFailure::HlsManifest`，防的是把流清单当歌单导进来）。
`RadioView.svelte` 是 11 行 PLANNED 占位。UI 里**没有任何拖放实现**（`ondrop` /
`onDragDropEvent` / `dragover` 全库 0 命中）。

**缺口四：播放条不认流。**
`PlayerBar.svelte:260` `disabled={!hasTrack || durationMs === 0}`（流确实禁拖，对），
`:271` `formatTime(durationMs)` → 时长位显示 **`0:00`**（错，应该「直播」）。
根因：`PlayerState.source` 在 `ui/src/lib/ipc/generated/PlayerState.ts:33` 上，
但**没有任何组件读它**。

**一处不一致**：`engine.rs::is_http_url` 大小写不敏感，`actor.rs:309` 只认小写
`starts_with("http://")`。`HTTP://` 路径会被引擎当流打开、播放态却标 `File`
（进度条可拖，一拖报错）。顺手修。

---

## 2. 能力目标（明确画线）

**能打开**：m3u8 的 http(s) URL；本地 `.m3u8` 文件；拖入的音频文件（不进库）。

**能播放**：HLS 里的 `ADTS AAC` 段（已有）+ `MPEG-TS` 段（本轮新增，AAC / MP3 两种 ES）。

**明确拒绝，且报人话**：

| 情况 | 报什么 | 为什么不做 |
| --- | --- | --- |
| fMP4 段 / `EXT-X-MAP` | 「fMP4 段不支持（HLS 走 TS/AAC 路线）」 | symphonia 0.5 无 fMP4 demuxer，0.6 冻结（docs/21 §4） |
| AES-128 / SAMPLE-AES | 已有报错 | 解密 + 密钥轮换，与"能听直播"无关 |
| LATM/LOAS（`stream_type 0x11`） | 「LATM 封装的 AAC 不支持」 | 极少数广播端，剥 ADTS 那套用不上 |
| 多码率 master playlist | 取第一个变体，日志记一行 | 手动选集留 v2（RTL 第一个变体就是 64k，够听） |
| LL-HLS（`EXT-X-PART`） | 不识别，忽略 | 低延迟是另一个坑（partial segment + blocking reload） |

**不做**：当前曲目元数据（ICY 不做，HLS 里本来就没有）、台标、目录检索、台站分类。
直播页显示的就是「流标题 + 编码/码率 + 已收听时长」。

---

## 3. TS 支持：为什么只是"剥离"，以及怎么剥

### 3.1 关键洞察

HLS 规范要求 TS 里的 AAC 用 **ADTS** 封装。也就是说：

```
TS 段（188 字节包） → PMT 找到 AAC 的 ES PID → 拼 payload → 去掉 PES 头 → ADTS 裸流
```

而 `hls.rs::open_segment` 现在做的事就是「把一段字节丢给 `probe(hint=aac)`」：

```rust
let bytes = /* 预取命中 or 同步拉 */;
let mut hint = symphonia::core::probe::Hint::new();
hint.with_extension("aac");
let stream = DecodedStream::open_with_backend(
    Box::new(Cursor::new(bytes)), &hint, DecodeBackend::Symphonia)?;
```

**新增的只有一步：`bytes` 进 `Cursor` 之前，如果是 TS 就先剥成 ES 字节。**
不需要写 demuxer、不需要接 symphonia、不需要碰 `DecodedStream` / engine / WASAPI /
频谱。这是最省事的一条路，也是唯一一条不用升级 symphonia 的路。

`stream_type` 对照：`0x0F` = ADTS AAC（hint `aac`）；`0x03`/`0x04` = MPEG-1/2 audio
（hint `mp3`，`mp3` feature 已开）；`0x11` = LATM（拒绝）。

### 3.2 新文件 `crates/aiya-audio/src/ts.rs`

```rust
/// TS 包剥离：段字节 → 目标 ES 的裸字节 + 编码种类。
pub enum TsAudio { Adts, MpegAudio }

pub struct TsState {
    /// PMT 里的目标 ES PID。有的分段器只在首段发 PAT/PMT，后续段只有 PES
    /// —— 所以这个状态必须挂在 HlsStream 上跨段存活。
    audio_pid: Option<u16>,
    /// PMT 的 PID（从 PAT 来），同样跨段缓存
    pmt_pid: Option<u16>,
}

pub fn is_ts(bytes: &[u8]) -> bool;         // 首字节 0x47 且 len % 188 == 0
pub fn strip(bytes: &[u8], st: &mut TsState) -> Result<(Vec<u8>, TsAudio), AudioError>;
```

实现要点（音频-only TS 的简化，不处理视频 / 多节目 / B 帧重排）：

1. **包步进**：`188` 字节一包，`data[0] == 0x47` 同步字；`transport_error_indicator`
   置位的包跳过。
2. **PID / 载荷**：`pid = ((data[1] & 0x1F) << 8) | data[2]`；
   `afc = (data[3] >> 4) & 0b11`——`0b00` 是保留值跳过、`0b10` 只有 adaptation field
   无载荷、`0b11` 先跳 `data[4] + 1` 字节再取载荷。
3. **PAT（PID 0）**：跳 `pointer_field` 后是 section，跳过 8 字节表头，按 4 字节一条
   取 `program_number != 0` 的 PMT PID。
4. **PMT**：`program_info_length` 之后是 ES 循环，每条
   `stream_type(1) + ES_PID(2) + ES_info_length(2)`，挑第一个
   `stream_type ∈ {0x0F, 0x03, 0x04}`。
5. **PES**：`payload_unit_start_indicator == 1` 时载荷以 `00 00 01 <stream_id>` 开头，
   `PES_header_data_length` 在偏移 8，音频从 `9 + header_len` 开始。
6. **跨包拼接**：一个 PES 会散在多个 TS 包里，直接把目标 PID 的载荷 append 起来；
   段内通常有多个 PES，拼完就是连续 ADTS。
7. **无 PAT/PMT 的段**：用 `TsState` 缓存；缓存也没有 →
   `Unsupported("这段没有 PAT/PMT，无法定位音频流")`。

**判定用内容嗅探，不用扩展名**——带 query 的段名（§1.2 那个 bug）用扩展名判会漏。
嗅探之后同时删掉 `parse_media_playlist` 里那条 `.ts` 提前拒绝，改成"收下来，
段打开时再判"，报错时机才和真实原因对齐。

### 3.3 施工点清单（一处主改 + 四处配套）

| 位置 | 改动 |
| --- | --- |
| `hls.rs::open_segment` | `bytes` → `if ts::is_ts(&bytes) { ts::strip(&bytes, &mut self.ts_state)? }`；hint 按 `TsAudio` 选 `aac`/`mp3` |
| `HlsStream` 结构 | 加 `ts_state: TsState` 字段（跨段存活） |
| `parse_media_playlist` | 删掉 206 行那段 `.ts` 提前拒绝 |
| `fetch_with_retry` / `Prefetch` | 无改动（拿的还是原始段字节，剥离在打开时做） |
| `lib.rs`（crate 根） | 加 `pub mod ts;` |

跨段参数校验（现在比对 `sample_rate`/`channels`）继续有效——每段独立 probe，
TS 剥出来的采样率若与上一段不一致，现有那条「流参数中途变化」会接住。**现成福利。**

### 3.4 验证

**不依赖网络、不依赖 ffmpeg**（本机 `which ffmpeg` 为空，fixture 生成不能写成
"跑个 ffmpeg"）。单测里自带一个**最小 TS 封装器**（约 60 行）：把已知 ADTS 字节
切成 PES、包成 188 字节 TS、带 PAT/PMT。于是可以写**往返测试**：

```
adts_bytes → mux_ts → strip → 必须逐字节等于 adts_bytes
```

覆盖矩阵：

- 正常段（PAT + PMT + 多 PES）；
- 段内无 PAT/PMT（用上一段缓存的 PID）；
- `adaptation_field_control` 三种取值（含 `0b11` 带 adaptation field）；
- 一个 PES 跨 3 个 TS 包；
- `stream_type 0x11` → 报 LATM 不支持；
- `stream_type 0x03` → 走 mp3 hint；
- 长度不是 188 整数倍 / 中途出现不同步 → 报坏段而不是 panic；
- 带 query 的 `.ts` 段名（回归 §1.2 那个 bug）。

**人工冒烟**（不进 CI）：France Inter / BBC Radio 4 的公开 m3u8 各跑 30 秒，
确认出声、频谱在动、`position_ms` 单调。

### 3.5 已知风险

1. **段边界丢帧**：若一个 PES 跨段（理论上存在），后段开头那些包没有 PES 起点，
   会被丢掉。HLS 要求段从帧边界开始，实践上音频段对齐；表现为极轻的咔哒，只能
   真机听。**兜底**：段首包 `payload_unit_start == 0` 且 continuity counter 不接上
   前段末值时，说明确实跨段 → 丢到下一个 PES 起点。
2. **`EXT-X-DISCONTINUITY`**：编码器重启后时间戳归零，但我们的 `position_ms` 是
   累加帧数、不读时间戳，所以**不会回跳**；采样率不变就能无感接上。采样率变了 →
   现有「流参数中途变化」报错断流。这个行为可接受（真变了确实只能断）。
3. **192 字节 M2TS**（蓝光包格式）：HLS 不出现，`is_ts` 会判否 → 落回 ADTS 路径报错。
   不在范围内，但错误信息别误导。

---

## 4. 本地 m3u8：`FileFetcher`（顺手把端到端验收从网络上摘下来）

`HlsStream` 的取字节口是 trait（`Fetcher`），现在有 `UreqFetcher` 和测试用的
`MockFetcher`。加第三个 `FileFetcher`，配 `HlsStream::new_from_local(path)`：
base = m3u8 所在目录（`Url::from_file_path` 拿 `file:` base），相对段地址照
`base.join()` 走同一套逻辑。`Fetcher` 按 scheme 分派（`file:` 读盘、`http(s):` 走
ureq），**一个 `HybridFetcher` 就够，不需要两套 HlsStream**。

价值不只是"支持打开本地文件"：

- **端到端验收不再挂在网络和远程版 soak 上**。hls.rs 头注释现在写着"端到端验收随
  远程版 R2 FM soak（Liquidsoap 出真段）"——有了本地 m3u8 + 本地 TS 段，可以进
  `verify`：拖一个 m3u8 进去 → 出声 → 位置单调 → 暂停收平。
- 单测也能覆盖"本地 m3u8 + 相对路径 + 跨段状态"，不用 fake 网络。

`open_source` 分流要加一条：`.m3u8` / `.m3u` 扩展名 → `HlsStream::new_from_local`，
**在 `open_file` 之前判**（否则就是"文件坏了"）。

---

## 5. 拖放入口（壳级，不在直播页里）

### 5.1 用 Tauri 事件，不用 HTML5 dnd

Tauri v2 窗口默认 `dragDropEnabled: true`（`tauri.conf.json` 里没显式配，取默认），
此时**文件拖放由 Tauri 接管**，HTML5 的 `ondrop` 拿不到文件。两个世界只能选一个，选 Tauri：

```ts
// App.svelte onMount（或新 lib/drop.ts）
import { getCurrentWebview } from '@tauri-apps/api/webview';
const un = await getCurrentWebview().onDragDropEvent((e) => {
  if (e.payload.type === 'over') dropOver = true;
  else if (e.payload.type === 'drop') handlePaths(e.payload.paths);
  else dropOver = false;
});
```

overlay 用现有 `OverlayHost` 的层级画一圈"松手即播"提示即可，不需要新浮层体系。
Windows 上事件里只有 `paths` 数组，修饰键得自己用 `keydown` 记录。

### 5.2 分类器只有一处（Rust 侧）

**不要在 TS 里写扩展名 if-else**——UI 和后端各写一份必然漂移（这个仓已经吃过
`is_http_url` 大小写不一致的亏）。放一条命令，拖放和文件对话框共用：

```rust
#[tauri::command] fn classify_paths(paths: Vec<String>) -> Vec<DroppedItem>;

pub enum DroppedKind { Audio, HlsPlaylist, Playlist, Directory, Unsupported }
```

- `Audio`：扩展名在 `decoder` 的格式矩阵里——**这个表本身要抽出来做唯一源**，
  现在散在 `known_unsupported_ext` 和 symphonia features 两处；
- `HlsPlaylist`：`.m3u8`；
- `Playlist`：`.m3u`（非 HLS）→ 解析成条目再逐个分类；
- `Directory`：v1 只提示「目录请走曲库导入」，不自动扫描（扫描是有副作用的重操作，
  不该由一次误拖触发）；
- `Unsupported`：走 `known_unsupported_ext` 的人话名字（WMA / APE / WavPack / DSD / Opus）。

### 5.3 队列语义

- **拖入 = 替换队列 + 播放第一项**（"识别 → 进入播放列表"就是这句话）。
- **追加**留 v2（Shift 拖入）：`QueueOp::Append` 已有，缺的只是修饰键。
- 多个文件按文件名自然序排（同一目录的曲目顺序）。
- 拖入后**不进曲库**：不写 `tracks`、不写 `artworks`、不触发扫描。

---

## 6. 队列多态（迁移 0010）——「能播但不进库」的核心

### 6.1 现在为什么不行

```sql
CREATE TABLE queue_items (
  position INTEGER PRIMARY KEY,
  track_id TEXT NOT NULL REFERENCES tracks(id) ON DELETE CASCADE
);
```

`PRAGMA foreign_keys = ON`（`0001_init.sql:12`）。非库条目一进队列：`INSERT` 违反
外键 → `save_queue_full` 的 `IMMEDIATE` 事务整笔失败 → 上层只有
`tracing::warn!("queue full persist failed")`，**UI 无感**，重启后队列是上一次成功
保存的样子。

同类问题在 `history.track_id`（`NOT NULL REFERENCES tracks(id)`）：
`history::record` 插行 + 更新 `tracks.play_count` 同事务，非库条目同样失败。

### 6.2 表结构

```sql
-- 0010_queue_transient.sql
CREATE TABLE queue_items_new (
  position INTEGER PRIMARY KEY,
  kind     TEXT NOT NULL DEFAULT 'track',                  -- 'track' | 'file' | 'stream'
  track_id TEXT REFERENCES tracks(id) ON DELETE CASCADE,    -- 允许 NULL，外键保留
  payload  TEXT,                                            -- transient：{"path","title","artist"}
  CHECK ((kind = 'track') = (track_id IS NOT NULL))
);
INSERT INTO queue_items_new (position, kind, track_id)
  SELECT position, 'track', track_id FROM queue_items;
DROP TABLE queue_items;
ALTER TABLE queue_items_new RENAME TO queue_items;
```

**为什么保留外键、只放 NULL**：`track_id` 为 NULL 时不校验外键（SQLite 语义），
`ON DELETE CASCADE` 只对命中的行生效——曲目被扫描淘汰，库内行照样自动清掉，
transient 行不受影响。比"整条外键拿掉、改应用层校验"少一处可能忘记校验的地方。

`payload` 只存 `TrackRef` 的最小显示字段（path / title / artist / durationMs）。
`artwork_id` 一律 NULL（不进曲库就没有 artwork 生命周期问题）。

### 6.3 要一起改的六处（漏一处就是"重启后队列少一项"）

| 位置 | 改法 |
| --- | --- |
| `aiya_data::save_queue_full` | 入参 `&[String]` → `&[QueueItem]`；transient 行写 kind/payload |
| `aiya_data::load_queue` | 返回 `Vec<QueueItem>` 而不是 `Vec<String>` |
| `lib.rs:4109` 启动恢复 | `fetch_track_refs(&ids)` → `build_refs(&items)`：track 查库，transient 由 payload 直接构造 `TrackRef` |
| `lib.rs::fetch_track_refs` | 拆成 `build_refs`（多态）+ 保留原查询（库内） |
| `PlayerActor::QueueSnapshot` | 队列面板要能显示 transient 行的标题与来源标记 |
| `lib.rs::record_history` | 前置闸门：只有库内曲目才记；**不改 history 表**（历史是"曲目"语义） |

`queue_state.position_ms` 对 stream 无意义：恢复时若当前项是 stream，`start_ms`
强制 0（直播没有"上次听到哪"）。

### 6.4 迁移编号

工作区的 `0008_track_enrichment` / `0009_drop_track_enrichment` 是并行会话的 beets
改动。直播的迁移必须是 **0010**，且要等 0009 落库再提升版本号，否则撞号。

---

## 7. 播放条与「直播」页

### 7.1 播放条：状态矩阵

`PlayerState` 已有 `source: AudioSource`（`File` / `Stream`），只是没人读。
按 `kind × source` 三态处理：

| 当前项 | 时长位 | 进度条 | 封面 | 副标题 | 可 seek |
| --- | --- | --- | --- | --- | --- |
| 库内曲目 | `3:42` | 正常可拖 | `aiya-artwork://` | 艺人（可点） | 是 |
| 拖入的本地文件 | 正常时长 | 正常可拖 | 图标兜底 | 所在目录 | 是 |
| 直播流 | **`直播`** + 已收听 `12:34` | **静态/呼吸条**（不可拖） | 图标兜底 | 编码 + 码率 | 否 |

落点：

- `PlayerBar.svelte:271`：`source === 'stream'` 时换成 `LIVE` 徽章 + 已收听时长
  （`positionMs` 单调，直接格式化）；`durationMs == null` 时不要再走 `formatTime(0)`。
- `:260`：`disabled` 逻辑保留（真禁拖），但滑块换成 `.live-bar`——一条有呼吸感的
  横线，用 CSS 动画即可，**不要另起 rAF**（守 docs/46 §7 单循环）。
- `:103` `durationMs ?? 0` 把"未知时长"和"0 秒"混成一件事，改成三态。
- 标题：stream 用用户给的流标题（§7.3），不是文件名。

### 7.2 队列面板

transient 行要能显示标题 + 来源标记（`文件` / `直播`），右键"从队列移除"；
不能有"跳到专辑/艺人"（`artist_id`/`album_id` 为 null——**要实测确认现有 UI 是判空
而不是无条件渲染链接**）。

### 7.3 「直播」页（`RadioView.svelte` → `LiveView.svelte`）

只做三件事，不做台站库：

1. **打开一个流**：地址输入框（粘贴 m3u8 URL）+「打开文件…」按钮（复用 `pick_folder`
   的线程模式加一个 `pick_files`，filter 只要 `.m3u8`）；标题默认取 URL 末段文件名
   或主机名，可手改。
2. **最近打开**：上限 20 条，按 URL 去重，存 settings KV（`live.recent`，
   `DEFAULT_SETTINGS` 加默认 `[]`）——**为 20 条记录建表 + CRUD 命令 + IPC 类型不划算**。
3. **正在播放的大画面**：标题 + 编码/码率 + 已收听时长 + 频谱（复用
   `NowPlayingSpectrum` 的形态，或直接用背景舞台）。**不做台标**（没有台站元数据）。

**卡片切换**（沿用上一轮结论）：那个 GSAP demo 是纯 CSS 3D（`perspective` +
`rotateY` + `translateX` + `scale` + class 交换 + CSS transition），用 Svelte + CSS
变量重写即可；**不引 GSAP**（自带 ticker，违反单 rAF 约定），**不新开 three.js
canvas**（第二个 GL 上下文会抢现有舞台的 GPU）。若最终想要"卡片漂在频谱里随低音
震"，那是 `lib/render/layers/` 里的新层，不是直播页自己的画布。

### 7.4 路由改名

- `AppPage`：`'radio'` → `'live'`；`Sidebar` 标签「电台」→「直播」，图标沿用 `Radio`。
- `restore()` 的退役表加一条 `radio → live`（已有 `tools → settings` 的先例），
  旧会话不炸。
- zod `page` 白名单同步、三处 `page === 'radio'` 诊断刷新、e2e 侧栏用例改文本、
  `App.svelte` 的 `import RadioView` → `LiveView`。

---

## 8. Opus：这轮不做（记录在案，别在实现时"顺手"）

**事实**：

- symphonia **0.5.5 和 0.6.1 都没有 opus 解码器**。0.6.1 的 `all-codecs` 是
  `aac / adpcm / alac / flac / mp1 / mp2 / mp3 / pcm / vorbis`。
- `symphonia-core` 有 `CODEC_TYPE_OPUS = 0x1005`，所以 Ogg(Opus) 能 probe 成功、
  `make()` 失败 → 落进既有的 `CodecUnsupported { codec: "Opus" }` 分类；而
  `.opus` 扩展名更早一步被 `known_unsupported_ext` 拦下（测试
  `known_unsupported_extensions_are_classified_before_probe` 断言
  `e.opus → "Opus"`）。
- **没有使用场景**：直播链路上 TS 里的 Opus 是提案级、几乎不存在；本地 `.opus`
  用户也没有。用户原话："对当前应用来说没有使用场景，只是格式支持。"

**三条路，成本差一个数量级**：

| 路 | 做法 | 代价 |
| --- | --- | --- |
| A. 不做（推荐） | 维持"Opus 不支持"的人话报错 | 0 |
| B. symphonia 插件路 | `moosicbox_opus`（自称 symphonia 的 Opus codec 实现）+ `CodecRegistry::register_all`。symphonia 0.5.5 的 `default::register_enabled_codecs` 是 pub，自建 registry 可行 | 它依赖 `audiopus ^0.2` = libopus **C 库**，拖 C 工具链进构建 + 二进制变大（BSD 许可要补署名） |
| C. 纯 Rust | `opus-decoder`（Rusopus，只依赖 thiserror）自己解包 | 要绕开 `DecodedStream` 现在"解码器必须是 symphonia `Decoder`"的抽象（在 `Inner` 加非 symphonia 变体）；Ogg 分页还要自己接 |

**结论**：A。真要补，等 B 的社区方案稳定（`moosicbox_opus` 0.5.0，2026-10 还在更新），
且先确认它的 `Decoder` 实现能吃我们的 `CodecParameters`——Opus 要 pre-skip 和声道
映射，很容易在"能解但声音不对"上翻车。**不要因为这轮在做 TS 就顺手把 Opus 带进来。**

---

## 9. 分期与验收

| 期 | 内容 | 验收（可自动化部分） |
| --- | --- | --- |
| **R0** 半天 | `is_http_url` 一致性；UI 消费 `source`；`.ts` 判定去误判 | 单测：`HTTP://` 标 Stream；e2e：mock 出 stream 状态，播放条出现 `直播` 且滑块不可拖 |
| **R1** 1–2 天 | `ts.rs` 剥离层 + `open_segment` 接缝 + `TsState` 跨段 | 单测往返（adts→ts→strip 逐字节相等）+ §3.4 覆盖矩阵；`cargo test -p aiya-audio` 全绿 |
| **R2** 1 天 | `FileFetcher` + `new_from_local` + `open_source` 认 `.m3u8` | **进 `verify`**：临时目录放 m3u8 + 手工 mux 的 TS 段 → 拖入 → 出声 → 位置单调 → 暂停两帧字节差 0 |
| **R3** 1–2 天 | 迁移 0010 队列多态 + `build_refs` + history 闸门 + 队列面板 | 单测：transient 行进队列 → 落库 → 重载 → 内容一致；曲目被删后 transient 行仍在 |
| **R4** 1 天 | 壳级拖放 + `classify_paths` + overlay + "替换队列并播放" | e2e：mock 拖放 → 队列替换 → 标题变化；`Directory` 只提示不扫描 |
| **R5** 1 天 | `radio → live` 改名 + `LiveView`（地址框 / 打开文件 / 最近 20 条 / 大画面） | e2e：侧栏文本、路由白名单、退役映射；最近列表去重与上限 |
| **R6** | 人工冒烟：France Inter / BBC Radio 4 / RTL 各听 2 分钟 | 出声、频谱在动、`position_ms` 单调、断网 8s 内报错不死锁 |

**人工冒烟不能省**：TS 剥离的正确性单测能锁，但"段边界有没有咔哒""8s 断流预算在
真实网络下够不够"只有真流能验。这也正是当初"端到端验收挂在远程 soak"一直没做的
原因，R2 之后这类验证终于有了本地替代品。

---

## 10. 已拍板（2026-10-31，用户）

上一轮列的五条未决项，用户逐条给了口径。**这一节是约束，实现时不要再"顺手"扩展**：

| # | 问题 | 拍板 | 落地位置 |
| --- | --- | --- | --- |
| 1 | 拖入目录怎么办 | **不识别**。只提示"文件夹不展开，走设置 → 曲库导入"，不做递归展开 | `drop.rs::classify` 判 `Directory`；`expand_dropped` 写进 `skipped` |
| 2 | 拖入 = 替换还是追加 | **替换并播放第一项**（维持本文档原方案，Shift 追加留 v2） | `player_open_paths` → `QueueOp::ReplaceAll { start: 0 }` |
| 3 | master playlist 多码率 | **给什么码率就播什么码率**，永远取第一个变体，**不做码率选择 UI** | `hls.rs::parse_master` 取首个 `EXT-X-STREAM-INF` |
| 4 | 提示长在哪 | **全部走播放器的吐司位**（`Toast`，`bottom: --player-h + 16px`），不新造浮层 | `notify.showToast` / `showError` |
| 5 | 直播的暂停语义 | **暂停 = 断开连接，播放 = 重连**。不做"暂停保持连接"，不做"断点续听" | `actor.rs` 的 `Stream` 分支 + `disconnecting` 标志 |
| 6 | 已收听时长要不要显示 | **显示**（`positionMs` 单调，暂停归零） | `PlayerBar` 左时长位 / `LiveView` 大画面 |

第 3 条的理由值得记一笔：它是**播放器本体的行为**，不是直播页的功能。码率切换要
重新起播、要处理不连续时间轴，收益只是"用户手动选一档"——而 m3u8 的变体顺序本身
就是服务端给的"推荐档位"。

第 5 条的连带 bug：引擎对 `Stop` 的回报是 `Eof`，而 `Eof` 的默认语义是"曲终，推进
队列"。所以断开直播必须带一个 `disconnecting` 标志把它吃掉，否则**暂停一次会跳到
队列下一首**（真机才会暴露，mock 状态机不经过引擎）。

---

## 11. 实施记录（2026-10-31，R0–R5 全部落地）

### 11.1 各期状态与证据

| 期 | 状态 | 证据 |
| --- | --- | --- |
| R0 | 完成 | `cargo test -p aiya-player` **50 passed**（含 `uppercase_scheme_is_still_a_stream`、`pausing_a_stream_disconnects`、`eof_from_disconnect_does_not_advance_queue`、`resuming_a_stream_reconnects`）；`ui/e2e/live.spec.ts` 4 条全绿 |
| R1 | 完成 | `cargo test -p aiya-audio` **84 passed**，其中 `ts.rs` 9 条（往返逐字节、无 PAT/PMT 复用、LATM 具名报错、PES 截断、失同步包序号、adaptation field 跳过、`is_ts` 拒绝 ADTS） |
| R2 | 完成 | `HybridFetcher`（http(s) + `file:`）；`open_local_hls` / `open_source` 分流；本地 m3u8 两条测试（读盘出声路径、缺段报段名） |
| R3 | 完成 | `cargo test -p aiya-data queue` **8 passed**（transient 落库往返、CASCADE 保留 transient、CHECK 拒收脏行）；`QueueSnapshot.transient` 让队列面板认得直播行 |
| R4 | 完成 | `drop.rs` 6 条单测（含 `.m3u` 按内容分流、目录只报不扫）；`apps` 层 `classify_paths` / `player_open_paths`；UI 拖放蒙版 + 吐司 |
| R5 | 完成 | 路由 `radio → live`（含 `RETIRED_PAGES` 映射与 zod 只读兼容）、`LiveView` 地址框 / 打开文件 / 最近 20 条 / 大画面；`ui/e2e/live.spec.ts` 覆盖去重与移除 |
| R6 | **未做** | 真流人工冒烟（France Inter / BBC Radio 4 / RTL）。本机没有可播的真流样本，只能等有网络条件的时机 |

### 11.2 与原方案的偏差（都是被事实推着改的）

1. **播放条副标题不是"编码 + 码率"，而是"来源"**（主机名 / 所在目录）。
   `TrackRef.audio` 由曲库单曲查询填，临时项恒为 `null`；要把 TS 里的
   `stream_type` / ADTS 帧率接到 UI，得给 `PlayerState` 加字段 + 重生成 ts-rs 绑定。
   用户已拍板"给什么码率就播什么码率、不做额外支持"——那就别为了一个副标题开新契约。
   主机名是此刻**确定**的信息，也是用户真正会拿来对号入座的东西。
2. **`QueueSnapshot` 加了 `transient: Vec<TrackRef>`**（原方案没写）。队列面板靠
   `library_tracks` 批量补元数据，临时项查库必然空手而归；不给这个口，那一行只能是
   "载入中…"永远转圈。只带临时项（通常 0–几条），不是全队列 refs。
3. **`track_refs_by_ids` 改成认临时项**（原方案隐含"面板点播只传库内 id"）。
   面板回传的是队列原 id，里面可能混着 `stream:`；只查 `tracks` 会把它们静默丢掉，
   于是"点第 3 行播的是第 1 行"，队列里只要有直播流整个点播就直接报"没有可播放的曲目"。
4. **`player_open_paths` 承担了"分类 → 展开 → 查库 → 混排"整条链**，UI 只负责
   按蒙版预告与吐司文案。原方案的 `classify_paths` 保留，但只用于拖拽悬停时那句预告
   （"3 项可播放 · 1 个文件夹跳过"）。
5. **原生拖放无法 e2e**：Tauri 的 `dragDropEvent` 由宿主发，浏览器预览里不存在。
   拖放这条路径靠 `drop.rs` 单测 + 人工拖一次；`live.spec.ts` 用 `?live=<url>` 直接
   摆出"正在播一个流"的状态来验播放条与队列面板的反向渲染。

### 11.3 已知缺口

- **真流冒烟未做**（R6）：TS 剥离的正确性有单测锁，但"段边界有没有咔哒""8 秒断流
  预算在真实网络下够不够"只有真流能验。
- **断流重连只有一次性行为**：断开后不自动重试，需要用户按播放。用户口径是"退出即
  断开、播放即重连"；自动重连（指数退避）属于 v2，做的时候要防止"无限重连把日志写满"。
- **Opus 仍不支持**（§8），TS 里出现 `stream_type=0x11`（LATM）会具名报错。
