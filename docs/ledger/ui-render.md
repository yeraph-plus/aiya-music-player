# ledger · UI 渲染与组件（ui-render）

> 代码位置：ui/src/lib/render/、ui/src/components/
> 相关档案：docs/19、27、38、39、46

## 条目（新→旧）

- 2026-11-19 · docs/78 整批审查 · docs/77 全批独立复审：A01–A13 逐项落地核对通过、旧实现（Canvas 2D/Knob/旧枚举/歌词 halo）删净；门禁重跑全绿（check 0/0、deps 164/633、layers/style 通过），regression 全项目 150 项有效全过（batch.spec 1 项 goto 超时为满载偶发，单跑 3/3 过）；P3 备忘两项：StatBlock 12/9px 圆角脱阶梯、dev 回归满载 goto 判据偏紧 · docs/78。
- 2026-11-18 · LyricsPanel / pages.css / tokens.css · 按视觉反馈移除歌词整区渐隐衬底及当前句 halo DOM，仅高亮行使用随主题切换深浅的字形阴影，普通行保持原样；此前衬底版歌词对比度数字不再代表现行设计 · docs/77 §18。

- 2026-11-18 · Header / Tooltip / 播放页阅读 token 与衬底 · 修正双重歌词透明与浅色衬底，装饰英文使用正文次级对比度，Tooltip 提供显式语义；真实合成文字及焦点环按明暗五配色取证 · docs/77 C。

- 2026-11-18 · NowPlayingSpectrum / AmbientBackdrop / render host、region、foreground · 六前景 F1/F2/F4/F5/F6/F7 接入共享 GlStage，播放页仅登记 DOM 区域，统一 viewport/scissor 与祖先裁切；前后景独立持源，暂停不重建，切换/退出/上下文丢失正确释放；删除播放页 Canvas 2D，统一硬件 WebGL/CSS 降级；18 组合与五次 HMR 已取证 · docs/77 B2，真实宿主与完整终验继续 C。

- 2026-11-18 · AmbientBackdrop / render / SpectrumSource / SettingsPanel · 背景保留 B0 山形，接入 B1 极光绸幕与 B4 漫射光帘；Gaussian/CR 平滑、艺术时钟暂停与隐藏恢复、单舞台图层所有权、背景硬件 WebGL/CSS 静态降级；删除背景 ripple/2D，山形参数仅该形态显示；live 以帧到达判断 · docs/77 B1，播放页共享区域与六前景继续 B2。


- 2026-11-18 · SettingsPanel / Beets panels / LiveView / station / OverlayHost / ui AlertDialog · 设置形态改共享 Select 并按父开关禁用；快捷键名称翻译及空绑定语义修正；Bits AlertDialog 统一确认，Dialog 在异步提交期通过交互层阻止关闭；歌单等待保存并保留失败草稿；周格 rem 刻度和局部横滚，Beets 勾选与不确定进度统一原语 · docs/77 A3。

- 2026-11-18 · ListToolbar / QueuePanel / PlaylistList / EqPanel / browse stores / NowPlayingView · 详情页播放主操作去重；队列拆成独立把手、播放和移除按钮，支持键盘排序及回焦；EQ 统一 Select 与 Slider 并删除 Knob；搜索代次与两类 facet 状态独立，返回栈只保存会话态；网格允许收缩以承接放大布局 · docs/77 A2。

- 2026-11-18 · ui/Button / IconButton / Field / Dialog / SideDrawer / TabList / shell · Button.Root 统一按钮基础与 Tooltip；Dialog 拥有主体留白；固定抽屉使用 Bits Popover.ContentStatic 管理非模态焦点与嵌套 Esc；Beets 使用真 Tabs，设置锚点保留导航；底栏按右侧控件实际宽度占位 · docs/77 A1。

- 2026-11-18 · AddLibraryFolder / LibraryTools / LibraryView · Web 使用 kit 目录输入模态框，设置页和空曲库共用；桌面沿用宿主目录选择器。POSIX 目录保留大小写 · docs/76。

- 2026-11-18 · station i18n / i18n.svelte.ts · 推流补齐普通异常与本地时区翻译、英文标点及曲目单复数、星期无障碍名称；参数按单次回调替换，保留用户名称中的占位符和美元字符 · 双语复核与验证见 docs/75。

- 2026-11-18 · WeekTimePicker / Field · 时段改为星期与起止同一行，无可见字段标题；kit 没有时间组件，统一为带 inputmode/pattern/过滤回写的受限文本输入，支持 HH:MM 与结束 24:00 · 方案与验收见 docs/75 第六阶段。

- 2026-11-18 · Card / Button / ProgramEditor / WeekTimePicker · 带正文卡片增加统一留白，开关顺排，时段改分隔线列表；星期选择使用整块可点击的小号按钮及 aria-pressed · 用户实地反馈修订，见 docs/75 第五阶段。

- 2026-11-18 · Card / WeekTimePicker · Card 增加正文槽容纳完整队列配置；周格按钮全尺寸命中、内部留白，时段使用半透明范围层，捕获网格指针并在拖动期间保持全局十字光标 · 现行交互与验证见 docs/75 第四阶段。

- 2026-11-17 · ui/Card.svelte、ui/Field.svelte、station/StationFields.svelte · Card 支持独立动作及元信息槽并避免按钮嵌套；Field 支持 time/url 和范围参数，频道表单、模式与时段配置复用 kit · 对齐既有专辑卡片及频道工作台，详见 docs/75 第三阶段。

<!-- 每条一行：- YYYY-MM-DD · 代码位置 · 事实/取舍 · 来龙去脉（可选） -->
- 2026-11 · components/AppShell.svelte · 队列/EQ 抽屉宿主用几何确定的 SideDrawer（贴右缘、定宽、定边距），不用 floating-ui 测量气泡 · 测量+碰撞翻转的开合位置有可见抖动。
- 2026-11-04 · components/LyricsPanel.svelte · 歌词时钟并入 subscribeRender 调度器（暂停即退订、页面隐藏整体停摆）· 它曾是调度器之外唯一常驻 rAF 循环。


- 2026-11-03 · ui/src/lib/render/ridge-curve.ts、layers/ripple.ts · 重采样结果钳回 0–1（Catmull-Rom 在"平台贴地"处下冲到 −0.125×外侧值），波纹层 pow 前再 max(curve,0) 兜底、uEnergy 补下限：负底数的幂在 GLSL 里未定义，NaN 会顺着 amp 传播到底、把整列水面写成 NaN · 用户报"音量极小时出现垂直断裂"；探针在 700px 宽上量到 12 列覆盖为 0（x 61–77，左右邻居每列 45 像素），与曲线负值点 22–27 落在同一条竖线上

- 2026-11-03 · ui/src/lib/render/layers/ripple.ts · 背景波纹加横向卷动：`t·SCROLL` 加在噪声采样坐标 x 上，全部层同一平移量（实测 5.2% 屏宽/秒、自右向左）；按层给速度差能换纵深视差，但相邻层相干性会随时间散架，故不做 · 用户要求"右到左卷动横移"
- 2026-11-03 · ui/src/lib/render/layers/ripple.ts · 波纹层**整体还原**到 c3e169a 之前（17276b5 的版本，即等深线画法）：撤销 07:10 的「深度切片相干波场」重写与当日随后的重写修复/横向卷动试验 · 用户对比后判定新画法问题更多，要求回旧版；还原后文件与重写前逐字节一致（`git diff c3e169a^ -- ripple.ts` 为 0 行），四个描述画法的文件（AmbientBackdrop / NowPlayingSpectrum / SettingsPanel / settings.ts）里的「深度切片」措辞同步回「等深线」。旧版自带横向漂移（`off = (l·2.6 + t·0.55)/48`，约 1.15% 屏宽/秒，方向同为右→左），卷动不依赖后加的那次改动
- 2026-11-03 · ui/src/lib/render/layers/ripple.ts · 波纹层重写：线宽改常量并取法向距离、逐层 max 取代累加、层间距 1.55→1.2 次幂，消掉上半屏线细下半屏粗与浅底上的白色亮带 · 用户反馈两处观感问题；把 alpha 单独导成灰度图后量出线宽随层深从 0.8px 变到 2px、线间 alpha 地板恒在 15–20/255 从不回 0（**已随上一条还原撤销**）

- 2026-11-03 · ui/src/styles/lists.css · 行间隙机制（2px 透明底边框 + background-clip）源于 2026-10-28 用户反馈「行间距没做」；同批定稿的还有表头半透明磨砂（替代深色横条）
