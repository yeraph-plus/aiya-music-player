# ledger · UI 壳与状态（ui-shell）

> 代码位置：ui/src/stores/、ui/src/views/、ui/src/App.svelte
> 相关档案：docs/19、23、25、28

## 条目（新→旧）

<!-- 每条一行：- YYYY-MM-DD · 代码位置 · 事实/取舍 · 来龙去脉（可选） -->
- 2026-11-04 · stores/facets.svelte.ts · loadAlbums/loadArtists 各持请求代次（同一轮会并发发两个查询，不能共享）· 聚合查询的过期响应曾可回写旧结果，同仓其余异步路径早有成套代次模式，此处是漏网。
- 2026-11-04 · components/QueuePanel.svelte · 行保持 role=button，删掉子元素 role=cell 与容器冗余 aria-label · button 子树里的 cell 对读屏是坏表；键盘操作不变。
- 2026-11-04 · components/AppShell.svelte · 播放错误去重键在状态离开 error 时复位 · 同曲同错重试后再失败要能再提示。
- 2026-11-04 · lib/track-window.ts + components/TrackListPanel.svelte · 失败行按页重试（retryPage），不再整列表重拉弹回顶部 · TrackWindow 本就有 per-page 错误态与 onretry(index) 签名，缺的只是实现。
- 2026-11-04 · components/QueuePanel.svelte · meta/failedAttempts 缓存随队列 ids 变化按集合清理 · 5 万队列滚完后数万条 TrackDetail 常驻。
- 2026-11-04 · ui/src/styles/pages.css、ui/src/views/HomeView.svelte · 首页统计块弃「与卡组几何互钉」（height 148 + margin-bottom 60 + flex-end），改普通块顶部对齐，`::before` 预留一个区块标题的盒子（上距 26 + 一行 fs-2xl 行盒 + 下距 10，不可见行镜像字体度量不写死像素）让问候语仍落在卡面顶沿 · 真机曲库歌单行为空态/加载态时行高不是 148+60，钉死的常量把统计块甩到半空（预览有 20 张 mock 卡看不出来）；占位锚换成固定组件（标题行）后卡组有无、多少都不影响，`--pl-caption-h` 一并删除


- 2026-11-03 · ui/src/views/HomeView.svelte、ui/src/styles/pages.css · 首页歌单计数改胶囊徽章（`--surface-hover` 底、无描边、18px 高、tabular-nums），删掉「常听」标题行右端的「清空播放历史」按钮连同 `.section-link` 样式 · 计数从裸灰字变徽章是为了让"共 N 个"在视觉上归属标题行而不是像一句游离的导航；清空历史是破坏性动作，首页是浏览的地方，设置页与曲库页各有一个入口，首页再放一个只是在只读面板上开洞；smoke 加两条守卫（徽章在位、`.section-link` 为 0），否则下一轮重排会把它带回来
- 2026-11-03 · ui/src/views/HomeView.svelte、ui/src/styles/pages.css · 首页重做：区块标题由三层（英文 / 标题 / 陈述句）改一行（大字标题 + 小字装饰 + 计数），统计由 2×2 小卡改纯文字四行贴右、与歌单行底部对齐（只留播放数与时长），三个短列表各加一层 surface-3 底、歌单行补横滚（snap + hover 滚动条）· 用户要求删掉描述行与卡片布局；标题行右端的「共 N 个」并入左侧，是它与缺失告警位争同一处；面板取 surface-3 而非 surface-2，因后者在深色主题下比背景更暗，屏幕左半的边界会消失
