# ADR-0006：原生音频引擎首发，PCM 链路内建"必要采样复制" tap

- 状态：Accepted
- 日期：2026-10-23
- 修订关系：修订 ADR-0001 中"v1 默认 mpv sidecar"的**实现选择**；ADR-0001 的边界决策（后端拥有音频、WebView 不出声）不变。ADR-0002（独立进程）不变。

## 背景

2026-10-23 底盘规划对齐（[10](../10-plan-alignment.md)）确认三项要求：

1. **频谱是必要体验**，动画级即可、无需专业分析——但必须在引擎数据链路内有 PCM tap；mpv 无 PCM tap API，做不到。
2. **EQ、LRC、WASAPI 输出必须与 PCM 数据链路联合设计**，接口一次定形，不随后续功能返工。
3. 音频采用**成熟 Rust 原生组件**（Symphonia / wasapi-rs），不走"借外部引擎"路线。

## 决策

1. v1 引擎 = **Symphonia 解码 + `wasapi` crate 输出**（Shared 先行，Exclusive 按 04 §3 分期），以 `aiya-audio-host` sidecar 进程交付。
2. DSP 链统一为：`decode → [resample(共享)] → [ReplayGain] → [EQ] → [tap] → [volume/fade] → WASAPI`。tap 在 EQ 之后、主音量之前。
3. tap 是**订阅制必要采样复制**：仅在 UI 开启频谱时生效；解码/DSP 线程把单声道混音样本写入 SPSC 无锁环；分析线程 30Hz 消费（FFT → 48 对数分带 → `u8[48]` 事件）；关闭时整条 tap 退化为一个布尔分支，零拷贝零分配。tap 是纯旁路复制，不修改样本，**不破坏 bit-perfect**。
4. EQ 的协议（`SetEq`）与链路位置本版定形，实现交付后置；LRC 复用同一位置时钟（`base + frames_written`，`AudioClock` 交叉校验），不进数据面。
5. **不采用 Rodio**：Sink/Source 模型与自有 DSP 链/位置时钟/独占模式冲突，且无独占支持。CPAL 仅作未来跨平台共享输出占位。mpv 降为 spike 对照与逃生实现，不进发布包。

## 后果

正面：

- v1 即有链内频谱（compact）；EQ 原生路径就绪；bit-perfect/独占可证明、可自检。
- 不携带 ~50MB mpv 二进制；许可面收窄（无 GPLv2+ 分发义务）。
- 音频行为完全自控：时钟、seek 精度、错误分类、gapless 演进都不受外部引擎约束。

负面：

- P2 由 1 周 → 2 周，总工期 4–6 周 → 5–7 周。
- 解码/时钟/gapless 全部自担；WMA/DFF 等格式缺口按 04 §2.2 处理（FFmpeg 后补或明示不支持）。
- 需要完整的音频测试基建（NullSink/FileSink/硬件自检，04 §9）。

## 否决的替代方案

1. **mpv sidecar 首发**（原 09 号推荐）：无 PCM tap，频谱/EQ 联合设计不可达；被本 ADR 取代，保留为逃生路径。
2. **mpv 出声 + 独立分析进程并行解码**：双解码、双时钟，动画与出声不同步，浪费一路 CPU。
3. **Rodio/CPAL 全家桶**：无法独占、模型冲突（见决策 5）。
