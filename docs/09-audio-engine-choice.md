# 09 — 音频引擎选型：mpv sidecar vs wasapi-rs 原生

> 这篇回答一个问题：**在"UI 体验 + beets"是重点、项目 GPLv3 的前提下，音频引擎应该买/借，还是自己写？**
> **修订（2026-10-23）**：决策改为**原生引擎首发**（Symphonia + wasapi-rs），见 §3.1 与 [ADR-0006](adr/0006-native-engine-first.md)。本文其余分析保留作依据与逃生路径参考。
> 原结论：v1 用 mpv sidecar、wasapi-rs 原生留作 v2——**已被 §3.1 修订取代**（原生首发，ADR-0006）。原 B 方案章节即现行主方案。

## 1. 需求排序

| 需求 | 权重 | 说明 |
|---|---:|---|
| 稳定出声、格式覆盖 | 高 | 用户的曲库不能“扫得到、播不了” |
| WASAPI 共享/独占、设备选择 | 高 | 这是换 Tauri 的核心理由之一 |
| 与 UI 状态机解耦、可恢复 | 高 | 窗口关闭/重载不影响播放 |
| 开发速度 | **最高** | 重点在 UI/beets，不在重写播放器 |
| bit-perfect / DSD / 自定义 DSP | 低（v1） | 可以后置，不要阻塞 |
| 二进制体积/许可 | 中 | GPLv3 可接受 GPL/LGPL 依赖 |

## 2. 候选对比

| 方案 | 工期（全职） | 控制力 | 格式 | 独占 | 许可 | 适配本项目 |
|---|---:|---|---|---|---|---|
| **mpv sidecar** | **3–5d** | 中 | 全（FFmpeg） | ✅ `--audio-exclusive` | GPLv2+，与 GPLv3 兼容 | **最高** |
| wasapi-rs 原生 | 2–3w | 高 | symphonia + FFmpeg 补 | ✅ | MIT / MPL / LGPL | 中 |
| GStreamer | 1–2w | 中 | 全（插件） | 需验证 `wasapisink` | LGPL | 中低（运行时重） |
| libVLC | 1–2w | 中 | 全 | 需验证 | LGPL 2.1+ | 中低（集成重） |
| BASS | 3–5d | 高 | 全 | ✅ |  proprietary | ❌ GPLv3 需商业授权 |

### 2.1 mpv sidecar：已验证的能力

mpv 官方手册明确支持：

- `--audio-exclusive=yes`：独占输出；明确列出 `wasapi` 是支持的 AO 之一。
- `--wasapi-exclusive-buffer=default|min|1-2000000`：独占缓冲（微秒），默认/最小设备周期。
- `--audio-device=<name>` / `audio-device-list`：设备枚举与选择。
- `--gapless-audio=no|yes|weak`：gapless。
- `--replaygain=no|track|album` + `--replaygain-preamp` / `--replaygain-clip`：标签增益。
- `--audio-samplerate` / `--audio-format` / `--audio-channels`：输出格式控制。
- `--speed` / `--audio-pitch-correction`：变速不变调。
- FFmpeg 解码：WMA/AIFF/DFF/DSD 等都能读（DSD 转 PCM，不是 DoP）。
- EQ 可用 FFmpeg `equalizer` 滤镜（`--af`），但 UI/参数需要自己设计。

实现方式不是嵌入 libmpv，而是**把 `mpv.exe` 当 sidecar**：

```
host spawn:
  mpv.exe --no-video --idle=yes --no-terminal
          --input-ipc-server=\\.\pipe\aiya-mpv-{pid}
          --audio-exclusive=yes
          --audio-device=<id>
          --gapless-audio=weak
          --replaygain=track
          --audio-fallback-to-null=no
```

host 通过命名管道发 JSON：

```json
{"command":["loadfile","D:\\Music\\a.flac","replace"]}
{"command":["set_property","pause",false]}
{"command":["seek",42.5,"absolute+exact"]}
{"command":["set_property","volume",72]}
{"command":["observe_property",1,"time-pos"]}
```

mpv 回事件/property change；宿主映射成 `AudioEvent`。这比 libmpv 绑定更简单：不需要 C 编译、不需要处理 libmpv 的 render API、不需要把 mpv 链接进进程。

**优点**

- 3–5 天能跑通：解码/格式/独占/设备/gapless/ReplayGain 都是现成的。
- 与 GPLv3 兼容：mpv 核心 GPLv2+，你的项目 GPLv3；分发时提供 mpv 源码/许可即可。
- 进程隔离：mpv 崩了宿主重启它；不会拖垮 UI/SQLite。
- 后续可换：`AudioPort` 后面接原生 wasapi-rs，UI/DB 不动。

**缺点/坑**

- 多一个 ~50MB 的 `mpv.exe`；安装包变大。
- 时钟/事件是 mpv 的语义：`time-pos` 更新频率、seek 精度、`eof-reached` 需要实测；不能假设和自研引擎一样。
- 独占失败/设备切换时 mpv 可能回退或报错；必须 `--audio-fallback-to-null=no` 并解析日志/`end-file`，不能静默。
- bit-perfect 不保证：mpv 内部可能重采样/加滤镜；要验证 `audio-out-params` 与源格式一致，且关闭 ReplayGain/EQ/volume 归一化。
- `--gapless-audio=yes` 会把后续曲目重采样到第一首的格式；混采样率曲库用 `weak` 或按专辑重开。
- 许可：分发 mpv 二进制要附 GPL 文本、源码获取方式；如果改用 libmpv，动态链接 LGPL 构建更省事。

### 2.2 wasapi-rs 原生：现在比过去好很多

`wasapi` crate 0.26.0（MIT）已经提供：

- 共享/独占模式、事件驱动/轮询。
- 设备枚举、默认设备、设备增删/默认变化通知。
- **独占模式格式探测**（采样率/声道/格式）。
- `IAudioClock` 封装、loopback、会话/音量通知。
- 原始 COM 接口逃生口。

这意味着之前最痛的“枚举 + 独占格式探测 + 设备通知”不用自己写 COM 了。原生路径剩下的工作是：

```
FFmpeg/symphonia 解码 → ring buffer → resample（共享时）
→ ReplayGain/EQ/volume → wasapi-rs 输出 → 时钟/事件
```

- 解码：`symphonia`（MPL-2.0）覆盖主流；WMA/AIFF/DFF 用 FFmpeg（LGPL）补，或直接 FFmpeg 全量解码。
- 重采样：`rubato`（MIT）。
- 变速：`signalsmith-stretch`（时间伸缩；`rubato` 只做采样率转换）。
- 标签：`lofty`（MIT）。
- 工作量：2–3 周（含设备/格式/seek/gapless 测试），比 mpv 多 2 周左右。
- 收益：完全控制 bit-perfect、时钟、DSP、DSD/DoP；二进制更小；不依赖 mpv 的行为。

### 2.3 GStreamer / libVLC / BASS

- **GStreamer**：`playbin3` + `wasapisink` 很成熟，但 Windows 运行时/插件打包重；`wasapisink` 的 exclusive 行为要实测。适合“视频/复杂管线”，不适合这个以 UI 为主的小播放器。
- **libVLC**：LGPL 2.1+，格式全；但事件/时钟/设备控制比 mpv 更绕，Rust 绑定生态弱。
- **BASS**：API 简单、独占强，但 proprietary；GPLv3 分发需要商业 license，直接排除。

## 3. 决策

### 3.1 修订（2026-10-23）：改判原生首发

原推荐（v1 mpv）基于"最快出声、把时间留给 beets/UI"。底盘规划对齐（[`10`](10-plan-alignment.md)）后新增硬要求：

1. **频谱是必要体验**（动画级即可），需要在引擎数据链路内有 PCM tap——mpv 无 PCM tap API，做不到；
2. **EQ、LRC、WASAPI 输出联合设计**，接口一次定形；
3. 音频采用成熟 Rust 原生组件。

因此改判：

- **v1 = 原生**：Symphonia 解码 + `wasapi` crate 输出（Shared 先行，Exclusive 按 [`04`](04-audio-engine.md) §3 分期），`aiya-audio-host` sidecar 交付；DSP 链内建订阅制 tap（04 §5.1）。
- **不采用 Rodio**（模型冲突、无独占）；CPAL 仅作跨平台占位。
- mpv 降为 spike 对照与**逃生实现**：若原生 spike 暴露硬伤，回退 mpv 的明确代价 = 无频谱、EQ 受限、+50MB、GPLv2+ 分发义务。
- 工期影响：P2 1 周 → 2 周；总 4–6 周 → 5–7 周。
- §4 spike 门槛不变，默认跑原生，mpv 仅作对照数据。
- §6 的 mpv 落地清单降级为"逃生路径参考"。

### 原推荐（已被 3.1 取代，保留作依据）

#### 推荐：v1 用 mpv sidecar

理由：

1. 你的重点是 **UI 体验 + beets**；mpv 把音频从“2–3 周工程”变成“3–5 天集成”。
2. mpv 已经覆盖 WASAPI 独占、设备、gapless、ReplayGain、全格式；这些正是自研最容易踩坑的部分。
3. GPLv3 与 mpv GPLv2+ 兼容；许可不是障碍。
4. `AudioPort` 抽象保留，wasapi-rs 原生路径可以随时替换；不会锁死。

### 备选：首发 wasapi-rs 原生

如果你明确要 bit-perfect/完全控制，或不想带 50MB mpv：

- 用 `wasapi` 0.26 + `symphonia` + FFmpeg 补格式；`rubato`/`signalsmith-stretch`/`biquad`。
- 工期 +2 周；先把 Shared 跑通，再 Exclusive。
- 仍然保留 `AudioPort`，mpv 可作为“逃生/对比”实现，不进入发布包。

### 不推荐：两个引擎同时上

不要在 v1 同时维护 mpv 和原生两条出声路径。两条路径 = 两套时钟、两套设备状态、两套错误；调试会互相污染。用 trait 隔离，但**同一版本只启用一个**。

## 4. 2 天 spike 门槛（先测再定）

无论选哪条，先做一个最小 spike，用同一批文件测：

| 用例 | 通过标准 |
|---|---|
| 44.1k/16bit 独占 | 成功；第二客户端打开同设备失败 |
| 48k/24bit 独占 | 成功；`audio-out-params` 与源一致 |
| 96k/24bit | 成功；无重采样（或明确记录） |
| 44.1k/10ms 对齐 | 不崩；缓冲恢复；无爆音 |
| seek ±50ms | 连续 20 次随机 seek，误差可接受 |
| 暂停/恢复 | 无爆音；位置不漂 |
| 设备拔出 | 暂停/报错；重插可恢复 |
| 1h 长播 | 无泄漏、无 underrun、CPU 稳 |
| 混采样率队列 | gapless 行为符合预期（或明确不 gapless） |
| 关闭/重载 UI | 声音不断 |

mpv 侧还要额外测：`time-pos` 频率/精度、`eof-reached`、`end-file` 原因、独占失败是否静默回退。
wasapi-rs 侧还要额外测：格式探测、`GetBufferSize` 对齐、设备通知、MMCSS。

**spike 结果写进 `docs/bench/`，再拍板。** 不要凭“应该可以”选引擎。

## 5. 许可清单（GPLv3 项目）

| 依赖 | 许可 | 处理 |
|---|---|---|
| mpv（sidecar） | GPLv2+ | 与 GPLv3 兼容；分发附 GPL 文本、源码获取方式；不要静态链接闭源插件 |
| libmpv（若用） | 核心 GPLv2+ / API ISC；可构建 LGPL | 动态链接最简单；LGPL 需允许替换/重链接 |
| wasapi-rs | MIT | 保留 copyright |
| symphonia | MPL-2.0 | 与 GPLv3 兼容；保留 notices；修改文件需开源该文件 |
| FFmpeg | LGPL 2.1+（默认构建） | 动态链接、附许可/源码；不要启用 `--enable-gpl` 除非接受 GPL |
| rubato / signalsmith-stretch / biquad | MIT/Apache/BSD | 保留 notices |
| beets | MIT | 保留 copyright |
| Python（若捆绑） | PSF | 保留 license |
| Tauri | MIT/Apache-2.0 | 保留 notices |

> 这不是法律意见；发布前让律师/合规过一遍，尤其是 mpv/FFmpeg 的二进制分发与源码提供方式。

## 6. 如果选 mpv：最小落地清单

1. 下载/固定一个 Windows mpv 构建（x86_64），记录版本/来源/哈希。
2. `mpv.exe` 作为 Tauri sidecar 资源；启动参数固定（`--no-video --idle --no-terminal --input-ipc-server`）。
3. 宿主 `MpvClient`：连接命名管道、JSON 行协议、请求 id、事件循环、重启。
4. 命令映射：`loadfile/pause/seek/volume/speed/stop`；属性观察：`time-pos/duration/pause/eof-reached/audio-params/audio-out-params/audio-device-list/metadata`。
5. 错误映射：`end-file` reason、日志、启动失败、设备丢失。
6. `AudioPort` 实现 `MpvAudio`；`PlayerActor` 只依赖 trait。
7. 打包：`mpv.exe` + licenses + 版本说明；安装器签名。
8. 自检命令：`aiya-audio --selftest` 输出设备/格式/独占结果 JSON。

## 7. 如果选 wasapi-rs：最小落地清单

1. `wasapi` 0.26 + `windows` 0.62；先 `DeviceCollection` 枚举 + `CapabilityProbe`。
2. `symphonia` 解码到 f32/i32；WAV/FLAC 先通；FFmpeg 后补。
3. ring buffer + 事件驱动 `AudioClient`；Shared 先通。
4. Exclusive：`initialize_client` + 格式探测 + `GetBufferSize` 对齐恢复。
5. 时钟：`AudioClock` + frames written；10Hz 事件。
6. 设备通知：`DeviceNotificationClient`；设备丢失 → 暂停/重建。
7. DSP：`rubato` 重采样、ReplayGain、音量 ramp；EQ/变速后置。
8. 自检矩阵同 §4；硬件测试 `#[ignore]`。

## 8. 结论

> **把音频当“基础设施”买进来，把时间花在 UI 和 beets 上。**
> v1：mpv sidecar；v2：如果 bit-perfect/DSD/自定义 DSP 真的成为需求，再用 wasapi-rs 原生替换。
> 两条路都通过 `AudioPort` 隔离，UI/DB/契约不变。

## 参考

- wasapi-rs 0.26.0（MIT，共享/独占/事件/设备通知/独占格式探测）：<https://docs.rs/wasapi/latest/wasapi/>
- mpv 手册（`--audio-exclusive`、`--wasapi-exclusive-buffer`、`--gapless-audio`、`--replaygain`、`--audio-device`）：<https://mpv.io/manual/master/>
- libmpv 许可说明（client API ISC；core GPLv2+）：<https://docs.rs/mpv/latest/mpv/>
- beets 2.14.1（MIT，Python 3.10–3.14）：<https://pypi.org/project/beets/>
- beets importer 与 CLI 解耦说明：<https://beets.readthedocs.io/en/stable/dev/importer.html>
- beets CLI（import 选项）：<https://beets.readthedocs.io/en/stable/reference/cli.html>