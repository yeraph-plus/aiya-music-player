# 38 · 超长列表滚动卡顿：诊断与改进方案（待确认，未开工）

日期：2026-10-28 · 状态：**诊断完成，方案待拍板** · 回答的问题："是 SQLite 性能问题还是读取模型？"

## 结论先行

**不是 SQLite 的锅，是读取模型的"执行位置"问题**——查询本身够快，但它们被放在了主线程上跑。另外今天实际遭遇的"卡顿 + 点击失效"主因是 rAF 指数失控 bug（已修复，见工作日志），本诊断针对的是修复后仍存在的结构性卡顿因素。

## 证据

### 1. SQLite 层：5 万行实测（perf_50k，release，本机跑）

```
灌 50000 行:          1.4s
首屏 200 首:         912.7µs
深分页 @25000:       20.0ms
深分页 @40000:       28.5ms
深分页 @49800:       35.2ms   ← LIMIT 200 OFFSET 49800，O(offset)
游标续页(中位):       1.7ms   ← 同样 200 行，快 20 倍
搜索"晴天":          33.4ms  （命中 7143 行）
```

- 单页查询 0.9–35ms，索引齐、有 `assert!(worst < 100ms)` 预算断言守护。**这个量级对 60fps（16.6ms/帧）不是灾难，但深分页的 35ms 单独就能吃掉两帧。**
- 关键事实：`list.svelte.ts` 的 `query()` **全视图统一用 `mode: 'offset'`**（源码注释：cursor 模式不读 offset）。列表滚动越深，每次翻页越贵。游标模式便宜 20 倍却没用在列表翻页上。

### 2. 读取模型层：三个主线程阻塞点（真正的放大器）

**① `library_list` 是同步命令，主线程阻塞等 DB。**
`lib.rs:901` 是非 async `#[tauri::command]`（Tauri 2 同步命令在主线程执行），内部 `DbHandle::send` 用 `reply_rx.recv()` 同步等 DB worker 回包（`db.rs:187`）。滚动时每次翻页 = 主线程冻结 (排队 + SQL + serde 100 行进出)。快速甩动时多页请求排队，主线程被连续小冻结串成卡顿带。1472 首实测单命令 ≤1.24ms 尚无感，5 万首深页 10–35ms 就是肉眼卡顿。

**② 封面协议 handler 同步跑在主线程——首次滚动最重的一击。**
`lib.rs:2542` `artwork_protocol` 走 `register_uri_scheme_protocol`（同步 handler，主线程执行），每个未命中缓存的封面请求在主线程上做：DB 往返 + 读原图 + **Lanczos3 缩放**（`make_thumb`，一张 1400×1400 封面缩 512 约 30–80ms）。缩略图有磁盘缓存（thumbs/），但**首次**滚过一段长列表 = 每张新封面都卡主线程几十毫秒。清缓存或新库首滚就是重灾区。

**③ 已修复的 rAF 指数失控**（AmbientBackdrop draw() 重复调度），今天的"点击失效 + 界面卡死"主因，修复后 CPU 已回落正常（渲染进程 ~11% 单核）。

### 3. 排除项（查过，不是问题）

- UI 虚拟化：固定行高 + overscan 6 + 稀疏分页（100 行/页、FIFO 24 页 ≈4MB 内存账），设计健康。
- 滚动落盘：`scheduleUiSave` 600ms 防抖，滚动只重置 timer。
- timing 埋点：内存环形队列，不发事件。
- 搜索：120ms 防抖 + LIKE 有索引支撑。

## 改进方案（按性价比排序）

### P1 · 封面协议异步化（小改，收益最大）
`register_uri_scheme_protocol` → `register_asynchronous_uri_scheme_protocol`：handler 立即返回挂起响应，DB/读文件/缩放丢 `spawn_blocking`，完成后回填响应。主线程零阻塞。首滚长列表的卡顿大头直接消失。
预估改动：`lib.rs` 协议段 ~40 行重构，前端零改动（URL 不变）。

### P2 · 热列表命令 async 化（小改，中等收益）
`library_list`（以及滚动路径上的同族命令）改 `async fn` + `tauri::async_runtime::spawn_blocking(move || state.db().send(...))`——仓库里 `beets_move` 已有完全相同的先例（`lib.rs:891`）。主线程不再等 DB。
预估改动：每命令 ~5 行；UI 零改动（invoke 语义不变）。

### P3 · 库来源列表翻页切 cursor 模式（中改，只在 5 万+ 库疼）
library 来源的 `TrackWindow` 翻页从 offset 改游标（续页 1.7ms vs 深页 35ms）：TrackWindow 需要按页缓存页尾 cursor，reset 时机不变。playlist/recent/mostPlayed 是固定顺序、没有游标，**维持 offset**（它们通常不会太长）。
预估改动：`track-window.ts` + `list.svelte.ts` + Rust `TrackWindowQuery` 透传，~100 行 + 单测。
**1.4k 曲库当下无感，建议等库真上规模再动**——避免为还不存在的规模付复杂度。

### P4 · 不做
- 换虚拟列表库、加 `content-visibility`：现有虚拟化没有测量出问题，不动。
- 缩略图预热后台任务：P1 落地后缩放不再阻塞主线程，预热只剩"省电"价值，不值得加后台扫描。

## 验收口径（实施时）

- 5 万行库、冷 thumbs 缓存，滚动全列表：渲染进程主线程无 >16ms 阻塞段（用 CDP Performance 或进程 CPU 曲线佐证）。
- perf_50k 基准保持通过；library cursor 翻页新增基准断言。

## 实施记录（2026-10-28，P1 + P2 已落地）

- **P1 封面协议异步化**：注册点换 `register_asynchronous_uri_scheme_protocol`（lib.rs 装配段），handler 只在主线程做"收包转交"，整个 `artwork_protocol`（DB 往返 + 读原图 + Lanczos3 缩放）经 `spawn_blocking` 在阻塞池跑完，再经 `UriSchemeResponder` 回包。`artwork_protocol` 函数体本身零改动，前端零改动。
- **P2 热命令 async 化**：`library_list` / `library_albums` / `library_artists` 三条滚动/翻页路径命令改 `async fn` + `spawn_blocking`（`DbHandle` 是 `#[derive(Clone)]` 句柄，clone 进闭包即可，先例 `beets_move`）。`library_stats` 每次进页只调一次、不在滚动链路，**未动**。P3 维持"等库上 5 万再动"。
- 验证：`cargo check --workspace` 通过；`cargo test -p aiya` 5/5（含 `registered_commands_have_callers`、`every_ui_argument_maps_to_a_rust_parameter` 两条契约测试，证明命令签名改动自洽）；冷启动 dev 日志无 ERROR，1472 首曲库恢复正常。
- 遗留验收：冷 thumbs 缓存滚动全列表的主线程阻塞段测量（需真机滚动操作，用户体感 + 诊断页 timing 埋点可佐证）。
