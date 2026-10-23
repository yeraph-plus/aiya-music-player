# 全量代码审查 — Tauri 重写仓库（P4 后基线）

- **日期**：2026-10-24
- **范围**：crates/aiya-audio、aiya-audio-host、aiya-data、aiya-player（交叉核验）、src-tauri、sidecar/beets_sidecar.py、ui/src + contracts（约 1.87 万行）
- **基线证据**：`cargo test --workspace` 全绿（67 测试：audio 16+3、data 30+3、player 15 等）；`svelte-check` 0 错误 0 警告
- **方法**：四区并行深审（音频/数据/宿主+sidecar/UI），全部发现经读码核实行号，4 个 P0 由主审逐一复核原文确认
- **总评**：架构纪律执行良好——compat 层唯一触点、状态机在 Rust、SPSC 环内存序正确、协议握手有锚点测试、UI 竞态防护（代次号/请求号/LRU）是亮点。但存在 **4 个 P0 功能性断裂**，均为"主路径跑通、边界路径失效"类，测试没抓住的原因都是单场景巧合（见各项说明）。

---

## P0 — 功能性断裂（4 项，均主审复核确认）

### P0-1 音频：暂停超过 2 秒必拆流，暂停期间所有命令失效
- `crates/aiya-audio/src/engine.rs:236`（`wait_for_event(2000)` 错误 `?` 传播）、`:241-245`（Pause → `stop_stream()`）、`:298-299`（paused → continue 回循环顶部）
- **机制**：`stop_stream()` 后 WASAPI 引擎不再为该流处理传递，事件停发；2 秒超时返回 `EventTimeout` 被 `?` 打成 Err → 流销毁 + `EngineEvent::Error`，引擎退回等 Load。暂停期间的 Resume/Seek/SetVolume 全部无人处理。
- **为何测试没抓住**：测试只验证命令收发，没有"暂停 2 秒后 Resume"场景。
- **修复**（方案 A，推荐）：Pause 不 `stop_stream`，保持流 running 只停喂数据（事件继续触发、命令循环存活）；如需立即静音，Pause 时补写一次静音帧。方案 B：把超时当正常唤醒，错误不 `?`，命令循环靠超时驱动。

### P0-2 宿主：Eof 自动换曲断裂，`pending_load` 无人消费
- `src-tauri/src/lib.rs:1583-1598`（事件泵 Eof 分支）；对照 `crates/aiya-player/src/actor.rs:215,387-403`（`Ended` 归约写 `pending_load`）
- **机制**：Actor 归约 `Ended` 后把 `instruct_load(true, 0)` 写入 `pending_load`，但全仓库只有 `drive_and_emit`（命令路径）和启动 hydrate 会 `take_pending_load`；事件泵 Eof 分支只 `record_history` + `reduce(Ended)`，从不取、从不 `engine_send(Load)`。引擎只播单文件。
- **后果**：自然播完一首后 UI 状态推进到下一曲，但声音停住；repeat=one 重播同样失效；手动 next/seek 才能救活。
- **修复**：Eof 分支 reduce 后 `take_pending_load()`，有则按 `drive_and_emit_inner` 顺序 `engine_send(Load/SetVolume/Pause)`，并走 `emit_player_state`（顺带修 P2-宿主#14 SMTC 元数据不更新）。

### P0-3 数据：歌单查询完全没按 playlist_id 过滤，返回所有歌单并集
- `crates/aiya-data/src/repo.rs:163-167`（`JOIN_PLAYLIST` 无 `pi.playlist_id = ?`）、`:180`（`TrackSource::Playlist { .. }` 忽略 id）、`:201-252`（`build_head` 无 playlist 条件）
- **为何测试没抓住**：repo.rs:526-541 测试库只建了一个歌单，并集恰好等于该歌单。
- **修复**：`build_head` 对 Playlist 来源追加 `pi.playlist_id = ?` 并绑定（注意占位符顺序与 binders 一致）；补"两个歌单互不串"测试。

### P0-4 sidecar：beets 取消机制实际失效，cancel 行排队而非立即置位
- `sidecar/beets_sidecar.py:437-461`
- **机制**：`_read_stdin` 把 cancel 行与其他请求一样 `req_q.put(line)`（440-443）；`CANCEL.set()` 在主循环才执行（458-459）。而主循环正同步执行 preview/apply（网络匹配+写文件），`req_q.get()` 轮不到 cancel 行，工作循环里的 `CANCEL.is_set()` 检查点永远看不到标志。
- **后果**：点取消后全部工作照跑完；宿主 reader 先见 `done` 退出会话，`cancelled` 事件永远不会发。B3 的取消功能是坏的。
- **修复**（十行内）：`_read_stdin` 里直接解析，`cmd == "cancel"` → `CANCEL.set()` 不进队列；或主循环用带超时 `get` 旁路处理。

---

## P1 — 高风险（7 项）

| # | 区 | 位置 | 问题 | 修复方向 |
|---|---|------|------|---------|
| P1-1 | 音频 | decoder.rs:77 | `SampleBuffer` 按 100ms 固定容量，FLANC/WAV 大 blocksize（最大 65535 帧）触发 symphonia assert panic，引擎线程死亡 | 按 `track.codec_params.max_frames_per_packet` 分配，或直接 ≥65535 帧安全容量 |
| P1-2 | 音频 | engine.rs:124,142 + lib.rs:1452-1461 | EOF 后 `pending_rate` 重置 1.0，UI 自动切曲不重发 SetRate → 跨曲倍速丢失且 UI 失同步 | 自动切曲路径补发 SetRate，或引擎侧 rate 跨曲目保持 |
| P1-3 | 宿主 | lib.rs:1526-1544 + smtc.rs:237-250 | 防休眠 per-thread API（SetThreadExecutionState）跑在可迁移的 tokio 任务里：标志可能设在 A 线程、恢复在 B 线程，且旧线程的 ES_SYSTEM_REQUIRED 无人清除 → **退出播放后系统仍不睡** | 移到独立 std::thread（AtomicBool 驱动） |
| P1-4 | 宿主 | lib.rs:228,238,249,457-463 + beets.rs:184-195 | 同步 Tauri 命令在主线程执行：beets probe 最多 3 候选 × 10s 超时 ≈ 30s 主线程冻结；`blocking_pick_folder` 在主线程有死锁风险（插件文档明确警告） | 相关命令加 `#[tauri::command(async)]` |
| P1-5 | 宿主 | beets.rs:360-364,378-381 | 会话槽在 spawn **之后**才 acquire：并发 preview/apply 双双 spawn sidecar，请求行已进 sidecar 队列会照常执行；败者无 reader → 事件不发、进程泄漏、beets 库可能锁冲突 | 先 try-acquire 再 spawn |
| P1-6 | 数据 | db.rs:78-79 | 迁移：schema 事务 COMMIT 后、`user_version` 写入前崩溃 → 下次启动重放 0001 报 "already exists"，**应用永久起不来** | `PRAGMA user_version` 放进同一 BEGIN...COMMIT |
| P1-7 | 数据 | scanner.rs:266,273 | 重扫把 beets 写入的 codec 清回 NULL（`codec = excluded.codec`，excluded 恒 NULL），与 beets_sync 形成**双向 churn 循环** | upsert 改 `codec = COALESCE(excluded.codec, tracks.codec)` |

（另有数据区 P1 级两项并入上表：路径大小写敏感重复入库 → P2-数#1 升级项、删除文件无处理 → P2-数#2。见下。）

---

## P2 — 缺陷/健壮性（按区分组）

### 宿主层 + sidecar
1. **导入会话无看门狗** + 错误信封无 `type` 字段不被视为终止事件（beets.rs:325-326 + py:463-476）：preview 网络卡死 → 槽位占死，而唯一恢复手段 cancel 又是坏的（P0-4）。建议错误信封带 `"type":"sessionFailed"` 入终止事件列表 + 宿主侧超时 kill。
2. **cancel 后台置位粒度**：修 P0-4 时一并确认 `CANCEL.is_set()` 检查点覆盖网络匹配阶段。
3. **`query_sidecar` 超时不收尸**（beets.rs:96-98）：kill+wait 缺失，卡死 sidecar 泄漏。
4. **status 探测不容错非 JSON 行**（beets.rs:88-95 只取第一行 break）：违反坑⑤精神，建议循环读到第一个可解析信封。
5. **`run_sync` scanning 标志无 panic 保护**（beets.rs:451-476）：panic 后标志永真 → 所有扫描 E_SCAN_BUSY。RAII guard。
6. **watchdog 恢复命令与 writer 线程交错**（sidecar.rs:360-377 每条单独拿锁）：用户命令可插进恢复序列中间，注释承诺的"无乱序"不成立。单次持锁拼写或恢复期挂起 writer。
7. **`connect_once` 失败泄漏子进程**（sidecar.rs:171-191）：Err 路径 kill+wait。
8. **host 事件队列 Full 静默丢任意事件**（sidecar.rs:275-278 只对 Disconnected break）：丢 Eof → 卡播放态不切曲（与 P0-2 叠加）。非 Position 事件用阻塞 send。
9. **Spectrum emit 在 player 锁内**（lib.rs:1604-1608）：emit 同步 IPC 序列化拖长锁持有；先收集、drop 后统一 emit。
10. **SMTC 封面 URI `file://C:/...` 少一个斜杠 + `%` 未编码**（smtc.rs:122）：用 `url::Url::from_file_path`。
11. **MediaControls 构造在主线程、使用在专用线程**（smtc.rs:44 vs 62-95）：跨 COM apartment 依赖 WinRT agility，稳妥做法是构造+attach 都进专用线程。
12. **关窗行为无视 closeToTray 设置**（lib.rs:1375-1380 一律 hide）：设置项完全没接线。
13. **Mutex 中毒连锁**：AppState 全部锁 `.expect("... poisoned")`，任一线程 panic 后应用变砖。统一恢复策略。
14. **切走的歌记整曲历史**（lib.rs:1583-1596 对每个 Eof 记 duration 全长）：completed/topTracks/totalMs 虚高。用最近 Position 估算 seconds_played。
15. **scan:progress 无宿主侧节流**（lib.rs:429-432）：10 万文件 = 10 万次 IPC。宿主 100ms 节流。
16. **`library_scan_body` 在 async 上下文同步 DB 往返**（lib.rs:401-407）：spawn_blocking。

### 数据层
1. **路径大小写敏感**（scanner.rs:198-209,267 精确匹配；beets_sync.rs:299,308 用 lowercase）：Windows 下同一文件可重复入库且两套对账语义不一致。统一 `path COLLATE NOCASE` 或 `path_lower` 列。
2. **删除文件无任何处理**（scanner.rs:305-408 无缺席检测；'missing' 状态全仓库无人写入；beets_sync.rs:240 还把行标回 'online' 与自身设计文档矛盾）：删文件后 UI 展示不可播曲目。availability 生命周期统一归 scanner。
3. **cursor 分页 limit+1 探测是死代码**（repo.rs:362-376）：total 为 limit 整数倍时多一次空页往返。用探测行判 has_more。
4. **搜索通配符语义扭曲**（repo.rs:208 `_` 被替换成空格）：`LIKE ? ESCAPE '\'`。
5. **DB worker 无 panic 隔离**（db.rs:87-89）：一个闭包 panic 永久杀死 DB。catch_unwind。
6. **queue load 不归一化 current_index**（queue.rs:66-69）：CASCADE 删行后 index 可越界。clamp。
7. **walk 错误静默吞**（scanner.rs:80）+ **mtime 兜底 0 边缘**（scanner.rs:130-135）+ **每文件一事务**（scanner.rs:345-349，5 万文件 5 万次 COMMIT）。
8. **artwork rename 失败被吞**（artwork.rs:66-67）：DB 有行磁盘无文件且永不重写。
9. **歌词**：无大文件防护（lyrics.rs:36）；offset 可推成负时间（:146-149）；unsynced 模式方括号标签行混入歌词（:185-193）；非 UTF-8 文件名 `to_str()?` 静默无歌词（:43）。
10. **迁移双连接竞态**（db.rs:34-44,56-61）：`BEGIN IMMEDIATE`。

### 音频
1. **`set_enabled(false)` 清环写 tail 破坏单写者前提**（spectrum.rs:94-101）：与消费者 tail.store 竞争，重订阅后可能重复一帧。清环移到消费者线程。
2. **引擎事件通道无界**（engine.rs:73）+ sidecar 写管道可阻塞：改 sync_channel + 满时丢最旧 Spectrum/Position。
3. **EOF 等待循环最长阻塞命令 500ms**（engine.rs:368-377）：间隙处理 Shutdown。
4. **设备丢失/格式切换无自动恢复**（engine.rs:148-153）：拔耳机后引擎空转等新 Load。至少 PlayerActor 对 Error 做一次重 Load。
5. **频谱按固定 48kHz 标定**（spectrum.rs:191-193）：44.1k 源偏移 8.8%。实际采样率经 AtomicU32 传入。
6. **环容量注释数学错误**（spectrum.rs:16：8192/48kHz = 170ms 不是 85ms）。

### UI
1. **QueuePanel 元数据拉取失败无限重试循环**（QueuePanel.svelte:62-77 + library.svelte.ts:54-57）：队列含已删 trackId 或一次 IPC 失败 → effect 永不收敛。缺失 id 负缓存（参照 lyrics-loader 的 attempted 上限）。
2. **拖拽排序轻微移动后仍触发"播放"**（TrackList.svelte:190-194 + drag.ts:80-88）：拖 6px~一行高内松手，排序和切歌同时发生。drag finish 设 click 抑制标志。
3. **歌词自动跟随被自己的平滑滚动抑制**（LyricsPanel.svelte:67-76,86）：程序滚动刷新 `userScrolledAt`，快节奏段落跟随反复脱节。改用 wheel/touchstart/pointerdown 打标。
4. **mock 通道双重信封**（compat/api.ts:186-192,294-312）：浏览器预览模式 beets 状态恒"未找到"。
5. **ConfirmDialog Enter 双触发**（ConfirmDialog.svelte:14-19,30）+ 危险操作默认焦点应落取消。
6. **invoke reject 无 catch**（player.svelte.ts:75、ui.svelte.ts:199-203、ImportPanel 6 处等）：store 层统一 catch→信封。
7. **队列总时长只统计已加载子集**（QueuePanel.svelte:51-56）：随滚动漂移。后端快照带总时长或先隐藏。
8. **ImportPanel 绕过 stores 直调 window.api 6 处**（与 ui.svelte.ts:19-20 约定冲突）；**面板中途关闭丢会话状态**（组件卸载 albums/results 全丢，会话仍后台跑）→ beets 会话状态上移 store。
9. **启动快照竞态**（App.svelte:764-765）：晚到 fetch 覆盖事件新状态，加代次号。
10. **可访问性**：模态无焦点陷阱/焦点不归还；QueuePanel 行 `tabindex="-1"` 键盘完全不可达；TrackList role=grid 残缺；toast/error 条无 aria-live。
11. **无界缓存**：accent.ts:16、QueuePanel meta 无上限（对照组 artwork-blob/track-window/lyrics-loader 都有纪律）。

---

## P3 — 轻微（记录在案）

- spectrum.rs:127-130 FFT 线程 spawn 失败被 `.ok()` 吞，无日志
- engine.rs:412-414 `resample_take` 尾帧 frac>1 时线性外插而非复制末帧
- host main.rs:225-232 `read_line` 无行长上限（可信进程间，低危）
- sidecar.rs:286/311/380 三处 `.expect` 线程创建失败直接 panic
- sidecar.rs:359 `let _ = shared;` 死代码
- spectrum.rs:136/184/195 `compute_bands` 每帧 3 次 Vec 分配可复用
- beets_sidecar.py:415 `_ = _status_payload` 没有调用，注释"提前暴露配置问题"不成立
- lib.rs:1148-1153 `player_move` 是 stub 但命令已注册（已知 TODO）
- py:466-468 错误信封带 4 层 traceback 进 UI（本地可接受）
- 诊断 AtomicBool 混用 SeqCst/Relaxed（纯诊断字段）

---

## 已检查无明显问题的项（覆盖证明，摘要）

- **音频**：SPSC 环内存序配对正确（head Release↔Acquire、tail 同）、判满环绕正确、丢新保旧有单测；Hann 窗/48 分带边界钳制/dB 归一数学正确；tap 点在 drain 前不乘音量（ADR-0006 合规）；SetSpectrum 关闭时生产端确为一个原子读；Seek 状态机（暂停态 seek 不二次 start、失败经 result 带出、未消费 Load 被外层接住）正确；线程退出路径全部收敛；协议握手版本不匹配 exit(2) 不静默；管道重连 generation 防冲突。
- **数据**：SQL 全参数绑定无注入面；beets None 保现值/play_count 等自有字段不被冲/幂等/orphaned 只计不删/单事务回滚均有代码+测试佐证；歌词编码顺序（BOM→严格 UTF-8→GBK）正确，ASCII 不会误判 GBK；LRC 多时间戳/负 offset 解析/稳定排序正确；queue 用 `current_index` 无保留字问题；PRAGMA WAL+FK+busy_timeout 正确；ids uuid v5 稳定。
- **宿主**：Eof→record_history 的 borrow 模式已修对；beets 坑①③④全数正确处理（显式 directory、destination() 先查冲突、mtime 转 int）；大 stdout 流式消费无死锁；smtc_tx 断连安全；托盘退出 flush 队列；单实例最先注册；无命令注入面；artwork 协议路径穿越双重防御。
- **UI**：事件订阅生命周期完整（closed 标志处理卸载竞态）；频谱走 canvas+rAF 不进响应式、订阅/退订一一对应；compat 层零直连 invoke（全量 grep 确认）；契约形状与 Rust 侧一致；TrackWindow 稀疏分页+行级虚拟化 50 万行不重渲染；快速切歌竞态全线有防护（代次号/请求号/LRU revoke）。

---

## 建议修复顺序

1. **P0 四连**：P0-1（一行级可缓解：超时不 `?` + 超时驱动命令循环）→ P0-2（连播断裂，与 P2-宿主#8、#14 同处一起修）→ P0-4（cancel，py 侧十行内）→ P0-3（playlist 过滤 + 补双歌单测试）
2. **P1 数据可靠性**：P1-6（迁移原子性）→ P1-7（codec churn）→ 路径大小写统一
3. **P1 体验**：P1-1（SampleBuffer 容量）→ P1-3（防休眠独立线程）→ P1-4（async 命令）→ P1-5（槽位前置）
4. **防回归补测**：暂停 2s 后 Resume、自然播完连播、cancel 端到端、双歌单隔离、相对路径库幂等 apply
5. P2 按区分批入 pass 2；scanner 批量事务/取消机制本就在 pass 2 计划内
