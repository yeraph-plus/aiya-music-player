# beets 模块解耦与批 1–3 缺陷修复（2026-10-30）

> 本文是**一次纠偏**的记录：批 1–3 的实现方向没错，但有一处 P0 漏接线、四处 P1
> 会咬人、四处 P2 门禁缺失；更关键的是"beets 独立"当时只做到了工具侧，宿主侧
> 仍是黏着的。这次把黏着点逐个切开，并把切开这件事变成可执行的门禁。

## 1. 一句话结论

分析（`enrich`）从"跑在用户的 beets 库上"改成"跑在应用独占的索引家"，
增强结论从"应用库的一张表"搬进"beets 模块自己的 SQLite 文件"，
beets 的配置从"应用的 settings 表"搬进"beets 模块自己的 json"。

结果：**应用的数据模型里不再有任何 beets 痕迹**（有断言守着），
**应用核心链路不 import beets**（有门禁守着），
beets 模块只剩两类单向依赖：读应用的文件清单、触发一次重扫。

## 2. 修了什么

### P0：`enrich` 跑在用户真库上

| 事实 | 后果 |
|---|---|
| `beets.rs::env_pairs` 从不下发 `AIYA_BEETS_PROFILE`，home 恒为 `{app_data}/beets` | `start_enrich` 落进**工作台家**（用户自己的库） |
| `enrich.py` 在 `force` 时先 `item.remove()` 再用**用户的 config** 重导 | 用户的条目被删掉重建；若 config 有 `copy/move` 或插件 `write: yes`，文件被搬/标签被改 |

修法（三条一起，缺一条都还危险）：

1. `Profile::{Workbench, Index}` + `index_home()`；`env_pairs` **显式**下发
   `AIYA_BEETS_PROFILE`（不靠工具侧默认）。索引家 `{app_data}/beets-index`
   与工作台家并列，不是子目录——beets 会往 home 里写 `config.yaml`/`library.db`/
   `state.pickle`，共父目录会让"手滑清工作台"顺带清索引。
2. `enrich_profile()` 抽成函数并被测试盯住：分析改回工作台家必须红。
3. 结论库迁出应用库（见下）。

**为什么这条在批 2/批 3 都没被发现**：docs/45 §11.2 写过"索引家 home 路径还没在
宿主侧落地……谁起这个会话属批 2/3"，而批 3 的 9 条偏差里没有它——批 2 以为批 3
接，批 3 以为批 2 接了。唯一"验证"过这条链路的是那条端到端测试，而它**自己**
塞了 `AIYA_BEETS_PROFILE=index`，正好绕开了生产缺的那一环。

### P0：结论库从应用库搬到模块自有文件

`track_enrichment` 原本在应用库（migration 0008），代价是三条耦合：

| 耦合 | 症状 |
|---|---|
| 写它要抢 `scanning`（应用刷新管线的互斥） | "beets 在分析"会挡住"用户在扫描"，用户看到的是"点扫描没反应" |
| 应用库 schema 里多一张与它无关的表 | 备份/迁移/第三方读库都得理解它 |
| 跟着应用库的生命周期走 | 卸载 beets 组件后它还在 |

现在：

- 新 crate `crates/aiya-beets-store`（不依赖 `aiya-data`），文件
  `{app_data}/beets-index/enrich.db`，自己建表、自己迁移。
- 它的 API 只认 `Arena`（一份 `path_lower → (path, size, mtime)` 快照），
  **不认识"曲库"**：谁提供快照、从哪来，它不关心。
- 不同文件 = 不同文件锁 ⇒ `scanning` 依赖整体消失（不是"收窄"，是没有了）。
- migration 0008 改成**空文件占位**（编号不能删：迁移按数组下标设
  `user_version`，删了会让 0009 的版本号从 9 变 8，跑过 0008 的库会跳过它）；
  0009 负责 `DROP TABLE IF EXISTS`。

### P0：beets 配置搬出应用 settings 表

`beets.toolPath / toolArgs / targetRoot / inboxPath / inboxImported` 五个键
原本存在应用 `settings` 里。它们不是应用设置——应用从不读它们做任何决定。

更糟的是 `clear_target_if_invalid`：**应用**在曲库根变化时去改 beets 的配置，
方向反了。现在改成**读时校验**：勾的输出根不在扫描根里就当没设（`effective_target`
回退第一个根），但**不清用户的选择**——目录加回来时它自己恢复。

- 存储：`{app_data}/beets/settings.json`（原子写：`.tmp` + rename，临时名带
  进程号，避免两个实例共用 `.tmp` 丢改动）。
- 迁移：启动时若 json 不存在，读旧键写 json 并**删掉旧键**（留着等于解耦只做
  一半）。幂等：用户在新文件改过配置后，旧值不会覆盖他。

### P1（四条）

| # | 缺陷 | 修法 |
|---|---|---|
| 1 | `Sink` 没有 `Drop`：`start_enrich` 先起落库线程再 `run_session_with`，后者失败（`Busy`/`Spawn`）时 `on_exit` 不执行 → 线程带 `Arc` 永久泄漏，点一次失败的分析漏一个 | 实现 `Drop` 调 `finish()`；`finish` 用 `finished` 标志做幂等（正常路径 + Drop 兜底只发一条 `enrichPersisted`） |
| 2 | `work_paths` 传 `usize::MAX` 当 limit，`as i64 == -1`，靠 SQLite"LIMIT -1 = 不限"侥幸成立 | 改 `i64::MAX`；注释写明"别传 usize::MAX，那个巧合一旦换绑定就变成只算一条" |
| 3 | `enrich.py` 分析循环一条进度都不发，400 首跑十几秒到几分钟，进度条停在 0/N | 每 5 首一条 `progress`，末条必发；不是每首都发（每条 `enrichItem` 后面再叠一条会让管道流量翻倍，反压的代价是"工具看起来卡住"） |
| 4 | 批 2 的 `query`/`fields` 宿主侧**零命令、零封装**，唯一调用方是冒烟脚本 | 加 `beets_query` / `beets_fields` 命令 + `client.ts` 的 `beets.query/fields` + 预览 mock；client 侧加通用 `request_data`（一次性请求，不解析形状） |

### P1（第五条）：`query.py` 全量反序列化

原本三条分支都是 `list(lib.items(...))`——几万首的库会先把整库读进内存再切片。

现在：

- `raw` / 无规则：`total = len(results)`（走 `SELECT COUNT(*)`，不反序列化）
  + `itertools.islice(results, limit)` 早停。
- 结构化规则：必须扫全库（规则在 Python 侧求值，见模块头注的网络语义理由），
  但**只保留前 limit 条**，内存峰值与库大小无关。

### P2（四条门禁 + 小修）

| # | 修法 |
|---|---|
| 1 | `verify` 加 `check:fmt`（`cargo fmt --all -- --check`）。加之前全仓从未格式化过，所以顺带做了一次性 `cargo fmt`；`scanner.rs` 里原来嵌着一个裸 NUL 字节（测试用的多值分隔），git 因此把整个文件当二进制、任何改动都不可审阅——改成 `"\0"` 转义后它变回文本文件 |
| 2 | `events.ts` 补 `beetsEnrich` 订阅（之前只有 mock 在往 `beets:enrich` 发，等于对着空房间演练）。注释写明"两条终态不冗余"，UI 判本轮结束要等 `enrichPersisted` |
| 3 | `fields()` 不再把 `sqlite3.Error` 吞成 `flexible = []` ——那让"插件装了却查不到"变成"看到一个空列表"。改成整条命令报错，附带原文 |
| 4 | `verify_config` 校验 `library:` 等于 `{home}/library.db`。只判非空时，写错路径会让 beets **静默新建一个空库**，症状是"索引跑完了，统计一直是 0"——配置看起来完全合规 |

## 3. 现在的耦合面

### 3.1 应用 → beets：零

有自动门禁（`tools/check-boundary.mjs`）：

```
核心链路（aiya-audio/aiya-audio-host/aiya-player/aiya-data）不依赖 beets 工具
```

`aiya-data` 里已经删掉 `beets_sync` 模块（`online_paths` 挪成 `repo::online_paths`）
和 `enrichment` 模块（整体搬到 `aiya-beets-store`）。

### 3.2 beets → 应用：两类，都单向

| 用途 | 位置 | 性质 |
|---|---|---|
| 拿文件清单（工作清单 + 查询结果换 `track_id`） | `repo::online_files` / `repo::track_ids_by_paths` | **只读**，各一条 SQL |
| 整理完通知重扫 | `refresh::spawn(Trigger::Action)` | 触发应用自己的逻辑，不改数据 |

`refresh.rs` 对 beets 是零接触（`Trigger::Action` 是通用枚举，注释里说明谁在用）。

### 3.3 数据模型

| 库 | 有无 beets 表/键 |
|---|---|
| 应用库（`aiya.db`） | **无**（0009 删表；`db.rs::migrate_creates_schema` 断言"不许出现 beets 表"） |
| 应用 settings 表 | **无**（迁移后旧键被删） |
| `{beets-index}/enrich.db` | 增强结论（模块自有） |
| `{beets}/settings.json` | 模块配置（模块自有） |

### 3.4 仍然存在、且有意保留的

- `AppState.enrich_store: OnceLock<Arc<Store>>`：应用状态里**持有**一个 beets 类型，
  只为给它进程内生命周期。不是数据耦合，也不是运行时耦合（谁也不因它改变行为）。
- Tauri 命令定义在 `lib.rs`：`generate_handler!` 必须集中注册，实现体都在
  `beets.rs` / `enrichment.rs`。
- `refresh.rs` 发的事件走 `beets:import` 通道名：UI 已在听，改名要动 UI。属于
  "通道名复用"，不是逻辑依赖。

## 4. 行为确认的验证方式

两条原则各自对应一个**能红的**检查，不是"读一遍代码觉得没问题"：

| 原则 | 检查 |
|---|---|
| 应用主体不依赖 beets | `npm run check:boundary`（4 个核心 crate） |
| 应用库没有 beets 表 | `aiya-data::db::tests::migrate_creates_schema` |
| 应用 settings 没有 beets 键 | 启动迁移删键（代码路径；见 §6 未验） |
| 分析不跑用户库 | `beets::tests::enrich_runs_in_index_home` + `env_always_states_profile` |
| 查询能力可用（转歌单的前提） | `beets_query` / `beets_fields` 命令 + `smoke:enrich` 的 `[1] fields` / `[3] query` |

## 5. 负向验证（改坏 → 红 → 撤销 → 绿）

| 破坏 | 期望 | 结果 |
|---|---|---|
| 摘掉 `AIYA_BEETS_PROFILE` 下发 | profile 测试红 | ✅ "profile 必须显式下发，不能靠工具侧默认" |
| `enrich_profile()` 改回 `Workbench` | 索引家测试红 | ✅ "分析改回工作台家 = 改用户的歌" |
| 0009 改成"建表"（模拟表回到应用库） | 应用库 schema 测试红 | ✅ "应用库里出现了 beets 模块的表" |
| `library:` 指向另一个库文件 | 配置闸红 | ✅ `smoke:config` 第 2 组 |
| 新加的测试代码不格式化 | `check:fmt` 红 | ✅ 加完后第一次 verify 就被它拦下 |

最后一条是这次唯一"门禁自己抓住自己"的例子：`check:fmt` 加进 verify 后，
立刻拦下了我刚写的一段未格式化断言。

## 6. 没验到的（别当已验证）

1. **配置迁移的真实路径**：`adopt_legacy_settings` 只在 json 不存在时执行，
   单测覆盖了 `load_at`/`write_at`/`config_view_of`，但"老用户的 settings 里有
   旧键 → 启动搬过去 → 删键"这条链路没有端到端测试（要起整个 Tauri app）。
   验证方式是拿一份有 `beets.toolPath` 的旧库启动一次，看 `settings.json` 出现
   且旧键消失。
2. **落库线程与真实会话失败的组合**：`Drop` 修好了泄漏，但没有测试能构造
   "`Sink::start` 成功 + `start_session` 返 Busy"（需要会话槽被占）。
   代码路径清楚，行为靠阅读。
3. **十万首级** `stats`（现在把整张结论表读进 HashMap 在 Rust 侧做集合运算）：
   1472 首是毫秒级，5 万条结论约 4 MB；`perf_50k` 尚未把 `track_enrichment` 纳入。
4. **配置迁移后旧键是否真的删干净**：`setting_set(..., Null)` 的语义（删行还是
   存 null）取决于 `setting_set` 实现，未单独断言。

## 7. 与下一步（查询页）的衔接

这次只把**能力**接通，没有动 UI。查询页需要的三样东西已经就位：

- `client.beets.query({ raw | rules | fields, limit })` → `BeetsQueryPage`
- `client.beets.fields()` → 字段清单（含插件写入的 flexible 键）
- `client.enrichment.*` → 索引增强的统计/清单/启动

关键在于查询页的产物必须是"**应用内歌单**"，而 `playlist_items.track_id` 是指向
`tracks` 的硬外键。所以"存为歌单"要做的是：

```
beets 查询结果（path 列表）
  → repo::track_ids_by_paths（一次批量反查）
  → 命中的进歌单；未命中的计入"未入库"，不造行
```

这条路径**不需要新表、不需要改查询**，也不该把 beets 的 item id 写进歌单——
item id 只在同一个 beets 库里成立，换库就失效；path 才是跨库唯一能对上的东西。
