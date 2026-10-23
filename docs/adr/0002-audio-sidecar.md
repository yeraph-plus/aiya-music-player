# ADR-0002：音频助手独立进程，命名管道通信

- 状态：Accepted（"v1 可用 mpv.exe"的实现选择已被 [ADR-0006](0006-native-engine-first.md) 修订为原生引擎；独立进程决策不变）
- 日期：2026-10-23

## 背景

音频引擎可以直接跑在 Tauri host 的线程里，也可以拆成独立进程。现版音频问题排障成本高，且解码器/COM/驱动异常可能拖垮整个应用。

## 决策

音频以独立进程运行，由 Tauri host 启动/监控/重启。v1 可以直接把 `mpv.exe` 当 sidecar（`--input-ipc-server` 命名管道 + JSON IPC）；B 方案用 `aiya-audio-host` 包原生引擎。两种都只传命令、状态、错误、频谱 band，不传 PCM。

音频 sidecar 的协议 v1 用 JSON；后续可换长度前缀 MessagePack/共享内存。`mpv` 的命名管道/JSON 由 supervisor 适配成同一 `AudioPort`，UI 不感知。

## 后果

正面：

- 崩溃隔离：sidecar 崩溃只影响音频，host 可重启并恢复播放位置。
- 实时线程不受 Tauri runtime/DB/UI 影响。
- 可单独运行 `--selftest`、单独看日志、单独诊断设备。
- 未来可以把 sidecar 降级为 in-process 线程，协议不变。

负面：

- 多一个 exe、supervisor、握手/心跳/重连逻辑。
- 打包/签名/杀软兼容需要额外工作。
- 控制命令多一跳（毫秒级），但播放命令频率低，可接受。

## 协议约束

- `PROTOCOL_VERSION` 握手；不兼容拒绝启动，不静默降级。
- 控制命令有界队列不丢；`Position`/`Spectrum` 可丢旧帧。
- 2s 心跳，5s 超时 → kill + restart。
- sidecar 不打开 SQLite，不依赖 Tauri 类型。

## 否决的替代方案

1. **同进程线程**：简单，但崩溃隔离/独立诊断/重启能力都没有；作为未来降级选项保留。
2. **N-API 模块**：仍与 host 同进程；崩溃拖垮应用；跨平台构建复杂。
3. **stdio JSONL**：可行，但日志与协议混流、重启/多通道管理弱；命名管道更干净。