# src-tauri/（规划）

Tauri 2 宿主只做适配与系统集成：

- commands/events 薄封装，调用 `aiya-app` 用例；
- 窗口/托盘/关闭到托盘/单实例/窗口状态；
- `aiya-artwork://` 协议；
- SMTC（隐藏顶层窗口 + WinRT）；
- audio sidecar supervisor（v1 原生 `aiya-audio-host`，ADR-0006；启动/握手/心跳/重启）；
- beets bridge（spawn Python sidecar，stdio JSON；只在用户启用时启动）；
- 日志、panic hook、诊断指标导出。

不在这里写 SQL、解码、队列规则。具体边界见 [`../docs/03-backend-boundary.md`](../docs/03-backend-boundary.md)。