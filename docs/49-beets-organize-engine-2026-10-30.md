# 整理引擎补齐 + 索引家指向曲库（docs/49）

> 状态：**分析侧已实现并实测通过；整理侧（待整理目录 / 落点 / 移动或拷贝）待审**。  
> 日期：2026-10-30。  
> 前置：docs/33（beets 是增强不是依赖）、docs/43 §1.1（「整理输出」radio 是假控件）、  
> docs/45 §6.1（索引家的 directory 是占位 scratch）、docs/47（模块解耦）、docs/48（整理库独立页）。

---

## 0. 一句话

三件事：① 索引家（只读分析库）的 `directory` 从占位 `scratch` 改成**曲库根**；② 把
「待整理目录 / 落点 / 移动或拷贝」补成整理库页里的一张卡；③ 整理完成后只对**落点**
跑一次增量扫描。

查过之后有个前提要纠正：**② 的后端其实早就写完了**，缺的是 UI 与一处口径统一（见 §1.1）。

---

## 1. 三处事实修正（与既有文档/记忆不一致，实测为准）

### 1.1 「整理引擎没做」只对 UI 成立，后端是全的

宿主已有、且仍在 `generate_handler!` 里注册：

| 命令 | 作用 |
|---|---|
| `beets_config_get` | 读 `inbox` / `target_root` / `effective_target` |
| `beets_set_inbox` | 设待整理目录 |
| `beets_set_target` | 设整理落点（**必须是某个扫描根**） |
| `beets_open_inbox` | 在资源管理器里打开待整理目录 |
| `beets_inbox_imported_add` | 记下已整理过的源文件（FIFO 防重复导入） |
| `beets_preview` → `beets_apply` | 预览 → 应用，`options.excludePaths` 自动注入上面那张表 |

工具侧 `apply_import` 已经支持三种动作：`apply`（只写标签）、`apply_move`、`apply_copy`，
落点由 `lib.directory` 决定（`maintain.py::move_items` 的注释也是这个口径）。

**所以 §2 的差距只在 UI 与「落点怎么落到 beets 配置」这一环。**

### 1.2 「directory 指扫描根 ⇒ 库里存相对路径 ⇒ path 定位失效」——不成立

beets 2.14.1 实测（探针见 §7.4）：

| 场景 | `library.db` 里 `items.path` 的形态 |
|---|---|
| 文件在 `directory` 之内 | **相对**（`Album/低音测试 220Hz.wav`） |
| 文件在 `directory` 之外 | 绝对 |
| copy / move 整理之后 | 相对（落点就是 `directory`） |

但**磁盘上存相对 ≠ API 读出来是相对**：

```
item.path（API）        → 绝对（beets 已用 config['directory'] 拼回）
Item.from_path(绝对).path → 绝对
PathQuery("path", 相对)  → 命中 1
PathQuery("path", 绝对)  → 命中 1
```

结论：只要走 Python API（`query.py`、`importer.py` 都走），口径是统一的。真正会失效的只有
**直接读 sqlite 的代码路径**，我们没走那条。所以 §3.1 可以放心把 `directory` 指向曲库根。

**但仍要加一道兜底**：出口一律 `normpath(join(lib.directory, item.path))`（绝对路径 join 后
不变，幂等）。理由不是"现在会坏"，是"beets 哪天改了 API 的相对化，症状会是所有曲目集体变
「未入库」——这种错法没有报错，只有 UI 上少几行能选"。

### 1.3 落点其实一直存在，只是隐式

`beets.rs::scan_root()`：`target_root`（若仍是某个扫描根）→ 否则第一个扫描根 → 下发
`AIYA_BEETS_SCAN_ROOT` → 工具写进 `config.yaml` 的 `directory`。

也就是说**整理落点当前恒等于曲库根**。这解释了 docs/43 §1.1 判「整理输出 radio 是假控件」的
真实原因：它写的 `config.directory` 确实有用（决定 move/copy 落在哪），但界面上没有任何地方
体现出这件事，用户改了什么也看不见。

---

## 2. 差距表

| 能力 | 后端 | 配置存储 | UI | 缺口 |
|---|---|---|---|---|
| 待整理目录 | ✅ preview/apply 接目录 | ✅ `settings.json.inbox_path` | ❌ 无入口 | 一张卡 |
| 落点 | ✅ `lib.directory` | ✅ `target_root` | ❌ 无入口 | 一张卡 + 落点口径 |
| 移动 / 拷贝 | ✅ `apply_move` / `apply_copy` | ❌ 不落盘（按次选） | ❌ 无入口 | 预览时的选择 |
| 索引家指向曲库 | ⚠️ 恒为 scratch | — | — | 改模板 + 重建 |
| 整理后同步曲库 | ⚠️ 只有整库 refresh | — | — | 收窄成落点 |

---

## 3. 设计

### 3.1 三个目录各归其位

| 名字 | 是谁的 | 落在哪 | 语义 |
|---|---|---|---|
| **曲库根** | 应用的扫描根 | 应用 DB `settings` | 索引家 `directory`，beets **只读**分析（`import.write/copy/move: no` 保持不变） |
| **待整理目录**（inbox） | 用户的"砧板" | `beets.settings.inbox_path` | beets `import` 的**源**，不写进 beets 配置（每次由宿主传路径） |
| **落点**（dest） | 用户的"成品架" | `beets.settings.target_root` | 整理家 `directory`，`copy`/`move` 的**目标** |

三者不能混：索引家要的是"整个曲库长什么样"，整理家要的是"整理完放哪"。**曲库根 ≠ 落点时，
两者必须分别下发**（见 §3.2）。

### 3.2 环境变量必须拆开（当前是混的）

现在的 `AIYA_BEETS_SCAN_ROOT` 一个变量喂两个家：

```
scan_root() = target_root（若仍是扫描根）否则第一个扫描根
   └→ AIYA_BEETS_SCAN_ROOT → 两个 profile 都收到同一个值
```

一旦落点 ≠ 曲库根，索引家会拿到落点当 `directory` —— 索引范围直接缩成落点那一块。所以拆成：

| 变量 | 值 | 谁用 |
|---|---|---|
| `AIYA_BEETS_SCAN_ROOT` | **曲库根**（第一个扫描根，永远不取 target） | index |
| `AIYA_BEETS_DEST` | **落点**（`target_root`，没设就省略） | workbench |

工具侧：`index` 家渲染 `directory: <SCAN_ROOT>`；`workbench` 家渲染 `directory: <DEST>`，
`DEST` 缺失时回退 `SCAN_ROOT`（与今天行为一致，老配置零迁移）。

### 3.3 索引家改口径 ⇒ 受控重建（用户选"干净重建"）

`index` 家的 `config.yaml` 从 `directory: {home}/scratch` 变成 `directory: <曲库根>`。
口径变了，旧库的记录（绝对路径）虽然仍能读，但会和新记录（相对）混存——用户已拍板**干净重建**。

做法：index home 下写一个 `layout.json`（只存一个字段：`directory` 的归一值）。每次
`ensure_config` 时比对：

* 相同 → 什么都不做（幂等，启动不重扫）；
* 不同 → **成对删** `library.db` + `library.db-wal` + `library.db-shm` + `state.pickle`
  （docs/45 的教训：state 才是增量记忆，只删库会让下次导入以为"全都导过了"），重写 config，
  把 `rebuild` 事实写进 `status` 事件让 UI 显示"索引已按下新口径重建"。

`scratch` 目录保留不删（可能被别的 home 引用），只是不再写进 config。

### 3.4 落点变更 ⇒ 外科式改 `directory` 一行

workbench 的既定规则是「`config.yaml` 存在就一个字都不动」（docs/33 §2.1）。这条规则和"用户
能改落点"直接冲突，解法是**只改那一行，其余字节原样保留**：

* 有 `directory:` 行 → 替换该行（保留原有引号风格、保留注释）；
* 没有 → 在 `library:` 之后插入一行；
* 其余行（含用户手改的 `plugins`）字节不变。

不做「整份重渲染」：那会把用户手改的插件列表冲掉，正是 docs/33 要防的事。

### 3.5 整理后只扫落点（用户选"自动只扫落点"）

`apply` 的 `on_exit` 现在走 `refresh::spawn(整库)`。改成：**把 roots 收窄成落点**。
`library_scan` 已经支持 `options.roots`（`refresh::roots_from_settings` 的注释也强调两处必须
同源），所以加一个 `refresh::spawn_scoped(app, params, roots)`，`Trigger::Action` 之外再带上
"本次只扫这些目录"的事实；事件里照样报 `trigger=action`，诊断页能看出范围。

落点本身是扫描根或扫描根的子目录时，这次扫描等价于"扫落点那一段"；否则按 §3.6 拒绝。

### 3.6 规则：落点必须落在某个扫描根之内

整理完的文件要是应用扫不到，"存为歌单 / 播放"全落空。所以：

* 落点 = 某个扫描根，**或是它的子目录**（子目录更实用：`曲库/整理/`）；
* 不满足 → `beets_set_target` 直接拒绝，提示「先把它加为扫描目录，或改选曲库内的目录」；
* 现有 `config_view_of` 用的是**严格相等**，要放宽成"子目录也算"（`canon_path` 前缀比较 +
  路径分隔符边界，避免 `D:\Music2` 误判成 `D:\Music` 的子目录）。

---

## 4. UI 形状：整理库页新增一张「整理」卡

放在查询面板**之下**（查询是读，整理是写，读写分区）：

```
┌ 待整理目录 ──────────────────────────────────────┐
│ C:\Users\me\Music\待整理      [选择] [打开]        │
│ 未设置时：说明这里放"还没进曲库的新文件"，给 [选择]  │
├ 落点 ───────────────────────────────────────────┤
│ D:\Music\整理                 [选择]              │
│ 提示：整理后的文件会按「艺人/专辑/曲序 标题」存放    │
└─────────────────────────────────────────────────┘
      [ 开始整理 ]   ← 未设待整理目录时禁用
```

点「开始整理」后：

1. **预览**（`beets_preview`，源 = 待整理目录递归出的音频文件，自动排除已整理过的）；
2. 预览表按**专辑分组**，每组一个动作选择：`移动进来` / `拷贝进来` / `跳过`；
   —— 这就是用户选的"模式每次选"，不写进配置；
3. 执行（`beets_apply`，决策里带 `apply_move` / `apply_copy`），进度走既有 `beets:import` 事件；
4. 完成：自动扫落点（§3.5），toast 报「整理 N 张专辑 / 新增 M 首到曲库」。

**默认动作**取「拷贝」而不是「移动」：移动是不可逆的（源文件没了），第一次用的人应该能反悔。
这是安全默认，不是性能选择。

---

## 5. 不做

* §5.5 智能歌单字段（docs/48 已排除）；
* beets 歌单分支；
* 多个待整理目录（一个 inbox + 每次选动作已经覆盖绝大多数用法；多 inbox 会引入"哪个 inbox
  配哪种动作"的组合爆炸）；
* 整理历史 / 撤销（要撤销就得留原件副本，那是另一套存储策略）。

---

## 6. 已知边界

1. **无标签文件**：`path_formats` 用 `$albumartist/$album/$track $title`，wav 没标签时会退化成
   `__00.wav`（实测），多首会互相覆盖。整理卡里对"标题为空"的文件给一条提示，或默认动作
   降级成「拷贝」（原件还在，撞名不至于丢文件）。
2. **`import.incremental: yes`**：整理过的目录再整理一次会被直接跳过。整理走的是显式文件列表
   （`preview_import(paths)`），不受 `incremental` 影响；但 `write: yes` 的副作用要留意。
3. **`duplicate_action: keep`**：同专辑同曲长会生成重复行，整理场景比分析场景更容易撞。
4. **待整理目录在曲库根内时**，索引家会把它也扫进分析库（整理前就能分析到）。不算错，但
   "整理完是不是重复计数"要实测确认（§8）。
5. **落点选到别的盘 / NAS**：扫描变慢，§3.5 的"只扫落点"成本可控，但首次扫会久。

---

## 7. 验证计划

### 7.1 单测（工具侧）
* `render_config(index)` 的 `directory` = 传入的曲库根，不再是 scratch；
* 配置闸：`directory` 不等于曲库根 → 必须红（负向：把曲库根改成 scratch 仍应**不**通过）；
* `layout.json` 变化 → 成对删四个文件；不变 → 一个文件都不动；
* 落点外科式修改：给一份带用户注释与自定义 `plugins` 的配置，改 `directory` 后**其余行逐字节相同**。

### 7.2 单测（宿主侧）
* `env_pairs`：`index` 收到 `SCAN_ROOT`，`workbench` 收到 `DEST`；`DEST` 缺失时 workbench 回退 `SCAN_ROOT`；
* `beets_set_target`：落点是扫描根子目录 → 接受；是 `D:\Music2`（`D:\Music` 的假兄弟）→ 拒绝。

### 7.3 e2e
* 整理卡在未设置待整理目录时禁用「开始整理」，给引导；
* 设置后预览表出现分组与三个动作，选「拷贝」执行 → toast 报新增数。

### 7.4 负向验证（必须）
1. 把 `layout` 比对改成恒等（永判"相同"）→ 重建测试必须红；
2. 把落点子目录判定改成裸 `starts_with`（不加分隔符边界）→ 假兄弟测试必须红；
3. 把索引家 `directory` 改回 scratch → 配置闸测试必须红。

探针脚本（判断 beets 行为用，不入仓）：`%TEMP%\path-probe2.py`、`path-probe3.py`。

---

## 8. 没验到的

1. **整理后只扫落点**在真机上的耗时与"路径跟随"（beets move 改了文件名 → 应用库里旧 path
   失效，靠扫描段重建。若撞上"同一位艺人同专辑换了曲序"，可能被当成新曲目）；
2. 索引家指向曲库根之后，**索引进度与 enirch 落库表**是否仍按 path 对得上（当前实现走 API，
   理论上没问题，但没跑过完整索引）；
3. 落点在 NAS / 别的盘时 `copy` 的耗时与中断恢复；
4. 无标签 wav 的撞名概率与用户实际接受度。
