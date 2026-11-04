# 61 · 全链路代码审查收口（2026-11-04）

> 类型：审计记录　状态：已完成
> 关联：docs/35（台账）、docs/ledger/（当日设计事实已按域落账）；上游审查
> 为四路独立深审（beets-tool / Tauri 宿主 / 核心 crates / 前端），方法与
> docs/10/11/30/36 同系。

## 0. 一句话

四路深审共报 1×P0、5×P1、14×P2、约 30×P3；全部处置完毕（修复或明示接受），
其中一条 P2 的审查前提被真机探针证伪、按原实现保留并加测试钉死。
`npm run verify` 全链绿。

## 1. 审查订正（防止后续轮次重蹈）

**共享模式帧数换算（原报 P2）不成立**。审查断言"autoconvert 下
GetBuffer/GetCurrentPadding 的单位是设备 mix 格式的帧"，据此推断 96k 源的
bufferMs 谎报一半、进度偏差 ~100ms。真机探针实测（Realtek mix 48kHz，建流
不 Start、全零数据）：96k 源 200ms 流 GetBufferSize=19200 帧（=200ms×96k），
写入 9600 帧后 GetCurrentPadding 立即=9600、150ms 后=0——**帧计数按传给
Initialize 的源格式走**。现有按源采样率的换算本来正确；改成 mix 率反而会把
延迟/排空估算翻倍。处置：口径抽成纯函数 `padding_ms` + 回归测试 + 证据注释。
教训：微软文档对 autoconvert 帧单位语焉不详，此类断言应先探针后定案。

## 2. 修复清单（要点；逐条事实见 docs/ledger/ 各域）

- **P0 uninstall 子命令 ImportError**（单轨化漏改 uninstall.py 导入，宿主
  卸载计划路径同断）：`venv_python` 内联回 uninstall.py。
- **P1 门禁失真三连**：bins_contract 过期断言（含 xtractor 第 9 节连带暴露
  的同类问题）改按蓝图推导；e2e 两条日期戳注释改写；`tools/check-style-
  allowlist.json` 落位。CI 补齐 9 个 beets smoke（原 6 个——两个真实破坏
  恰好都发生在缺席套件里）。
- **P1 队列删除同步**（宿主）：三条曲库删除命令后 `prune_queue_for_deleted`
  摘除 Actor 队列中的已删 id 并无条件全量落库；否则外键违规使状态落库永久
  静默失败。
- **P2 装载窗口暂停失灵**：`pending_pause` 改翻转语义并补引擎出口
  （`cmd_toggle` 归约后按意图发 Pause/Resume）；读-发-写三段合并进同一
  临界区（SMTC 与 UI 同帧触发的分叉不再可能）。
- **P2 独占协商**：按 `wValidBitsPerSample` 分流（20/24→I24In32、32→新
  I32 编码、其他拒绝该候选）；`best_reason` 保留首个根因。
- **P2 scan 标志 panic 永真**：`library_scan` 挂 FlagGuard（refresh 链
  同款防线补齐）。
- **P2 主线程阻塞命令**：play_query / play_tracks / open_paths / lyrics_get
  / ids_for_selection / library_tracks 全部 async + spawn_blocking；
  `track_refs_by_ids` 分批 IN（900/批）、仅含临时前缀才读队列表。
- **P2 beets 卸载互斥与超时**：`start_uninstall` 共用 `beets_installing`
  CAS；`uninstall_plan` 子进程 30s 超时（超时报错，不回落自算删除）。
- **P2 schema Installer 区漂移**：按单轨化后实际帧重写
  InstallerEvent/InstallerStage，`proto:sync` 全链同步，protocol_contract
  补 dry-run 实线；enrich"提取器未就绪"改 `E_CONFIG`。
- **P2 前端**：facets / refreshBeets 补请求代次；QueuePanel ARIA 结构修正；
  根 `bindings/` 过期快照删除（生成唯一出口 `ui/src/lib/ipc/generated/`）。
- **P3 全部处置**：PlayNext 空队列自动起播；解码中途规格变化报错（首包
  对齐保留）；FFT 旋转因子预生成表；enrich 补登批间取消+可杀子进程；
  apply_import 进度口径；find_duplicates 防御；server 非数字 id 归零；
  Required-by 卸载护栏；stderr UTF-8；`AIYA_BEETS_PYTHON`；sidecar 握手
  10s 超时；`player_set_volume` 保留 muted；缩略图 basename 防御；失败行
  按页重试；播放错误去重键离开 error 复位；QueuePanel 缓存随 ids 清理；
  isRetryable 收敛；`ui.view` 单写入点；动态事件逐字段收口；LyricsPanel
  并入 subscribeRender；`Trigger::Manual` 死代码与孤儿注释删除；beets-tool
  README / crates/README / docs/35 P1-4 / AGENTS.md C9·C10 / 根 README
  协议版本表述同步。

## 3. 明示接受的取舍（不修，留档）

- `playlist_export` 仍接受任意路径：本地单用户应用，UI 侧有保存对话框、
  渲染层被 CSP 收紧，收紧收益不抵交互成本。
- beets 五条同步小命令（config_get 等）仍在主线程做 KB 级文件 IO：量级
  无害，async 化收益接近零。
- `start_install`/`start_plugins_set` 线程 spawn 失败时的占位释放缺失
  （本次为 start_uninstall 补了）：spawn 失败近乎不可能，两处既有路径
  留待下次触碰该文件时顺手补。
- `EventTx` 关键事件 100ms 阻塞窗：设计前提（panic=abort + 看门狗）未变，
  已补注释登记"裸跑模式无保护"。
- 手动刷新入口（Trigger::Manual 原语义）确认删除而非补命令：UI 无此入口，
  需要时随命令一起加回。

## 4. 门禁状态（收口时点）

- `npm run verify` 全链绿（cargo 全量 + svelte-check 0/0 + vite build +
  e2e 44 过 + clippy -D warnings 0 + gen:types:check + style strict 阻断 0
  + 9 smoke 全绿 + check:deps/boundary/proto:check 绿）。
- check-style 存量：L2 延后 ~209 / 语域 report ~82，均为登记过的节奏。
- Python 侧 9 套 smoke 全部进了 CI；首推后需盯一次 runner 实跑（与
  2026-10-27 首跑同一注意项）。
