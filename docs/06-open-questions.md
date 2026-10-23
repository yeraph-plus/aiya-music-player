# 06 — 需要拍板的问题

> 这版只留 9 个真正影响结构/工期的分叉。每项给出建议与代价；没有旧库兼容问题。

## 1. 音频引擎：mpv sidecar 还是 wasapi-rs 原生？

**建议：v1 用 mpv sidecar；wasapi-rs 原生作为 v2 / B 方案。**

- mpv：3–5 天；独占、设备、全格式、gapless、ReplayGain 都有；GPLv2+ 与项目 GPLv3 兼容。代价：+~50MB、行为受 mpv 约束、bit-perfect 不保证。
- wasapi-rs：2–3 周；完全控制、二进制小；`wasapi` 0.26 已支持独占/格式探测/设备通知。代价：解码/时钟/gapless 自己扛。
- 你的重点是 UI/beets，**先买音频，后补控制**。
- 详见 [`09-audio-engine-choice.md`](09-audio-engine-choice.md)。

## 2. UI 是原样搬，还是趁重写？

**建议：原样搬 + 薄兼容层，不重写。**

- 旧 UI 有 54 个 `window.api.*` 调用点、48 个方法；写 `compat/api.ts` 即可。
- 需要换的只有：`AudioSink`（远程出口）、`SpectrumSource`（compact）、`artworkUrl`（scheme）、`initialLook`（3 行）。
- `.svelte` 文件最多改 1 个（频谱 pro 条件）。
- 等 beets 审阅界面需要新结构时，再按 feature 重写；现在推倒重来是负收益。
- 详见 [`07-ui-reuse.md`](07-ui-reuse.md)。

## 3. beets 是“依赖用户安装”，还是“随应用捆绑”？

**建议：v1 依赖用户已安装的 `beet`（pipx/uv/系统 Python），检测 + 指引；v2 再考虑捆绑 Python。**

- 依赖用户安装：工期最短、体积最小；缺点是新用户要多一步。
- 捆绑 `python-build-standalone` + beets：零安装；体积 +40–80MB，打包/签名/升级更复杂。
- 无论哪种，都锁定 beets 2.14.x；sidecar 独立进程，崩溃不影响播放。
- 详见 [`08-beets.md`](08-beets.md)。

## 4. beets 与自有库，谁是主？

**建议：自有 DB 是“读模型”，beets 是“写权威”。**

- 自有 DB 存播放/队列/历史/播放列表/封面；beets 不在也能跑。
- 标签匹配、写标签、移动/复制由 beets sidecar 做；完成后宿主刷新受影响项。
- 不直接让 UI 读 beets DB，也不在宿主复刻 autotag。
- 可选存 `beets_item_id`，便于同步；不要求唯一。

## 5. 导入默认动作：只写标签，还是复制/移动？

**建议：默认只写标签，复制/移动必须显式勾选。**

- 预览阶段必须无副作用；不要在预览时写文件。
- 复制/移动涉及路径/冲突/回滚，v1 先不默认做。
- 目标路径冲突时报错，不覆盖；失败项可重试。
- 不做 `beet import -q` 静默自动选择；低置信度必须人工确认。

## 6. 默认输出模式？

**建议：默认 `Shared`，`Auto` / `Exclusive` 作为显式选项。**

- 默认独占会静音浏览器/游戏/会议；蓝牙设备也常不支持。
- 回退必须可见：“已回退共享，原因：设备/格式不支持”。
- 音频引擎若用 mpv：`--audio-exclusive=yes` + `--audio-fallback-to-null=no`，失败要能报出来。

## 7. v1 是否砍掉这些？

**建议：砍掉 EQ / DSD / crossfade / watcher / 持久 jobs / 频谱 pro / 多窗口。**

- 它们都不服务“UI 体验 + beets”这个目标。
- 频谱 v1 可只做 compact，甚至先不做；不要为了它把 Web Audio 搬回前端。
- 旧库迁移/旧 `media://`/旧 preload：**直接删除**，不需要讨论。
- 如果某项是你的核心体验，单独提，我们重排优先级。

## 8. “音频 WS”到底指什么？

当前仓库没有 WebSocket 音频链路。两种理解：

- 如果指 **Web Audio / Chromium 音频链路**：新版删除，音频进 sidecar（ADR-0001）。
- 如果指 **WebSocket 传输**：不要传 PCM；控制面可以，但 terminate 在 host，不要让 UI 直连。若有具体 WS 服务/协议，给消息格式与用途再定。

## 9. 许可/分发怎么处理？

**建议：项目 GPLv3；mpv/FFmpeg/beets 的 notices 与源码提供方式从第一天就记录。**

- mpv GPLv2+ 与 GPLv3 兼容；分发附许可/源码获取方式。
- wasapi-rs MIT；symphonia MPL-2.0；FFmpeg LGPL 2.1+（动态链接）；beets MIT；Python PSF。
- 不用 BASS（proprietary，GPLv3 需商业授权）。
- v1 可先内测未签名；公开发布前做 Authenticode + 安装器测试。

---

**最小拍板组合**：1 选 mpv、2 选原样搬、3 选用户安装、5 选只写标签。其余按建议默认即可。