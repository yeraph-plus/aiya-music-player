# docs/21 — 播放器本体格式能力调研（HLS / 音频格式 / m3u·m3u8）

日期：2026-10-25
性质：调研。承接 docs/20（远程版）之后，回到**播放器本体**的三个格式问题：
① 本体直接吃 HLS 的改动面；② 音频格式支持现状；③ m3u/m3u8 播放列表支持（为将来导出歌单铺路）。

## 1. 已核实事实（决定性证据都在本地）

| # | 事实 | 证据 |
|---|---|---|
| 1 | 当前解码 feature：`mp3, flac, ogg, wav, aac, isomp4, pcm`（`default-features = false`） | 根 `Cargo.toml` L22 |
| 2 | Symphonia 0.5.4 的 feature 全集里**不存在** `opus`/`wma`/`ape`/`wavpack`/`dsd`——这些编解码永远不支持 | 本地 registry `symphonia-0.5.4/Cargo.toml [features]` 全文（codec 只有 AAC/ADPCM/ALAC/FLAC/MP1/MP2/MP3/PCM/Vorbis；format 只有 CAF/ISOMP4/MKV/OGG/RIFF/AIFF/WAV） |
| 3 | **`ogg` 只是容器 feature，不含 Vorbis 编解码**；当前构建 `ogg` 开了、`vorbis` 没开 → **OGG Vorbis 文件今天就是放不出来的**（probe 成功、make decoder 失败） | 同上 registry feature 定义 + deepwiki 矩阵 |
| 4 | **`alac` 是存在的 feature 但没开** → m4a(ALAC) 当前放不出来 | registry + 根 Cargo.toml 对照 |
| 5 | 扫描器收 14 种扩展名：`mp3 flac wav ogg oga opus m4a aac wma ape wv aiff aif dsf`——其中 **opus/wma/ape/wv/dsf 五种 Symphonia 永远解不了**（事实 2），即扫描器在收集"注定放不出"的文件 | `aiya-data/src/scanner.rs` L24 |
| 6 | Symphonia **没有 MPEG-TS demuxer**（format 全集见 #2）→ HLS 的 .ts 段无法直喂 | 同上 |
| 7 | Symphonia 0.6.1 已发布（2026 最新稳定）：isomp4 **能解 fMP4**，但 live 流有**内存泄漏**——isomp4 reader 永不释放旧 moof（issue #586，2026-10-13 报，未修；社区 workaround：每 4000 fragment 重建 reader，内存从 +195MB 降到 +6.5MB） | Symphonia issues #586/#583 |
| 8 | 0.5.x 的 isomp4 fMP4 支持"limited"，HLS 特性明确不支持 | 官方文档站格式页 |
| 9 | 0.6 带来 gapless 框架改进（signed 时间戳、trim_start/trim_end 语义，PR #422）；MP3 解码 gapless 状态 Excellent，isomp4 gapless 仍 ❌ | Symphonia PR #422 + README 矩阵 |
| 10 | 本仓库 **m3u/m3u8 相关代码为零**（全仓 grep 零命中）——导入导出都是空白 | 全仓 grep |
| 11 | `PlayerState.duration_ms` 已是 `Option`、`QueuePanel` 已有虚拟滚动——"时长未知的流"与"无限队列"UI 语义已备 | state.rs L165 + 大件 A 改动 |
| 12 | 解码入口是 `MediaSourceStream::new(Box::new(src))` → probe → default_track → make decoder，`src` 由调用方给（现在传 File） | `aiya-audio/src/decoder.rs` L39-72 |

## 2. 问题一：播放器本体吃 HLS，改动面打不打？

**结论：改动面中等，不伤架构——HLS 是给 aiya-audio 加一个新"源"，不是动解码/输出链。** 三条段格式路线：

| 路线 | 可行性 | 依赖 | 判定 |
|---|---|---|---|
| A. **ADTS AAC 段（packed audio）** | ✅ 今天就能做 | 仅现有 `aac` feature：每段是自包含 ADTS 流，逐段解码、样本计数累加即可，无 demuxer 依赖 | **起步首选**（FM 本来就是有损） |
| B. **fMP4 段** | ⚠️ 要先升级 Symphonia 0.6.1 | 0.5.x fMP4 limited；0.6.1 可解但有 live 泄漏（#586），需 reader 定期重建 workaround | 二期升级时顺路切换 |
| C. MPEG-TS 段 | ❌ 不可行 | Symphonia 无 TS demuxer，且无计划 | 排除；服务端侧选段格式时避开 TS |

改动清单（路线 A 口径）：

```
crates/aiya-audio 新增 hls.rs（估 400–600 行）
  ├ m3u8 解析：EXTINF/EXT-X-MAP/EXT-X-ENDLIST/live 判别（PLAIN/EVENT/LIVE）
  ├ 段调度：顺序 fetch → live 轮 playlist 刷新（targetduration 间隔）→ 断流重试
  ├ 源装配：ADTS 段拼接喂 probe（每段独立解码、累计样本数报 position），
  │   采样率/声道跨段必须一致（同频道同编码器，成立）
  └ 不支持项显式拒绝：TS 段 / AES-128（首期）/ 多 renditions（取第一个）
engine.rs
  └ 新构造器：open_hls(url)（现有 open_file 旁加一条，共用 probe→decoder→PCM 链）；
      duration=None、seek=禁用、spectrum tap 照常（PCM 链不动）
PlayerActor / 协议
  └ 曲目新增 source=stream 标记（DTO 加字段，wire 已是 number|null 风格）；
      Live流切歌 = 服务端通知换 URL（远程版）或本地直接换
```

**不动的东西**：WASAPI 输出、频谱 tap、音量 ramp、队列状态机——全在 PCM 链之外。ADR 无冲突（音频所有权仍在 Rust）。

**与 docs/20 的协同**：远程版 Liquidsoap 出段时直接配 `packed audio (aac)` 段（liquidsoap `%ffmpeg(format="adts", %audio(codec="aac"))` 原生支持），桌面本体和浏览器 hls.js 吃**同一份段**，服务端不用为两类客户端各出一份。这把"本体 HLS 支持"和"远程 FM"钉成同一条管线。

### 2.1 打分

| 维度 | 评分 | 说明 |
|---|---|---|
| 架构侵入 | 低 | 一个新模块 + 一个构造器，PCM 链零改 |
| 工作量 | 中 | hls.rs 主体 + live 重连边界情况（实测项多） |
| 风险 | 中 | live 场景的段缺口/断网恢复要 soak；#586 型泄漏在路线 A 不存在（不碰 isomp4） |
| 可回退 | 高 | 失败不影响任何既有功能，纯增量 |

## 3. 问题二：音频格式支持现状

### 3.1 真实矩阵（feature 决定，不是愿望）

| 格式 | 扫描器收 | 解码现状 | 处置 |
|---|---|---|---|
| MP3 | ✅ | ✅（gapless Excellent） | — |
| FLAC | ✅ | ✅ | — |
| WAV / AIFF | ✅ | ✅（wav+pcm；aiff feature 未开——`.aiff/.aif` 收了但**放不出**，补一行 feature） | 🔧 补 `aiff` |
| AAC（.aac / m4a-AAC） | ✅ | ✅ | — |
| **OGG Vorbis** | ✅ | ❌ **今天放不出**（ogg 容器开了、vorbis 编解码没开） | 🔧 **补 `vorbis`（最优先，一行）** |
| m4a ALAC | ✅ | ❌（alac feature 存在没开） | 🔧 补 `alac` |
| Opus | ✅ | ❌ **Symphonia 永不支持** | 见 3.2 决策 |
| WMA / APE / WavPack / DSF | ✅ | ❌ **永远不支持** | 见 3.2 决策 |

也就是说：**今天有一个真 bug（OGG Vorbis、ALAC、AIFF 收了放不出）+ 一个长期定位矛盾（五种永不支持的扩展名照常入库）**。前者一行 feature 修复，后者要产品决策。

### 3.2 永不支持格式的处置（决策点）

| 选项 | 做法 | 判定 |
|---|---|---|
| ① 扫描器收，播放时报 `codecUnsupported` | 诚实入库，播放/详情页标"不支持"，诊断页可见计数 | **推荐**：用户看得见为什么放不出；错误模型现成（`AudioError.kind`） |
| ② 扫描器不收 | 库里干净，但用户困惑"文件去哪了" | 否 |
| ③ ffmpeg 兜底解码 | 加 ffmpeg 子进程解码链 | 大改动，除非确有 WMA/APE 藏量，否则不值 |

补 feature 的同步动作：`opt-simd-sse/avx` 顺手开（x86 目标免费提速，README 明确 SIMD 默认关）。

### 3.3 Symphonia 0.5.4 → 0.6.1 升级（独立决策）

收益：fMP4 可解（路线 B 前置）、gapless 框架改进（isomp4 gapless 仍无但 mp3 语义更严谨）、活跃修复线。代价：API breaking（`make_audio_decoder`/`TrackType`/signed 时间戳，decoder.rs+engine.rs 适配，估 1–2 天）；建议**不与 HLS 捆绑**，单独小批做，先跑全量回归。

## 4. 问题三：m3u / m3u8 播放列表（现状零支持）

### 4.1 导入（Import）

```
新模块 crates/aiya-data/src/playlist_io.rs（估 300–400 行 + 测试）
  ├ 解析：#EXTM3U 头、#EXTINF:<dur>,<artist> - <title>、注释行、URL 解码、
  │   相对路径（相对 m3u 文件位置）、BOM 容忍
  ├ 编码：UTF-8 默认；m3u8=强制 UTF-8（标准如此）；纯 m3u 允许 GBK 探测（决策点）
  ├ 匹配：路径 → path_lower 精确匹配入库曲目；未匹配项收集（复用扫描错误报告的形状）
  ├ 拒绝语义：出现 #EXT-X-TARGETDURATION/#EXT-X-MEDIA-SEQUENCE 等 HLS live 标签
  │   → ApiError.notSupported("这是流媒体 manifest，不是静态播放列表")
  │   （m3u8 一词两义，必须在错误信息里分清，不能默默失败）
  └ 落库：走现有 playlist/playlistItems API，歌单名取文件名
命令面：playlist_import(path) → {playlistId, matched, unmatched[]}（2 个新命令）
UI：库页"导入播放列表"入口 + 未匹配清单展示
```

### 4.2 导出（Export，为将来铺路）

```
playlist_export(id, path, format: 'm3u8' | 'm3u', encoding: 'utf8' | 'gbk')
  ├ 写绝对路径版（本机用）与相对路径版（拷贝迁移用）二选一
  ├ 车机/老设备兼容：GBK m3u 是中文生态真实需求（决策点：是否首期带）
  └ 远程版延伸：服务端同名端点导出"URL 版歌单"；
      歌单 → FM 频道一键转换（与 docs/20 §6.1 的 FM 队列天然对接）
```

### 4.3 与 FM 的协同闭环

导入的 m3u 歌单 = FM 频道的天然内容源：`createFmChannel(playlistId)` 一条命令。这样"m3u 导入 → 歌单 → FM 推流"成为一条产品故事线，三块工作互相供电。

## 5. 建议批次

| 批次 | 内容 | 性质 |
|---|---|---|
| F1（快） | 补 `vorbis`/`alac`/`aiff` feature + SIMD；播放失败报 `codecUnsupported` | 一行级 + 错误语义，**性价比最高** |
| F2 | m3u/m3u8 导入 + 拒绝 HLS manifest 语义 + 导出 m3u8 | 纯增量，无架构风险 |
| F3 | 本体 HLS（路线 A：ADTS 段）+ 协议 source 标记 | 中改动面，live 边界需 soak |
| F4（独立） | Symphonia 0.6.1 升级（顺路切 fMP4 路线 B） | 单独回归，不与 F3 捆绑 |
| F5 | GBK m3u 导出/导入（若拍板要）、歌单→FM 频道 | 随远程版批次走 |

## 6. 决策点（2026-10-25 G1–G4 执行后回写，2026-10-26 修正）

1. 永不支持格式（opus/wma/ape/wv/dsf）：**收+标注**（推荐）还是不收？
   → **已落地「收+标注」**，但要分清两条路径（G1 首版只做了后一条，
   2026-10-26 补前一条）：
   - 容器认、编码不认（**Opus**）：probe 成功、`make()` 失败 →
     `codecUnsupported`，错误消息带人话格式名；
   - 容器都不认（**WMA/APE/WavPack/DSD**）：symphonia 0.5.x 没有这些
     demuxer，probe 阶段就失败，原实现会落进 `Decode`（"文件坏了"），
     用户看到的是 symphonia 原文。**已改成按扩展名在 probe 之前拦**，
     直接 `codecUnsupported`（`decoder.rs::known_unsupported_ext`）。
   - 教训：`make()` 失败分类只覆盖"容器认、编码不认"这一半，"永不支持"
     名单里 4/5 的格式根本走不到那里。
2. GBK m3u：首期要不要（车机场景）？
   → **首期不做，参数位已留**（G2）：`playlist_export` 的 `encoding` 只收
   `utf8`，越界值 `invalid_arg`；GBK 解码/写出等真实车机需求出现再加。
   （注：G2 的提交信息里"GBK(参数指定) 解码"一句与代码不符，以本行为准。）
3. F3 本体 HLS 的优先级：紧跟远程版（同一条 ADTS 管线，建议紧跟）还是独立排期？
   → **独立排期已完成**（G3，先于远程版）：`hls.rs` + 引擎分流 +
   `PlayerState.source` 全部落地；端到端验收吃远程版 R2 的 Liquidsoap
   真流 soak（Liquidsoap 出 adts 段，桌面与浏览器同源）。
4. Symphonia 0.6.1 升级窗口：建议在 F3 之后、远程版打包前，单独一批。
   → **维持冻结**：0.5.x 交付主流格式够用；0.6.1（fMP4 段 + live 泄漏
   #586 修复）按原窗口——远程版打包前单独一批，不与 G 批次混。

## 7. 2026-10-26 审查修复记录

审 G1–G4 四刀时发现的问题与修法（细节见 docs/22 §6）：

| # | 问题 | 修法 |
|---|---|---|
| 1 | m3u 相对路径不规范化，`../` 条目全部误报未匹配（`base_dir.join` 保留 `..`，而导入按 `path_lower` 精确对账） | `playlist_io::normalize_lexical` 词法规范化 + 4 条单测 |
| 2 | 导入"一条都没匹配上"借 `DbError::Custom` 文案 + `contains` 判回（自造"解析文案"反模式，且清单丢失） | 改成正常结果：`playlist = null` + 完整 unmatched 清单 |
| 3 | 段边界同步 fetch 落在音频填充路径上（独占 10–50ms 缓冲每段必断） | `hls.rs` 加预取线程（`Prefetch`），一段 lookahead，段边界零等待；`Fetcher: Send` |
| 4 | 退避全程同步睡在 `next_packet` 里，按次数上限能睡出 3 分钟（叠加 HTTP 超时更长），期间 Pause/Stop 无响应 | 改成墙钟预算 `OUTAGE_BUDGET = 20s`，末次睡眠裁剪到死线 |
| 5 | live「没有新段」无放弃条件：URI 复用会被判成旧段 → 500ms 一轮永远静默轮询 | 解析 `#EXT-X-MEDIA-SEQUENCE` 按序号判新旧；停滞超 3×TARGETDURATION（下限 15s）报错 |
| 6 | 兜底格式名用 `Debug`（`CodecType(4096)`），对不回源码常量 | 改 `Display`（`0x1000` 形态） |
| 7 | `playlist_export` 是同步命令（Tauri 默认 Blocking → 主线程），DB 往返 + 写盘会冻窗口 | 改 `async fn`，落到达 async runtime |
| 8 | mock 预览缺 `playlist_import`/`playlist_export`（docs/22 G2 第 6 条要求） | 补两条（导入造假结果用于渲染弹层，导出只回形状不写盘） |
| 9 | `PlayerState.source` 停止/清空队列后不复位，界面一直"直播中" | 统一走 `PlayerActor::go_idle()`（4 处 Idle 出口） |
| 10 | **m4a 一打开就 panic**：MP4 容器不填 `codec_params.channels`，旧代码 `unwrap_or_default()` 得空布局，`SampleBuffer::new` 除零（symphonia-core audio.rs:727）→ 引擎线程死亡。ALAC/AAC 都中，而 AAC 是 Apple 生态主力格式 | `open` 里探首包，规格以解码器为准（逐字段重建缓冲），首包缓存后在首次 `next_packet` 归还；补 ALAC/AAC fixture 锁死 |
| 11 | seek 之后先吐旧音频、位置还会回退：未 `decoder.reset()`（Vorbis 把上一段 MDCT 重叠带进落点）+ Ogg 按页/MP4 按 chunk 落点，落点前的整包照样输出 | `seek_ms` 里 `decoder.reset()` + 记目标；`next_packet` 丢掉"结束时间 ≤ 目标"的整包 |
| 12 | Vorbis 时长报 616ms（尾部 end delay 算进去了）、MP3 开头 576 样本编码器延迟照放 | 打开 gapless（`FormatOptions { enable_gapless: true }`），trim 交给解码器消费；不按 duration 硬裁（那个值对无 Xing 头的 MP3 是估算的） |
| 13 | **已知未修**：AAC 的 MP4 `elst`（prime/padding 裁剪表）0.5.5 不读 → 0.6s 报 623ms | 记在 `decode.rs` 的 `FX_EXPECT` 注释，等 0.6 升级或自己解 elst |

**真文件 fixture（2026-10-26 补上，不再是"待人工验收"）**：
`crates/aiya-audio/tests/fixtures/` 下 5 份二进制样本（Vorbis / ALAC / AAC / MP3 / AIFF，
共 ~111KB），由同目录 `gen_fixtures.py` 生成（soundfile 写 OGG/AIFF、PyAV 写 MP4/MP3，
**本机不需要装 ffmpeg 本体**；PyAV 自带的 ffmpeg 编译里没有 libvorbis，所以 Vorbis 走
libsndfile）。测试口径：解出 26460 帧（600ms×44.1kHz）、正弦段峰值 >0.3、静音段 <0.02、
每格式 duration 与 `FX_EXPECT` 表逐项对齐、seek 到静音区必须安静且位置不回退。

这份 fixture 不是形式主义——第一次跑就挖出了上表第 10–12 条（m4a 除零 panic 是
**打开即崩**级别，此前所有单测都碰不到，因为 test-music 只有合成 WAV）。

仍未做（不是本轮范围）：
- HLS 端到端 soak（连 30 分钟 + 断网恢复）仍随远程版 R2。
- 契约层文档里的 Symphonia 版本号写 0.5.4，Cargo.lock 实际是 **0.5.5**
  （`^0.5.4` 解析结果），D2 的"停留 0.5.x"不受影响。
