# ledger · UI 渲染与组件（ui-render）

> 代码位置：ui/src/lib/render/、ui/src/components/
> 相关档案：docs/19、27、38、39、46

## 条目（新→旧）

<!-- 每条一行：- YYYY-MM-DD · 代码位置 · 事实/取舍 · 来龙去脉（可选） -->
- 2026-11 · components/AppShell.svelte · 队列/EQ 抽屉宿主用几何确定的 SideDrawer（贴右缘、定宽、定边距），不用 floating-ui 测量气泡 · 测量+碰撞翻转的开合位置有可见抖动。
- 2026-11-04 · components/LyricsPanel.svelte · 歌词时钟并入 subscribeRender 调度器（暂停即退订、页面隐藏整体停摆）· 它曾是调度器之外唯一常驻 rAF 循环。


- 2026-11-03 · ui/src/lib/render/ridge-curve.ts、layers/ripple.ts · 重采样结果钳回 0–1（Catmull-Rom 在"平台贴地"处下冲到 −0.125×外侧值），波纹层 pow 前再 max(curve,0) 兜底、uEnergy 补下限：负底数的幂在 GLSL 里未定义，NaN 会顺着 amp 传播到底、把整列水面写成 NaN · 用户报"音量极小时出现垂直断裂"；探针在 700px 宽上量到 12 列覆盖为 0（x 61–77，左右邻居每列 45 像素），与曲线负值点 22–27 落在同一条竖线上

- 2026-11-03 · ui/src/lib/render/layers/ripple.ts · 背景波纹加横向卷动：`t·SCROLL` 加在噪声采样坐标 x 上，全部层同一平移量（实测 5.2% 屏宽/秒、自右向左）；按层给速度差能换纵深视差，但相邻层相干性会随时间散架，故不做 · 用户要求"右到左卷动横移"
- 2026-11-03 · ui/src/lib/render/layers/ripple.ts · 波纹层**整体还原**到 c3e169a 之前（17276b5 的版本，即等深线画法）：撤销 07:10 的「深度切片相干波场」重写与当日随后的重写修复/横向卷动试验 · 用户对比后判定新画法问题更多，要求回旧版；还原后文件与重写前逐字节一致（`git diff c3e169a^ -- ripple.ts` 为 0 行），四个描述画法的文件（AmbientBackdrop / NowPlayingSpectrum / SettingsPanel / settings.ts）里的「深度切片」措辞同步回「等深线」。旧版自带横向漂移（`off = (l·2.6 + t·0.55)/48`，约 1.15% 屏宽/秒，方向同为右→左），卷动不依赖后加的那次改动
- 2026-11-03 · ui/src/lib/render/layers/ripple.ts · 波纹层重写：线宽改常量并取法向距离、逐层 max 取代累加、层间距 1.55→1.2 次幂，消掉上半屏线细下半屏粗与浅底上的白色亮带 · 用户反馈两处观感问题；把 alpha 单独导成灰度图后量出线宽随层深从 0.8px 变到 2px、线间 alpha 地板恒在 15–20/255 从不回 0（**已随上一条还原撤销**）

- 2026-11-03 · ui/src/styles/lists.css · 行间隙机制（2px 透明底边框 + background-clip）源于 2026-10-28 用户反馈「行间距没做」；同批定稿的还有表头半透明磨砂（替代深色横条）
