# ledger · UI 壳与状态（ui-shell）

> 代码位置：ui/src/stores/、ui/src/views/、ui/src/App.svelte
> 相关档案：docs/19、23、25、28

## 条目（新→旧）

<!-- 每条一行：- YYYY-MM-DD · 代码位置 · 事实/取舍 · 来龙去脉（可选） -->
- 2026-11-04 · stores/notify.svelte.ts、components/AppShell.svelte、stores/overlay.svelte.ts 及各 store/view 失败路径 · 运行时报错三档通道：后端/数据错误→界面错误条、不阻断运行的播放报错→播放条吐司、须用户介入的（设备丢失/未知故障）→overlay 弹窗 · 判据是"是否需要用户介入才能继续"；吐司档曲名不进文案（播放条正显示着它）。
- 2026-11-04 · stores/beets-session.svelte.ts · beets:import 事件订阅必须懒触发（首次发命令时）· 模块在静态 import 链上、求值早于 main.ts 装 mock transport：构造期订阅在浏览器预览下落到 Tauri 通道被吞、此后再无订阅者（表现为会话停在"匹配中"）。

- 2026-11-05 · lib/i18n/、lib/i18n.svelte.ts、settings.ts、pages.css · 英语支持落地：i18n 词典模块（9 个域表 ~700 键，zh/en 成对 + `Record<keyof zh>` 键位奇偶类型约束）、`settings.language` 空值首启语义（宿主 GetUserDefaultUILanguage 探测写库，前端只跟随）、`<html data-lang>` 驱动装饰字取消 CSS（英文界面里装饰字与标题同语言属重复信息）、format.ts/首页问候/设置选项标签（label 改存 Msg 键，值/文案分离）双语化；Rust 宿主/crates/beets-tool 用户面报错硬迁移英文（中文界面下原样显示，P2-16 同批口径） · 分片由 9 个子代理执行（并发 2 实测上限），终扫确认域表外字符串零残留；E2E 以 addInitScript 预置 zh 不受首启探测影响
- 2026-11-04 · stores/facets.svelte.ts · loadAlbums/loadArtists 各持请求代次（同一轮会并发发两个查询，不能共享）· 聚合查询的过期响应曾可回写旧结果，同仓其余异步路径早有成套代次模式，此处是漏网。
- 2026-11-04 · components/QueuePanel.svelte · 行保持 role=button，删掉子元素 role=cell 与容器冗余 aria-label · button 子树里的 cell 对读屏是坏表；键盘操作不变。
- 2026-11-04 · components/AppShell.svelte · 播放错误去重键在状态离开 error 时复位 · 同曲同错重试后再失败要能再提示。
- 2026-11-04 · lib/track-window.ts + components/TrackListPanel.svelte · 失败行按页重试（retryPage），不再整列表重拉弹回顶部 · TrackWindow 本就有 per-page 错误态与 onretry(index) 签名，缺的只是实现。
- 2026-11-04 · components/QueuePanel.svelte · meta/failedAttempts 缓存随队列 ids 变化按集合清理 · 5 万队列滚完后数万条 TrackDetail 常驻。
- 2026-11-04 · ui/src/styles/pages.css、ui/src/views/HomeView.svelte · 首页统计块弃「与卡组几何互钉」（height 148 + margin-bottom 60 + flex-end），改普通块顶部对齐，`::before` 预留一个区块标题的盒子（上距 26 + 一行 fs-2xl 行盒 + 下距 10，不可见行镜像字体度量不写死像素）让问候语仍落在卡面顶沿 · 真机曲库歌单行为空态/加载态时行高不是 148+60，钉死的常量把统计块甩到半空（预览有 20 张 mock 卡看不出来）；占位锚换成固定组件（标题行）后卡组有无、多少都不影响，`--pl-caption-h` 一并删除


- 2026-11-03 · ui/src/views/HomeView.svelte、ui/src/styles/pages.css · 首页歌单计数改胶囊徽章（`--surface-hover` 底、无描边、18px 高、tabular-nums），删掉「常听」标题行右端的「清空播放历史」按钮连同 `.section-link` 样式 · 计数从裸灰字变徽章是为了让"共 N 个"在视觉上归属标题行而不是像一句游离的导航；清空历史是破坏性动作，首页是浏览的地方，设置页与曲库页各有一个入口，首页再放一个只是在只读面板上开洞；smoke 加两条守卫（徽章在位、`.section-link` 为 0），否则下一轮重排会把它带回来
- 2026-11-03 · ui/src/views/HomeView.svelte、ui/src/styles/pages.css · 首页重做：区块标题由三层（英文 / 标题 / 陈述句）改一行（大字标题 + 小字装饰 + 计数），统计由 2×2 小卡改纯文字四行贴右、与歌单行底部对齐（只留播放数与时长），三个短列表各加一层 surface-3 底、歌单行补横滚（snap + hover 滚动条）· 用户要求删掉描述行与卡片布局；标题行右端的「共 N 个」并入左侧，是它与缺失告警位争同一处；面板取 surface-3 而非 surface-2，因后者在深色主题下比背景更暗，屏幕左半的边界会消失
