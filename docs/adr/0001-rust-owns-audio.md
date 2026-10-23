# ADR-0001：Rust 拥有音频输出，WebView 不再出声

- 状态：Accepted
- 日期：2026-10-23

## 背景

现版由渲染层 `<audio>` 出声，主进程 JS 拥有播放状态机。这个组合在 demo 阶段可行，但已经在以下场景暴露问题：

- 窗口重载/关闭会中断声音，需要 `did-finish-load` 补发 load。
- Web Audio 建图后跨源媒体静音，必须补 CORS；图的生命周期/资源释放要手工管理。
- `playbackRate` 换源后被重置，必须重放。
- Chromium 无法按源采样率开流、无法证明 WASAPI 独占、失败静默回落。
- MediaSession → SMTC 在 WebView2 不直通。
- 设备/格式/缓冲/欠载不可观测。

## 决策

音频输出、解码、DSP、设备枚举、播放时钟全部由 Rust 负责。WebView 不创建 `<audio>`、`AudioContext`、`MediaElementSource`，不接触音频文件路径。

## 后果

正面：

- 播放与 UI 生命周期解耦；关窗/重载/崩溃不中断声音。
- 可以做 WASAPI 共享/独占、按源格式、bit-perfect、设备热插拔、结构化诊断。
- SMTC、托盘、频谱可以在后端统一。
- 解码/音频错误可用原始 HRESULT/设备/格式表达。

负面：

- 需要重写 `audio-sink.ts`、`media://` 音频协议、Web Audio 图、`audiomotion` 接入。
- 速率/时间伸缩、EQ、DSD 都要在 Rust 实现。
- 需要一套音频测试基础设施（NullSink/FileSink/硬件自检）。

## 否决的替代方案

1. **继续用 Chromium `<audio>`，只修修补补**：无法满足独占/bit-perfect/设备诊断；补丁会继续累积。
2. **用 `--enable-exclusive-audio`**：进程级、固定格式、失败静默回落，不可接受。
3. **用 BASS/闭源引擎**：能快速拿到功能，但授权/绑定/与 `media://` 的整合成本高，且不可控。
4. **WebSocket 把 PCM 推给前端播放**：延迟/抖动/设备时钟/独占都不可控。