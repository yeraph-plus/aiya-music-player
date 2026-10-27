# 04 — 音频引擎：WASAPI、解码、时钟与 DSP

> **路径说明**：本文是 **v1 主方案（原生引擎）** 的实现细节（ADR-0006，2026-10-23 改判；依据见 [`10`](10-plan-alignment.md) §3.3）。mpv sidecar 降级为 spike 对照与逃生实现，不进入发布包。
> 目标：把"能出声"变成"可证明地按正确格式、正确设备、正确时钟出声，并且失败可解释"。
> 现版实测与坑位原本来自 Electron demo 的 `04-WASAPI独占播放方案.md`、`10-WASAPI与解码与EQ调研.md`
> 与 `../spike/p4-wasapi/` 探针。**这三处本地目录已于 2026-10-27 删除**：可复现的结论已折进本文，
> 不可复现的部分（原始探针数据与录屏）不再保留。

## 1. 输出模式

| 模式 | 用途 | 格式 | DSP | 设备占用 |
|---|---|---|---|---|
| `Shared` | **默认**、兼容性最好 | 设备 mix format（通常 f32/48k） | 重采样 + 归一化 + EQ + 音量 | 共享 |
| `Exclusive` | 发烧/bit-perfect | 源采样率 + 16/24bit 整数 | 默认直通；音量/归一化/EQ 需显式开启 | 独占 |
| `Auto` | 可选策略 | 先探测独占，失败回共享 | 按实际模式 | 自动 |
| `ExclusiveRequired` | 用户明确要求 | 独占；失败报错 | 按设置 | 独占或错误 |

**默认 `Shared`**：音乐播放器默认独占会静音掉用户的浏览器/游戏/会议，首次体验不可接受。设置里提供 **共享 / 自动 / 独占（失败时提示）**；`Auto` 也必须是用户显式选择。绝不静默把“独占”降级成“共享”而不告诉用户。

## 2. 解码

### 2.1 PCM 族

首选 **symphonia**（纯 Rust，MPL-2.0）：

- MP3、AAC（ADTS/MP4）、FLAC、ALAC、Vorbis、Opus、WAV。
- 输出统一到内部 `AudioBuffer`：优先保留整数样本（i16/i24/i32）用于 bit-perfect；需要 DSP 时转 f32。
- seek 用 `FormatReader::seek`；对 VBR MP3 的精度做实测，验收容差 ±50ms。

### 2.2 覆盖缺口

| 格式 | symphonia | 方案 |
|---|---|---|
| WMA | 不支持 | FFmpeg（LGPL 动态链接）；或明确不支持 |
| AIFF | 需验证 | FFmpeg；或 `aiff` crate |
| DFF/DST | 不支持 | FFmpeg（LGPL 动态链接） |
| DSF | 不支持 | FFmpeg 解码为 PCM；DoP 后续 |

> 不要在“支持格式列表”里写一个格式，却没有解码/播放验收。扫描支持 ≠ 能播。

### 2.3 解码线程

```
decode thread:
  read packet → decode → (optional DSP) → push to ring buffer
  seek: flush ring + decoder.seek + refill
  EOF: mark end; render thread drains then emits Ended
```

- 环形缓冲容量 `max(500ms, 4 × period)`。
- 低水位 250ms；低于则立即补，高于则睡 5ms。
- 解码线程不做实时等待；渲染线程永不阻塞。

## 3. WASAPI 输出

> 实现直接引入 `wasapi` crate **0.26.0**（HEnquist/wasapi-rs，MIT，已核实为当前最新）：共享/独占、事件/轮询、设备枚举、设备增删/默认变化通知、**独占模式格式探测**、`IAudioClock` 都有封装。**已拍板（2026-10-23）：不做底层 COM/`windows` crate 考据**，wasapi-rs 就是 WASAPI 边界；若真撞到库缺口，先查上游 issue/PR 或提补丁，确需直调 COM 再局部引入 `windows`，不作为计划项。版本在 P1 锁定，并把探针结果写成自检。
>
> **为什么不用 Rodio（ADR-0006）**：Rodio 的 Sink/Source 模型要求把数据流所有权交给引擎，与自有 DSP 链、位置时钟、独占模式直接冲突，且不支持 WASAPI 独占。CPAL 仅作未来跨平台共享输出占位（Linux/macOS），不进 v1。

### 3.1 初始化流程

```
enumerate endpoints
  → choose device (stored id / default / follow default)
  → get mix format (shared)
  → choose mode:
       Shared: Initialize(SHARED, mix_format)
       Exclusive:
         candidate = source_format (rate, channels=2, bits=16/24)
         IsFormatSupported(EXCLUSIVE, candidate)
         Initialize(EXCLUSIVE, EVENTCALLBACK, period = buffer, candidate)
  → SetEventHandle
  → Start
```

### 3.2 已实测的硬约束

| 约束 | 现象 | 处理 |
|---|---|---|
| `hnsPeriodicity` 必须等于 `hnsBufferDuration`（事件驱动） | 传 0 / 不等 → `0x88890016` / `0x88890013` | 两者用同一个值；默认 10ms |
| 帧数可能不对齐 | 44.1k/10ms=441 帧 → `0x88890019` | `GetBufferSize()` 拿对齐值（442），重算时长，**重新 Activate + Initialize** |
| 独占不支持 f32 | `0x88890008` | 输出 16/24bit 整数；内部 f32 混音后转换 |
| 32bit int 容器 | `E_INVALIDARG` | 不要传 32bit PCM；24bit 用 3 字节打包 |
| `GetCurrentPadding` 独占下恒等于 buffer | 不能判断可写空间 | 用“可写 = buffer - 已排队帧数”或事件驱动 + 固定周期写 |
| `GetBufferSizeLimits` 独占下 `OFFLOAD_MODE_ONLY` | 查不到周期区间 | 用设备周期 + 实测；不要依赖该 API |
| `IsFormatSupported(EXCLUSIVE)` 偏乐观 | 对 16/24bit 全采样率 S_OK，Initialize 才失败 | 以 Initialize 为准；失败分类 |
| 独占锁验证 | 第二个客户端 `0x8889000A DEVICE_IN_USE` | 自检里做“二次打开应失败”断言 |

### 3.3 缓冲与周期

- 默认 10ms；设备最小 3ms。**不要硬编码 3ms**，用 `GetDevicePeriod` 探测。
- 缓冲 = 周期（事件驱动），这是实测约束。
- 独占下 buffer 过小会增加欠载风险；音乐播放 10ms 足够，低延迟不是首要目标。
- 共享模式可用 `IAudioClient3` 的 default/low-latency period；v1 不必。

### 3.4 写数据

渲染线程每个事件：

```
wait event (timeout = 2 × period)
frames_writable = buffer_frames - frames_queued
take frames from ring
convert to output format (i16/i24/f32) + apply gain envelope
WriteToDevice
update frames_queued / position anchor
if ring empty → fill silence + underrun += 1
```

- 渲染线程不做分配、不加锁、不调用日志、不回调 JS。
- 音量/淡入淡出用每样本线性 ramp；不要在每个 buffer 边界突变。
- `AUDCLNT_E_DEVICE_INVALIDATED` / `RESOURCES_INVALIDATED` → 停止、上报、重建。

### 3.5 时钟与位置

**位置来自设备/写出的帧，不来自 wall clock。**

- 维护 `base_position_ms`（seek/load 后的起点）与 `frames_written`。
- `position = base + frames_written / sample_rate * 1000 - latency_ms`。
- 用 `IAudioClock::GetPosition` 做交叉校验；偏差 > 50ms 记 warning。
- seek：暂停 → flush ring → decoder seek → 重置 `base = target`、`frames_written = 0` → 预填 → 恢复。
- 位置事件 10Hz；`Ended` 在 ring 真正播完且 decoder EOF 后发一次。

### 3.6 暂停/恢复/释放

| 模式 | 暂停 | 恢复 |
|---|---|---|
| Shared | `Stop()`，保留 client | `Start()` + 重填 ring |
| Exclusive | 100ms 淡出 → `Stop()`；空闲 2s 后释放 client | 重新 Open + seek 到暂停位置 + 预填 + `Start()` |

独占暂停还长期占着设备会惹恼用户；释放是产品正确性，不是优化。释放/重开会有 100–300ms 延迟，UI 显示 `loading`。

### 3.7 设备与热插拔

- 枚举：`IMMDeviceEnumerator`；保存 endpoint ID（不是友好名）。
- 监听 `OnDefaultDeviceChanged` / `OnDeviceAdded` / `OnDeviceRemoved` / `OnDeviceStateChanged`。
- 策略：
  - `follow_default=true`：默认设备变化 → 淡出 → 关闭 → 新设备 → 从当前位置恢复。
  - 固定设备：设备消失 → `DeviceLost`，暂停，提示选择新设备。
- 蓝牙：独占通常不可用 → 按策略回共享或报错。
- 设备切换后采样率可能变；必须重建流并重新协商格式。

### 3.8 错误分类

```rust
pub enum AudioErrorCode {
    DeviceUnavailable,
    DeviceInUse,
    UnsupportedFormat,
    ExclusiveDenied,
    DeviceLost,
    DecodeFailed,
    SeekFailed,
    EndOfStream,
    Underrun,
    Internal,
}
```

每条错误带：`hresult`（原始）、`device_id`、`mode`、`requested_format`、`actual_format`、`retryable`。UI 只按 code 分支；详情进诊断页/日志。

## 4. DSP 链

```
decode → [resample if shared/rate] → [ReplayGain] → [EQ] → [volume/fade] → output
```

- **Bit-perfect 模式**：跳过 resample/ReplayGain/EQ；音量必须 1.0；输出整数；UI 显示“Bit-perfect”。
- **Shared 模式**：解码 → f32 → resample 到设备率 → ReplayGain（gain + peak 限幅）→ EQ → 主音量。
- **Exclusive 非 bit-perfect**：源率/位深，但允许 gain/EQ；此时不是 bit-perfect，UI 不要标。
- EQ：10 段 biquad + preamp；系数随采样率重算；DSD 禁用。
- 音量：每样本 ramp；静音淡出，避免爆音。
- 削波：ReplayGain 用 `min(gain, 1/peak)`；最终输出 clamp，但记录 clipping 计数。

## 5. 频谱与 PCM 数据链路（ADR-0006）

**在引擎内部 tap，不用系统回环。** 频谱只做动画效果，不做专业分析（无声学加权/标定）。

### 5.1 tap：订阅制"必要采样复制"

统一数据面（EQ、频谱、LRC、WASAPI 输出共用，见 §5.2）：

```
decode(symphonia) → [resample(共享模式)] → [ReplayGain] → [EQ] → [tap] → [volume/fade] → WASAPI
```

- **tap 点**：EQ 之后、主音量之前——频谱反映 EQ 效果，且低音量时仍有可视数据。
- **订阅制**：`SetSpectrum{enabled}` 控制；关闭时整条 tap 退化为一个布尔分支，零拷贝、零分配。
- **必要复制**：解码/DSP 线程把样本混音成**单声道 f32** 写入 SPSC 无锁环（容量 8192，2 的幂）；渲染线程完全不参与 tap。环满丢弃旧帧并计数，不影响音频。
- **分析线程**（普通优先级）：30Hz 消费滚动 4096 样本窗 → FFT 2048/4096 + Hann 窗 → 48 个对数分带（40Hz–16kHz）→ attack/release 时间平滑 → `u8[48]` 事件。
- 暂停/无声：不冻结，按 release 衰减到 0；seek 时环随解码 flush。
- **纯旁路复制，不修改样本**：独占/bit-perfect 模式下频谱照常可用，且不破坏 bit-perfect（破坏 bit-perfect 的只有 resample/ReplayGain/EQ/音量≠1，UI 已按 04 §6 提示）。
- 不用系统回环：独占时捕获不到本应用，且会混入系统声音。
- 前端只画；不传 PCM/FFT，只传分带能量。

### 5.2 EQ、LRC、WASAPI 联合设计

三项共享同一条数据面与位置时钟，接口一次定形、不随后续功能返工：

| 能力 | 链路位置 | 接口 | 交付 |
|---|---|---|---|
| EQ | tap 之前（频谱反映 EQ） | `SetEq{enabled, bands}`：10 段 biquad + preamp，系数随采样率重算，DSD 禁用 | 协议/链路位置 v1 定形，**实现后置** |
| LRC | 不在数据面——复用位置时钟 | 位置 = `base + frames_written / rate`（§3.5），10Hz 事件 + 前端 rAF 外推 | P4 |
| WASAPI 输出 | 链路终点 | `OpenDevice{device_id, mode, policy}`；Shared 默认，Exclusive 按 §3 分期 | v1（P2 Shared，P4/后续 Exclusive） |
| 频谱 | tap（§5.1） | `SetSpectrum` + `Spectrum{bands:[u8;48]}` 30fps | P4（链路与协议 P2 就位） |

> 关键约束：EQ 在解码/DSP 线程执行（非实时路径），音量 ramp 在渲染线程（§3.4）；tap 只在解码/DSP 线程写入，渲染线程保持"无分配、无锁、无日志"。

## 6. 播放语义

| 行为 | 实现 |
|---|---|
| 自动下一首 | 引擎发 `Ended`，Actor 决定 next 并 `Load`；引擎不自己切歌 |
| 单曲循环 | Actor 收到 `Ended` 后 `Load(same, 0)` + `Play`；或引擎 `LoopOne`（不推荐，状态机会分叉） |
| gapless | 同格式时预解码下一首；v1 可后置，但协议预留 `PrepareNext` |
| crossfade | 双 decoder + 双 gain；v1 可后置；协议预留 `Transition` |
| seek | flush + decoder seek + 预填；位置先报目标，再跟设备时钟 |
| 播放速率 | 变速不变调需要时间伸缩（WSOLA/相位声码器，如 `signalsmith-stretch`）；`rubato` 只做采样率转换，不能替代；v1 可先只支持 1.0，再补 |
| DSD | DoP：DSD64→24bit/176.4k，标记字节 0x05/0xFA；禁用 DSP；后续 |

> 速率在独占/bit-perfect 下会破坏 bit-perfect；UI 要明确：开变速即退出 bit-perfect。

## 7. 线程模型

| 线程 | 优先级 | 职责 |
|---|---|---|
| 控制线程 | 普通 | 收命令、开/关设备、状态机、事件发送 |
| 解码线程 | 普通 | 读包/解码/重采样/推 ring |
| 渲染线程 | MMCSS `Pro Audio` + `AVRT_PRIORITY_CRITICAL` | 等事件、取 ring、DSP、写设备 |
| 监控线程 | 低 | 设备通知、心跳、统计 |

- COM 在每个使用线程初始化（MTA/STA 按 API 要求）；不要跨线程传 COM 指针。
- 渲染线程失败不能 panic 穿过 FFI；捕获并上报，静音而不是崩溃。
- MMCSS 注册失败回落 `Audio` 类别，`stats.mmcss` 如实标记。

## 8. 协议摘要

完整定义见 [`02-target-architecture.md`](02-target-architecture.md) §7。音频侧只关心：

```rust
pub enum AudioCommand {
    OpenDevice { device_id: Option<String>, mode: OutputMode, policy: FallbackPolicy },
    Load { generation: u64, path: PathBuf, format: FormatHint, start_ms: u64, autoplay: bool, gain_db: Option<f32>, rate: f32 },
    Play, Pause, Seek { ms: u64 }, Stop,
    SetVolume { value: f32, muted: bool },
    SetRate { value: f32 },
    SetEq { enabled: bool, bands: Vec<EqBand> },
    SetSpectrum { enabled: bool, bands: u8, fps: u8 },
    Shutdown,
}
```

- `Load` 带绝对路径（host 解析，sidecar 不查库）。
- `generation` 由 host 生成；所有事件回带。
- `format` 是 hint；引擎以实际解码/设备协商为准，回报 `actual_format`。

## 9. 自检与验收

### 9.1 无设备测试（CI）

- 解码 fixtures：每种格式解码到 PCM，SHA-256 与 golden 比对。
- seek：目标 ±50ms 内，连续 20 次随机 seek 不崩。
- ring：慢消费者/快生产者注入，验证 underrun 计数与静音填充。
- 格式协商：用 fake device 返回各种 HRESULT，验证重试/回退/错误映射。
- 协议：golden JSON 双向解析；乱序/重复/未知字段。

### 9.2 硬件测试（手动/`#[ignore]`）

| 用例 | 断言 |
|---|---|
| 枚举设备 | 至少一个 active render endpoint |
| 共享打开 | 44.1/48/96k 源均能播 |
| 独占 44.1k/16 | `S_OK`；第二客户端打开同设备失败 `DEVICE_IN_USE` |
| 独占 48k/24 | `S_OK`；事件频率约 100/s @10ms |
| 独占 f32 | `UNSUPPORTED_FORMAT`，不崩溃 |
| 帧不对齐 | 44.1k/10ms 走 `GetBufferSize` 恢复路径 |
| 设备拔出 | `DeviceLost`，暂停，重插后可恢复 |
| 默认设备切换 | 按策略跟随/提示 |
| 欠载 | 人为阻塞解码，`underruns` 增加且输出静音不爆音 |
| 长播 | 2h 无漂移、无句柄泄漏、内存稳态 |

### 9.3 诊断指标

`AudioStats` 至少包含：

```
device_id, device_name, mode, exclusive, sample_rate, bits, channels,
period_ms, buffer_ms, event_count, underruns, mmcss, xruns,
decode_format, decoder, resampler, bit_perfect, fallback_reason,
position_ms, latency_ms, last_hresult
```

诊断页展示这些字段，比“有没有声音”更能定位问题。

## 10. 实施顺序

1. `NullSink` + 解码 + ring + 时钟（无 WASAPI，CI 可跑）。
2. WASAPI Shared + 设备枚举 + 事件驱动。
3. PlayerActor 接入 + 位置/结束/错误事件。
4. Exclusive + 格式协商 + 回退策略 + 自检。
5. 设备热插拔 + 暂停释放 + 恢复。
6. ReplayGain/EQ/频谱 tap。
7. 速率/gapless/crossfade（按产品优先级）。
8. DSD/DoP（可选）。

## 11. 不要做的事

- 不要用 Chromium 的 `--enable-exclusive-audio` 冒充独占（进程级、固定格式、静默回落）。
- 不要用系统回环做频谱（独占时不可用、混入系统声音）。
- 不要在渲染线程里分配/加锁/打日志/回调 JS。
- 不要把 PCM 放进 WebSocket/JSON IPC。
- 不要把“独占失败”静默降级成共享。
- 不要在没有硬件自检的情况下宣称 bit-perfect。
- 不要让音频 sidecar 打开 SQLite 或依赖 Tauri 类型。