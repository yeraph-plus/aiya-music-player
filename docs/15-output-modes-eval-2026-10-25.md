# 15 · WASAPI 实现质量评估与输出模式设置设计（2026-10-25）

> 触发问题：当前 WASAPI 实现和主流播放器比如何？能否把「共享/独占 × 事件/轮询」四种组合
> 都做成设置，不启用 WASAPI 时走 Tauri 原生后端？
>
> 先给结论：**四种组合的提议方向是对的，但暴露的维度选错了；"Tauri 原生后端"这条会
> 复活第二条音频路径，不建议做。** 而在测量过程中发现当前独占模式实际上是**静音的**，
> 已修，证据与修法都记在 §2。

---

## 1. 测量方法

新增 `crates/aiya-audio/examples/output_matrix.rs`（`npm run probe:matrix`），对每种组合：

1. 真实 `Initialize` + `Start`，连续写 2 秒静音；
2. 记录缓冲区帧数、设备周期、`GetCurrentPadding()` 全程范围、事件间隔 p99；
3. 判据 `lag = 已写帧数 − 墙钟 × 采样率`：
   * 健康流 ≈ 0..buffer（领先设备一个缓冲深度），且**不随时间漂移**；
   * 持续变负 = 设备在插静音，即"放不出声"。

三种写入策略对照：

| 策略 | 做法 |
|---|---|
| `padding` | 引擎当时的做法：`可写 = buffer − GetCurrentPadding()` |
| `accounted` | 自己记账：`可写 = 已消费 + buffer − 已写`，不问 padding |
| `exact` | 事件驱动独占的标准做法：每个事件写满一个缓冲 |

本机：`扬声器 (Realtek(R) Audio)`，mix format 48 kHz / 2 ch / 32 bit，设备周期默认 10 ms、最小 3 ms。

---

## 2. 关键发现：独占模式此前是静音的（P0，已修）

### 2.1 数据

48 kHz（正是 `smoke:mode` 用的采样率），每栏 2 秒：

```
组合 / 策略                          格式        buf    pad范围      lag 首→末    事件p99  结论
excl+events(默认周期) / padding   i24-in-32    480   480-480     -107→-96046    10ms   欠载
excl+events(20ms)     / padding   i24-in-32    960   960-960     -135→-96056    20ms   欠载
excl+events(20ms)     / accounted i24-in-32    960   960-960      851→-94891  2014ms   欠载
excl+events(20ms)     / exact     i24-in-32    960   960-960      849→904       20ms   正常
excl+polling(200ms)   / accounted i24-in-32   9728  4640-4779   9727→9727         -    正常
excl+polling(200ms)   / padding   i24-in-32   9728    0-9728   9351→-86432        -    欠载
```

`lag` 掉到 −96046 = 2 秒 × 48000 帧全部没写。换 44.1 kHz 复现同样结论（−89003 / 2 秒 × 44100）。

### 2.2 机理

独占 **事件驱动** 下 `GetCurrentPadding()` 恒等于 `buffer`：`buffer − padding` 恒为 0，
引擎只在首轮写过一缓冲，之后 `if available == 0 { continue; }` 一路空转，设备持续欠载。
事件照常每 10–20 ms 触发，`AudioMode { exclusive: true }` 照常上报——**除了拿不到声音，
一切指标都正常**。

这不是本机驱动的怪癖，wasapi-rs 自己也承认：

```rust
// wasapi-0.26.0/src/api.rs:1341
pub fn get_available_space_in_frames(&self) -> WasapiRes<u32> {
    let frames = match (self.sharemode, self.timingmode) {
        (Some(ShareMode::Exclusive), Some(TimingMode::Events)) => {
            // 直接返回整个 buffer，不查 padding
            buffer_frame_count
        }
        ...
```

docs/04 第 82 行早就写了「**GetCurrentPadding 独占下恒等于 buffer**，不能判断可写空间」，
但实现没照做。冒烟测试只断言了 `AudioMode` 事件，于是绿灯放行——典型的"假绿"：
**"引擎报告在播"和"声音真的在走"是两件不同的事**。

顺带一条反直觉结论：`accounted`（预填一整个缓冲再 start）在本机让事件**再也不触发**
（p99 2014 ms = 只等到了超时）。所以 wasapi-rs 文档里"先填满再 start"的建议在独占事件
驱动下未必适用；实际可用的是「空缓冲 start + 每个事件写满一缓冲」。

### 2.3 修法（已提交）

1. `writable_frames(mode, buffer_frames, padding)` 统一裁决可写空间：
   * `Shared` → `buffer − padding`（实测 padding 正常跟随）；
   * `Exclusive` → `buffer`（每个事件写满一缓冲，buffer 恒等于 period）。
2. **周期逐个试**：设备自报的"默认周期"不可信。实测 44.1 kHz 下默认 10 ms 直接
   `0x88890019`（`AUDCLNT_E_INVALID_DEVICE_PERIOD`），20 ms 才成功；同一设备 48 kHz 下
   10 ms 反而可用。现在按 `[默认, 默认×2, 50ms]` 逐个 `Initialize`，取第一个成功值。
   不修这条，44.1 kHz（绝大多数音源）在本机永远进不了独占。
3. **EOF 排空判据**：独占下 `padding == 0` 永不成立，原来每曲白等满 500 ms 才报 EOF。
   改用"在途 = 一整个缓冲"的时长估算。
4. **写入帧计数** `Engine::written_frames()`：`smoke:mode` 新增断言——1 秒内写入帧数
   ≥ 70% 采样率，否则直接失败。这是唯一能证伪静音的指标，诊断页后续也要用它。

修后实测：`独占：拿到 ✓ / 写入速率 48480 帧/秒（期望 ≈48000）/ 切回共享 OK / 再切独占 OK`。

---

## 3. 与主流播放器的差距（按维度）

参照对象：foobar2000（WASAPI exclusive 的事实标准）、JRiver、Roon、Audirvana、MusicBee、mpv。

| 维度 | 现状 | 主流 | 差距 |
|---|---|---|---|
| 进程隔离（音频独立进程 + 看门狗） | 有（ADR-0002） | 无，全是单进程 | **领先** |
| 独占格式协商 | f32→i24-in-32→i16 降级 + 周期枚举 + 失败原因上报 | foobar 也是探测后降级 | 持平 |
| 失败可解释性 | `AudioMode.reason` 文本直达 UI | 多数只弹窗/静默回落 | **领先** |
| 设备失效恢复 | 指数退避 5 次重建，位置连续 | 多数会断流 | 持平偏好 |
| 共享模式重采样 | 交给 Windows `AUTOCONVERTPCM` | foobar 用 SoX/PPHS，Audirvana 自研 | **落后**（音质上限受系统 SRC 限制） |
| 位深下降抖动 | `round` + clamp，无 dither | foobar/JRiver 有 TPDF dither | 略落后（−90 dBFS 量化噪声，实测不可闻） |
| 位置时钟 | 解码时间戳 − padding | `IAudioClock` 为主时钟（实测本机可用，比值 0.9978–1.0004） | **落后**（长播漂移、seek 对齐） |
| 音频线程优先级 | 无 MMCSS / `AvSetMmThreadCharacteristics("Pro Audio")`，无 `timeBeginPeriod` | Roon/Audirvana/JRiver 都做 | **落后**（爆音/咔哒的首要外部因素） |
| 解码与渲染 | 同线程（Symphonia + 文件 IO 在音频循环内） | 解码线程 + 环形缓冲 | **落后**（大 FLAC/网络盘可能卡顿） |
| 无缝播放 | 无（EOF → 停流 → 下曲重开） | 全都有 | **落后**（专辑连听必需） |
| 静音切换（pause/seek/stop 去咔哒） | 只有音量 ramp；seek 直接 `stop_stream` 硬切 | 有 5–10 ms declick 淡出 | **落后**（可闻） |
| 设备选择 | 只用默认设备 | 可选设备、跟随默认、按设备记忆 | **落后** |
| EQ / ReplayGain | 未实现（协议已留位，归一化只有 gain 缩放） | 全都有 | 待做 |
| 独占暂停时释放设备 | 未做（暂停仍占设备，自家 docs/04 第 129 行明确要求释放） | Audirvana/JRiver 会释放 | **落后且违反自家规约** |

**综合定位**：

* 默认路径（共享）**能用，工程健壮性在中位偏上**，但音质上限受系统 SRC 限制；
* 独占路径**刚刚做到"能出声且 bit-perfect 条件成立"**（修前是坏的），纯直通部分与
  foobar 持平，缺 SRC 意味着采样率不匹配时只能回落共享；
* 发烧向能力（SRC / dither / 精密时钟 / 线程优先级）大约只有主流发烧播放器的 **50–60%**；
* 产品向能力（可解释性、进程隔离、恢复）**领先**。

一句话：**"可靠性像正规军，发烧性像半成品。"**

---

## 4. 对"四种组合都做成设置"的评估

### 4.1 四种组合是真实存在的，但不等价

| 组合 | 实测结论 | 适合 |
|---|---|---|
| 共享 + 事件 | 稳定（lag 稳定在 buffer 深度，事件 p99 34 ms） | **默认** |
| 共享 + 轮询 | 可用，无优势 | 事件失效驱动的逃生 |
| 独占 + 事件 | buffer 被锁死等于 period（10–20 ms），抗欠载最弱；本机 padding 不可信，必须换写策略 | 低延迟 / purist |
| 独占 + 轮询 | buffer 可独立设 200 ms，实测最稳（lag 9727→9727 不漂移，padding 也正常） | **独占更稳的选择** |

关键点：**真正决定抗爆音能力的是缓冲深度，不是"事件还是轮询"**。独占事件驱动受
`buffer == period` 约束，等于把缓冲深度和延迟绑死；独占轮询能把 200 ms 缓冲配 10 ms 周期。

所以把四个组合平铺给用户，本质是让用户替我们跑 4 倍兼容性矩阵，而用户能理解的维度是
"独占会不会抢设备 / 延迟多大"，不是"事件还是轮询"。

### 4.2 建议的设置模型

```
输出后端   WASAPI（默认）              ← 不做第二套出声路径（见 §5）
输出模式   共享（默认） / 独占（失败回落） / 独占（必须，失败报错）
缓冲       自动（共享 200ms，独占跟随设备周期） / 10 / 50 / 200 ms
时钟       ‹高级› 自动（按模式选） / 事件 / 轮询          ← 默认折叠，诊断用
```

* 三个模式对应 docs/04 已有的 `Shared / Auto / ExclusiveRequired` 语义，别退化成布尔开关；
* 缓冲是真正影响体验的旋钮，值得露出来；
* 时钟方式保留但折叠进"高级/诊断"，出问题时一键切换验证——这才是四组合的真实价值。

若坚持四组合全暴露，**三个前置条件**：每种组合都有写入速率冒烟断言、切组合不打断播放、
诊断页显示实际生效的组合+缓冲+周期。工作量约 1–2 天，收益主要是驱动兼容逃生。

---

## 5. 关于"不开启 WASAPI 就走 Tauri 原生后端"

Tauri 本身没有音频 API，"原生后端"有三种解释，代价差一个数量级：

| 方案 | 实质 | 代价 |
|---|---|---|
| a. WebView `<audio>` | Electron 版的老路 | 第二条音频路径（ADR-0003 状态双源冲突）、无链内 tap（频谱要二次解码或系统回环）、无独占、无 EQ、seek 精度退化 |
| b. rodio / cpal | 跨平台抽象 | ADR-0006 已否决（无独占、Sink 模型与自有 DSP 链冲突）；Windows 上 cpal 底层还是 WASAPI，等于换个名字丢掉格式协商 |
| c. 软件输出（NullSink 静音时钟） | 设备彻底不可用时 UI/进度/歌词照常，只是不出声 | 小，几十行 |

建议 **c**：WASAPI 初始化失败不必有第二套出声路径，主流播放器也没有（foobar2000 无设备时
只是报错）。真要逃生，ADR 里保留的 mpv 路径比 WebView 更靠谱。

---

## 6. 下一步优先级（按"能听见的差异"排序）

1. **MMCSS Pro Audio 线程优先级 + 大缓冲**（独占尤其）——爆音/咔哒的第一大外部因素；
2. **pause/seek/stop 的 declick 淡出**（5–10 ms）——当前硬切，可闻；
3. **独占暂停释放设备**——违反自家 docs/04 §6；
4. **gapless**（预开下一首 decoder，连续写流）——专辑连听；
5. **IAudioClock 做主时钟**（已实测可用）；
6. 设备选择 + 跟随默认；
7. dither（f32→i16 时）与自建 SRC（后者是大工程，可与 EQ 一起做）。

---

## 7. 本批改动清单

* `crates/aiya-audio/src/engine.rs`：`writable_frames()`（含 2 条单测）、周期枚举、
  EOF 排空判据、`Engine::written_frames()` 计数器；
* `crates/aiya-audio/examples/output_matrix.rs`（新）：四组合 × 三策略实测；
* `crates/aiya-audio/examples/mode_smoke.rs`：新增写入速率断言、测试音延长到 8 s；
* `package.json`：`npm run probe:matrix`；
* 测试：`cargo test --workspace` 142 passed / 1 ignored（perf_50k）；
  `npm run smoke:mode` 三路径全通且写入速率 48480 帧/秒。
