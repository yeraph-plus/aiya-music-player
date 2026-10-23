# ADR-0005：只为封面保留自定义协议，音频不再走 media://

- 状态：Accepted
- 日期：2026-10-23

## 背景

现版 `media://` 同时服务音频与封面：Range、流计数、CORS、缓存头都是为 `<audio>` 和 Web Audio 图服务的。音频迁到 Rust 后，这些逻辑不再需要；但封面仍要在 WebView 里显示。

## 决策

- 删除 `media://track/*` 音频协议、Range/流计数/CORS 音频路径。
- 保留一个仅用于封面的自定义协议 `aiya-artwork`，路径 `/artwork/{id}?size=96|256|512`；Windows 运行时 URL 是 `http://aiya-artwork.localhost/...`（Tauri 平台差异，见 `07`）。
- 协议只接受 artworkId，不接受任意路径；后端解析缓存并返回 immutable 响应。
- 封面响应带 `Access-Control-Allow-Origin: *`，以便前端 canvas 取主色。

## 后果

正面：

- 去掉一整层只为 WebView 播放服务的协议代码与生命周期计数。
- 不向 UI 暴露文件路径；安全边界更小。
- 封面缓存/尺寸白名单集中在一个协议。

负面：

- 如果未来需要远程音频/视频流，需要另设计 `aiya-stream://`（带 Range），不能复用旧的音频协议假设。
- 前端取主色仍需 CORS 正确。

## 约束

- 协议 handler 不查业务表以外的数据；只按 ID 找缓存。
- 不缓存未命中/错误响应；404 不落盘。
- 尺寸白名单与现版 `ARTWORK_SIZES` 一致。
- 音频文件路径只存在于 host 与 sidecar 的 `Load` 命令中。