# 发布前全量审查修复台账（2026-10-29）

本文登记发布前最后一轮全量代码审查（前后端 + 子进程 + 竞品对比）的处置结果：
哪些修了、哪些明确不修、哪些只能人工验证。方案细节不再展开——每条的一句话
理由即全部，背景可回看审查当时的结论（六路并行审查：宿主/数据/音频引擎/HLS/
状态机+beets-client/UI 基础层+视图层）。

**读法**：`[x]` 已修复（随本台账提交）/ `[ ]` 留守（有明确理由，别凭印象重开）。

## 已修复

### 宿主（src-tauri）

- [x] **泵循环门控错误**：播放中每 500ms 的 Position 广播会触发队列全量落库
  （`save_queue_full` = DELETE + 全量重 INSERT），且持 player 锁。现 `ingest`
  返回 `(effects, acted)`，仅真实下发引擎意图（切歌/停止）才落库，且锁内取数、
  锁外写库；命令路径同样挪出锁。回归：`position_ticks_never_report_acted` 等。
- [x] **scan_cancel 残留**：取消过一次手动扫描后自动刷新永久静默跳过。
  `refresh::run` 拿到扫描互斥后先清零。
- [x] **启动降级**：sidecar 首连失败/二进制缺失不再让整个应用静默退出
  （release 无窗口，`setup` Err = 用户双击后什么都不发生）。现在应用照常起、
  UI 收到一条音频错误事件，看门狗按代重试（杀软拖慢冷启动可自愈）。
- [x] **数据库打不开的用户可见出口**：`fatal_dialog`（Win32 MessageBox）。
  损坏库本身已在 `aiya_data::open` 内 quarantine + 重建，这里兜的是真 IO 故障。
- [x] **看门狗/关闭窄竞态**：`connect_once` spawn 前后各查一次 `shutting_down`。
- [x] **beets 安装器**：加整体超时（20min）+ 行间空闲超时（60s）+ 收尸；
  安装子进程句柄入 AppState 槽，`RunEvent::Exit` 一并回收。
- [x] **beets 段失败退避**：连续失败 ≥10 次自动刷新才重试，手动必跑；
  成功清零。删除失去调用方的 `last_sync_failed`。
- [x] 恢复现场记录按 token 过滤（含 StateChanged）；`historyDays`/`refreshSeconds`
  钳制（含 saturating_mul）；扫描根列表读取失败不再吞成"未配置目录"。

### 数据层（aiya-data）

- [x] **歌单写路径 IMMEDIATE**：7 处先读后写事务统一 `write_tx()`（DEFERRED
  锁升级在 30s 自动刷新持写锁时会 BUSY_SNAPSHOT）；`save_queue_full` 同改。
- [x] **文件改名/移动重链**：upsert 按 `(mtime, size)` 对账索引 relocate——
  保行身份/播放统计/歌单引用，不再产生"一灰一正常"重复行。多条候选不猜。
- [x] **艺人/专辑 ID 派生过 norm()**：大小写/空格/全半角不再裂出重复实体。
  ⚠️ 此项赶在发布前落地——发布后改派生规则要付 ID 迁移代价。
- [x] 智能歌单 LIKE 转义（复用 `escape_like`）；扫描批失败逐条重放隔离坏行；
  producer 取消标志（DB 错误后不再白解析整库）；`DbHandle` 发送端计数
  （close 不再被克隆卡死）；损坏库 quarantine（NOTADB/CORRUPT，瞬态 IO 错误
  刻意不含）；迁移 0005 补 `artists(name_norm)`/`albums(title_norm)` 索引；
  artwork GC（扫描收尾对账删行删文件）；beets_sync relocate 失败按原键放回；
  批量写 prepared 复用 + `IN` 存在性分块查询。

### 音频引擎（aiya-audio / aiya-audio-host）

- [x] **panic 恢复**（两级）：sidecar 构建统一加 `-C panic=abort`
  （`tools/build-sidecar.mjs`，线程 panic = 进程死 = 心跳停 = 看门狗换代重建，
  走既有已测试路径）；engine worker 另用 `catch_unwind` 兜底发
  `EngineEvent::Error{kind:"internal"}`（防裸跑场景）。
- [x] **默认设备变更监听**：独立 watcher 线程每 2s 比对默认渲染端点，
  变更走既有 `pending_switch` 淡出切流路径（播放中在系统里切耳机/音箱即刻生效）。
- [x] **EOF 排空期设备失效**：padding `Err` 与 `Ok(0)` 分开，Err 走设备恢复，
  不再被抹成正常曲终；连带修了重建流后 `last_write` 未重置的隐患。
- [x] 预装载期 `SetVolume` 进 pending（对齐 rate/eq 账法）；频谱 tap 关闭时
  跳过单声道混算（"关闭零开销"声明成立）；删除从未写入的死字段
  `Engine.duration_ms/playing`；HTTP 协议前缀大小写不敏感。
- [x] **HLS 最坏阻塞上界**：`OUTAGE_BUDGET` 20s→8s、HTTP 超时 15s→10s、
  全部 sleep 100ms 分片、拉取耗时计入断网预算回拨。单次 `next_packet`
  最坏 ≈18s（8s 预算 + 一次满 HTTP 超时），修复前实测路径最长 ~62s。
- [x] 段/播放列表字节上限（8MB/2MB，含 gzip 解压炸弹）；played 去重集容量上限；
  Pong 检查移到事件前（不再被 Position 流饿死）；入站行 1MB 上限；
  孤儿退场窗口握手成功后清零。
- [x] clippy `unused_mut` ×2（eq.rs 测试代码，此前 `npm run verify` 门禁是红的）。

### 状态机 + beets-client（aiya-player / aiya-beets-client）

- [x] **会话槽世代竞态**（P0）：`release` 带世代号，旧 reader 迟到清理不再
  偷走新会话 stdin/清掉 busy 位（此前"取消长导入后再操作"是确定性触发，
  后果是两个 beets 进程并发写同一 sqlite）。
- [x] `request_once` 跳过非 JSON 首行（对齐自家注释与 spawn_reader 行为）；
  一次性查询纳入会话互斥（Busy）；取消后 15s 强杀兜底；崩溃补发
  `sessionFailed{E_SESSION_CRASHED}`；spawn 写失败收尸；行读取 10MB 上限。
- [x] **随机 Append 不再整队重洗**：新曲插入未播尾部随机位，已播前缀与
  未播相对序不动（与 Remove 策略对齐）。
- [x] **随机模式播放历史栈**：Prev 优先走历史（容量 1000），洗牌序首也能
  回到上一首；非随机行为不变。
- [x] Loading 期 Pause 记 pending，装载后生效；洗牌种子掺 cursor/version/自增熵
  （同毫秒不再同排列）；hydrate 复用 ToggleShuffle 的钉首策略；index 注释
  以实现为准写清两个下标的区别。
- [x] **依赖纯度**：aiya-player 删除未使用的 `aiya-audio`/`thiserror`/`tracing`
  （此前传递引入 wasapi/windows-sys/ureq，字面违反 crate 自我声明与发布门禁）。

### 前端（ui/）

- [x] **全局快捷键穿透**：`defaultPrevented` 守卫 + isTyping 扩展到可交互元素
  （此前 Tab 到按钮按空格 = 按钮失效 + 播放乱切）。
- [x] **拖拽插入线定位**：计入 scroller 内表头/内容顶偏移，滚动任意位置都贴
  目标行；边缘自动滚动 rAF 自续；补 `body.is-dragging` 样式。
- [x] 翻页失败显示 `[object Object]` → `errorMessage()`；歌单批量删除
  try/finally 对账 + 失败汇总 toast；队列快照版本回退丢弃；applySettings
  请求代次；EQ 下发失败可见提示；beets 终态逐字段收口；启动引导 8s 超时兜底；
  手动扫描收尾去重；倍速滑杆防抖对账；mock 后端补 10Hz tick（e2e 从此覆盖
  进度链路）；智能歌单空数字条件不再落 0；KitView 改 DEV 动态加载；
  确认条移焦 + Esc；SideDrawer Esc 与全局守卫协调、去掉失实的 aria-modal。
- [x] 死代码：overlays.css 旧换皮残留（~200 行）、player.css 孤儿类、
  BrowseView 无效 drag-region、12 处手写空态收口到 EmptyState。
- [x] `--text-3` 对比度提到 ≥4.5:1（深 0.47 / 浅 0.64，按公式核算并保持
  text-2 层级）；grid 行内补 `role="cell"`；过期注释指向 Electron 时代的
  三处修正。

## 明确留守（发布后处理，勿凭印象重开）

- [ ] **跨曲完整 gapless**（预装载双解码器 + 输出流复用）：功能项，不是缺陷。
  本轮已把 Eof→Load 的可压缩延迟压掉一截；剩余接缝 = 排空窗口（音频本来
  就在播）+ 换流成本。发布说明如实写"暂无跨曲无缝"。
- [ ] **HLS 同步阻塞模型根因**：命令黑洞从 20-62s 压到 ≤18s；彻底解决要把
  重试挪进预取线程，涉及 engine/decoder 接口改动，排 v0.2。
- [ ] **F32 输出无削波防护/limiter**：声音设计决策（硬限幅改变听感），
  需要先定产品行为再加。
- [ ] **解码规格中途变化的防御路径**：本地文件规格恒定、HLS 跨段已判死，
  防御分支本身正确；完整处理要走 SetOutputConfig 重建路径，收益低。
- [ ] **TrackList/PlaylistList 行模板合并**：~90% 重复但 800 行级别的重构，
  发布后第一批技术债。
- [ ] **歌单批量删除后端批量接口**（现为逐条 IPC + finally 对账兜底）；
  **mostPlayed/Recent 聚合物化**（索引 0005 已缓解 Artist/Album 排序）。
- [ ] **命名管道 DACL/PIPE_REJECT_REMOTE_CLIENTS**：miow 未暴露该标志，
  手写 SECURITY_ATTRIBUTES ~百行 unsafe；写端未授权（命令注入不成立），
  残余面 = 同机低权进程可读事件流/抢占实例，接受为 v1 风险。
- [ ] **Win11 snap layouts flyout**（需 WM_NCHITTEST 子类化）：自定义窗控的
  固有取舍，拖到屏幕边缘贴边分屏仍可用，发布说明记录。

## 只能人工验证（发布日流程）

- [ ] docs/35 P0-4：干净机「安装 → 运行 → 卸载」闭环（本轮改动涉及安装期
  路径：启动降级、fatal_dialog、许可清单，必须真机过一遍）。
- [ ] docs/35 P0-5：4h soak（重点观察：长时间播放内存/句柄稳定、30s 刷新
  与歌单写并发不再 BUSY、sidecar 换代恢复）。
- [ ] docs/35 P0-6：杀软误报观察（未签名 exe；本轮 sidecar 改为 panic=abort
  构建，行为变化在杀软侧不可预期，更要看）。
- [ ] release.yml workflow_dispatch 空跑 → 四资产齐全 → 打 v0.1.0 tag
  （beets 资产并入同一 release 的设计依赖"先有真 tag"）。
- [ ] 发布说明写明：未签名（SmartScreen 处置）、无自动更新、Opus/WMA/APE/
  WavPack/DSD 与 CUE 暂不支持、无跨曲 gapless、 snap layouts flyout 不可用。

## 竞品对比留档（roadmap 优先级）

P0 感知缺口：Opus（现代分发格式，最痛）、输出设备选择（独占模式卖点被
默认设备绑定对冲）、ReplayGain album 模式 + pre-amp。P1：CUE、Last.fm
scrobble、睡眠定时器、交叉淡化、标签编辑（走 beets modify_items）、内嵌歌词。
依据与详情见审查记录（foobar2000/MusicBee/VLC/musikcube/Dopamine/Feishin 对齐）。
