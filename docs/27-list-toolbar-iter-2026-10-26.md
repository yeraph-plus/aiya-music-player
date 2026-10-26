# docs/27 — 列表组件工具条批（2026-10-26）

7 点反馈的落地记录。决策三问已由用户拍板：**保留歌手列**（列 = #/标题/歌手/专辑/时长，
只删入库时间与次数列）、**首页预览表改纯列表**（悬浮批量条全应用退役）、
**时间筛选用预设档位**（不引日期选择器）。

## §1 工具条内置（ListToolbar）

- 新组件 `components/ListToolbar.svelte`，挂在 TrackList 顶部、不随列表滚动
  （TrackList 根改成 `.list-shell` = 工具条 + `.list` scroller，虚拟滚动的几何不变）。
- 左组动作：播放全部（当前视图 `list.playAll`）/ 加入播放队列 / 加入歌单 /
  从曲库中删除（歌单视图自动变「从播放列表移除」）。后三个对**选择集**生效，
  禁用条件只看选择数——批量开关关着时单击选中的一行同样可用。
- 右组：筛选器按钮（角标显示生效档位数）+ 批量操作 Switch。
- **悬浮 BatchBar 与 stores/batch.svelte.ts 删除**。首页 TrackTable 去勾选/选择集，
  变纯预览表（双击播放、右键保留）；批量动作就地内联到 LibraryView
  （enqueueSelection / addSelectionToPlaylist / removeSelection）。

## §2 批量模式与选择语义（第 5/6 点）

- `ui.batchMode`（会话态，不持久化）：开 = 勾选列渲染 + 网格多一列
  （`.list-shell.no-batch` 双模板，后 6 列定义一致防跳位）；关 = 隐藏且清空选择集。
- **Ctrl+点行自动开批量**（用户不用去找开关）：TrackList 行点击里
  `ctrlKey && !batchMode → onbatchmode(true)` 再走 onselect。
- 单击 = 选中一行；双击 = 播放（B4-1 语义不变）。
- 右键：目标行不在选择集 → 先单选它；在选择集 → 整组保留。
  菜单 `multi` 语义（`overlay.trackMenu.multi`）：**播放 / 下一首播放 / 查看艺人 /
  查看专辑 / 在文件夹中显示** 单选专属（多选时隐藏）；加入队列 / 加入歌单 /
  移除对整组生效，统一走 `list.selectionIds()`（右键前保证目标行在选择集里，
  单选多选同一条路径）。

## §3 列与筛选器（第 3/7 点）

- 列：`[勾选] / # / 封面 / 标题 / 歌手 / 专辑 / 时长`。入库时间、次数列撤
  （次数仍显示在首页预览表）。表头排序保留标题/歌手/专辑。
- **歌手/专辑名是行内跳转链接**（`.link-cell`）：有 artistId/albumId 就可点直达
  详情页（stopPropagation，不触发选中/播放）；无 id 退化为纯文本。
- 筛选器模态框（overlay.dialog `trackFilter`）：入库时间 / 上次播放两组预设档
  （全部/今天/7天内/30天内/今年，上次播放多「从未播放」）。草稿在 OverlayHost
  本地，应用才写 `list.setTimeFilter`；生效时 Toolbar 出「时间筛选 ×」chip。
- **后端扩展**（`aiya-data/repo.rs`）：`TrackWindowQuery` 增加
  `addedFromMs/addedToMs/lastPlayedFromMs/lastPlayedToMs/lastPlayedNever`
  （serde default 容缺省，旧调用方零改动）；`build_where` 加范围条件
  （毫秒 /1000 转秒绑定，to 开区间）与 `last_played_at IS NULL`。
  never 与范围互斥，never 优先（前后端同规则）。新单测
  `time_filters_added_and_last_played`（aiya-data 97 全过）。

## §4 封面悬浮播放键（第 4 点）

- 播放键从 # 列搬到**封面叠加层**（`.cover-slot` 相对定位 + `.play-btn` 绝对
  inset:0，行 hover 显示，半透明压暗底）。# 列恒显序号，正在播放行显示跳动条。
- 行点击守卫扩为 `.k-check, .play-btn, .link-cell`。

## §5 验证与遗留

- 验证：`cargo test -p aiya-data` 97 全过（新时间筛选测试在内）；
  svelte-check 0/0；e2e 20/20（batch.spec 重写为工具条/Ctrl 批量/首页纯列表三例）。
- **注意**：`cargo check --workspace --all-targets` 在本机被环境锁挡住——
  tauri-build 拷贝 `binaries/aiya-audio-host-*.exe` 时应用正在运行（拒绝访问）。
  关掉应用后重跑即可，与代码无关（src-tauri 只是 Json 透传，未改）。
- 后续可选：筛选器模态框预留 beets 标签字段位（同样的「档位 → 查询参数」模式）；
  添加日期排序仍在协议里但列表无表头入口。
