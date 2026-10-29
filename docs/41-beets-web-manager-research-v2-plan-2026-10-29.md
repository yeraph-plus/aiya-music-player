# beets-web-manager 调研与协议 v2 加固方案

> 状态：**调研完成；方案已评审待实施（本批不改代码）**。日期：2026-10-29。
> 拍板记录见 §8（两条）：① 协议升 v2（含 schemaHash 运行时比对，闭合 P1-4）；② 砍 import 钩子项，换 searchIds 钉死候选 + blankIdentity 空白身份门。
> 前置与关联：docs/08（beets 设计）、docs/29（扫描工具调研）、docs/31（集成验收基线）、docs/33（两侧分离架构）、docs/35（遗留台账）。
> 实施时本文 §6–§7 直接当作施工图；落地记录追加在 §10。

---

## 1. 调研对象与一句话结论

[beets-web-manager](https://github.com/Iranman/beets-web-manager)（下称 BWM）是一个自托管 beets 库管理端。调研结论：

**它不是 beets 套壳，而是把 beets 的交互式导入/变异改造成了可编程的"预览→决策→应用→审计"系统。它的强项在库变异护栏（幂等、锁、审计、证据分层），恰好是我们目前较薄的一层；它的 Web 运维功能（Plex/SLSKD/Lidarr/auth）与我们无关。设计可搬，代码不可依赖。**

| 项 | 值 |
|---|---|
| 许可 / 形态 | MIT；Flask 后端 + React(Next.js 静态导出) 前端，单 Docker 容器 |
| beets 集成 | beets 2.14.1 作为 Python 依赖**嵌进程**（ADR 0006），不走官方 web API、不走 CLI 写路径 |
| 规模 | ~960 commits；`beetsplug/webmanager/` 约 6000 行 beets 原生变异逻辑；backend 约 90 个模块 |
| 健康度 | 2026-07-16 创建，2026-10-28 仍在提交（单人重度开发） |
| 社区 | **14 stars / 0 forks / 0 watchers** —— 不能当依赖引入，只能当设计蓝本 |

---

## 2. 它的架构演化史：ADR 0006 的三次故障（对我们的验证价值）

BWM 最初也是"两个进程"：管理端容器 + 官方 `lscr.io/linuxserver/beets` 容器，通过 HTTP 调 beets 的 web 插件。这条路摔了三次（ADR 0006 Context 原文）：

1. **插件版本漂移**——`webmanager` 插件拷进 beets 容器、重启才生效；管理端先升级就会对着旧插件说话：能力缺失、协议不匹配、`autotag_not_allowed`。
2. **配置耦合**——管理端要往用户 `config.yaml` 写 `plugins:`/`pluginpath:`、配 bearer key、等 Compose 健康序，每步都可能单独挂。
3. **HTTP 桥太慢且失控**——每次读都 JSON 序列化（`/item/` 逐文件统计），长任务靠轮询，线上超时停不掉 beets；beets 2.14 相对路径经 ContextVar 展开，Werkzeug 线程没有这个上下文，还要打补丁。

它最终把 beets 变成自己的依赖嵌进同一进程。**对照我们（docs/33）：这三条教训已被架构消化**——版本化协议 schema 双侧可验（治漂移）、sidecar 代管 config（治耦合）、`dump_items` 批量吐数据（治逐文件序列化）。它选嵌进程有它的理由（单容器自托管），我们不分包跟随：GPL 隔离与发布解耦（docs/33 §8.3）的拍板依然成立。**但 ADR 0006 从反面证明：任何"两个进程 + 桥"的形态，成本都会花在桥的维护上——我们的桥（协议）必须是唯一且严格版本化的耦合面，这正是 P3。**

---

## 3. 可借鉴设计逐项分析

每项格式：**它怎么做（含出处）→ 我们怎么映射 → 现状差距**。

### 3.1 ImportSession 子类化做程序化导入（它最核心的资产）

**它怎么做**：`beetsplug/webmanager/operations.py` 的 `_import_session_class()` 继承 beets 的 `TerminalImportSession`（`beet import -q` 的同款路径），只覆写三个决策点：

- `choose_match(task)`：先转发 `import_task_before_choice` 插件事件（**踩坑注：覆写时必须先发这个事件，否则 badfiles 之类插件的导入前检查被跳过**——他们注释里明确写了这条），再做 pinned release 部分匹配（`_pinned_partial_match`：钉死某个 MB Release，允许缺轨补录），否则回落父类；
- `choose_item(task)` / `get_duplicate_action(task, dups)`：记录 task、打 duplicate 标记；
- `_identity_checked()`：**asis 前的 blank-identity 门**（D3）——按原样落库前检查 tag 非空，空则强制转 skip，"宁可人工审核也不落脏数据"。

`run_import()` 路由上的策略校验同样值得抄：`quiet_fallback=asis` 直接 409 拒绝（`ASIS_REFUSED`——无匹配的专辑按原样入库拿不到已验证身份，这是拒绝不是门禁，任何设置都补不回缺失的证据）；`search_ids` 必须小写 MB UUID 且必须配合 autotag；`duplicate_action` 显式枚举。

**我们怎么映射**：我们的 apply 不走 ImportSession（docs/33 拍板，"所见即所写"成立），**不必重写成它的形态**。可搬的是三个门：blank-identity（→ §6 `blankIdentity`）、pinned release（→ §6 `options.searchIds`，在 preview 侧用 `autotag.tag_album(search_ids=…)` 达成同等效果）、插件事件的教训（→ 被我们逐文件 try/except 覆盖，见 §7 砍除理由）。

**现状差距**：无（映射后补齐）。

### 3.2 幂等操作注册表（durable operation registry）

**它怎么做**：import 路由要求 `Idempotency-Key` 头 + 对 payload 算 fingerprint；`register_operation()` 返回 `collision | exists | new`：同 key 同 payload 重入 → 直接返回上次结果；同 key 不同 payload → 409 `OPERATION_COLLISION`；还在跑 → 202。注册表**落盘持久化**（`_save_durable_locked`），进程重启不丢。

**我们怎么映射**：`requestId`（可选字段）+ `{data_home}/ops.jsonl` 追加式注册表（§6 ops.py）。**诚实的价值评估**（实施前想清楚的三点）：

1. 单实例下重复提交已被宿主**单会话槽**挡掉（第二个 `spawn_session` 直接 `ToolError::Busy`）；
2. 纯写标签的 apply 本来就路径幂等（`PathQuery` upsert 重跑收敛）；
3. 所以注册表的真实价值 = **双实例防护的协议层安全网 + 操作审计日志**（ADR 0004 的 audit 落点，未来 P1-5 任务注册表的数据层）。

**现状差距**：请求 `id` 硬编码为 1（cancel 为 0），宿主与 sidecar 之间没有关联键；sidecar 无任何持久化状态。

### 3.3 单写者纪律 + 库锁 + 启动自检

**它怎么做**：`beets_embedded.py` 在库 db 旁放 flock 文件（`library.db.webmanager.lock`），进程启动独占，抢不到把 holder 报进启动自检；Windows 无 flock 有降级路径；纪律是 **CLI 只读、写必须走受控路径**；自检还带 `READONLY` 标记和"beets 库能否打开、插件是否加载"的 startup self-test。

**我们怎么映射**：见 §6 lockfile.py。关键设计差异——**短进程模型不需要心跳**：锁随进程生死，崩溃自动释放，BWM 需要的 TTL/心跳/stale-reclaim（ADR 0005 里的 120s TTL + 15s 心跳 + heartbeat 过期回收）在我们这里整个消失。锁文件放 `{data_home}/library.lock`（data_home 就是 `{app_data}/beets`，tool 自有领地，见 §5.6）。**读共享、写独占**：status/dump/preview 拿共享锁（或探测），四个变更命令拿独占锁。**为什么值得做**：剩余风险恰好是宿主防线管不到的两处——双开播放器（两个会话槽）和用户在播放器外面手跑 `beet` 写库。

**现状差距**：无锁无自检；`beetsBusy` 让路规则（docs/33 §9 M4）只管播放器自己的两个 sidecar 互斥。

### 3.4 Workflow job contract（ADR 0005 + job_contract.py）

**它怎么做**：所有长任务统一契约：持久化锁 + 心跳 + **持久化 checkpoint**（stage + 可续跑位置）+ 协作式取消（等锁时也响应）+ 重启后 `recovery_required` + `release_interrupted()` 立即释放死进程的锁 + **幂等性要求（重跑/续跑不得重复 staging、重复导入、重复落库）** + 有界且分类的重试。

**我们怎么映射**：不需要它的进程模型（我们一命令一进程），需要的是 **apply 的 per-decision checkpoint**。**实施前分析发现的真实缺口比预想大**：docs/31 A3.5 写的"取消于写入中途 → 重跑能把剩余补上"**只对纯写标签成立**；对 `apply_move`/`apply_copy`，重跑会对已移动的文件报错——源文件已不在 `decisions` 给的路径上，`Item.from_path` 直接失败。也就是说 move/copy 模式下**今天没有任何恢复路径**，checkpoint 补的正是这个洞（§6 checkpoints.py + `resume:true`）。

**现状差距**：apply 是单趟内存循环；cancel 后已写的保留（A3.5 语义）但没有完成记录；宿主看门狗（600s 静默杀进程）杀掉后一切从头。

### 3.5 证据分层的匹配模型（ADR 0002/0003/0007）——直接提升刮削 UI

**它怎么做**：候选按证据强度分层，UI 每个推荐都附带"为什么是这个"：

- 层级：`DETERMINISTIC_TRACK_RECORDING_ID`（嵌入 recording ID 或确认的 AcoustID 指纹）**>** `DETERMINISTIC_METADATA_SLOT`（ADR 0007 新增：**双源印证**——原始下载文件名 + 嵌入 tag 两个独立来源的版本保持归一化标题都精确匹配、精确碟轨位置、时长容差）**>** `RELEASE_GROUP_ID` **>** 相似度；
- ADR 0002：专辑级唯一身份是 `mb_releasegroupid`（edition 级才是 `mb_albumid`），不允许混写；"不能因为一个 edition 级标题不符就否掉 recording/AcoustID/时长/位置证据都一致的 release-group 候选"；
- ADR 0003：**AI 只是可选排序，永远不是身份来源**，缺了要优雅降级；
- 语义区分：`NO_RESULT`（查了，库里真没有——小众曲常见，转人工）vs `UNAVAILABLE`（没查成，key 缺失/断网——可重试）。两者混在一起是最常见的刮削 UX 灾难；
- 三条踩坑教训：① `title_similarity == 1.0` **不可信**——它的归一化剥掉 `(Live)`/remaster/`feat.`，错歌也能得 1.0，所以门禁必须用**版本保持**的 canonical title；② 喂给匹配器的必须是**原始下载名**而不是系统重命名后的名字（否则循环论证——BWM 真的踩过这个 bug）；③ 双源印证缺任何一源就 fail closed 进人工审核。

**我们怎么映射**：§6 evidence.py。我们拿得到 beets `AlbumMatch` 里的 `info.releasegroup_id`、`TrackInfo.track_id`（recording ID）、`item.mb_trackid`（嵌入 recording ID）、原始路径、嵌入 title/track/disc/length——**recordingId / metadataSlot / releaseGroup / similarity 四层都能纯本地诚实计算**，不需要联网（AcoustID 层留给 chroma 插件启用后再加，不虚标）。

**现状差距**：schema 只有 `confidence`（1−distance）、`fromScratch`、`extraItems/extraTracks`、`recommendation`（beets 原生枚举，sidecar 直通 `proposal.recommendation.name`）。UI 有"低置信度不默认勾选"规则，但没有"为什么推荐这个"。

### 3.6 受控变异流程（ADR 0004）

**它怎么做**：所有写库操作统一走 inspect → plan → validate → diff → apply → **audit** → verify → checkpoint → recover；写操作必须有 before/after diff（元数据 + 路径）、审计记录、恢复信息；破坏性操作需要更强证据 + 显式确认。

**我们怎么映射**：apply/adopt 的 `applied`/`adoptDone` 事件已报计数与失败清单；§6 方案里 ops.jsonl 就是 audit 层（时间戳 + requestId + cmd + 终态摘要）。before/after diff 留作后续（§9 明确不做），理由：本批先把"防重复、可恢复、可解释"补齐，diff/undo 是独立的 UI 大项。

### 3.7 值得抄的小细节

- **超时随工作量缩放**：`max(300, min(1200, 180 + 20×文件数))`——我们的看门狗是固定 600s 静默，靠逐文件 progress 当心跳（importer.py:168 注释、206–208），短期不需要改；记录备查。
- **双路径清单**：`allowed_roots`（一般操作）与更窄的 `import_roots`（import 源）分离，**库目录永远不能作为导入源**。桌面单用户场景价值低，但"库根 ≠ 扫描源"值得写进约定（我们的 preview 本来只扫 inbox，天然满足）。
- **启动自检**：beets 起来先自证（库能开、锁能拿、插件能查）再服务。我们的 `serve()` 已有 beets 导入探测 + `_not_installed` 信封退出（server.py:148–156），锁自检并入 §6 status。
- **错误信封附加上下文**：`EnvelopeError` 是 `additionalProperties:true`，他们放 `requested`/`configured`（如 `E_LIBRARY_DIR_MISMATCH` 附两个目录）。我们的 `move_items` 已经这么用了（maintain.py:305–314）——`E_LIBRARY_BUSY` 沿用同一模式放 holder 详情。

---

## 4. 与 AIYA 现状的对照

### 4.1 已有防线（不需要重建）

| 防线 | 位置 | 覆盖 |
|---|---|---|
| 单会话槽互斥 | `session.rs:54–98`（reserve 先于 spawn） | 单实例重复提交 → `ToolError::Busy` |
| 路径幂等 upsert | `importer.py:223–231` | 纯写标签重跑收敛，不产生重复行 |
| 逐文件 progress 心跳 | `importer.py:206–208` | 600s 看门狗误杀防护（docs/12 P1-8） |
| cancel 世代机制 | `runtime.py:21–34`、`server.py:175–178` | 取消即时 ACK，终态唯一（契约测试保证） |
| 双实例让路 | refresh.rs:197–208（`beetsBusy`） | 播放器自身扫描/刷新 vs sidecar 互斥 |
| 协议字节钉死 | `SCHEMA_SHA256` + 双侧契约测试 + `proto:check` | P3 机器可验 |
| 库对账双键 | `beets_sync.rs:280–286,356–401` | 路径优先、`beets_item_id` 兜底，row id 稳定 |

### 4.2 薄弱层（本方案目标）

1. **无请求级幂等**：`id` 恒为 1，无 requestId；双开播放器/未来任务系统裸奔。
2. **move/copy 无恢复路径**：中断后重跑对已移动文件报错（§3.4）。
3. **无库锁、status 无占用感知**：外来写者（用户手跑 beet、第二实例）不可见。
4. **候选无证据解释**：只有置信度数字，用户不知道"为什么推荐这个"；`confidence` 单数字掩盖了"指纹级一致"与"纯相似度"的本质差别。
5. **schemaHash 运行时比对未接线**（docs/35 P1-4）：`status.schemaHash` 报了但宿主不比，安装错版本 sidecar 只能在协议不匹配时才炸。

### 4.3 公平地说，我们领先的部分

- 逐文件进度即心跳（BWM 靠 HTTP 轮询）；
- 协议 schema 机器可验 + 改协议有明文顺序（BWM 的桥协议没有这层）；
- "候选经 UI 回传"的所见即所写：preview 与 apply 不需要共享进程状态（短进程模型下反而更稳）；
- 独立发布解耦 GPL（BWM 嵌进程 = beets 是它的依赖，分发义务更重）。

---

## 5. 现有代码关键事实（实施时直接引用）

### 5.1 协议机制

- 真源 `beets-tool/protocol/v1.schema.json`；播放器镜像 `protocol/aiya-beets-v1.json`（字节级拷贝 + `aiya-beets-v1.meta.json` 存 sha256/toolVersion/tag）；`proto_mirror.rs` 断言镜像一致。
- 改协议顺序（beets-tool/README）：改 schema → `protocol.py` 常量（**改名才 +1，加可选字段不加**）→ 更新 `SCHEMA_SHA256` → `tests/protocol_contract.py` → 播放器侧 `npm run proto:sync`（手工、diff 可见，"不允许静默跟随"）→ `cargo test -p aiya-beets-client`。
- 所有请求/消息 `additionalProperties:false`（唯一例外：`EnvelopeError` 对 `trace`/`requested` 等开放；`preview_import.options` 是自由 object）。**加任何字段都要动 schema**（字节变 → 哈希变）。
- `ErrorCode` 枚举冻结 10 值（`protocol.py:55–66` 与 schema 双向断言相等）：`E_BAD_JSON, E_INVALID_ARG, E_UNKNOWN_CMD, E_PROTOCOL_MISMATCH, E_NOT_INSTALLED, E_NO_LIBRARY_DIR, E_LIBRARY_DIR_MISMATCH, E_BEETS_ERROR, E_CONFIG, E_INTERNAL`；**`E_CANCELLED` 故意不存在**（取消是正常路径）。**`OpError.code` 是自由字符串**（`v1.schema.json:772–781`，现有值 `conflict`）——新码零成本。
- `Session` 模式 `^(imp|app|sync|adopt|update|dup|move|plug)_[0-9]+$`，会话名 `f"{prefix}_{req_id:02d}"` 内联散落在各 cmds 函数。

### 5.2 sidecar 派发（`server.py`）

- `handle(req, gen)` if/elif 链（server.py:29–104）；**handle 持有原始 req dict**（`id`/`cmd` 在 34–35 自取）——集中读新字段不用改 cmds 签名。
- 流式命令返回 None（事件即答案）；终态事件集合 `["done","applyDone","cancelled","sessionFailed","pluginDone"]`（Rust `TERMINAL_EVENTS`，session.rs:305）；错误信封自动加 `type:"sessionFailed"`（server.py:231–234）兼任终态。
- cancel：stdin 读线程 `bump_cancel()` + `cancelAck`（175–178）；每个入队请求快照 gen；各 handler 在循环边界查 `cancel_gen() != gen`。
- `serve()`：stdout 重定向到 stderr（`open_protocol_stream`，防 beets 2.14 的 `Created database backup` 之类裸 print 污染协议流）→ `cfg.ensure_config()` → beets 导入探测，失败 `_not_installed` 退出。

### 5.3 preview/apply（`cmds/importer.py`，252 行）

- 分组：`beetslib.group_albums()` 同目录 = 同专辑；`options.excludePaths` 是 preview 目前唯一 option（85–91）。
- 候选：`Item.from_path` × n → `autotag.Source.from_items` → `autotag.tag_album(source)`；beets 2.14 的 `proposal.candidates` 是 `list[AlbumMatch]`。
- `candidate_payload()`（52–68）：`id`（`cand_{专辑序号:03d}_{候选序号:02d}`）、`confidence`（`1 − distance.distance` 截到 [0,1]）、`fromScratch`、`extraItems`/`extraTracks`、`album`（`album_meta`：id/album/artist/year/genre/dataSource/tracks）、`mapping`（`{源路径: track_info_meta}`，含 DIFF_FIELDS + length）。`recommendation` 是 beets 算的（`importer.py:124` 直通）。
- apply（155–252）：decisions `[{album, action, paths, candidate{album, mapping}}]`；合并 = 专辑级字段打底、单曲 mapping 覆盖非 None；`item.write()` → `PathQuery("path",…)` upsert（同路径在库 = 更新行）→ move/copy 走 beets `Item.move`（`dest_conflict` 冲突不覆盖不改名，`failed[].code="conflict"`）；逐文件 progress 是心跳；终态 `applyDone{applied, skipped, failed, moved}`。

### 5.4 宿主（Rust）

- **请求无类型**：`src-tauri/src/beets.rs` 用 `json!` 手拼（`preview_import`:944、`apply_import`:973、`adopt`:1030、`update`:1039、`duplicates`:1048、`move`:1097、`plugin_install`:1082、`dump`:1130）；`spawn_session` 给每个请求注入 `clientProtocol`（session.rs:117–142）；`id` 硬编码 1（cancel 0）。
- `request_once`（session.rs:191–221）一次性进程跑 status（10s）/plugin_catalog（20s）/dump（180s 每行超时，必须见到 `dumpDone` 否则全弃）。
- status 类型链：schema `StatusData` → `proto.rs StatusReport`（serde rename 驼峰）→ `proto_mirror.rs`（`status_report_fields_match_schema_required` 精确键集相等）→ `beets.rs status_from/absent` → `dto/beets.rs #[ts(export)]` → `npm run gen:types` → UI。**候选/decision 走 Value 透传，Rust 无类型**；UI 手写镜像在 `ui/src/lib/beets.ts`。
- apply 后处理：`start_apply` 的 `on_exit` → `refresh::spawn(Trigger::Action)` → 本地扫描 → `dump_items` → `sync_items`（无 apply→DB 直写）。`beets.inboxImported` 设置键 = 已应用 inbox 路径排除表（防重扫）。
- UI 触点：`BeetsSection.svelte`（status 徽标）、`ImportPanel.svelte`（候选 select ~313–322、apply 汇总 ~243–247）、`beets-session.svelte.ts`（180 行附近"低置信度不默认勾选"）、`ui.svelte.ts:190`（beets 状态）。

### 5.5 测试基线（docs/31 持续有效）

- L1 只读真库 / L2 可回滚隔离 BEETSDIR / L3 只打临时副本；A3.5 = checkpoint 验收原文（"取消于写入中途 | 已写文件不回滚，重跑能把剩余补上"——本方案将其对 move/copy 补真）；A5.x 幂等（adopt 重跑全 skipped；move 重跑全 same）；A6.4 并发（"不许两边同时写"）。
- `protocol_contract.py`：59 离线消息 + 12 cancel 场景；加用例 = 往 `reqs` 列表加 JSON 行 + 扩 `seen`/错误码断言；`run()` 用临时 BEETSDIR + 手写 config（`directory:` + `import.copy: no`）。
- `smoke_maint.py` **断言中文报错子串**（"不一致"、"没有 directory"）——本次不改 `move_items` 报错文案。

### 5.6 可持久化位置

`cfg.data_home()`（cfg.py:29–41）= `AIYA_BEETS_HOME`（播放器传 `{app_data}/beets`）→ `apply_env()` 转 `BEETSDIR`，现有内容仅 `config.yaml` + `library.db`。**tool 在此目录下自有写入权**（P2 不违：这是 beets-tool 的领地，不是播放器 DB）。`dataHome` 已在 status 上报，新状态文件天然可发现。

---

## 6. 方案：协议 v2 加固（施工图）

### 6.1 拍板记录（2026-10-29）

| # | 决策 | 理由 |
|---|---|---|
| 1 | **协议升 v2**：`PROTOCOL_VERSION=2`，schema 改名 `v2.schema.json`，新字段按 required 定（生产端总是计算）；顺带接线 schemaHash 运行时比对 | 用户拍板"升 v2"。闭合 docs/35 P1-4（"等协议升 v2 一起做"）；新字段定为 required 使契约更强——版本协商边界正好兜住旧端。旧宿主（clientProtocol=1）被 v2 sidecar 以 `E_PROTOCOL_MISMATCH` 拒绝，消息说明升级方向 |
| 2 | **砍 `import_task_before_choice` 钩子项**，换 ① apply blankIdentity 门 ② preview `options.searchIds` 钉死候选 | 用户拍板"砍钩子加 searchIds"。钩子在非 ImportSession 架构无机械对应物；badfiles 场景已被逐文件 try/except 覆盖（Item.from_path/write 失败即 per-file error）。searchIds 是 beets autotag 原生能力（`tag_album(search_ids=…)`），且 `options` 是自由对象、协议零结构改动 |

### 6.2 协议变更表（source of truth 一处改，双侧跟随）

| 位置 | 变更 | required? |
|---|---|---|
| `ErrorCode` 枚举 | += `E_LIBRARY_BUSY` | 枚举值 |
| `StatusData` | += `libraryLock`（enum `free\|held\|unknown`）、`libraryLockHolder`（string\|null，free/held 之外可 null） | required |
| `Candidate` | += `evidence`（新 $def `EvidenceTier`：`recordingId\|metadataSlot\|releaseGroup\|similarity`）、`reasons`（string[]，中文展示文案） | required |
| `apply_import` 请求 | += `requestId`（string≤128）、`resume`（bool）；`options` 描述补 `searchIds` 语义（结构零改动） | optional |
| `adopt_library`/`update_library`/`move_items` 请求 | += `requestId` | optional |
| `applyDone` 事件 | += `resumedSkipped`（int≥0，无续跑时 0） | required |
| `OpError.code` 描述 | 补 `blankIdentity` 取值说明（自由字符串） | 文档 |
| 头部描述 | v2 changelog 段 | 文档 |

执行顺序照 5.1：schema → `protocol.py`（VERSION=2、新 SHA256、ERROR_CODES）→ `npm run proto:sync`（生成 v2 镜像 + meta，toolVersion 0.2.0）→ `tools/proto-sync.mjs` 内 v1 硬编码路径同步改 → 双侧契约测试。**`PROTOCOL_VERSION` 的"改名才 +1"在 v2 这次一次性兑现；此后回到增量规则。**

### 6.3 sidecar 新模块（`beets-tool/src/aiya_beets/`）

- **`ops.py`（幂等注册表）**：`{data_home}/ops.jsonl` 追加式；条目 `{requestId, cmd, payloadHash, pid, ts, status: running|done|failed, terminal?}`。
  - `begin(requestId, cmd, payloadHash)`：命中 同 requestId+同 hash+`done` → 返回存储 terminal 供重放；同 requestId 不同 hash → `E_INVALID_ARG`（"requestId 已被不同内容的请求使用"）；`running` → 覆盖（短进程下 running 必为死进程残留；活进程并发已由库锁先行排除）。
  - **终态只重放 `done`**：failed/cancelled 不重放——用户修完冲突重跑必须真执行（A3.5 语义）。payloadHash = 整个请求去掉 `id`/`cmd`/`requestId`/`clientProtocol` 后的规范 JSON sha256。
- **`lockfile.py`（库锁）**：`{data_home}/library.lock`，holder 写 `{pid, cmd, startedAt}` 进文件内容；POSIX `fcntl.flock` / Windows `msvcrt.locking`；API 不可用 → fail-open（照旧执行、status 报 `unknown`，对齐 BWM "锁只是 advisory" 的姿态）。读命令共享锁、`apply_import`/`adopt_library`/`update_library`/`move_items` 独占锁；非阻塞，失败 → `ProtocolError(E_LIBRARY_BUSY)`，holder 详情放信封 extras。**无心跳无 TTL：锁随进程生死**（短进程模型红利，§3.3）。
- **`checkpoints.py`（apply 断点）**：`{data_home}/checkpoints/apply.jsonl` 追加；key = sha256(sorted(decision paths) + candidate hash)；容忍撕裂尾行；按年龄修剪。decision 级粒度（decision 全部文件成功才算完成）。

### 6.4 sidecar 改造

- **`server.py`**：`handle()` 开头集中读 `requestId`；顺序 = **先查 ops 重放 → 再加锁 → 再派发**（重放不触碰库、不加锁）。重放时把存储 terminal 的 `session` 字段改写为当前会话名再 emit（UI 按会话名关联，session.rs 客户端语义不变）。ops recorder 挂在 `runtime`（emit 钩子：事件类型 == 本 cmd 终态时自动 `finish(done)`；`handle()` 异常分支 `finish(failed)`）。cmds 函数签名**不动**。
- **`env.py`**：`status_payload()` += 探锁（try-exclusive 探测 + 读 holder），两字段必填。
- **`cmds/importer.py`**：
  - preview：校验 `options.searchIds`（小写 MB UUID 正则；非法 → `E_INVALID_ARG`），透传 `autotag.tag_album(source, search_ids=…)`；文档注明"对预览中每个专辑组都尝试（通常配合单文件夹使用）"；
  - `candidate_payload()` += evidence/reasons，分层判定逻辑在新模块 `evidence.py`（§6.5）；
  - apply：`resume:true` → 启动载入 checkpoint，已完成的 decision 跳过（不 emit applied，计入 `resumedSkipped`，心跳照常）；每 decision 完成即追加 checkpoint；`applyDone` 带 `resumedSkipped`；逐文件合并后校验 **title 非空、album 非空、artist/albumartist 至少一个非空**，缺失 → `failed[].code="blankIdentity"`（该文件不写不入库不移动，其余继续）。**`adopt_library` 不加此门**——它就是原样入库通道（= `beet import -A`），按设计吃无标签文件；它的空白身份语义由 `dump_items` 后宿主侧展示处理。

### 6.5 evidence 分层判定（`evidence.py`，纯本地、不联网）

对单个候选（AlbumMatch）自上而下取第一个命中的层：

1. **`recordingId`**：全部映射对中 `item.mb_trackid` 非空且与 `TrackInfo.track_id` 相等（大小写不敏感）→ 理由示例"嵌入的 MusicBrainz Recording ID 与候选完全一致（12/12 首）"。
2. **`metadataSlot`**（ADR 0007 双源印证）：**全部映射对**满足——版本保持归一化（**不剥** `(Live)`/remaster/`feat.` 括注；仅 casefold + 空白折叠）后：嵌入 title == TrackInfo.title，**且** 原始文件名主干（剥扩展名 + 剥 `01 - `/`01.` 曲目号前缀）== TrackInfo.title，**且** disc+track 精确对位，**且** 双方时长都在场且差 ≤2s。
3. **`releaseGroup`**：`info.releasegroup_id` 非空（形如 UUID）→ "候选锚定 Release Group {id}"。
4. **`similarity`**：其余；`fromScratch` → similarity + 理由"无外部匹配，按现有标签猜"。

AcoustID/指纹层**预留不实现**（依赖 chroma 插件启用情况，宁可缺层不虚标——BWM 的教训 ③）。

### 6.6 Rust 宿主

- `proto.rs`：`PROTOCOL_VERSION=2`；`StatusReport` += `library_lock`/`library_lock_holder`（Option，serde rename 对齐驼峰）。
- `proto_mirror.rs`：路径 v1→v2；status 键集断言更新；请求仍 13 条、terminal 集合仍 5 个。
- `beets.rs`：4 个变更命令 `json!` += `requestId`（`{cmd}-{unix_millis}-{rand}`，**每次调用新生成**——单实例下宿主不重试，重放是协议层安全网与双实例防护，见 §3.2）；`beets_apply` Tauri 命令 += 可选 `resume` 参数透传；`EXPECTED_SCHEMA_HASH` 自 `protocol/aiya-beets-v2.meta.json` `include_str!` 解析，status 路径比对产出 `schemaMatched`；`E_LIBRARY_BUSY` → 友好文案（"beets 库正被占用：可能是另一个播放器实例或外部 beet 命令，pid={holder}"）；镜像引用改 v2。
- `dto/beets.rs`：`BeetsStatus` += `schemaMatched`/`libraryLock`/`libraryLockHolder` → `npm run gen:types`。

### 6.7 UI（Svelte）

- `ui/src/lib/beets.ts`：`CandidatePreview` += `evidence?`/`reasons?`；事件联合类型 `applyDone` += `resumedSkipped?`。
- `ImportPanel.svelte`：候选行证据徽标（指纹级/双源印证/ReleaseGroup/仅相似度四档，沿用现有徽标样式）；应用失败态加**「续跑」**按钮（重发上次 decisions + `resume:true`）；汇总行显示"跳过（已应用）N"。
- `BeetsSection.svelte`：库占用徽标（空闲/被占用 pid=x/未知）+ schema 不匹配警告条；`schemaMatched:false` 时禁用整理按钮。
- `beets-session.svelte.ts`：保留最近一次 decisions 供续跑；resumedSkipped 透出。
- **不动**：低置信度不默认勾选照旧——evidence 本批只做展示层，不改自动选择策略（§9）。

### 6.8 测试计划

- `protocol_contract.py`：v2/哈希/错误码断言更新；新场景——① 同 requestId 二次请求 → 重放 terminal（用 adopt 验证：重放返回 `added=3` 而真执行是 `added=0 skipped=3`，区分度明确）；② requestId 冲突 → `E_INVALID_ARG`；③ 空身份 candidate → blankIdentity 失败条目；④ searchIds 非法 → `E_INVALID_ARG`；⑤ `applyDone` 必含 `resumedSkipped`。锁并发场景放 lockfile 模块单测（双进程），不塞单进程契约 harness。
- `smoke_apply.py`：新增续跑场景（中途 cancel → `resume:true` 重发 → 断言已完成专辑文件 mtime 未变 + **move 模式下源文件已移走不再报错**——本方案的验收核心）+ blankIdentity 场景。
- lockfile/checkpoints/evidence 三模块单测；`smoke_maint.py` 不动。
- Rust：proto_mirror 更新；dto wire 测试补新字段。
- 收口：`npm run verify`（test:all + clippy -D warnings + gen:types:check + beets smokes，`PYTHONUTF8=1`）+ beets-playground 手工回归（preview → apply → 杀会话模拟中断 → 续跑）。

### 6.9 实施顺序

协议（schema → protocol.py → proto:sync → 契约测试）→ sidecar（ops/lockfile/checkpoints/evidence → server/env/importer）→ Rust（proto.rs → beets.rs → dto → gen:types）→ UI → 测试补齐 → 全量 verify。

---

## 7. 明确不做（含理由）

| 项 | 理由 |
|---|---|
| `import_task_before_choice` 钩子等效转发 | 非 ImportSession 架构无机械对应物；强行做 = 定义一套新插件事件约定，风险/收益不成比例。badfiles 类场景已由逐文件 try/except 覆盖（`Item.from_path`/`item.write()` 失败即 per-file error）。拍板记录 #2 |
| `allowMissingTracks` 部分补轨 | 修复场景（补专辑缺的几首）在播放器里低频；`searchIds` 已覆盖"钉死候选"的主需求 |
| before/after diff + undo | ADR 0004 的完整受控变异是大 UI 项；本批先补"防重复、可恢复、可解释"，diff 留待独立批次 |
| evidence 改变自动勾选策略 | 展示层先行，观察真实使用再定（避免"低置信度不默认勾选"之外再叠新规则引出意外行为） |
| ops.jsonl 的 UI 展示 | 数据层本批就位（兼 audit），渲染留给 P1-5 任务注册表一起做 |
| 超时随工作量缩放（§3.7） | 逐文件心跳已让固定 600s 静默看门狗安全；记录备查即可 |

---

## 8. 遗留登记

| 项 | 说明 |
|---|---|
| `beets-v0.2.0` tag 待发布 | 本方案 toolVersion 升 0.2.0 后，安装轨道 B 的 meta.json 指向该 tag；发布前轨道 B 校验会失败（P1-1"真 release tag 从未切过"的实例） |
| P1-3 | 双轨道干净机器安装验证仍欠（与本批无关，不新增） |
| P1-4 | **本批闭合**：schemaHash 运行时比对随 v2 接线 |
| P1-5 | 任务注册表：本批 ops.jsonl 是其数据层前身 |

---

## 9. 调研来源索引

- BWM 仓库：ADR 0001/0002/0003/0004/0005/0006/0007（docs/adr/）、`beetsplug/webmanager/operations.py`（`_import_session_class`/`run_import`）、`backend/job_contract.py`、`backend/beets_embedded.py`（锁与自检）、`backend/import_service.py`。
- 本仓代码事实（§5）来自 2026-10-29 三路代码探索：sidecar（server/importer/maintain/dump/market/protocol/env/runtime/cfg + tests）、宿主（aiya-beets-client 全部 + beets.rs + dto + UI 触点 + 测试布局）、文档（08/29/31/33/34/35/40 + adr + fixtures）。
- 行号以探索当日（2026-10-29）为准，实施时以符号定位复核。

---

## 10. 落地记录

### 10.0 实施前评审（2026-10-29，第一轮 bug 修补之后）

用户在两批之间做了一轮 beets 兼容 bug 修补（会话槽世代号防迟到清理、取消 15s 强杀兜底、
单行 10MB 上限、一次性查询纳入会话互斥、对账 back_key 修复、genre 迁移等，全部未提交）。
逐行读最新代码后评审结论：**基于现状迭代，不重构 beets-tool**。判据：

1. 修补的每一刀都落在局部，没有一处穿越层界——bug 全是实现层的，没有一个是架构层的；
2. 代码浸着昂贵实测教训（cp936 编码、beets stdout 噪声、`~/Music` 默认值陷阱、uv venv
   无 pip……每条都有"为什么"注释），重写等于重新交一遍学费；
3. ~2000 行 Python + 双侧契约测试资产，测试是真正的护城河；
4. 本方案的五个缺口全部是加法（新模块），不需要先推翻什么。

可证伪的重写边界：beets 3.0 大版本破坏 autotag API / 单曲级决策成为需求（协议 v3 级重设计）/
sidecar 出现常驻形态。三者之外一律迭代。

**对本文的修正**（读码发现）：① 那轮修补把 `query_status`/`query_plugin_catalog` 纳入了
会话互斥——§6.6 的 schemaHash 比对**不能**设计成"每次变更命令前现查 status"（会撞 Busy），
实施改为宿主在 status 探测路径算 `schemaMatched`，UI 据此禁用整理段；② 积弊清单三项
（dump 无取消、market 取消杀不掉 pip、候选 id pattern 999 上限）随本批顺手清掉。

### 10.1 实施记录（2026-10-29，按 §6.9 批次）

**批 1 协议 v2**：`v1.schema.json` → `v2.schema.json`（git mv），全部 §6.2 变更落地；
`PROTOCOL_VERSION=2`、`ERROR_CODES` 11 个、toolVersion 0.2.0；镜像
`protocol/aiya-beets-v2.json` + meta（sha256 `a9f9490e…`，tag `beets-v0.2.0` 指向待发版本）。
`Candidate`/`AlbumPreview` 的 id pattern 放宽到 `[0-9]+`；`Stage` 枚举加 `sync`
（dump 的 cancelled 事件需要阶段值）。

**批 2 sidecar**：新增 `ops.py`（ops.jsonl 登记簿：重放只认 done，payloadHash =
去 id/cmd/requestId/clientProtocol 的规范 JSON）、`lockfile.py`（命令级独占锁，
`LOCKED_CMDS = apply/adopt/update/move/dump`，POSIX flock / Windows msvcrt）、
`checkpoints.py`（decision 级断点，键 = sorted(paths)+candidate 的 sha256，零失败才记账）、
`evidence.py`（四层判定，版本保持归一化，fromScratch 短路 similarity，整层计算
try/except 降级不连累 preview）。server 次序：**重放 → 锁 → 登记 running**；重放改写
session 字段；错误路径 `_op_fail()` 落 failed。apply 的 blankIdentity 门看**合并后**
标签（文件原 tag 可以补候选的缺）。adopt 不设门（原样入库按设计吃无标签文件）。

**批 3 Rust**：`new_request_id()`（`{cmd}-{毫秒}-{进程内序号}`）；`start_apply` 加
`resume` 参数透传到 `beets_apply` Tauri 命令（Option<bool>）；`status_from` 算
`schema_matched`（`include_str!` 嵌入 meta.json 的 sha256，严格比对：没报 = 不匹配）；
`E_LIBRARY_BUSY` 的 holder 同时进 message（Display 原样带 UI）与 extras（机器可读）。
**顺带修掉一个既有 bug**：`EnvelopeError` 误挂 `rename_all = "camelCase"`，而线上
字段是 snake 的 `beets_message`——ok:false 信封永远解析失败（正常 status 走 ok:true
所以从未暴露，v2 拒旧组件的路径一跑就现形）。改回按线上原名字段并修正 wire 锚点测试。

**批 4 UI**：候选卡证据徽标（四档，similarity 弱化显示）+ reasons 行；失败态「续跑」
按钮（`resumeLast()` 重发 lastDecisions + resume:true）；汇总行显示续跑跳过数；
`BeetsSection` 库占用徽标（held 时显示 holder）+ schema 不匹配警告条（不匹配时
整理段整体收起）；mock 的 status 桩补 v2 字段。

**批 5 测试与门禁（实测数据）**：

| 门禁 | 结果 |
|---|---|
| `smoke:proto`（契约） | ALL PASS：59 条消息 + 取消场景 + **新增场景 2.5**（重放 added=3 ≠ 重执行 0、键冲突 E_INVALID_ARG、blankIdentity 拒写 3/3、searchIds 拒绝）；Event 校验 97 行、Response 9 行 |
| `smoke:beets`（apply 冒烟） | ALL PASS：7 场景；**新增 [6] 断点续跑**（move 中断重跑：不带 resume 失败 3/3，resume 后 resumedSkipped=1 且 6 首全在库）、**[7] 续跑不重写**（mtime 逐字节不变） |
| `smoke:maint` | ALL PASS（15 断言，未受影响） |
| `smoke:lock`（新增） | ALL PASS：跨进程互斥 + holder 可读（`pid=…,cmd=…`）、持有者被杀后锁立即释放（5s 内轮询到 free）、本进程接管后 probe=held、同进程续持幂等 |
| `cargo test --workspace` | 415 通过 / 0 失败（含镜像契约 5 条、dto wire 20 条） |
| clippy `-D warnings` | 通过 |
| `gen:types:check` / `proto:check` / `check:boundary` / `check:deps` / `check:ui-layers` | 全部通过 |
| `svelte-check` | 0 errors / 0 warnings |
| `build:ui` | 通过（4.8s） |

**实施中发现并修正的两个环境事实**（后续测试都会再踩）：

1. **Windows 的 msvcrt 字节锁会波及同文件其他字节的读**——锁 [4096,4097) 时用 Python
   `open()` 读头部会直接 `PermissionError`（越界加锁把文件语义整个拽歪）。解法：
   锁与持有者信息**分成两个文件**（`library.lock` 纯锁对象 + `library.lock.holder`
   纯元数据），探针永远不读锁字节。
2. **uv 建的 venv，`python.exe` 是 trampoline**——`Popen.pid` 不是真解释器的 pid，
   `kill` 只杀跳板留下真身继续持锁。测试断言要用持有者**自报**的 pid，清理要用
   `taskkill /F /T` 树杀。播放器真机路径不受影响（sidecar 由宿主直接管理生命周期）。

### 10.2 遗留（更新 §8）

- `beets-v0.2.0` tag：bundle 自检已报 0.2.0，meta.json 指向它；**发布前安装轨道 B 的
  下载校验会 404**——发版时切 tag 即闭合。
- P1-3（双轨道干净机器安装验证）仍欠，与本批无关。
- evidence 的指纹层（AcoustID/chroma）按 §6.5 预留未实现：等 chroma 插件启用后有
  可靠的指纹一致性事实再补层，宁缺勿滥。
- ops.jsonl 的 UI 展示留给 P1-5 任务注册表。

### 10.3 独立审查记录（2026-10-30）

对本批（v2 + 四模块 + 宿主/UI）的独立审查。**修复 3 处（均为本批引入）**：

1. **P1·UI 死胡同**：`schemaMatched=false` 时警告条写"请重装"，但安装按钮只在
   "未启用"分支渲染——用户看得见提示、找不着入口。修复：安装动作抽成 snippet，
   契约不一致分支同样渲染。
2. **P1·canResume 误报**：`cancelled` 事件不区分阶段，预览阶段取消会把**上一批**
   已完成的 decisions 误报成"可续跑"（checkpoint 会全跳过，白转一圈还重跑刷新）。
   修复：按 `stage === 'writing'` 门控；`reset()` 一并清 lastDecisions。
3. **P3·锁 fail-open 时 status 谎报**：锁文件建不了（如数据目录只读）时 acquire
   静默放行、probe 却报 "free"。修复：记 `_unavailable`，probe 如实报 "unknown"。

**核实无问题的关键交互**（逐条对着代码走查）：collect_items 对 `cancelled`/缺失
dumpDone 一律报错丢弃（不会拿半份 dump 对账）；安装 finished.status 走
`readBeetsStatus` 收口；重放路径不碰库不加锁、
session 改写、终态唯一；Recorder 线程安全（cancelAck 无副作用）；checkpoint 键
稳定性成立（UI 回传候选先于首次键计算，续跑重发同一对象）；searchIds 校验先于
任何事件；requestId 冲突在拿锁之前抛出（不留 running 残留）。

**审查顺带发现的既有问题（非本批引入，已被本批修复）**：`collect_items` 起 dump
不经会话槽——v2 之前用户 apply 可与后台 dump **并发写同一 sqlite 库**（真实竞态，
静默发生）；现在被库锁关闭，apply 得到带 holder 的 `E_LIBRARY_BUSY` 明确拒绝。

**接受的权衡（记录不修）**：① schema 哈希严格比对的操作性后果——纯增量 schema
改动（不升版本号）也会让"新工具 + 旧播放器"停用整理段，即任何 schema 字节改动
都要求双侧同步发版；这是 P1-4 的本意（严格不猜），若实际频繁咬人，软化方案是
"protocolVersion 相同时降级为警告"。② pretend update 也拿独占锁并进审计（略过严，
安全方向）。③ dump 期间 apply 被拒（此前是静默竞态，报错文案带 holder，可接受）。
④ 排队期间被取消的请求会先拿锁再在首个检查点取消，ops.jsonl 留少量噪声条目
（有修剪，无害）。

审查后修复的回归验证：`svelte-check` 0/0、`smoke_lock`/`smoke:proto` ALL PASS、
`build:ui` 通过。
