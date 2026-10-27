# beets 两侧分离架构设计

> 状态：**已拍板（2026-10-27）；M1–M4 全部完成（2026-10-27）**。日期：2026-10-27。
> 拍板结果见 §10：monorepo 独立发布 + 短进程 stdio 胶水 + **播放器不代装 Python**（两条安装轨道）。
> M1 落地记录见 §13，M2/M3/M4 落地记录见 §9 各节（含实测数据与对本文的修正）。
> 前置：docs/29 批 A–D 已交付；docs/31 集成验收计划、docs/32 组件化调研已完成。
> 本文取代 docs/32 §5（分发形态）与 §6（刷新管线所有权），并明确作废批 C 的随包运行时决策。
>
> **一句话结论**：beets 从"播放器的内置依赖"降级为"可独立安装的外部工具"，播放器只留
> client（发现 → 起进程 → 说协议 → 消费结果），中间只隔一份版本化协议。

---

## 1. 为什么必须切（现状耦合点实测）

用户要求"播放器本身只有播放器能力"。按这条尺子量，现在的实现是不合格的——
**播放器的 Rust 代码在替 beets 做决定**：

| 耦合点 | 位置 | 问题 |
|---|---|---|
| Python 源码编进二进制 | `beets.rs:18` `include_str!("../../sidecar/beets_sidecar.py")` | 964 行 Python 成为播放器发布物的一部分；改一行 sidecar 要重发整个安装包 |
| 安装脚本同样编入 | `beets.rs:21` `INSTALL_SCRIPT` | 同上，~200 行 |
| 依赖锁编入 | `beets.rs:24` `include_str!(... beets-requirements.lock)` | 播放器知道 beets 该装哪些包、什么版本 |
| **插件清单硬编码** | `beets.rs:129` `FIRST_WAVE_PLUGINS` | 播放器决定 beets 开哪 6 个插件，且全是猜的（`lyrics` 缺 `bs4` 却照样列出） |
| **config YAML 在 Rust 里拼** | `beets.rs:148` `render_config` | 播放器要知道 beets 的配置语法（`library`/`directory`/`plugins` 键名、YAML 单引号转义） |
| **解释器探测在 Rust 里** | `beets.rs:209` `interpreter_candidates` | 播放器要知道 venv 布局（`Scripts/python.exe`、`pyvenv.cfg` 父目录）、候选优先级 |
| **pip 安装在 Rust 里编排** | `beets.rs:448` `start_install` | 播放器要知道"建 venv → pip install → 复探"这套流程 |
| 随包运行时路径推导 | `beets.rs:63` `beets_paths` | `resource_dir/beets-runtime`、`tools/beets-runtime`、`{app_data}/beets-venv` 三处硬编码 |
| 构建期下载 Python | `tools/pack-beets-runtime.mjs` | 播放器的 `npm run bundle` 依赖下载 Python embeddable + pip |

**总账**：`beets.rs` 1316 行里，真正属于"播放器职责"的只有会话管理、事件转发、
对账调度；剩下约一半是 beets 的环境知识。这些知识一旦 beets 升级就得跟着改播放器——
这正是"两侧维护"要根治的。

还有一个法律侧的连带影响（docs/31 遗留的 P5 项）：现在随包分发 beets（GPL-2.0）
触发 GPL 义务，要维护 `SOURCE-OFFER.txt` 与 notices。**分包且不随播放器分发 beets
二进制之后，这项义务基本消失**（详见 §8.3）。

---

## 2. 边界原则（三条，作为后续评审的尺子）

> **P1 播放器不知道 beets 怎么装、怎么配、开什么插件。**
> 它只知道"有个外部工具，命令是 X，协议版本是 N"。
>
> **P2 beets 侧不知道播放器的数据模型。**
> 它不知道有 `tracks` 表、不知道 `beets_item_id`、不写播放器的 DB。
> 它只吐结构化数据，怎么存是播放器的事。
>
> **P3 协议是唯一耦合面，且必须是机器可读、双侧可验的。**
> 任何"口头约定"（比如"事件名就这么叫"）都不合格。

### 2.1 职责表（终态）

| 能力 | beets 侧 | 播放器侧 |
|---|---|---|
| 判断 Python 是否可用、版本是否在窗口内 | ✅ | ❌ |
| 建 venv / 装 beets / 装 extras | ✅ | ❌ |
| 生成 `config.yaml`、选插件、插件健康检查 | ✅ | ❌ |
| 决定 `library` / `directory` 库根 | ✅ | ❌（只**读** status 报回来的库根用于展示） |
| 调用 beets API（import/update/dup/move） | ✅ | ❌ |
| 协议服务端（NDJSON 收发） | ✅ | — |
| 协议客户端 | — | ✅ |
| 发现工具位置 | ❌ | ✅ |
| 起进程 / 收事件 / 超时 / 取消 | ❌ | ✅ |
| 把 dump 结果写进自有库、路径漂移对账 | ❌ | ✅ |
| 何时刷新（操作后 / 定时 / 聚焦） | ❌ | ✅ |
| 播放、频谱、EQ、歌单、歌词显示 | ❌ | ✅ |

### 2.2 一条容易被忽略的推论

**beets 是增强能力，不是播放依赖。** 没装 beets 时，播放器的全部核心功能
（播放、扫描、歌单、队列、频谱、EQ）必须照常可用。这一条要写成测试断言，
不是口头承诺——现在事实上满足（beets 只在"扫描入库"页出现），但分包之后
更容易被无意破坏（比如 client 初始化失败拖垮启动流程）。

---

## 3. 两侧形态

### 3.1 beets 侧：`beets-tool/`（monorepo 内的独立发布单元）

**拍板形态**：代码在本仓，但有独立的 `pyproject.toml` / 版本号 / tag / 发布流程。
好处是协议变更能在同一个 commit 里两侧一起改、一起跑测试；坏处是 tag 要区分
前缀（`beets-v0.1.0` vs 播放器的 `v0.x`）。

```
AIYA_MUSIC_TAURI/
├── beets-tool/                   # ← 独立发布单元（pip 包名 aiya-beets）
│   ├── pyproject.toml            # [project.scripts] aiya-beets = "aiya_beets.cli:main"
│   ├── protocol/
│   │   ├── v1.md                 # 人类可读协议说明（唯一真源的说明部分）
│   │   └── v1.schema.json        # 机器可读：命令/事件/错误码/字段类型
│   ├── src/aiya_beets/
│   │   ├── cli.py                # 入口：serve / install / doctor / check-python
│   │   ├── server.py             # NDJSON 主循环（现 beets_sidecar.py 的骨架）
│   │   ├── cmds/
│   │   │   ├── status.py         # 环境现状 + capabilities
│   │   │   ├── import_.py        # preview / apply
│   │   │   ├── dump.py           # dump_items
│   │   │   └── maintain.py       # adopt / update / duplicates / move
│   │   ├── env.py                # 解释器探测、venv 管理、Python 版本窗口
│   │   ├── cfg.py                # config.yaml 生成（原 render_config 搬来）
│   │   ├── plugins.py            # 插件清单 + 健康检查（三态）
│   │   └── install.py            # A 轨道安装器（原 beets_install.py）
│   ├── tools/
│   │   ├── build-bundle.mjs      # B 轨道套件包构建（新）
│   │   └── build-installer.mjs   # A 轨道 .pyz 打包（新）
│   └── tests/
│       └── protocol_contract.py  # 对着 v1.schema.json 验自己的输出
├── crates/aiya-beets-client/     # ← 播放器侧 client（见 §3.2）
├── src-tauri/                    # 播放器
└── ui/
```

「独立发布」的具体含义（要在 M2 落地）：

| 项 | 取值 | 说明 |
|---|---|---|
| 版本号 | 独立于播放器（`beets-tool/pyproject.toml` 的 `version`） | 改 beets 侧不发播放器版 |
| git tag | `beets-v0.1.0` 前缀 | 与播放器 `v0.x` 区分，便于按 tag 出发布物 |
| 发布物 | `aiya-beets-installer.pyz` + `aiya-beets-bundle-win-x64.zip` | 见 §6.2 |
| CI | 独立 job（先手动 `npm run beets:release`，P5 再接 GitHub Actions） | `.github` 目前不存在，属 P5 |
| 打包边界 | `beets-tool/` **不进** Tauri bundle | 删 `tauri.conf.json` 的 `resources` 项 |

**与播放器共享的东西只有两样**：协议 schema（真源在 `beets-tool/protocol/`，
播放器镜像一份）、以及 `{app_data}/beets` 这个数据目录约定。

**关键变化（相对现状）**：

1. **`FIRST_WAVE_PLUGINS` 从 Rust 搬到这里**，并且从"静态清单"升级为
   "**声明 + 自检**"：每个插件声明自己需要的依赖，启动时试 import，
   三态上报（`ready` / `missing_deps` / `broken`）。这直接解掉 docs/31 的
   P0-1（`lyrics` 假绿）——因为**只有 beets 侧能知道自己能不能干活**。
2. **`render_config` 从 Rust 搬到这里**。播放器不该知道 YAML 长什么样。
3. **`capabilities` 是新命令**（见 §4.3）。播放器不再硬编码任何插件名，
   也不再从 `config['plugins']` 这个"愿望清单"里读能力。
4. 新增 `doctor` 子命令：人肉可跑的体检（`aiya-beets doctor`），输出
   解释器、beets 版本、库根、每个插件的状态与缺失依赖。出问题时用户不用
   翻应用日志。

### 3.2 播放器侧：`aiya-beets-client`（新 crate，纯 Rust）

```
crates/aiya-beets-client/
├── src/
│   ├── lib.rs        # pub trait BeetsTool { status, capabilities, dump, run(...) }
│   ├── locate.rs     # 发现：env → 配置记录 → 约定路径 → PATH
│   ├── proto.rs      # 协议类型（serde），字段与 v1.schema.json 一一对应
│   ├── session.rs    # 进程生命周期、NDJSON 行读、超时、取消、背压
│   └── error.rs      # 错误分类：未安装 / 版本不符 / 崩溃 / 超时 / 业务错误
└── tests/
    └── proto_mirror.rs  # 对着随仓镜像的 schema 验字段（防冻结后漂移）
```

**这个 crate 里不允许出现的东西**（评审硬门禁，已机器化：`npm run check:boundary`，
脚本 `tools/check-boundary.mjs`，纳入 `test:all`）：

- 不出现解释器/虚拟环境/包管理的痕迹：`venv`、`virtualenv`、`pyvenv.cfg`、
  `site-packages`、`pip`、`python.exe`、`Scripts/`
- 不出现任何 beets 插件名（`replaygain` / `lyrics` / `fromfilename` /
  `duplicates` / `scrub` / `smartplaylist` / `chroma` / `lastgenre` /
  `fetchart` / `embedart`）
- 不出现 `library.db` / `config.yaml` / `path_formats`
- 不出现 `Library` / `MoveOperation` / `ImportSession` 这些 beets 对象模型

**两条刻意的豁免**（写进脚本注释，否则门禁会变成噪音然后被关掉）：

| 豁免 | 理由 |
|---|---|
| `pythonVersion`（只在 `#[serde(rename)]` 处） | 那是**协议线上字段名**，改名就不是这份协议了。crate 的 Rust 标识符是 `runtime_version`，只在 serde 映射处出现真名——这正好是"知道字段、不知道它意味着什么"的分界 |
| `beets` 本身不作禁用词 | 包名 `aiya-beets-client` 与协议字段 `beetsVersion` / `beetsMessage` 必然带它。禁的是**内部概念**（插件名、库文件、对象模型），不是产品名 |

门禁用大小写敏感 + 词边界：否则 `Stdio::piped` 会命中小写 `pip`、
`library_path` 会命中 `Library`——第一版就是这么误报的，全靠词边界才干净。

`beets.rs` 拆分后的归属：

| 现有内容 | 去向 |
|---|---|
| `query_sidecar` / `spawn_session` / `spawn_reader` / `session_slot` / `cancel_session` | → `aiya-beets-client/src/session.rs` |
| `interpreter_candidates` / `beets_paths` / `venv_python` / `system_python` | → **删除**（beets 侧 `env.py`） |
| `render_config` / `FIRST_WAVE_PLUGINS` / `ensure_config` | → **删除**（beets 侧 `cfg.py` / `plugins.py`） |
| `start_install` | → **删除**（beets 侧 `cli.py install`） |
| `start_preview` / `start_apply` / `start_maintenance` / `start_*` | → 保留在 `src-tauri`，改为调 client |
| `collect_items` / `run_sync` / `sync_now` | → 保留在 `src-tauri`（对账是播放器职责） |
| `beets_sync.rs`（295 行） | **原样保留**（数据层，消费 dump 结果，不含 beets 知识） |
| `dto/beets.rs` | 拆：协议 DTO → client crate；UI 展示 DTO → 留原地 |

预计 `beets.rs` 从 1316 行降到 ~400 行。

### 3.3 关于 `beets_item_id` 这个命名（已知的概念泄漏）

`tracks.beets_item_id` 是 beets 概念渗进播放器 schema 的唯一一处。

**建议：保留不改。** 理由：schema v1 已冻结、迁移要动 DB + 对账逻辑 + 测试，
而实际只有 beets 一个 provider——抽象成 `external_item_id` + `external_provider`
是为不存在的第二个 provider 付真实成本。**在注释里标注为"外部工具条目 id，
当前唯一 provider 是 beets"** 即可。

---

## 4. 协议契约

### 4.1 现状盘点（可直接冻结的部分）

已实测：9 条命令、17 种事件、`PROTOCOL_VERSION = 3`、NDJSON over stdio。

| 层 | 命令 | 形态 |
|---|---|---|
| 管理 | `status` / `ping` | 一次性响应 |
| 导入 | `preview_import` / `apply_import` | 会话式事件流 |
| 对账 | `dump_items` | 会话式事件流 |
| 维护 | `adopt_library` / `update_library` / `find_duplicates` / `move_items` | 会话式事件流 |
| 控制 | （取消走 `cancel` 命令） | 一次性响应 + `cancelAck` 事件 |

**这套形状已经经过 4 批迭代与 15 条冒烟验证，不需要重设计。**分包时冻结成 `v1`。

### 4.2 帧格式（冻结）

```
请求（客户端 → 服务端，一行一个 JSON）：
{"id": 7, "cmd": "dump_items", ...args}

一次性响应：
{"ok": true,  "data": {...}}
{"ok": false, "error": {"code": "E_...", "stage": "...", "beets_message": "..."}}

会话事件：
{"type": "progress", "session": "sync_07", ...}
```

三条硬规则（现状已满足，写进协议固定）：

1. **stdout 只允许出现 JSON 行**。beets/依赖库的首写杂音走 stderr 或过滤
   （现状 `_emit` 前有非 JSON 行容错，这是被 2.14 实测逼出来的，保留）。
2. **`id` 必须原样回带**，用于并发命令的请求-响应配对。
3. **会话式命令必须先发终态事件**（`*Done` / `cancelled` / `sessionFailed`），
   然后客户端才关 stdin。缺终态 = 客户端只能等超时。

### 4.3 新增 `capabilities`（解 P0-1 的关键）

```jsonc
{"id": 1, "cmd": "capabilities"}
{"ok": true, "data": {
  "protocol": {"version": 1, "minClient": 1},
  "beets": {"version": "2.14.1", "python": "3.14.7"},
  "library": {"directory": "D:\\CkMsr\\Music", "items": 1845, "dbPath": "..."},
  "features": [
    {"id": "import",     "state": "ready"},
    {"id": "update",     "state": "ready"},
    {"id": "duplicates", "state": "ready", "note": "默认按 title+artist 分组；无 MBID 时不建议用 mb_* 键"},
    {"id": "move",       "state": "ready", "note": "库根取自 config.yaml，未配置则不可用"},
    {"id": "lyrics",     "state": "missing_deps", "missing": ["bs4"], "hint": "pip install beautifulsoup4"},
    {"id": "replaygain", "state": "missing_deps", "missing": ["ffmpeg"], "hint": "需外部 ffmpeg 或 bs1770gain"}
  ]
}}
```

**`state` 三态语义（必须严格）**：

- `ready` —— 试过，能干活
- `missing_deps` —— 缺依赖，`missing` 列表说清缺什么
- `unavailable` —— 本环境不支持（比如无 config directory 时的 move）

**这条是分包的核心收益之一**：能力的真相只有 beets 侧知道。现在播放器从
`config['plugins']` 读愿望清单，于是 `lyrics` 明明 import 失败还报"已启用"
——UI 六项全绿、功能全废。

### 4.4 协议真源与版本策略

**真源放 beets 侧**（它是服务端，服务端定义契约），播放器仓库**镜像一份**：

```
beets-tool/protocol/v1.schema.json      # 真源（随 beets 侧发版）
protocol/aiya-beets-v1.json             # 镜像（播放器仓根，带 sha256 + 来源 beets-v* tag）
```

**格式**：JSON Schema draft 2020-12，用判别联合（`oneOf` + `const`）描述三组消息：

| `$defs` | 判别键 | 说明 |
|---|---|---|
| `Request` | `cmd` | 9 条命令的请求体（含各自的参数） |
| `Response` | 有 `ok` 字段 | `ok:true` 带 `data`，`ok:false` 带 `error` |
| `Event` | `type` | 17 种会话事件 + 错误码枚举 |

选 JSON Schema 而不是自定义 DSL：Python 侧 `jsonschema` 能直接验，Rust 侧
虽然要手工镜像（没有成熟的 codegen），但 `proto_mirror.rs` 可以对着 schema
逐字段断言——比"人肉对照文档"强得多。

两侧 CI 各跑一份契约测试：
- beets 侧：`tests/protocol_contract.py` 验自己的输出符合 schema
- 播放器侧：`proto_mirror.rs` 验 `proto.rs` 的 serde 字段与镜像 schema 一致
- 镜像更新是**手动动作**（`npm run proto:sync`，复制 + 记 sha256），
  diff 里能看到协议变了——不允许静默跟随

**版本语义**：

| 字段 | 含义 | 不兼容时 |
|---|---|---|
| `protocolVersion`（status/ping/capabilities 各报一次） | 服务端支持的版本 | 宿主探测到不符 → `found: false` + `E_PROTOCOL_MISMATCH` |
| `clientProtocol`（每个请求可选携带） | 客户端支持的版本 | 服务端直接拒 → `E_PROTOCOL_MISMATCH`，message 里说清升哪边 |
| `MIN_CLIENT`（服务端常量） | 能接受的最低客户端版本 | 低于它 → 同上 |

**M3 落地时改了一处**：原表里有两个码（`E_PROTOCOL_TOO_OLD` / `E_PROTOCOL_MISMATCH`），
实际只保留 `E_PROTOCOL_MISMATCH` 一个——方向信息放进 message，不放进码。
理由：宿主的分支动作是**同一个**（禁用功能 + 提示升级），分两个码只会让 UI 写两遍
一样的处理；而"往哪边升"是一句话的事，塞进文本比塞进枚举更合适。

**不做自动降级协商。** 版本不匹配就明确报错并说清往哪边升——自动协商会
产生"一半功能能用一半不能"的模糊状态，比直接失败更难排查。

### 4.5 错误码表（**M3 已冻结，10 个**）

冻结前只有 `E_UNKNOWN_CMD` 一个具名码，其余靠 `beets_message` 自由文本。分包后
错误要能被宿主分类处理，必须枚举。**这张表是双向的**：schema 里的 `ErrorCode` 枚举
与 `protocol.ERROR_CODES` 必须一字不差，两侧各有一条测试守着。

| 码 | 触发 | 客户端行为 |
|---|---|---|
| `E_BAD_JSON` | 请求行不是 JSON / 不是对象 | 记日志（调用方拼错了） |
| `E_INVALID_ARG` | 参数缺失或形状不对 | 开发者问题，记日志 |
| `E_UNKNOWN_CMD` | 命令不存在 | 记日志，说明协议漂移 |
| `E_PROTOCOL_MISMATCH` | 版本不兼容（方向在 message 里） | 提示升级，禁用功能 |
| `E_NOT_INSTALLED` | 这个解释器里没有 beets | 引导安装 |
| `E_NO_LIBRARY_DIR` | move 时配置里没有 `directory`，或目录不存在 | 禁用整理按钮，指向扫描目录设置 |
| `E_LIBRARY_DIR_MISMATCH` | move 传入根 ≠ 配置根 | 拒绝（不猜） |
| `E_BEETS_ERROR` | beets 自己抛的（库损坏、权限、缺 ffmpeg…） | 展示 `beets_message` |
| `E_CONFIG` | 数据目录/配置不可写（只读盘、被占用） | 提示换位置或改权限 |
| `E_INTERNAL` | 工具自身 bug（带 `trace`） | 请用户反馈，附 trace |

**两处与原表的差异（M3 拍板）**：

- **`E_CANCELLED` 不做**。取消是正常路径，用 `cancelled` **事件**表达。做成错误码会让
  宿主的"事件流"和"一次性应答"两条路径重叠，还要为一个用户自己按的按钮显示红色。
- **补了 `E_BAD_JSON` / `E_CONFIG` / `E_INTERNAL`**。原表漏了这三类真实大量发生的失败：
  拼错请求、目录不可写、工具自己炸。没有它们，这些情况只能落到某个"大口袋"码上，
  宿主分不出该提示用户、还是该让开发者看日志。

原表里的 `E_NO_DIRECTORY` 与 `E_BAD_ARGS` 已改名为 `E_NO_LIBRARY_DIR` / `E_INVALID_ARG`；
改名是**破坏性变更**，但这次改名发生在契约第一次对外发布之前（代码从未离开本仓），
所以不涉及任何已安装的组件。改名同时修了一个真 bug：`move_items` 的三种拒绝理由
原来抛 `ValueError`，被顶层兜底成 `E_INTERNAL`——宿主只能显示"工具内部异常"，
而这三条恰恰都是用户能自己解决的事。

---

## 5. 胶水形态：三选一

> **已拍板：A（短进程 stdio NDJSON）**。下面保留三方案对比与选择理由，
> 供后来者判断"是不是该换"。

用户提到"worker 或者 client 之类的"。三个候选：

| 方案 | 形态 | 优点 | 缺点 |
|---|---|---|---|
| **A. stdio NDJSON 短进程**（现状） | 每次操作起 `aiya-beets serve`，操作完 EOF 退出 | 零端口、零残留、无并发问题；协议已跑通 4 批 | 每次启动 0.79 s（实测） |
| B. 常驻 worker + 本地 HTTP | 独立进程监听 `127.0.0.1:随机端口` | 多客户端共享；可被外部工具用 | 端口管理、生命周期、防火墙弹窗、僵尸进程 |
| C. 命名管道 | 复用 audio-host 的管道方案 | 与 audio-host 风格一致 | Python 侧跨平台管道实现差异大；Windows 上还得处理 ACL |

### 5.1 推荐 A，理由三条（都有实测支撑）

1. **成本可接受**：`dump_items` 全量 1845 首约 1 s，其中进程启动 0.79 s。
   一次维护操作几秒级，短进程的启动开销淹没在操作本身里。
2. **故障模型最简单**：进程死了就是死了，没有"连着一个坏 worker"这种状态。
   现在 `session_slot` 那套槽位管理（`reserve_session` / `acquire_session` /
   `release_session`）本质就是在用进程模型模拟互斥——短进程天然互斥。
3. **符合 P1**：播放器不需要知道"有个常驻服务在跑"。它只起一个命令、等结果。

**但要给 B 留一条后路**：`session.py` 的接口设计成 `trait BeetsTransport`，
以后要换常驻模式只换实现，不动上层。**现在不做**——30 秒定时刷新如果实测
CPU 占用不可接受，再回来评估（用短进程方案，30 s 一次 = 0.8 s 启动 + 1 s dump，
约 6% 单核占用；如果只做"mtime 变了才 dump"，实际几乎为零）。

### 5.2 取消语义（保留现状）

`cancel` 靠的是一个"取消代数"（`_cancel_gen()`）：主循环收到 `cancel` 就 bump
代数，正在跑的长任务每处理 N 条检查一次，发现代数变了就发 `cancelled` 退出。
这个设计在批 B/D 验证过，分包时原样搬。

---

## 6. 发现机制与引导安装

### 6.1 发现顺序（客户端 `locate.rs`）

```
1. AIYA_BEETS_BIN 环境变量                      # dev / 测试 / 高级用户
2. settings 里记录的绝对路径（beets.toolPath）    # 上次跑成功的位置，两条轨道都写这里
3. {app_data}/beets-tool/venv/Scripts/aiya-beets.exe   # A 轨道（本机安装器）
4. {app_data}/beets-tool/bundle/aiya-beets.cmd         # B 轨道（免安装套件）
5. PATH 里的 aiya-beets                         # 用户自己 pip install 过
6. 都没有 → NotInstalled（正常状态，不是错误）
```

第 2 条优先于第 3/4 条：用户可能两条轨道都装过（先装 A 后来换 B），
settings 里的记录是"最后一次成功的那条"，不该被固定的默认路径盖掉。
第 3/4 条只是"从没成功过，但目录里好像有东西"的兜底——真跑不通会被
`status` 判失败，不会误报可用。

**注意**：不再探测 `python.exe`，不再判断 beets 版本，不再读 venv 布局。
客户端只做一件事：找到 `aiya-beets` 这个可执行文件。

**发现失败时的行为**：`status = NotInstalled`，UI 显示"未安装 / 点此安装"，
其余功能完全不受影响（§2.2 那条推论）。

### 6.2 两条安装轨道（拍板：播放器**不代装 Python**）

用户拍板：播放器不负责准备 Python 环境。发布时在 release 提供两样东西，
用户按自己的情况走其中一条：

| 轨道 | 前提 | 发布物 | 播放器做什么 | 产物落点 |
|---|---|---|---|---|
| **A. 本机安装器** | 本机已有合格 Python（3.10–3.14） | `aiya-beets-installer.pyz` | 下载 → `python <installer> --target <dir>` → 收进度 | `{app_data}/beets-tool/venv/` |
| **B. 免安装套件** | 无 Python / 版本不符 / 不想污染系统 | `aiya-beets-bundle-win-x64.zip`（~110 MB） | 下载 → 解压 → 校验入口存在 | `{app_data}/beets-tool/bundle/` |

**播放器做的是同一件事**：下载 + 展开 + 校验入口在 + 显示进度。「装什么、
什么版本、怎么建 venv」全部由 beets 侧的发布物决定。改依赖只发 beets 新版本，
播放器一行不动。

#### B 轨道的技术前提（已实测 2026-10-27）

B 轨道能否成立，全看「自带解释器的套件包能不能搬到任意路径跑」。批 C 的结论
「venv 不可重定位」容易让人怀疑这条路，但两者是不同的东西：

| 形态 | 可重定位 | 原因 |
|---|---|---|
| venv | ❌ | `pyvenv.cfg` 的 `home` 指向系统 Python 绝对路径，是硬指针 |
| 自带解释器（embeddable / standalone） | ✅ | 解释器、stdlib、DLL 全在包内，`sys.prefix` 由可执行文件位置推出 |

实测方法：把 `tools/beets-runtime`（112 MB）整个复制到
`%TEMP%/aiya-reloc-XXXX/moved`，在新路径下跑

```
$ ./python.exe -c "import sys,beets;print(sys.prefix);print(beets.__version__)"
C://Users//...//Temp//aiya-reloc-GT6p//moved
2.14.1
```

再跑完整协议（不只是 import）：

```
$ printf '{"id":1,"cmd":"status"}\n{"id":2,"cmd":"ping"}\n' | ./python.exe sidecar/beets_sidecar.py
{"id": 1, "ok": true, "data": {"protocolVersion": 3, "beetsVersion": "2.14.1", ...}}
{"id": 2, "ok": true, "data": {"protocolVersion": 3}}
```

`sys.prefix` 跟着走、beets 可导入、协议正常应答 —— **B 轨道地基成立**。

> 顺带一个必须说清的区分：既然自带解释器可重定位，**批 C 的「随包运行时」
> 技术上是可行的**。作废它的理由不是技术不可行，而是**产品与维护边界**
> （不该让不用 beets 的用户也付 112 MB，也不该让 beets 升级绑住播放器发版）。
> 这个区分写在这里，否则后来者会误以为「这条路走不通」而放弃整条 B 轨道。

#### 两条轨道的一致性要求

不管走哪条，播放器的视角必须完全相同：

1. **入口同一个名字**：`aiya-beets`（Windows 上 `aiya-beets.exe` 或 `.cmd`）
2. **`status` / `capabilities` 输出完全一致**（同版本协议）
3. **用户数据位置不变**：`{app_data}/beets`（BEETSDIR）与库根都由 beets 侧定，
   与轨道无关 —— 所以 A 装到一半改走 B，库不用重建
4. **播放器只记录「当前入口路径」到 settings**（`beets.toolPath`），不判断轨道

#### 套件包的构建契约（归 beets 侧 CI）

```
aiya-beets-bundle-win-x64.zip
├── python/              # standalone 解释器（uv python install 的产物）
├── Lib/site-packages/   # beets + 依赖 + aiya_beets
├── aiya-beets.cmd       # 入口：转发到 python/ + 设 PYTHONPATH
└── BUNDLE.json          # 版本、python 版本、构建时间、每个文件的 sha256
```

三条硬要求：

- **不带 pip**（现状 112 MB 里 13 MB 是 pip，运行时用不上）
- `BUNDLE.json` 要能被 `status` 读出来并回带（`source: "bundle"`），出问题时
  能一眼看出用户装的是哪条轨道、哪个构建
- 构建脚本归 beets 侧（`beets-tool/tools/build-bundle.mjs`），播放器不参与

> 注意：B 轨道的 Python 用 **uv 的 standalone build**，不是 Python 官方的
> embeddable。embeddable 要手写 `python314._pth` 决定 `sys.path`（批 C 就踩过
> 这个坑），standalone 自带完整 `sys.path` 处理，长期维护更省事。上面验证用的
> 是现有 embeddable 产物，证明的是「可重定位」这个性质；换成 standalone 只会
> 更容易，不会更难。

#### 为什么不代装 uv

调研阶段曾倾向「本机没 Python 就按需下载 uv（31 MB）由播放器代跑」。拍板改为
**B 套件包**，三个理由：

1. uv 下载 Python 要联网、要处理镜像源、失败模式多 —— 这属于「beets 怎么装」，
   不该让播放器背
2. 套件包是**一次构建、处处可用**的静态产物，播放器只需解压，胶水更薄
3. uv 自己也是要维护的版本（31 MB 二进制升级），包进播放器又回到「绑版本」的老问题

代价是套件包按平台出（win-x64 先做，跨平台是 beets 侧的事）。

#### 探测 Python 的判断权仍在 beets 侧

播放器不解析版本号。它只跑 `aiya-beets check-python`（或安装器自己的前置检查），
拿结构化结果：

```jsonc
{"ok": false, "found": true, "version": "3.15.0",
 "reason": "E_PYTHON_TOO_NEW",
 "hint": "beets 要求 3.10-3.14；本机 3.15 超出窗口，请走免安装套件"}
```

版本窗口是 beets 的事实（`pyproject.toml` 的 `requires-python`），由 beets 侧读。
这条的直接效果：现在宿主里 `interpreter_candidates` 那 40 行硬编码探测整体消失。

### 6.3 卸载

播放器提供"移除 beets 组件"：删 `{app_data}/beets-tool`，保留
`{app_data}/beets`（用户库与配置不删，除非用户明确勾选）。这条要写进 UI 文案
——删库是不可逆的。

---

## 7. 变更刷新（路径 / 元数据变的传播）

这是用户提的第一个问题，与分发形态**独立**——现在全仓没有 watcher、没有
焦点刷新、没有定时重扫（`notify` crate 不在任何 Cargo.toml 里）。

### 7.1 管线（顺序不能颠倒）

```
① 增量扫描（自有 scanner，60 ms / 1846 首）→ 磁盘现状（路径存在性、mtime、标签）
② dump_items（全量，~1 s）                → beets 库现状（item id、path、标签）
③ sync_items（一次事务）                   → 合并、路径漂移跟随、is_orphan 判定
④ 单一 bump（list.version += 1）           → UI 只收到一次刷新
```

**为什么不能反过来**：先对账 beets 会用旧标签写库，紧接着扫描又改一遍 →
两次列表刷新、一次视觉回退。

**为什么不需要"变更水位表"**：`items` 表 97 列但没有 `updated_at`（实测），
做不出可靠的增量；而全量 dump 只要 1 s，维持一张水位表反而多一份状态要维护。

### 7.2 触发点（P1 三个，不上 FS watcher）

| 触发 | 时机 | 说明 |
|---|---|---|
| 操作后 | 任何 beets 命令的终态事件之后 | adopt / move 必刷；update / duplicates 视情况 |
| 定时 | 30 s 空闲时（窗口失焦暂停） | 覆盖"用户在外部工具改了标签" |
| 窗口聚焦 | `WindowEvent::Focused(true)` | 覆盖"用户刚在资源管理器/mp3tag 改完回来" |

**不上 watcher 的理由**：watcher 能带来"即时"，但代价是事件去抖、忽略自身
beets move 造成的回环（"先搬走再清目录"会触发一串事件）、以及每个平台一套
路径规范化坑。这里 30 s 内发现已经够用，而重扫 1846 首只要 60 ms。

**实测成本**：30 s 一次 = 60 ms 扫描 + 1 s dump ≈ 单核 3.5% 的瞬时占用，
平均可忽略；如果实测偏高，优化手段是"mtime 快照未变则跳过 dump"。

**M4 落地时的三条修正**（都是"照原文做会发现不对"的地方）：

1. **"mtime 快照未变则跳过 dump"不是优化，是必需**：定时刷新真正每次都跑的只有
   扫描（几十毫秒），dump（起进程 + 读全库，1 s 量级）只在**磁盘侧真有变化**
   或**命令收尾/手动触发**时才跑。判据见 §9 M4 的 `Outcome::changed`。
2. **"无变化也要 bump"不成立**：每 30 s 发一次事件会让 UI 每 30 s 重拉当前页，
   用户滚动时列表跳一下而什么都没变。改成"无变化不发事件"，手动触发例外
   （用户点了按钮必须看到回应）。
3. **要多一条让位规则**：beets 会话（导入/整理）进行中时刷新必须静默跳过。
   否则两个 sidecar 同时读同一个 beets 库，而且刷新发出去的 `librarySynced`
   会让正在应用导入的界面误以为"对齐完成"，把 applying 状态提前收掉。

### 7.3 所有权（分包后归属）

刷新是**播放器的责任**：它知道自己的 DB 长什么样、什么时候该重绘列表。
beets 侧只提供 `dump_items` 这个能力，不推送变更通知——**beets 侧不知道也
不该知道谁在消费它的数据**（P2）。

---

## 8. 作废的既有决策

用户已明确"既有决策可以放弃"。逐条列出，避免后面对不上：

| 决策 | 出处 | 处置 |
|---|---|---|
| 随包 embeddable 运行时 `resources/beets-runtime` | docs/29 批 C | **作废**。删 `tools/pack-beets-runtime.mjs`、`tools/beets-runtime/`、`tauri.conf.json` 的 `bundle.resources` 项 |
| venv 不能分发（批 C 的结论） | docs/29 批 C §15 | **结论仍正确但不再相关**。它说的是"预构建 venv 不可重定位"，否定不了"在目标机器上现场建 venv"——现在走的就是后者 |
| Python 源码 `include_str!` 进二进制 | docs/08 B0 | **作废**。改从发现的工具跑 |
| 播放器渲染 config.yaml | docs/08 | **作废**。迁 beets 侧 |
| `FIRST_WAVE_PLUGINS` 硬编码在 Rust | docs/29 批 C | **作废**。迁 beets 侧 + 升级为健康检查 |
| 播放器给系统 Python 建 venv（`start_install`） | docs/29 批 C | **作废**。迁 beets 侧 `install.py` |
| 内置整合包验收（docs/31 §5） | docs/31 | **降级为可选离线包**（见 §8.2） |
| 组件注册表 + 双后端（docs/32 推荐） | docs/32 §7 | **整体作废**：注册表不需要了（beets 侧用 `capabilities` 自报）；"播放器代装 uv"也不做（§6.2 拍板 B 套件包），播放器彻底不碰 Python 工具链 |

### 8.1 保留的决策（别顺手推翻）

- `items.path` 存相对路径、绝对路径靠 `Library.directory` 拼 → move 库根只认
  config 原文（docs/29 §16.1 事实 2/3）**继续有效**，只是实现搬到 beets 侧
- `beets_item_id` 双键对账（path 优先，未命中看 item_id）→ 继续有效
- 协议 v3 的命令面与事件形状 → 冻结成 v1（只是重新编号）
- 查重只读、move 两段式（预览 → 确认）→ 继续有效，属安全设计不是形态问题

### 8.2 离线包（可选，后置）

给无网络环境的用户准备"beets + 依赖 + aiya-beets"的离线 wheel 集合。

**但要注意 GPL**：分发 beets 的 wheel 属于再分发，必须提供对应源码
（beets 是 GPL-2.0）。所以离线包必须自带 `SOURCE-OFFER.txt` 与源码地址，
这件事**由 beets 侧仓库负责**，播放器只是提供下载链接/本地导入入口。
不做进常规安装包（P5 再说）。

### 8.3 GPL 义务的变化（一个顺带的收益）

现状：播放器安装包内含 beets → 播放器是再分发者 → 要随包提供源码 offer、
notices、可能还有书面要约。

分包后（走 `pip install beets`）：用户机器直接向 PyPI 请求，播放器不分发
beets 二进制 → **播放器不再是 beets 的再分发者**，GPL 义务大幅减轻。

但要留两个尾巴：
- 播放器仍分发 `aiya-beets`（我们自己的代码），它 `import beets` 但不再分发
  beets → 自身许可自定，不传染（GPL 的"聚合"与"衍生"边界在这里是清楚的：
  独立进程 + 无链接）
- 若做离线包（§8.2），分发场景回来，义务也回来 → 由 beets 侧承担

---

## 9. 迁移路径（四步，每步可单独回滚）

分包最大的风险是"改一半不能用"。所以按**可独立验证**的步子走。

### 9.0 文件级处置清单（M2 执行时按这张表搬）

| 现有资产 | 体量 | 处置 | 去向 |
|---|---|---|---|
| `sidecar/beets_sidecar.py` | 964 行 | 拆分搬迁 | `beets-tool/src/aiya_beets/{server.py, cmds/*.py}` |
| `sidecar/beets_install.py` | ~200 行 | 搬迁 | `beets-tool/src/aiya_beets/install.py` |
| `tools/beets-requirements.lock` | — | 搬迁 | `beets-tool/requirements.lock` |
| `tools/pack-beets-runtime.mjs` | — | **删除** | 换 `beets-tool/tools/build-bundle.mjs` |
| `tools/beets-runtime/`（112 MB，已 gitignore） | 112 MB | **删除** | 换 release 产物按需下载 |
| `tools/smoke_apply.py`（5 场景） | — | 搬迁，断言保留 | `beets-tool/tests/smoke_apply.py` |
| `tools/smoke_maint.py`（15 断言） | — | 搬迁，断言保留 | `beets-tool/tests/smoke_maint.py` |
| `tools/beets-home/`（dev 库） | 3 条 | 搬迁 | `beets-tool/tests/fixtures/beets-home/` |
| `src-tauri/src/beets.rs` | 1316 行 | 拆分 | 见 §3.2 的归属表（预计剩 ~400 行） |
| `crates/aiya-data/src/beets_sync.rs` | 295 行 | **不动** | 数据层，不含 beets 知识 |
| `tauri.conf.json` 的 `resources` | 1 项 | **删除** | 不再随包 |
| `package.json` 的 `bundle` / `pack:beets` | 2 条 | 改写 | `beets:installer` / `beets:bundle` / `beets:release` |
| `ui/src/components/BeetsMaintenance.svelte` | — | 改文案 | 「运行时」→「组件」；能力改吃 `capabilities` |

**搬完之后 `sidecar/` 目录应该消失**，`tools/` 只留播放器自己的脚本
（`run.mjs` / `gen-types.mjs` / `dep-graph.mjs` / `gen-notices.mjs` / `cdp-probe.mjs`）。

### M1 — 抽 client crate，不动行为（无功能变化）

- 新建 `crates/aiya-beets-client`，把 `query_sidecar` / `spawn_session` /
  `spawn_reader` 搬进去，接口做成 `BeetsTool` trait
- Python 侧暂时仍 `include_str!`（**这一步刻意不搬**，保证行为不变）
- 验收：`cargo test --workspace` 全绿、playwright 24 条全绿、功能零变化

### M2 — beets 侧独立化，播放器瘦身

- Python 代码搬到 `beets-tool/`（`sidecar/beets_sidecar.py` → `beets-tool/src/aiya_beets/`，
  `sidecar/beets_install.py` → `beets-tool/src/aiya_beets/install.py`），加 `pyproject.toml`
- `env.py` / `cfg.py` / `plugins.py` 从 Rust 搬过去
- 新增 `capabilities` 命令，UI 改吃它（P0-1 lyrics 假绿在此时消失）
- 播放器删 `include_str!`、`render_config`、`FIRST_WAVE_PLUGINS`、
  `interpreter_candidates`、`start_install`，并删 `tools/pack-beets-runtime.mjs`
  与 `tauri.conf.json` 的 `resources` 项
- 发现机制：env → 配置记录 → 约定路径 → PATH
- 验收：`aiya-beets status` 在三种环境（dev 源码目录 / 系统 PATH / 未装）下
  行为正确；未装时播放器核心功能全通；`cargo build` 产物里搜不到 `beets_sidecar`
  字符串（证明 include_str! 真删干净了）

#### M2 完成记录（2026-10-27）

**搬完的形态**：`beets-tool/` 是完整 Python 包（`aiya-beets` 0.1.0，协议 v3，
`requires-python >=3.10`），有 `pyproject.toml` / `requirements.lock` / `README.md` /
两个发布物构建脚本 / 两条冒烟测试。`sidecar/` 目录已删除，`tools/` 只剩播放器
自己的脚本（`run.mjs` / `gen-types.mjs` / `dep-graph.mjs` / `gen-notices.mjs` /
`cdp-probe.mjs` / `build-sidecar.mjs` / `check-boundary.mjs`）。dev 环境跟着搬：
`beets-tool/.venv`（89 MB）+ `beets-tool/tests/fixtures/beets-home`（1.3 MB），
两者都不入库。

**播放器瘦身实测**：`src-tauri/src/beets.rs` 1316 → 1044 行，但更关键的是**删掉了
哪一类东西**——`include_str!` ×3、`render_config`、`FIRST_WAVE_PLUGINS`、
`interpreter_candidates`、`system_python`、`pick_interpreter`、`venv_python`、
`sidecar_env`、`beets_paths`、`yaml_path`、`web_open` 全部消失。现在这个文件里
没有任何一行知道 beets 怎么装、装在哪、开什么插件。

**发现层四来源**（`discover(app)`）：`AIYA_BEETS_BIN` → 设置 `beets.toolPath`/
`beets.toolArgs` → PATH 上的 `aiya-beets`（Windows 按 `PATHEXT`）→ 源码树
（仅 debug 编译）。**刻意不猜 venv 布局**：猜错的代价是"看起来找到了但跑不起来"，
比找不到更难排障。找到之后一律跑一次 `status` 才认 `found: true`。

**协议等价比对**（新旧实现对同一请求）：`status` 共有字段**零差异**，只多 4 个
新字段（`dataHome` / `pluginHealth` / `pluginSummary` / `pythonSupported`）；
`ping` 完全一致；未知命令码 `E_UNKNOWN_CMD` 与 `type=sessionFailed` 都一致。

**capabilities 落地**（解 P0-1）：`python -m aiya_beets doctor` 实测输出
`ready 4 / missing_deps 2`——`replaygain` 缺 `ffmpeg`、`lyrics` 缺 `bs4`，而这两条
在旧实现里因为 `config['plugins']` 有名字被报成"有"。三态用
`importlib.util.find_spec` 逐个试依赖，**不 import 插件模块**（导入有注册钩子副作用）。

**两个发布物已构建并自检**：`aiya-beets-installer.pyz` **33 KB**（`python tools/
build_installer.py`，zipapp，自检版本 0.1.0 通过）；`aiya-beets-bundle-win-x64.zip`
**30.7 MB**（`python tools/build_bundle.py`，展开 112 MB，`manifest.json` 写入口）。

**踩到的坑（重要，写进 README 也写进长期记忆）**：embeddable 带 `python3xx._pth`
时 Python **忽略 `PYTHONPATH`**。所以套件包必须把 `aiya_beets` 装进
`Lib/site-packages`——第一版把包放在旁边靠 `PYTHONPATH` 喂，`-m aiya_beets` 直接
`No module named aiya_beets`。那条旧测试 `probe_finds_bundled_runtime_beets`
断言的正是"`tools/beets-runtime` 能答 status"，而它本来就答不了（那个目录只有
解释器、没有包，以前靠 `include_str!` 把脚本写盘再 `PYTHONPATH` 指过去）。
现在改成 `probe_finds_bundle_runtime`，验的是 B 轨道套件包**自洽**——测试里仍设了
`PYTHONPATH`，但它被 `_pth` 忽略，所以能过就证明包真在 `site-packages` 里。

**门禁加了两道**（`tools/check-boundary.mjs`）：
1. client crate 无环境知识（M1 已加，12 条规则）；
2. **核心链路不许依赖 beets 工具**——`aiya-audio` / `aiya-audio-host` /
   `aiya-player` / `aiya-data` 的 `Cargo.toml` 里出现 `aiya-beets-client` 即红；
   外加 `sidecar/`、`tools/beets-runtime/` 目录存在即红。这样"未装 beets 的机器
   能完整播放"从承诺变成编译期事实。**负向验证做过**：临时往
   `crates/aiya-data/Cargo.toml` 加一行依赖 → 门禁 exit 1 并指出行号，撤销后 exit 0。

**验证数据**：`cargo test --workspace` **312 passed / 0 failed**（新增
`absent_status_carries_code_and_entry`，扩写 `beets_status_serializes_camel_case`
覆盖新字段）；`npm run verify` exit 0（含 beets apply 5 场景 + 维护 15 断言，
搬迁后路径全改成相对推导，实测 ALL PASS）；playwright **24 passed**；
`svelte-check` 0 error / 0 warning。

**产物字符串实测**（`cargo build -p aiya` 后 `strings target/debug/aiya.exe`）：
`beets_sidecar` 0、`beets_install.py` 0、`beets-runtime` 0，连插件名
`replaygain` / `smartplaylist` / `fromfilename` 也都是 **0**。前半是"include_str!
真删干净了"，后半更能说明问题——**播放器二进制里连"对面有哪些插件"都不再存在**，
这正是 §3.2 想守的那条线。

**UI 侧**：`BeetsStatus` 重新生成（多了 `entrySource` / `pythonSupported` /
`pluginHealth` / `pluginSummary` / `dataHome`）；删掉 `beets_web_open` 命令与
"在浏览器中打开 beets Web"按钮（beets web 缺 flask 必死，docs/31 P0-3）；
安装区从一行 flex 改成块：未装时并排两个按钮（快速安装 / 免安装包，标注各自
前提），装上后显示入口来源 / 库根 / 数据根 + 插件三态明细（缺什么、为什么开它）；
`ScanToolsView` 的"插件"行从逗号串改成逐条三态列表。

### M3 — 协议冻结 v1 + 双侧契约测试（**已完成，2026-10-27**）

计划里的三件事都做了，另有三处是做的过程中发现必须一起做：

| # | 内容 | 落点 |
|---|---|---|
| 1 | schema 定稿并镜像 | `beets-tool/protocol/v1.schema.json`（真源）→ `protocol/aiya-beets-v1.json`（字节副本）|
| 2 | 错误码表落地 | `protocol.py` 的 `ERROR_CODES`（10 个）+ `$defs.ErrorCode` 枚举 |
| 3 | 两侧契约测试 | `beets-tool/tests/protocol_contract.py` / `crates/aiya-beets-client/tests/proto_mirror.rs` |
| +4 | **stdout 独占** | `runtime.open_protocol_stream()`（详见下） |
| +5 | **版本闸门真的接上** | `beets.rs::version_mismatch`（原来只判断不拦截） |
| +6 | **B 轨道能重建** | `build_bundle.py` 自己负责下载 embeddable + 装依赖 |

**为什么协议版本从 3 变成 1。** 分包之前两侧永远同版本一起发，那三个数字是同一份
代码内部的迭代次数，从未作为对外契约存在过。契约第一次对外发布就是 v1，于是
`schema 文件名 v1`、`protocol.version`、`PROTOCOL_VERSION` 三者同一个 1。若保留 3，
以后每次有人看代码都要解释一遍"为什么 schema 叫 v1 而版本号是 3"，而收益是零。

**schema 的形状决策**（都是"冻结时定下来，以后改要付出代价"的那类）：

- **三组入口 + 一组单列**：`Request`（11 条命令）/ `Response` / `Event`（16 种纯事件）
  / `InstallerEvent`（A 轨道安装器的 stdout 帧）。第 17 种会话终态是**带 `type` 的错误
  应答**——它有 `ok` 字段，形状属于 Response，硬塞进 Event 只会让判别逻辑含糊。
- **`additionalProperties: false` 几乎处处开**：拼错字段名必须当场报错。唯一放开的是
  `EnvelopeError`（允许附 `trace` / `requested` / `configured` 等上下文）。
- **`stage` 用 enum**：新增阶段值要改协议。它出现在 UI 文案和进度条逻辑里，不是纯数据。
- **`data` 不约束**：应答的 `data` 形状取决于 `cmd`，在单条消息里无法判别。改为
  schema 里放 `StatusData` / `PingData` / `CapabilitiesData` 三个子 schema，契约测试
  按命令各自验——比把 `data` 写成 `oneOf` 更准，也不会退化成"什么都接受"。
- **`pluginSummary` 的键改成 camelCase**（`missingDeps`）。这条是**修 bug**：工具侧原来
  发 `missing_deps`，宿主 DTO 是 `rename_all = "camelCase"`，而落地写法是
  `from_value(..).ok()` —— 解析失败静默变 None，UI 只是"什么都没显示"。手写 JSON 的
  单测、ts-rs、类型检查全都发现不了，因为两边都是自己写的；只有对着**对面发的** schema
  验才抓得住。原生 bug 就这么躺了一个批次。

**stdout 独占（顺手修掉的硬规则违反）**：beets 2.14 在库迁移时会往 **stdout** 打
`Created database backup at: ...`。以前靠客户端的"裸行容错"活着，但那意味着任何第三方
`print` 都能在事件流中间插噪声。现在 `runtime.open_protocol_stream()` 把原始 stdout
交给协议帧、把 `sys.stdout` 指向 stderr，于是 `print` / warnings / traceback 全落到
stderr。客户端的容错**保留**——它防的是进程外污染（父进程重定向、控制台代码页），
不是我们自己的 bug。这条也让契约测试里"stdout 出现非 JSON 行就红"成为可写的断言。

**版本闸门接上**：`speaks()` 之前只回答"是不是同一版"，没人调用。现在探测后立刻判定：
不符 → `found: false` + `E_PROTOCOL_MISMATCH`，message 里写清升哪边；入口、版本、库根
照样全部显示（用户要能看到"我装的是什么"）。UI 侧对应加了"版本不匹配"的专门分支——
不能让用户以为"没装"而反复安装同一份。

**镜像同步是手动动作**：`npm run proto:sync` 复制 + 记 sha256/版本，`npm run proto:check`
只校验（进 `test:all`）。镜像必须是**字节副本**，不是"我以为等价"的重排版本。
工具还在 `status.schemaHash` 里自报同一个 sha256，这是唯一的运行时凭据——
将来"播放器与组件分别升级"时必须用它。

**验收：负向验证（四条，全部实跑）**

| # | 故意做的破坏 | 期望 | 实测 |
|---|---|---|---|
| A | 工具侧把 `missingDeps` 改回 `missing_deps` | 契约测试红 | `[StatusData] 不符合契约：Additional properties are not allowed ('missing_deps')`，路径 `pluginSummary` |
| B | 客户端把 `dataHome` 的 serde 名改成 `dataRoot` | 镜像测试红 | `StatusReport 与 schema 的 StatusData 不一致：协议有而客户端没建模 ["dataHome"]` |
| C | 只改镜像一个字节 | `proto:check` 红 | exit 1，同时报"字节不一致"与"meta sha256 不符" |
| D | 改真源但忘了 sync | `proto:check` 红 | exit 1，指名跑 `proto:sync` |

**契约测试覆盖**：离线 59 条消息、11 种事件（`progress` / `applied` / `applyDone` /
`item` / `dumpDone` / `updateDone` / `adoptDone` / `duplicateGroup` / `duplicatesDone` /
`moveItem` / `moveDone`）+ 取消场景 12 个请求各自恰好一个终态 + 4 个错误码精确命中。
`album` / `albumError` / `done` 需要联网匹配 MusicBrainz，默认跳过，
`AIYA_BEETS_ONLINE=1` 时一并验。

**B 轨道重建**：`build_bundle.py` 以前依赖播放器仓的 `tools/pack-beets-runtime.mjs`
（M2 已删），于是**干净仓库打不出套件包**。现在它自己负责：下载官方 embeddable →
自己写 `_pth`（必须含 `Lib/site-packages`，否则包 import 不到）→ 按 lock 装依赖 →
放包 → 自检。实测产物 **28.1 MB**（展开 72 MB）。

**验证数据**（全部实跑）：

| 项 | 结果 |
|---|---|
| `cargo test --workspace` | **320 passed / 0 failed**（1 ignored 是 perf_50k） |
| `npm run verify` | exit 0（含 `smoke:proto`：59 条消息 + 12 个取消请求全绿） |
| playwright | 24 passed |
| `npm run proto:check` | 一致，v1，`sha256=630b73816102…` |
| 安装包（`npm run bundle`） | **6.53 MiB**（M2 前 5.74 MiB + 104 MB 运行时；现在只多 0.79 MiB —— 新增的是 zip/ureq 依赖与协议层） |
| 套件包（`npm run beets:bundle`） | 28.1 MB（展开 72 MB） |

**留给 M4 的**：`RELEASE_BASE` 还是占位地址；刷新管线（增量扫描 → dump → sync →
单 bump、30s 定时 + 焦点触发）没动；两条安装轨道的**真实发布物下载**没在真机上走过
（`AIYA_BEETS_INSTALLER` / `AIYA_BEETS_BUNDLE` 能指本地文件，这条路径验过）。
另外"组件与播放器分别升级"的运行时校验只做到 `status.schemaHash` 自报，
宿主还没拿它做比对——等真有第二个版本在世时再接。

### M4 — 刷新管线 + 两条安装轨道（**已完成，2026-10-27**）

计划原文：增量扫描 → dump → sync → 单 bump（§7.1）；30 s 定时 + 焦点触发（§7.2）；
安装 UI 两条轨道（§6.2）；beets 侧两个发布物；验收 = 外部改标签 / 外部移动文件
30 s 内（或切回窗口时）列表自动更新，且两条轨道能各走一遍。

#### 落地形态

| 位置 | 内容 |
|---|---|
| `src-tauri/src/refresh.rs`（新） | 管线本体 + 定时线程 + 焦点闸门 + 快照记账 |
| `crates/aiya-data/src/scanner.rs` | `upsert` 从 `bool` 改三态（`Inserted/Rewritten/Unchanged`）；`ScanResult` 新增 `updated`（真实值）与 `missing` |
| `crates/aiya-beets-client/src/session.rs` | 新增 `busy()`：让位判断只读会话槽 |
| `src-tauri/src/beets.rs` | 删掉 `run_sync`/`sync_now`；`collect_items` 开放给管线；安装物下载/校验重写 |
| `src-tauri/src/dto/refresh.rs`（新） | `RefreshSnapshot` / `RefreshState`（诊断区数据源，ts-rs 生成） |
| `src-tauri/src/lib.rs` | `AppState.refresh`（`Arc<RefreshParams>`）+ `refresh_state` 命令 + `WindowEvent::Focused` 接线 + 启动定时线程 |
| `ui/src/stores/scan.svelte.ts` | `watchAutoRefresh()`：收到 `libraryRefreshed` 且 `changed` 才走 `afterScan()` |
| `ui/src/views/ScanToolsView.svelte` | 诊断区新增「自动刷新」块（上次时间 / 触发源 / 结果 / 耗时） |
| `ui/src/components/SettingsPanel.svelte` | 「曲库自动刷新」：关闭 / 30 秒 / 2 分钟 / 5 分钟 |

#### 一处必须先修的前提：扫描器不报"更新"

管线要靠"磁盘侧变了没有"决定跑不跑 dump，而**旧扫描器答不了这个问题**：
`upsert` 只回 `bool`（是不是新行），于是"重新读了标签并写了行"这一档被并进
`unchanged`。用户用 mp3tag 改完标签，扫描结果永远是"跳过 1846 首"。

改成三态之后数字才有意义（`ScanResult.updated` 以前恒 0，是个从未被填的字段）：

```
scanned 1846 · imported 0 · updated 2 · unchanged 1844 · missing 0 · failed 0
                                       ↑ 这两首就是外部改过标签的
```

`missing` 单独一个字段而不是并进 `updated`：它是"库里有、磁盘没有"，与
"磁盘有、重新读了"是相反方向的变化，混在一起就没法向用户解释。

#### 管线（顺序不能颠倒，理由见 §7.1）

```text
① scanner::scan_roots_ext   （mtime+size 预筛，没变连标签都不读）
② beets::collect_items      （dump_items 全量，仅在需要时）
③ beets_sync::sync_items    （分块事务，路径漂移跟随）
④ 一次 libraryRefreshed     （changed 才发）
```

**②③ 什么时候跑**（这是"定时刷新几乎零成本"的全部秘密）：

| 触发 | 扫描 | beets 段 |
|---|---|---|
| `timer` / `focus` | 每次都跑 | 仅当 ① 报 `imported+updated+missing > 0`，或上次 beets 段失败过 |
| `action`（apply/adopt/move 收尾） | 跑 | **总是**（adopt 只登记不写文件，磁盘侧看起来"没变化"，但 `beets_item_id` 必须落地） |
| `manual`（手动按钮） | 跑 | **总是**（用户点了就说明他不信缓存） |

"上次 beets 段失败过 → 这次强制重试"是一条容易漏的规则：没有它，一次瞬时失败
（工具刚崩、库被锁）会让 beets 侧**永久**不同步——定时刷新看磁盘没变就跳过，
而用户完全无从知道"那几首的 `beets_item_id` 一直没落地"。

#### 单 bump 的口径

```rust
changed = scan.imported > 0 || scan.updated > 0 || scan.missing > 0
       || sync.updated > 0 || sync.imported > 0
```

`failed` **不算**变化：一个坏文件每 30 s 让列表重拉一次，看起来像在抽风。
它进快照与日志，但不触发重绘。`changed == false` 时事件根本不发（手动触发除外）。

#### 让位规则（三条，都实跑验证过）

| 情况 | 行为 |
|---|---|
| `scanning` 已被占（手动扫描 / 上一次刷新没完） | 自动触发静默放弃；手动触发回 `libraryRefreshSkipped{reason:"busy"}` |
| beets 会话进行中（导入/整理） | 同上，`reason:"beetsBusy"`。**这条是必须的**：两个 sidecar 同时读同一个 beets 库不只是锁竞争，刷新发出的 `librarySynced` 会让正在 applying 的界面以为"对齐完成"，把状态提前收掉 |
| 窗口失焦 | 定时器空转（不扫），保留聚焦触发 |

#### 触发点接线

- **定时**：一条独立线程（`library-refresh-timer`），`recv_timeout(间隔)` 超时即刷；
  与聚焦触发**共用一条线程**，天然不会两次刷新并发写库。
- **焦点**：`WindowEvent::Focused(true)` → `refresh::set_focused()` → 往同一条线程塞一条 `Wake::Focus`。
- **启动后补一次**：延迟 3 s（给 boot 的首屏查询让路），覆盖"上次退出到现在改过标签"。
- **设置变更**：`app_set_settings` 里 `wake_settings()` 叫醒线程重读间隔，
  否则"改成 2 分钟"要重启才生效。

#### 前端：只认一条事件

`libraryRefreshed` 是管线终态，`scan.watchAutoRefresh()` 收到且 `changed` 才
走 `afterScan()`（`loadStats` + `loadPlaylists` + `list.reload({restore:true})`）。
与手动扫描共用同一条收尾路径——两条链路"扫完之后该做什么"必须一致，否则
"自动刷新后专辑封面没了"这类偏差极难查。

`librarySynced` 保留（`beets-session` 的 `syncVersion` 还在用，ImportPanel 靠它
在导入完成后重拉），两条事件是同一件事的两层，不是重复。

**自动刷新刻意不弹提示**：它执行的是"用户在外面改了东西，界面自己跟上"，
每 30 秒弹一次 toast 会让人以为出了事。要确认它活着看诊断区。

#### 安装轨道：从"能跑"到"可验证"

原文只写了"下载 → 展开 → 校验入口"。M4 补了三件让它能被验证的事：

1. **`RELEASE_BASE` 可配置**：`AIYA_BEETS_RELEASE_BASE` 环境变量 > 常量默认。
   真发布时改常量或让 CI 指过去，播放器不用发版；
   本地可以用 `python -m http.server` 把整条下载路径真跑一遍。
   *（2026-10-27 补记：原先中间还有一层「设置 `beets.releaseBase`」，因没有任何 UI
   入口——设置页、schema、mock 都没有——已删除，见 docs/35 P1-2。留着等于写了没人能设。）*
2. **发布物形状校验**（`verify_asset`）：两条轨道的产物都是 zip 容器。
   没发布时 GitHub 回的是一个 **200 的 HTML 错误页**，`ureq` 不认为是错误——
   不校验的话用户看到的是后面解压时报的"不是有效 zip"，完全指不到原因。
   现在会明说"不是有效的压缩包（N 字节）——多半是下到了错误页"。
3. **展开逻辑可单测**：`install_bundle_to(target, zip, emit)` 从 `AppHandle` 上摘下来，
   用假套件包验证"manifest 说哪个入口就记哪个"、"没 manifest 走约定"、
   "一个入口都没有就明确失败"。

#### 验收（实跑数据）

| 项 | 结果 |
|---|---|
| `cargo test --workspace` | **332 passed / 0 failed**（1 ignored 是 perf_50k） |
| 新增单测 | 管线判据 3 条（`action/manual` 强制、自动触发静默、失败不算变化）+ 安装轨道 5 条（真实 HTTP 下载落盘、HTML 错误页拒绝、空文件拒绝、manifest 入口、无入口失败） |
| `npm run verify` | exit 0（含 `proto:check`、三个 smoke） |
| `npm run check`（svelte-check） | 0 errors / 0 warnings |
| playwright e2e | 24 passed |
| dev 真机 | 见下（用户实机验收步骤） |

**"真实 HTTP 下载"是怎么测的**：单测里起一个只 `accept` 一次、回一段固定字节的
迷你 TCP 服务（`TcpListener::bind("127.0.0.1:0")`），让 `download_to` 真走 ureq
的请求/响应/写盘，再逐字节比对。这条覆盖的是"下到的字节是否原样落盘"与
"形状校验是否放行"，与是否有外网无关。

#### 真机验收步骤（三条，对应原文的验收标准）

1. **外部改标签**：用 mp3tag 改一手库里某首的标题 → 切回播放器（或等 30 s）→
   列表里的标题变了，且「扫描入库 → 诊断 → 自动刷新」显示"更新 1、触发：切回窗口"。
2. **外部移文件**：在资源管理器里把一首歌移到库内别的位置 → 同上，那一行变成
   "离线 1"（`missing`），文件移回则恢复（扫描器 upsert 的 `availability='online'` 兜底）。
3. **两条安装轨道**：本机有 Python 走 A（`.pyz` → venv），没有走 B（zip → 展开）→
   两条都以 `status.found == true` 为成功判据。用 `AIYA_BEETS_INSTALLER` /
   `AIYA_BEETS_BUNDLE` 指本地产物即可复现，不必等 release。

#### 仍欠的

- **真 release 地址**：`RELEASE_BASE` 还是占位域名。发布流程（beets 侧打
  `.pyz`/`.zip` → 传 release → 改常量或设环境变量）没有走过一遍。
- **两条轨道真机走完整流程**：本机有 Python，A 轨道的"系统 Python 建 venv"
  与 B 轨道的"28 MB 包下载解压"都只在逻辑层与单测里验过，没有在"从没装过
  beets 的干净机器"上跑过。
- **`status.schemaHash` 的运行时比对**：宿主仍只是自报，没有拿它跟镜像比。
  现在只有一个版本在世，接了也验不出来；等第二版发布时一起接。

**M1、M2 是必须的；M3、M4 可以合并或分批**。但如果 M2 之后不冻结协议，
分包就只剩"物理分开"而没有"契约约束"，很快会漂回去。

---

## 10. 拍板记录（2026-10-27）

### 10.1 用户已定

| # | 问题 | 结论 |
|---|---|---|
| 1 | beets 侧代码放哪 | **本仓 monorepo 独立发布单元**：`beets-tool/` 子目录，但有自己的 `pyproject.toml`、版本号、`beets-v*` tag、发布流程 |
| 2 | 胶水形态 | **短进程 stdio NDJSON**：`aiya-beets serve` 起进程 → 收事件流 → EOF 退出 |
| 3 | 安装策略 | **播放器不代装 Python 环境**。release 提供两条轨道：A 本机安装器（用户已有 Python）/ B 免安装套件包（自带 standalone 解释器）。播放器只做「下载 + 展开 + 校验入口 + 显示进度」 |

### 10.2 第 3 条与我原建议的差异（值得记下来，它简化了一处架构）

原建议是「本机没 Python 就由播放器按需下载 uv（31 MB）代跑」。拍板改走
**B 套件包**，播放器彻底不碰 Python 工具链。后果：

- 播放器少一个组件：不下载 uv、不管 pip 镜像源、不管 uv 版本升级
- 多一个发布物（`aiya-beets-bundle-win-x64.zip`），但构建与内容全归 beets 侧
- `interpreter_candidates` 那 40 行硬编码探测**整体删除，不留兜底**
- 技术前提已实测：自带解释器可重定位，套件包能搬到任意路径跑（§6.2）

### 10.3 沿建议默认（用户未反对，按建议执行）

| # | 问题 | 结论 | 依据 |
|---|---|---|---|
| 4 | 协议真源位置 | beets 仓 `protocol/` + 播放器镜像 | 服务端定义契约；双侧契约测试防漂（§4.4） |
| 5 | 刷新触发 | 30 s 定时 + 窗口聚焦 + 操作后 | 覆盖外部修改；不上 FS watcher（§7.2） |
| 6 | `beets_item_id` 改名 | 保留不改，加注释 | 只有一个 provider，改名成本 > 收益（§3.3） |
| 7 | 离线包 | 后置到 P5 | GPL 义务随分发回来，不与分包同批（§8.2） |

> 4–7 随时可翻，它们不动架构骨架，只动实现细节。

---

## 11. 风险清单

| 风险 | 影响 | 缓解 |
|---|---|---|
| 用户装不上（Python 版本不符 / 网络） | beets 功能不可用 | 明确状态机 + `doctor` 命令给可读诊断；核心播放不受影响（§2.2） |
| 协议漂移（两侧各改各的） | 静默不兼容 | 双侧契约测试 + 镜像更新进 diff（§4.4） |
| `aiya-beets` 名字与生态冲突 | 用户困惑 | 加命名前缀；入口名与包名一致 |
| beets 升级破坏内部 API 依赖 | 维护命令失效 | 现状已依赖 `Item.move` / `Library.directory` 等非公共接口；升 beets 版本必须跑 smoke_maint（15 断言） |
| 分包后 dev 联调变麻烦 | 开发效率 | M2 用 monorepo（建议 2）；`AIYA_BEETS_BIN` 指向本仓源码目录即可 |
| 播放器侧无意引入 beets 知识 | 边界腐化 | §3.2 的禁词清单写成 CI 检查（`grep -r` 门禁） |

---

## 12. 与既有文档的关系

| 文档 | 状态变化 |
|---|---|
| docs/08-beets.md | B0 的"sidecar include_str! + 播放器渲染 config"作废，其余（读模型同步）有效 |
| docs/29 §15（批 C 随包） | **整体作废**，改为本文 §3.1 |
| docs/29 §16（批 D 四件套） | 实现有效，只是搬到 beets 侧 |
| docs/31（集成验收计划） | §5 内置包验收降级为可选离线包（§8.2）；§10 合并进本文 §10；**§3 测试分层、§4 插件适配度矩阵、§6 固定调用命令继续有效**，成为 beets 侧仓库的测试基线（矩阵结论改成 `capabilities` 三态上报） |
| docs/32（组件化调研） | §5 分发形态、§7 推荐方案（组件注册表 + 双后端）由本文取代；§4 的实测数据（89 vs 112 MB、dump 1 s、无 watcher、beets items 无 `updated_at`）继续引用 |

---

## 13. M1 落地记录（2026-10-27）

### 13.1 做了什么

新建 `crates/aiya-beets-client`（纯 Rust，不依赖 Tauri），把协议层从 `beets.rs`
整体搬走：

| 模块 | 内容 | 为什么不放在宿主 |
|---|---|---|
| `launch.rs` | `Launcher`：入口 + 参数 + 环境变量的启动描述 | 这是"环境知识"与"协议"的分界。crate 只认"起这个程序"，不知道它是解释器还是 exe |
| `proto.rs` | `Envelope` / `StatusReport`（serde 名与线上 JSON 严格一致） | 协议镜像的地基；M3 的 `proto_mirror.rs` 直接对着它验 |
| `session.rs` | 起进程、NDJSON 行读、超时收尸、会话槽、取消、`EventSink` | 与业务无关的进程生命周期，最容易两边各写一份、各漂各的 |
| `tool.rs` | `BeetsTool` trait + `StdioTool` 实现 | 将来换常驻 worker / socket 时，`src-tauri` 调用点一个字不改 |
| `error.rs` | `ToolError` 六分类（Spawn/Timeout/Protocol/Rejected/Busy/Io） | UI 要按失败类型给不同 UX，`String` 做不到分派 |

`EventSink` 是这次唯一新增的抽象：crate 里不再有 `app.emit(...)`，
`TauriSink` 把它接回 Tauri 事件。这层适配只有一行实质代码，但它把
"crate 不依赖 Tauri"从约定变成了**编译期事实**。

`beets.rs` 侧：删掉 `query_sidecar` / `spawn_session` / `spawn_reader` /
`session_slot` / `session_flag` / `reserve_session` / `acquire_session` /
`release_session` 的实现，改为 `run_session()` 一个统一入口。**环境知识
（`ensure_script` / `beets_paths` / `interpreter_candidates` / `render_config` /
`sidecar_env` / `start_install`）一行没动**——那是 M2 的事。

### 13.2 两处对本文的修正

**① §3.2 的禁词清单按"精确规则 + 豁免"落地，不是字面 grep。**
照字面执行时，第一版门禁把 `Stdio::piped`（命中小写 `pip`）、
`library_path`（命中 `Library`）、`pythonVersion`（协议字段）全判成违规。
噪音化的门禁最后一定会被关掉，所以落成 `tools/check-boundary.mjs`：
大小写敏感 + 词边界 + 两条写明理由的豁免（见 §3.2 表格）。

**② `StatusReport` 的 Rust 字段名用中性词，serde 名用协议真名。**

| 线上字段 | crate 标识符 | 宿主 DTO |
|---|---|---|
| `beetsVersion` | `tool_version` | `beets_version` |
| `pythonVersion` | `runtime_version` | `python_version` |
| `libraryDirectory` | `library_root` | `library_directory` |

这样 crate 里连"对面是 Python 写的"都读不出来，而线上形状一个字节没变
（`proto.rs` 的单测把 camelCase 字段名钉住，改了就红）。

### 13.3 验证（全部实跑）

| 项 | 结果 |
|---|---|
| `cargo test --workspace` | **311 passed**（原 295 + client 新增 16） |
| 其中真进程测试 | `probe_finds_dev_venv_beets` / `probe_finds_bundled_runtime_beets` / `dump_parse_and_sync_end_to_end` 全过——走的是新 crate 的 status 查询与 dump 收集 |
| `npm run verify` | exit 0（含 `check:boundary`、`smoke:beets` 5 场景 ALL PASS、`smoke:maint` 15 断言 ALL PASS） |
| `npm --prefix ui run test:e2e` | 24 passed（UI 未改，作为"行为零变化"的对照） |
| `beets.rs` 行数 | 1316 → **1044**（M2 删环境知识后目标 ~400） |
| 边界门禁 | 12 条规则，0 违规 |

### 13.4 刻意没做的事（免得被当成遗漏）

- Python 源码仍 `include_str!`（M1 明确不搬，保证行为不变）
- `interpreter_candidates` / `render_config` / `FIRST_WAVE_PLUGINS` /
  `start_install` 原地不动
- 协议版本校验没加（`StatusReport::speaks()` 只提供判断，不拦截——
  现状是版本不符也照常工作）
- `tools/pack-beets-runtime.mjs`、`tauri.conf.json` 的 `resources` 未动

### 13.5 留给 M2 的接口面

`src-tauri/src/beets.rs` 里现在只剩三处需要改：

```rust
// ① 发现：interpreter_candidates → client 的 locate 策略
// ② 启动：launcher_for() → 由工具侧自报入口
// ③ 安装：start_install → 换成"下载 + 展开 + 校验 + 跑一次 status"
```

`run_session()` / `collect_items()` / `run_sync()` 三处的形状在 M2 之后
不需要改——它们已经只依赖 `StdioTool` + `EventSink`。
