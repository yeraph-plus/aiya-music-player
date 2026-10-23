# 06 — 需要拍板的问题

> 这些不是“实现细节”，而是会影响仓库结构/依赖/工期的分叉。每项给出建议与代价，评审时逐条确认。

## 1. 音频是否独立进程？

**建议：是，v1 就用 sidecar。**

- 代价：多一个 exe、命名管道、supervisor、打包签名；估时 +3–5 天。
- 收益：崩溃隔离、独立重启/自检、实时线程不受 UI/DB 影响。
- 备选：同进程线程，代码量少，但音频崩溃会带走整个应用；`AudioPort` 保留未来切换能力。

## 2. 默认输出模式？

**建议：默认 `Shared`，把 `Auto` / `Exclusive` 作为用户显式选项。**

- 默认独占会静音浏览器/游戏/会议，首次体验不可接受；蓝牙设备也常不支持独占。
- `Shared` 最稳；发烧用户可主动切 `Auto` 或 `Exclusive`。
- 回退必须可见：“已回退共享，原因：设备不支持 44.1k/24bit 独占”。

## 3. 是否接受 FFmpeg 依赖？

**建议：v1 不默认捆绑；先明确支持矩阵。**

- symphonia 覆盖 MP3/AAC/FLAC/ALAC/Vorbis/Opus/WAV 等主流格式。
- WMA/AIFF/DFF 需要 FFmpeg 或额外 crate；FFmpeg 是 LGPL，动态链接/分发要合规。
- 如果曲库里 WMA/AIFF 占比很低，v1 可标注“暂不支持”，后续加可选 FFmpeg 包。
- 不要在 UI 里宣称支持一个实际不能播的格式。

## 4. v1 parity 范围？

**建议：先不做 EQ / DSD / crossfade / gapless。**

- 这四项都会显著增加音频引擎复杂度与测试矩阵。
- 先保证 Shared/Exclusive、seek、设备、恢复、SMTC、频谱、歌词、播放列表。
- 协议预留 `SetEq` / `Transition` / `PrepareNext` / DSD 格式，但不在 v1 实现。

## 5. UI 是否继续 Svelte 5？

**建议：继续。**

- 现版 renderer 5,500 行、纯逻辑测试充分；换框架的收益低、风险高。
- 只做 API 层替换（`window.api` → generated bindings）与 App 拆分。
- 如果未来要换 React/Vue，`lib/` 纯函数与视觉可复用，但这是独立决策。

## 6. 旧库如何切？

**建议：默认用副本库（`library-v2.sqlite`），验证稳定后再决定是否原地切换。**

- 旧版迁移 runner 遇到 `user_version` 高于自己支持的版本会直接报错（`db.ts:78-84`），所以原地升到 v5 后旧版无法回滚。
- 副本策略：旧版继续用原库；新版在副本上迁移；回滚只是切回旧版。
- 若确定原地迁移：必须先备份，回滚只能“停新版 + 恢复备份”，并做一次演练。

## 7. 安装包/更新/签名怎么解决？

**建议：v1 先做未签名内测包；公开发布前买 Authenticode 证书。**

- host 与 sidecar 都要签，否则 SmartScreen/杀软会拦。
- Tauri updater 需要签名密钥与更新源；可以先手动分发，后接 updater。
- WebView2 运行时：Win10/11 通常已有；安装器带 bootstrapper 或检测提示。

## 8. “音频 WS”到底指什么？

当前仓库没有 WebSocket 音频链路。本文按两种理解处理：

- 如果指 **Web Audio / Chromium 音频链路**：新版删除，音频进 Rust（ADR-0001）。
- 如果指 **WebSocket 传输**：不要传 PCM；控制面可以，但 terminate 在 host，不要让 UI 直连。若你有具体的 WS 服务/协议，需要单独给出消息格式与用途，再决定是否保留。

## 9. Pro 频谱/audioMotion 是否保留？

**建议：v1 只做后端 bands + 自研 compact/pro 绘制；不绑定 audioMotion。**

- audioMotion 依赖 `AnalyserNode`，与“音频在 Rust”冲突。
- AGPL 对闭源分发有约束。
- 后端已经能提供 48 bands；自研 canvas 绘制足够，后续可做更多形态。

## 10. 是否需要多窗口？

**建议：v1 单主窗口 + 隐藏 SMTC 窗口。**

- 多窗口会让“哪个窗口是活跃出口/哪个窗口显示队列”重新复杂化。
- 后端权威状态已经支持多窗口读取；但先不做多窗口控制，避免重复现版补丁。

## 11. 播放队列上限？

**建议：统一 50,000，和后端 `list_all_ids` 上限一致。**

- 现版 IPC 入参 20,000、内部可 50,000，是暗差异。
- 统一后，UI 在入队前就能提示；不会“能选但一操作就报错”。

## 12. 是否保留“最近/常听”作为主视图？

**建议：保留。**

- 现版 `TrackQuery.source` 四来源共用分页协议，设计好。
- 新版继续用 source 维度，不要做成排序维度。