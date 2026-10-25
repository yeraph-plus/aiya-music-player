# docs/22 — 格式能力迭代方案（依据 docs/21 拍板）

日期：2026-10-25
依据：docs/21 调研 + 用户拍板。**本文档是施工图**，确认后按批执行。

## 0. 拍板记录（不可回退）

| 决策 | 内容 |
|---|---|
| D1 | **不接入 ffmpeg、不内置 mpv 全格式**。支持主流格式：MP3 / FLAC / WAV / AIFF / AAC(.aac、m4a-AAC) / m4a-ALAC / OGG Vorbis |
| D2 | **维持当前基础设施**：Symphonia 停留 0.5.x（0.6 升级与 fMP4 路线整体**冻结**，仅当本体 HLS 需要吃 fMP4 段时重新立项） |
| D3 | 不支持的格式（opus/wma/ape/wv/dsf）**照常入库、播放时报错**——错误信息说人话（格式名 + 为什么），诊断页可见计数 |
| D4 | **mpv 解码预留插入缝**：解码器构造收敛到单一入口 + 后端枚举，将来引入 mpv（子进程 PCM 管道方案）只改一处，不动 engine/输出/队列 |
| D5 | GBK m3u 暂不做（导出歌单是"后面可能"的需求，真要时按 F2 预留的 encoding 参数位加，半小时工作量） |

## 1. 批次计划

### 批次 G1：格式矩阵修正 + codecUnsupported（小，半天级）

改动文件：根 `Cargo.toml`、`crates/aiya-audio/src/{decoder.rs,lib.rs}`、`crates/aiya-audio/src/proto.rs`、`src-tauri/src/dto/*`（AudioError kind）、TS 生成物再生成。

1. 根 Cargo.toml symphonia features：`["mp3","flac","ogg","wav","aiff","aac","isomp4","pcm","vorbis","alac","opt-simd-sse","opt-simd-avx"]`
   - 补 `vorbis`（修 OGG Vorbis 放不出）、`alac`（修 m4a-ALAC）、`aiff`（修 .aiff/.aif）
   - 开 SIMD（x86 免费提速；README 注明默认关）
   - **不开** `adpcm`/`mp1`/`mp2`/`mpa`/`mkv`/`caf`（非主流，维持最小面）
2. 解码失败分类（D3+D4 的落点，见 §2）：
   - `decoder.rs` 的构造入口收敛为 `open_with_backend(src, hint) -> DecodedStream`；
   - probe 成功但 `get_codecs().make()` 失败 → `AudioError::codecUnsupported { codec: String }`
     （在产生处判 `track.codec_params.codec`，映射成用户可读名：`Opus/WMA/Ape/WavPack/DSD/…`）
   - probe 失败（坏文件/未知容器）→ 维持现有 decodeFailed 类
3. `AudioErrorCode` 增 `codecUnsupported`；协议 kind 字符串随之（协议加值向后兼容，宿主/侧car 改完必须 `cargo build -p aiya-audio-host` —— beforeDevCommand 已自动）
4. UI：播放条错误 toast 显示格式名；诊断页 recent_errors 天然可见（任务 B 的台账直接受益）
5. 测试口径：**无 ffmpeg 生成不了 Vorbis/ALAC 样本**——单测只锁"error 分类路径"（喂伪数据断言 codecUnsupported 与 codec 名），真格式播放走人工验收（test-music 目录现有合成 WAV + 手头 mp3/flac）；fixture 不进 git（二进制体积）

验收：`cargo test --workspace` 全绿 + 生成物 gen:types:check；人工放 .ogg(Vorbis)/.m4a(ALAC)/.aiff 各一首；伪数据单测断言错误分类。

### 批次 G2：m3u/m3u8 导入导出（中，1–2 天级）

改动文件：`crates/aiya-data/src/playlist_io.rs`（新）、`crates/aiya-data/src/lib.rs`、`src-tauri/src/lib.rs`（+2 命令 → 55）、`ui/src`（导入入口 + 结果弹层）、`src-tauri/tests/dto_wire.rs`。

1. `playlist_io.rs` 解析器（纯函数，全力单测）：
   - `#EXTM3U` 头、`#EXTINF:<dur>,<artist> - <title>`（标题分隔容错：`Artist - Title` / 裸标题）、注释/空行跳过、URL 解码（`file://` 与 `%20`）、相对路径按 m3u 所在目录解析
   - 编码：m3u8 强制 UTF-8（BOM 容忍）；纯 m3u 先 UTF-8、失败即报 encodingUnsupported（**不做 GBK 自动探测**，D5）
   - **HLS manifest 拒绝**：出现 `#EXT-X-TARGETDURATION|#EXT-X-MEDIA-SEQUENCE|#EXTINF` 之外的 `#EXT-X-` 关键标签 → `ApiError.notSupported("流媒体 manifest（HLS）不是静态播放列表，请用 FM 频道功能")`
2. 匹配落库：路径 → `path_lower` 精确匹配；未匹配收集 `{line, raw, reason}` 返回（形状对齐扫描错误报告）；歌单名 = 文件名去扩展名
3. 导出：`playlist_export(id, path, format: m3u8|m3u, style: absolute|relative)`——**encoding 参数位现在就留**（`utf8` 枚举单项），GBK 未来加枚举值即可（D5）；EXTINF 写 `#EXTINF:<秒>,<artist> - <title>`
4. 命令：`playlist_import(path)` / `playlist_export(...)` → 55 命令（注释同步）；返回类型走 ts-rs 生成
5. UI：库页/歌单页"导入播放列表"文件选择 + 结果弹层（匹配 N / 未匹配 M 列表）；导出先只出命令与诊断页隐藏入口（正式按钮随导出歌单需求一起做——用户原话"后面可能会加"）
6. 测试：解析器全矩阵单测（相对路径/URL 解码/BOM/HLS 拒绝/空文件/超长行）；wire 测试锁返回形状；mock backend 同步两命令（内存版）

验收：单测全绿 + gen:types:check；CDP 实测导入一个手工 m3u8（含 3 匹配 + 1 不匹配）→ 歌单计数 3、未匹配清单 1 行。

### 批次 G3：本体 HLS（路线 A：ADTS packed-audio，中，2–3 天级 + soak）

前置：无（不依赖远程版开工；但**首尾两端**要等远程 FM 出流才能端到端，先做可单测的部分）。改动文件：`crates/aiya-audio/src/hls.rs`（新）、`engine.rs`（+1 构造器）、`proto.rs`/DTO（`source: 'file'|'stream'`）、QueuePanel 无改动。

1. `hls.rs`（估 400–600 行）：
   - m3u8 解析：`EXTINF` 段列表、`EXT-X-MAP`（首期拒绝——ADTS 无 MAP）、`EXT-X-ENDLIST`（VOD）vs live 判别
   - 调度：顺序 fetch 段 → live 模式按 `EXT-X-TARGETDURATION` 轮询 playlist → 段缺口跳报错重试 → 断网退避重连
   - 解码：每段独立 probe（hint=adts）→ 解码 → **样本计数累计为 position**；跨段采样率/声道不一致视为流错误
   - 显式拒绝（不是静默错）：TS 段（`#EXTINF` 后 `.ts`）、AES-128 `#EXT-X-KEY`、多 rendition master playlist（取第一个 media playlist 并记日志）
2. `engine.rs`：`open_hls(url)` → 复用现有 probe→decoder→PCM 链；`duration=None`、seek 拒绝（返回 notSupported）、spectrum tap 照常
3. DTO：`TrackAudio`/`PlayerState` 加 `source` 字段（wire: `"file"|"stream"`）；UI 进度条对 stream 曲目隐藏拖动、时长显示"直播中"
4. **D4 插入缝在此落锤**：`decoder.rs` 的 backend 选择收敛为枚举 `DecodeBackend::{Symphonia}`（唯一成员）+ `open_with_backend()` 单入口；将来 mpv 以 `DecodeBackend::Mpv` 加入：子进程 `mpv --no-video --audio-format=s16le --ao=pcm` 管道输出 PCM → 现有 WASAPI 链，**engine/output/queue 零改动**。在 decoder.rs 头注写明这个设计意图与 mpv 参数草案
5. 测试：本地起一个 2 段 ADTS 静态 m3u8（用现有合成 WAV 经…无 ffmpeg！）——**改用工程内手写最小 ADTS 段 fixture**（ADTS 头 + 静音 AAC 帧可程序化生成， AAC 帧编码困难——务实方案：单测锁 m3u8 解析/调度状态机（mock HTTP），端到端留给远程 FM 流 soak（Liquidsoap 出真段））

验收：单测（解析+状态机 mock）全绿；端到端验收随远程版 R2 的 FM soak 一起做（连 30 分钟不断流 + 断网 10s 恢复）。

### 批次 G4：D4 收尾检查（随任一批次顺带）

- 全仓 grep 确认除 `decoder.rs` 单入口外无第二个 `get_codecs()`/`get_probe()` 调用点
- decoder.rs 头注补一段"后端扩展指南"（Symphonia 特性怎么加、mpv 怎么插、错误怎么分类）
- docs/21 §6 的 4 个决策点逐条回写状态

## 2. mpv 插入点设计（D4，防将来返工）

```rust
// crates/aiya-audio/src/decoder.rs（现状收敛目标）
pub enum DecodeBackend { Symphonia }          // 将来 + Mpv

pub fn open_with_backend(src: MediaSource, hint: &Hint, backend: DecodeBackend)
    -> Result<DecodedStream, AudioError>
// DecodedStream 统一「逐包 PCM + 元数据」迭代器形状；
// Symphonia 实现今天就有（现有 probe→decoder 链原样搬入）；
// mpv 实现将来 = 子进程 + stdout PCM 管道 + progress 行解析，产同一 DecodedStream
```

原则：**engine 永远只认 `DecodedStream`**，不知道后端是谁。这是"以后想引入 mpv 也可以从中插入"的全部保障，成本≈一次 100 行内的现有代码重排。

## 3. 与远程版（docs/20）的排期衔接

| 本方案批次 | 远程版批次 | 关系 |
|---|---|---|
| G1/G2 | — | 独立，随时开工 |
| G3（可单测部分） | R2（Liquidsoap FM） | G3 端到端验收吃 R2 的真流；Liquidsoap 段格式定 **adts**（`%ffmpeg(format="adts", %audio(codec="aac"))`），桌面与浏览器同源 |
| G1 的 codecUnsupported | 远程版能力位 | 浏览器侧不支持格式的报错文案复用同一 DTO |

## 4. 明确不做（本期）

- ffmpeg 依赖、mpv 内置（连可选依赖都不进 Cargo）
- Symphonia 0.6 升级、fMP4 段、TS 段
- GBK m3u 读写（参数位已留）
- 加密 HLS（AES-128/SAMPLE-AES）、多 rendition ABR（音频播放器无意义）
- WMA/APE/WavPack/DSD 的任何解码路径（含外挂）

## 5. 执行顺序与依赖

G1 → G2 → G3 串行即可（互不阻塞，也可 G1‖G2 并行）。每个批次独立提交、独立全量回归（cargo test + svelte-check + gen:types:check + E2E）。
