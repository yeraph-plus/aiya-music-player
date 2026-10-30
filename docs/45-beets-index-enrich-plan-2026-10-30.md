# beets 索引增强（关联表 + 宿主回填）实施方案

> 状态：**批 1 已实施**（§11 记实际落点与偏差）；批 0 仍受 Essentia 二进制阻塞。
> 日期：2026-10-30。
> 前置：docs/44（双家隔离与单向查询调研）、docs/43（UI 重做 / 零接触终态）、
> docs/42（进程形态不变）、docs/33（两侧分离）、docs/40（动态歌单规则）。
> 本文取代 docs/44 §9 的实施细节；**docs/44 的能力分类与拍板方向不变**，
> 但其中 5 处技术前提经实测证伪或需收窄，逐条记在 §3。

---

## 1. 结论

**方案成立，可以开工。** 但要按本文的三段式（补登 / 清旧 / 分析）实现，
而不是 docs/44 §9.1 画的"import -A + 插件分析"一趟做完——后者在 beets 2.14.1
上跑不通，原因见 §3.1。

本批在 `D:\MUSIC_Player\.probe44\`（隔离 BEETSDIR + 隔离库，与项目无关）
做了 14 组实测，所有结论都能用附录 A 的命令复现。

一句话概括修正后的形态：

```
宿主（唯一写应用 DB 的人）          sidecar（只算不写）                DB1（可丢的计算工作台）
 ├ 算工作清单（LEFT JOIN 关联表）
 ├ 发 enrich{paths}          →  ① 补登 import -A -s -I -R
 │                              ② 清旧 remove -f path::<regex>（仅过期）
 │                              ③ 分析 插件命令（跳过已有字段）
 ├ 逐条收 enrichItem ←──────  NDJSON：path + 实际字段 + 状态
 └ 批量 upsert（持 scanning 互斥）
```

---

## 2. 实测环境

| 项 | 值 |
|---|---|
| Python | 3.14.7（与 sidecar 同一 venv） |
| beets | 2.14.1 |
| 插件 | 自写探针插件 `probeplug`（声明 `item_types` + `import_stages` + 一条命令）；`beets-xtractor 0.4.2`（PyPI wheel，隔离装在 `--target`） |
| 隔离 | 独立 `BEETSDIR`、独立 `library.db`、4 个 3 秒 wav + 400 个复制品 |
| 位置 | `D:\MUSIC_Player\.probe44\`（在仓库外，不污染工作区） |

---

## 3. 五处技术前提的实测判定

### 3.1 `xtractor` 不在 import 链路上（推翻 docs/44 §9.1 的一趟式流程）

`beets-xtractor 0.4.2` 的 `config_default.yml` 有 `auto: no`，`command.py`
里只读不用（`self.cfg_auto = cfg.get("auto")`），**整包没有任何
`import_stages` / `register_listener`**（grep 全空）。`beet xtractor --help`
证实它只有命令形态：`beet xtractor [options] [QUERY...]`。

对照组（都在 import 链路上，实测/源码均确认）：

| 插件 | 挂载点 | 默认 |
|---|---|---|
| `lastgenre` | `import_stages = [self.imported]`（`lastgenre/__init__.py:139`，`auto: True`） | 开 |
| `chroma` | `register_listener("import_task_start", …)`（`chroma.py:194`） | 开 |
| `xtractor` | **无** | — |

→ 结论：**索引（成员资格）与分析必须分两步**。好在"分析"本来就需要宿主
算工作清单（缺失/过期），两步分离反而让"重建 DB1 不触发全量重分析"成立。

### 3.2 `xtractor` 没有 `energy`，也没有单一 `mood`

`config_default.yml` 的 target 全量（25 个）：

```
low_level:  bpm  danceability  beats_count  average_loudness
high_level: danceable  gender  is_male  is_female  genre_rosamerica
            voice_instrumental  is_voice  is_instrumental
            mood_acoustic  mood_aggressive  mood_electronic  mood_happy
            mood_sad  mood_party  mood_relaxed  mood_mirex
            mood_mirex_cluster_1 … _5
```

`energy` 不存在，`mood` 不是字段（只有 8 个 mood_* 概率 + `mood_mirex` 串）。
docs/44 §9.2 的 `mood TEXT / energy REAL` 两列都是空源；`§9.3` 的
"energy > 0.8 的歌"这个例子、以及 `smart.rs:434`（把 `energy` 当"未知字段
必须拒绝"的测试样本）都要跟着改。

关键字段的来源路径与类型（`config_default.yml`，实现期照抄）：

| 字段 | Essentia JSON 路径 | 类型 | required |
|---|---|---|---|
| `bpm` | `rhythm.bpm` | integer | **yes** |
| `danceability` | `rhythm.danceability` | float | no |
| `average_loudness` | `lowlevel.average_loudness` | float | **yes** |
| `danceable` | `highlevel.danceability.all.danceable` | float | **yes** |
| `mood_mirex` | `highlevel.moods_mirex.value` | **string** | **yes** |
| `mood_*`（8 个） | `highlevel.mood_happy.all.happy` 等 | float | no |
| `mood_mirex_cluster_1..5` | `highlevel.moods_mirex.all.ClusterN` | float | no |

四个 `required: yes` 的含义待批 0 验证：**缺任意一个是否会整条判失败**
（决定 `status` 的三态怎么切）。这也解释了为什么 `mood_mirex` 可以直存
TEXT 列——它本来就是标签串，不是概率。

→ 字段契约按 §5.1 重写。

### 3.3 数值范围查询本身没问题，是字段不存在 + 未注册类型

| 探针 | 结果 |
|---|---|
| `probe_typed:0.4..0.6`（插件声明 `item_types={"probe_typed": Float(6)}`） | **3 命中** |
| `probe_plain:0.4..0.6`（未注册，存字符串 "0.55"） | **0 命中，静默** |
| `probe_plain:0.55`（精确） | 3 命中 |

→ docs/44 §3 的 `energy:0.7..0.9` 零命中不是"beets 范围查询不可靠"，而是
①`energy` 字段根本不存在 ②未注册类型的 flexible 字段是字符串，范围查询
不适用。拍板 E（数值落 SQL REAL 列）依然正确，但**理由要换成"索引是派生物、
SQL 列才可索引可 join"**，别再写成"beets 做不到"。

### 3.4 过期重分析：`import` 会跳过，`-I` 与 `remove+重导` 能重跑

| 探针 | 结果 |
|---|---|
| 二次 `import -A -q <dir>`（`incremental: yes`） | `Skipped 1 paths.`，import_stage **不再执行**（日志 3→3） |
| `beet update` | 只刷标签，**不跑** import_stage（3→3） |
| `import -A -q -s -I -R <dir>` | import_stage 重跑 400 次，条目数不变（405→405，幂等） |
| `beet remove -f 'path::099-高音'` + 单文件 `import -s -I` | 精准清掉重登，字段清空后由 stage 重算 |

→ "size/mtime 变了自动重排队"只有走 ②清旧 + ③分析 才成立。docs/44 §9.2
的"upsert 天然幂等"没错，但漏了"beets 侧的字段不会自己清"。

### 3.5 DB1 的 `directory` 必须是占位目录（否则路径桥变相对路径）

| `directory` | DB 里存的 path | `path:` 查询 | `path::regex` 定位 |
|---|---|---|---|
| = 曲库扫描根 | 根内文件存**相对**（`b'中音测试 440Hz.wav'`），根外存绝对 | 绝对路径不命中 | 只能按相对串匹配 |
| = 空占位目录 | **全部绝对**（`b'D:\\…\\music\\中音测试 440Hz.wav'`） | 仍不稳 | **绝对路径正则命中 ✓** |

`item.path` API 两种情形都回绝对路径（docs/44 §3 的结论成立），但"清旧/定位"
要靠查询串，相对路径会让精确操作退化成猜。

→ DB1 的 `directory:` 指向 `{app_data}/beets-index/scratch`（空目录、仅满足
beets 必填），**不指向任何扫描根**。

### 3.6 附带发现（三条都会咬人）

1. **`state.pickle` 才是增量记忆**：`incremental` 的判据是
   `{BEETSDIR}/state.pickle` 的 `taghistory`，不在 `library.db` 里。删库不删
   state → 再 import **全部跳过，索引恒空且零报错**。docs/44 拍板 A 的
   "可随时删除重建"必须写成**成对删除**。
2. **跳过的尝试也会被记进 state**：一次被跳过的导入（本例是"专辑已在库"）
   之后，该路径再也导不进来，除非删 state 或加 `-R`
   （`--incremental-skip-later`）。→ 索引导入固定带 `-R`。
3. **重复检测会吃掉同内容文件**：默认 `import.duplicate_action: ask`
   （`duplicate_keys.item: artist title`），`-q` 下退化为跳过；同标签同长
   的两个文件只有第一条进库，第二条的路径永远拿不到增强数据。
   → DB1 固定 `duplicate_action: keep`（实测 400 文件全进）。

### 3.7 `xtractor` 的两个坑（源码 + 实测）

1. **`write: yes` 是默认值**，`-w/--write` 的 default 取 config，命中即
   `item.try_write()` —— **会把 bpm 写回音频文件**。这是"文件不动"拍板前提
   的正面冲突。`write: no` 能关掉（`cfg_write=False` 时 default 为 False，
   且不支持用 `-w` 之外的方式打开）。→ DB1 模板必须硬钉 `xtractor.write: no`，
   且渲染后校验。
2. **缺 Essentia 二进制时静默成功**：`beet xtractor -f <query>` 打印
   `Finished: [1/1]`、**退出码 0**、`bpm` 仍是 0、没有任何错误输出。
   → 宿主**不能按退出码判定分析成功**，必须逐条校验字段是否真的出现。

### 3.8 许可与分发（批 5 的前置，不是实现细节）

- Essentia 本体 **AGPLv3**，静态提取器二进制随应用分发 = 分发 AGPL 作品
  （要提供源码/许可证）；子进程调用不改变"分发"这件事。
- SVM 模型（`danceability.history` 等）**CC BY-NC-ND 4.0**：非商业 +
  禁止演绎，随应用打包需评估（本应用接受捐赠，NC 条款有灰色地带）。
- PyPI 的 `essentia` **没有 Windows wheel**（macOS/Linux only），Python
  绑定在 Windows 上不可用；只能用官方静态二进制。
- `essentia.upf.edu` 在本机网络**不可达**（502），Windows 构建是否仍提供
  静态 zip（含 `streaming_extractor_music.exe`）**尚未验证**。

→ 批 0 探针必须先跑通"拿到二进制 + 3 个文件出数"，再谈接入。

### 3.9 本会话拿不到的东西（批 0 的剩余未知，需你的网络或手动下载）

| 尝试 | 结果 |
|---|---|
| `essentia.upf.edu`（下载页/安装文档/extractors 文档） | 全部 502（代理不可达） |
| `archive.org` 快照 | 502 |
| PyPI `essentia` / `essentia-tensorflow` | 只有 macOS/Linux wheel，**无 Windows** |
| conda-forge | 404，没有这个包 |
| `beets-xtractor` 本体 | **可装、可加载**（纯 Python wheel，装在隔离目录后 `beet xtractor --help` 正常） |

批 0 的三个出口条件（缺一不可，全部要实测）：

1. 官方 Windows 静态提取器 `streaming_extractor_music.exe`（含 Gaia 支持）
   + beta5 SVM 模型可用；
2. 3 个 3 秒 wav 能出 `bpm%danceability/average_loudness/mood_mirex`，
   并记录**单曲耗时**与**模型体积**；
3. 故意删掉一个 required 字段的模型，确认"缺字段"是整条失败还是部分成功
   （决定 `status` 三态）。

拿不到 (1) 的兜底：DB1 只接 `lastgenre`（曲风）+ 现有标签字段，`mood/danceability`
整条能力线下线，关联表仍保留 schema（列留空），等二进制就绪再启用。

---

## 4. 架构落点

### 4.1 两家的命名与职责（docs/44 §4.1 只改一处）

| | DB1 索引（`beets-index`） | DB2 工作台（`beets`，现状零迁移） |
|---|---|---|
| `BEETSDIR` | `{app_data}/beets-index` | `{app_data}/beets` |
| `directory:` | `{app_data}/beets-index/scratch`（**占位空目录**） | 整理输出根（现状） |
| 导入参数 | `-A -q -s -I -R` + `incremental: yes` + `duplicate_action: keep` | 现状 preview/apply |
| 写文件 | **永不**（`import.write/copy/move: no` + `xtractor.write: no` + 白名单禁写盘插件） | 用户操作决定 |
| 重建 | 删 `library.db*` **与 `state.pickle`** 成对 | 不重建 |

### 4.2 sidecar 侧：三条命令

协议 v2 纯加法（docs/43 §5 的"不升版"规则沿用）：

| 命令 | 形态 | 说明 |
|---|---|---|
| `enrich {want:[...], paths:[...], force:bool}` | 流式（事件） | 三段式主体，逐文件回吐 |
| `query {rules\|raw, fields, limit}` | 信封 | 结构化规则 **或** 原生语法，二选一 |
| `fields {}` | 信封 | 可用字段（内置带类型 + 库里真出现过的 flexible）+ 各源三态 |

**`target` 不进协议（对 docs/44 §5.1 的修正）**：查哪个库由**进程**的
`AIYA_BEETS_PROFILE` 决定（批 1 的双家隔离）。协议里再放一个 `target` 就是
第二个真源——宿主传 `index` 而进程跑在 `workbench` 上时，会**静默**查出另一个
库的全部内容，不抛异常，只让用户看见"索引里有几万首我没导过的歌"。

事件增量（`Event` 的 oneOf 加一条，`additionalProperties:false` 保持）：

```json
{ "type": "enrichItem", "session": "enr_02",
  "path": "D:\\Music\\a.flac", "source": "xtractor",
  "status": "ok",              // ok | empty | failed | skipped
  "fields": { "bpm": 122.0, "danceability": 0.61, "mood_mirex": "mood_happy", "...": "..." },
  "message": null }
```

终态是 `enrichDone {total, succeeded, empty, failed, skipped, sources}`。
**成功条数必须叫 `succeeded` 不能叫 `ok`**：契约规定"有 `ok` 就是应答信封"，
事件里出现 `ok` 会被客户端当信封去解析 `data`（批 2 踩过一次，schema 里已写明）。

- `status: empty` = 命令跑完但字段没出现（§3.7 的静默失败）——宿主记
  `analyzed_at` 但**不写字段**，并把它当"该源对该文件不支持"缓存下来，
  避免每轮重试烧 CPU。
- **逐文件 emit 是看门狗要求，不只是粒度偏好**：客户端 `SESSION_IDLE_TIMEOUT`
  = 600 s，空闲到点直接强杀（`session.rs:35`）。批级 emit 在慢机上会被误杀。

协议面要同步动的三处（都属协议变更，须 `proto:sync`）：

| 位置 | 改动 |
|---|---|
| `$defs/Stage` 枚举 | 加 `"enrich"`（会出现在 UI 文案与进度条） |
| `$defs/Session` 正则 | 加前缀 `enr`（`^(imp\|app\|…\|enr)_[0-9]+$`） |
| `$defs/Event` oneOf | 加 `enrichItem`、`enrichDone` |
| `$defs/Request` oneOf | 加 `enrich` / `query` / `fields`；新增 `$defs/QueryRule` |
| 客户端 `TERMINAL_EVENTS` | 加 `enrichDone`（现有五个：done/applyDone/cancelled/sessionFailed/pluginDone） |

`enrich` 同时进 `lockfile.LOCKED_CMDS`：它要 remove + import，是索引家库的写者。
**不进 `ops.TERMINAL_OF`**（幂等重放）：enrich 的语义是"要什么算什么"，
把上次的结果重放给一个新请求，会掩盖"这批应该重算"的事实。

### 4.3 sidecar 内部三段式（`cmds/enrich.py`）

```
① 清旧  仅 force：把清单里的项从库里摘掉（字段随之消失 → 插件下次才会重产出）
② 补登  import -A -q -s -I -R <分批路径>
        -I 让"已在库"的项也重走流程（否则增量直接跳过，插件根本不跑）
        -R 是 incremental_skip_later：**不把跳过项记进 state.pickle**，
           否则该路径之后再也导不进来（表现为"索引恒空且零报错"）
③ 分析  逐个源产出字段，再按 (path, source) 读库回吐 enrichItem
```

**顺序是 清旧 → 补登 → 分析**。本文档早先写成"补登 → 清旧"，因果错了：
先 remove 就什么都不剩，之后必须重新 import 才回得来。

批次粒度：`paths` 由宿主按目录聚类后分批（例如 ≤200/批），批间检查 cancel；
每批结束即 emit，宿主落库后即使进程被杀也不会重算已完成部分（工作清单是
"缺失/过期"，天然续跑）。

状态判定（关键）：`status = ok` 当且仅当**至少一个期望字段非空**；
否则 `empty`。**不看退出码。**

---

## 5. 宿主侧

### 5.1 关联表（migration 0008，`crates/aiya-data`）

```sql
CREATE TABLE track_enrichment (
  path_lower   TEXT NOT NULL,        -- 与 tracks.path_lower 同键；不挂外键
  source       TEXT NOT NULL,        -- 'xtractor' | 'lastgenre'
  -- 常用维度提升为可查列（全部来自 xtractor 真实字段，见 §3.2）
  bpm            REAL,
  danceability   REAL,
  loudness       REAL,               -- = average_loudness（不是"energy"）
  mood_mirex     TEXT,               -- essentia 原始串（如 mood_happy）
  mood_label     TEXT,               -- 宿主对 8 个 mood_* 取 argmax 得到
  mood_score     REAL,               -- argmax 的概率
  -- 其余 ~18 个 xtractor 字段 + lastgenre 的 genre 进 JSON
  data         TEXT NOT NULL DEFAULT '{}',
  -- 失效判断用的文件事实（取自 tracks，不快照 beets）
  size_bytes   INTEGER NOT NULL,
  mtime        INTEGER NOT NULL,
  analyzed_at  INTEGER NOT NULL,
  status       TEXT NOT NULL,        -- ok | empty
  tool_version TEXT,                 -- 记 sidecar/插件版本，升级后可选择性重算
  PRIMARY KEY (path_lower, source)
);
CREATE INDEX idx_enrich_mood ON track_enrichment(mood_label);
CREATE INDEX idx_enrich_dance ON track_enrichment(danceability);
```

`source` 进主键（docs/44 §9.2 的单列 + 单主键会互相覆盖）。
不挂 `tracks` 外键（根移除后关联仍存活，查询期再 join）——但见 §7 的 GC。

### 5.2 工作清单（宿主算，一条 SQL）

```sql
SELECT t.path, t.size_bytes, t.mtime
FROM tracks t
LEFT JOIN track_enrichment e
  ON e.path_lower = t.path_lower AND e.source = ?
WHERE e.path_lower IS NULL
   OR e.status = 'empty'   -- 可选：不重试不支持的文件
   OR e.size_bytes <> t.size_bytes OR e.mtime <> t.mtime;
```
`status='empty'` 的默认**不再重试**（除非用户点"强制重算"），否则每次分析
都会把上次静默失败的文件再跑一遍。

### 5.3 写入与锁（docs/44 §9.1 第二条理由的落地）

- `enrichment_upsert` 只在**宿主进程**里执行，且持 `aiya-data` 的写者身份；
- `scanning` 互斥（`lib.rs:381` 的 `Arc<AtomicBool>`，与 `library_scan` /
  `refresh` 同一把）**只在批量写事务期间持有**，按批（≤200 行）或每次
  emit flush 取放；**不整场持有**，否则 30s 定时刷新全被静默跳过、手动扫描
  一律 `E_SCAN_BUSY`；
- `db.rs` 的 `busy_timeout`：**本方案最初以为缺，实际从 P1-1 就有**
  （`db.rs::configure`，5000ms；`git log -S busy_timeout` → `afa65fa`）。
  批 3 只做复核、没改代码——记在这里是因为"文档说缺、代码其实有"这类偏差
  会让下一批的人白改一遍。

### 5.4 Tauri 命令（4 条）

`enrichment_worklist`（算清单 + 计数）、`beets_enrich_start`（起会话、推事件）、
`enrichment_query`（按维度过滤，给曲库页/智能歌单用）、
`enrichment_gc`（清理残留行，§7② 的欠账落在本批）。
取消复用现有 `beets_cancel` 通道（会话槽是同一个）。

### 5.5 动态歌单与查询（拍板 E 的落地）

> **批次日修订（2026-10-30，批 3 复盘）：本节整体挪到批 4。** 原因不是工作量，
> 而是风险面比看上去宽：`conds_from_value` 的产物同时被 `repo::list_tracks` 与
> `SmartPlan` 消费，加一个 `en.` 前缀的条件意味着 `repo.rs` 的四个 FROM 变体
> （library / playlist / recent / mostPlayed）和 `count_from` 都要按需补
> `LEFT JOIN track_enrichment en`，而"要不要补"还得按条件动态判断。
> 在没有 UI 筛选入口的批次里改这条**最热的查询路径**，等于用最贵的查询去背
> 一次无法验证的改动。批 4 有筛选器可以真点，届时一并改、一并量。

- `smart.rs` 的 `Field` 枚举加 `Mood`(text) / `Danceability`(real 范围) /
  `Bpm`(int 范围)，`allows()` 与 `build_clause()` 各加一支；`plan()` 编译出的
  SQL 需要 join `track_enrichment`（别名固定 `en`，避免与 tracks 列名撞）。
- `smart.rs:434` 那条"未知字段 energy 必须被拒绝"的测试改为别的未知字段名
  （例如 `tempo`），否则它锁死的是旧假设。
- **未装 beets / 未装 Essentia**：字段不可用 → UI 隐藏入口，SQL 层因
  `LEFT JOIN` 得 NULL，规则天然不命中，不需要分支。

---

## 6. 配置模板与边界（批 1 的全部内容）

### 6.1 DB1 模板（`cfg.py` 增加 `render_config(home, root, profile)`）

`profile = "index" | "workbench"`，index 版在现有三行之外硬钉：

```yaml
import:
    write: no
    copy: no
    move: no
    incremental: yes
    quiet: yes
    duplicate_action: keep
xtractor:
    auto: no
    write: no
```

**渲染后自检**：模板固定 → 生成文本后断言关键键存在且为期望值（gate），
不依赖"默认值恰好对"。DB1 的 YAML 编辑器**不开放**（docs/43 §4.4 的编辑器
只服务 DB2），避免用户改一行就破"文件不动"前提。

### 6.2 白名单不能只有名字（docs/44 §4.2 收窄）

`plugins:` 白名单仍是 `xtractor / lastgenre / chroma`，但每个名字要带
"强制参数"与"是否允许出现在写路径"：白名单校验变成"名字 + 参数 + 写盘标志"
三元组，`write: yes` 直接拒绝渲染。

另外按 §3.1：`chroma` 需要 `fpcalc` 二进制 + 网络，`lastgenre` 需要网络，
两者放进 DB1 会让**索引步骤**（不是分析步骤）带上网络依赖。建议：
- 首批 DB1 默认只开 `xtractor`；
- `lastgenre` / `chroma` 作为可选勾选，开启时 UI 明示"需要联网/需要 fpcalc"。

### 6.3 环境变量加固

`cfg.apply_env()` 现在遇到外部已存在的 `BEETSDIR` 直接 return（`cfg.py:52`），
而锁文件路径取 `cfg.data_home()`（`AIYA_BEETS_HOME`）——**二者不一致时，
beets 写 A 库、锁建在 B 家**，双家隔离静默失效。两条一起做：

1. 宿主 `tool_env` 显式 `.env("BEETSDIR", <同 AIYA_BEETS_HOME>)`（或
   `.env_remove("BEETSDIR")` 并保证 AIYA_BEETS_HOME 一定下发）；
2. sidecar 启动时若二者都存在且不等 → `E_CONFIG` 明确报错（fail-fast，别猜）。

---

## 7. 边界内的已知欠账（登记，不在本批解）

| # | 欠账 | 归属 |
|---|---|---|
| ① | 路径漂移（改名/移动）→ 关联行按 `path_lower` 失效 → 重分析；历史连续性同 docs/43 §8-E① | 扫描器批 |
| ② | 关联表悬挂行 GC：曲库根移除后行仍存活（这是设计），但需要"无对应 track 且 N 天未更新 → 清"的策略 | 本方案批 3 附带一个 `enrichment_gc` |
| ③ | `lastgenre` 的网络失败是否中断整批：**未实测**（离线环境不可用） | 批 2 实现时补探针 |
| ④ | `mood_*` argmax 的语义（8 类互斥概率）未经真实数据校准，`mood = mood_mirex` 与 `argmax` 可能不一致 | 批 5 有真数据后定 |

---

## 8. 批次（取代 docs/44 §9.4）

| 批 | 内容 | 状态 |
|---|---|---|
| **0** | Essentia 探针（Windows 静态二进制 + beta5 模型 + 单曲耗时/体积/许可） | **延后**：发布不携带 beets/Essentia，改由用户在应用内点装；探针并入批 5 |
| **1** | 双家隔离（`cfg.py` profile 化 + 模板自检 + 白名单三元组 + `BEETSDIR` 加固 + 契约测试） | ✅ 完成 |
| **2** | `enrich` / `query` / `fields`：三段式 + 协议加法 + 假源打通全链路 | ✅ 完成 |
| **3** | 宿主：migration 0008 + `enrichment.rs` + 工作清单 + 批量 upsert（锁粒度）+ 4 条命令（含 GC）+ `busy_timeout` 复核 | ✅ 完成 |
| **4** | UI：曲库索引卡（状态/重建/分析/进度/字段列表/强制重算）+ 工作台卡收编 docs/43 + **§5.5 智能歌单字段**（从批 3 挪来） | 待开工 |
| **5** | `xtractor` 真接入：安装轨道（A 轨 pip / B 轨预置）+ `status` 报三态 + 数值字段实测 | 待开工（合并原批 0） |

**2026-10-30 拍板：beets 与 Essentia 都不进发布包**，全部改为"用户在应用内点一下
安装"。所以批 0 不再是前置阻塞——探针的价值（决定 field 契约要不要留 mood 列）
移交批 5 现场验证；批 2 用**内置假源**（`probe`，环境变量开启）先把协议与
三段式跑到尾，真插件只在批 5 接入。

这个决定还有一个副作用值得记住：**装不装 beets 完全不影响其余功能**。
所以 `enrich`/`query`/`fields` 在未装 beets 的机器上必须整体不存在（`serve`
启动时就回 `E_NOT_INSTALLED`），宿主拿这条错误当"索引能力不可用"，不能当故障弹窗。

批 4 与批 5 可并行。

---

## 9. 门禁与测试（每批都要过）

- `npm run check:boundary` 不动：关联表在 `aiya-data`，beets client 仍无表知识；
- `npm run proto:check`：新事件/命令两侧镜像同步；
- **负向验证**（本项目惯例）：把 DB1 模板里的 `write: no` 改成 `yes`、
  把 `source` 从主键里去掉——两处都必须让测试/自检变红，否则门禁是装饰；
- Python 侧：`configops` 式纯函数单测（模板渲染、白名单三元组）+ 契约用例
  （`enrichItem` 的 `ok/empty/failed`、`BEETSDIR` 冲突拒绝）；
- Rust 侧：migration 幂等、工作清单 SQL（缺失/过期/empty 三态）、
  upsert 在既有行上覆盖、GC 的删除边界。

---

## 10. 待拍板

| # | 问题 | 建议 |
|---|---|---|
| A | 批 0 是否先做（半小时级探针） | **是**。它决定 mood 列是否成立，也决定批 5 是否存在 |
| B | 字段契约：`loudness`（average_loudness）替代 docs/44 的 `energy`，`mood_mirex` + `mood_label` 双列 | **是** |
| C | `lastgenre` / `chroma` 是否进 DB1 首批默认 | **否**：先只有 xtractor，另两个作为"需要联网/需要 fpcalc"的可选项 |
| D | DB1 的 YAML 编辑器 | **不开放**（只服务 DB2），否则"文件不动"前提守不住 |
| E | `status='empty'` 的文件默认是否重试 | **不重试**，只在"强制重算"时再排队 |
| F | Essentia 许可（AGPL 二进制 + CC BY-NC-ND 模型）是否可接受 | 需你判断；不接受的话整条 mood/energy 能力线去掉，只留 bpm/danceability 之外的 lastgenre |

**当前处置（2026-10-30 用户指示"开工"）**：B/C/D/E 按建议值执行，A 未执行（受 §3.9 阻塞），
F 仍待用户判断。批 1 与字段契约无关，故先做——§8 里"批 0 必须先于批 1"的理由是
mood 列是否成立，那是批 3 的事。

---

## 11. 实施记录

### 11.1 批 1（双家隔离）已落地

| 落点 | 文件 | 做法 |
|---|---|---|
| profile 化 | `cfg.py` | `AIYA_BEETS_PROFILE=index\|workbench`（默认 workbench，行为与 v1 一致）；未知值 → `ConfigConflict`，不静默回落 |
| 索引家模板 | `cfg.py::_render_index` | `directory` 恒为 `{home}/scratch`（忽略传入的扫描根）；`import` 六键 + 每个插件的强制参数按表渲染 |
| 渲染后自检 | `cfg.py::verify_config` | 自写极简 YAML 子集解析（不用 `yaml.safe_load`：门禁必须在被手改坏的文本上也能给可读结论）；缺键也报错，不靠默认值 |
| 自愈 | `cfg.py::ensure_config` | 索引家每次启动重渲染（内容相同不落盘），落盘后过闸——外部污染会被写回模板 |
| 白名单三元组 | `plugins.py::IndexPluginSpec` | 名字 + `forced` 参数 + `writes_files` 标志；写盘插件由结构拒绝（`embedart`/`convert`/`scrub` 不在表内） |
| `BEETSDIR` 加固（宿主） | `src-tauri/src/beets.rs` | 抽出纯函数 `env_pairs`，显式下发 `BEETSDIR = AIYA_BEETS_HOME`（`env()` 覆盖继承值），两条单测锁住 |
| `BEETSDIR` 加固（工具） | `cfg.py::apply_env` | 两者都在且不等 → `ConfigConflict`；`cli.main` 捕到后打一条 `E_CONFIG` 信封（id=0）+ 退出码 2 |
| 配置时机 | `cli.py` / `server.py` | `ensure_config` 从 `serve()` 前移到 `cli.main`（apply_env 之后、import beets 之前），`status/capabilities/doctor/serve` 四条命令看到同一份事实 |
| 契约测试 | `beets-tool/tests/config_contract.py` | 8 组：模板 / 负向五连 / 三元组 / 环境两态 / 自愈 / 两家 status / 两家锁 / 冲突 fail-fast；`npm run smoke:config`，已进 `verify` |

**顺手修掉的既有问题**：`status` 以前对新家（还没有 config.yaml）报的是 beets 默认家的
库路径——因为它只 `apply_env` 不 `ensure_config`，而 beets 的默认 `library` 落在
`%APPDATA%\beets`。双家隔离一上来就会被这个"报错位"带偏，故一并前移。

**负向验证（都做过，改坏→红→撤销→绿）**：

| 破坏 | 期望 | 结果 |
|---|---|---|
| 模板 `import.write` 改 `yes` | `[1]` 红 | ✅ 断言打印出整张表，`write: yes` 一眼可见 |
| 摘掉 `verify_config` 的 import 段检查 | `[2]` 红 | ✅ "闸没红：import.write 被改成 yes" |
| 索引家改成"只写一次"（不自愈） | `[5]` 红 | ✅ 闸在 `ensure_config` 内直接抛 `ConfigGateError` |
| 宿主摘掉 `BEETSDIR` 下发 | Rust 单测红 | ✅ `left: "" right: "D:\\data\\beets-index"` |

### 11.2 与文档的偏差

1. **批 0 未做**（`essentia.upf.edu` / `archive.org` 本机均 502，PyPI 无 Windows wheel）。
   批 1 不依赖它；批 0 的出口条件（3 个文件出数 + 耗时 + 体积）仍待网络或手动投喂 zip。
2. `INDEX_PLUGINS` 里的 `lastgenre` / `chroma` 强制了 `auto: no`：它们本来默认在 import
   阶段跑，而索引步骤不该在"补登"时顺带联网。这两个键在批 2 联调时用真插件复核。
3. 索引家 home 路径（`{app_data}/beets-index`）**还没在宿主侧落地**：批 1 只保证
   "给哪个 home 就是哪个家"。谁起这个会话属批 2/3（`enrich` 命令 + Tauri 侧两套 launcher）。

### 11.3 批 2 开工时要注意

- 协议面三处改动（`Stage+=enrich`、`Session` 前缀 `enr`、`Event+=enrichItem/enrichDone`）
  必须走 `proto:sync`，客户端 `TERMINAL_EVENTS` 同步加；
- 假插件轨道：先让 `enrich` 在**没有 Essentia** 时也能跑通全链路（写死字段），
  真 xtractor 只在批 5 接入；
- `status` 现已有 `dataHome` / `libraryLock`，批 2 加"这个家是谁"时注意别破坏
  `config_contract.py` 对四路路径的断言。

### 11.4 批 2（`enrich` / `query` / `fields`）已落地

| 落点 | 文件 | 做法 |
|---|---|---|
| 三段式 | `cmds/enrich.py` | 清旧（`item.remove()`）→ 补登（子进程 `beet import -A -q -s -I -R`，分块到 6000 字符）→ 分析 → 逐 (path, source) 回吐 |
| 源注册表 | `cmds/enrich.py::SOURCES` | 名字 + `implemented` 标志；未接线的源在 `want` 里被 **拒**（`E_INVALID_ARG`），不返回空——否则宿主会把 `empty` 缓存成"不支持"，批 5 接完线还得清缓存 |
| 自测源 | `cmds/enrich.py::_apply_probe` | 按路径 sha256 造确定性字段（bpm/danceability/average_loudness/mood_mirex）；**需 `AIYA_BEETS_ENRICH_PROBE=1`**，默认关（假数据进真库最难排查） |
| 查询 | `cmds/query.py::query` | 结构化规则**在工具侧求值**；`raw` 走 beets 解析器；两者互斥；`total` 是 limit 前的真实命中数 |
| 字段 | `cmds/query.py::fields` | 内置字段带类型 + `item_attributes` 里真出现过的 flexible 键（只读 SQL）+ 各源三态 |
| 协议 | `protocol/v2.schema.json` | `Stage+=enrich`、`Session` 前缀 `enr`、`Request+=enrich/query/fields`、`Event+=enrichItem/enrichDone`、新增 `$defs/QueryRule`；**不升版**（全是加法） |
| 锁 | `lockfile.py` | `enrich` 进 `LOCKED_CMDS`（remove+import 是库写者）；**不进** `ops.TERMINAL_OF`（重放旧结果会掩盖"该重算"） |
| 客户端 | `session.rs` | `TERMINAL_EVENTS += enrichDone`；`proto_mirror.rs` 的命令/事件计数改 16/20（那两条断言是"协议变了，来看一眼"的闹钟） |
| 冒烟 | `tests/smoke_enrich.py` | 7 组：fields / enrich 首跑 / query 四态 / force 幂等 / 拒绝路径 / 文件指纹不变 / 空库首跑。`npm run smoke:enrich`，已进 `verify` |

**与文档的偏差（都是实现期才暴露的）**：

1. **`enrichDone.ok` 改名 `succeeded`。** 契约规定"有 `ok` 就是应答信封"，
   事件里带一个叫 `ok` 的计数会被客户端拿去当信封解析——冒烟第一条就炸。
   这是本批唯一的"设计错误"，不是实现 bug。
2. **三段顺序改为 清旧 → 补登 → 分析。** 原文的"补登 → 清旧"先 remove 就空了。
3. **`-R` 不是 `--noresume`，是 `--incremental-skip-later`。** 语义恰好更有用：
   不把跳过项记进 state.pickle（那正是"索引恒空且零报错"的根因）。
4. **未接线的源直接拒绝**，而不是跑一遍返回空（理由见上表）。
5. `fields` 返回 **97 个内置字段**（beets `Item._fields` 全量）。够用但很吵，
   哪些进 UI 选择器属批 4 的决定，工具侧不做裁剪。
6. **`Library.remove` 在 beets 2.x 已不存在**（方法挂在模型上：`item.remove()`）。
   写错不报错、只是被 `except` 吞掉，`force` 看起来跑了其实没摘——本批实测踩到。
7. **`beet import` 只要 argv 里有一个不存在的路径，整批一条都不导**
   （实测：6 个真文件 + 1 个幽灵 → 库里 0 条）。宿主的清单来自数据库，
   文件在扫描与分析之间被删是正常事件，所以 `_import_paths` 先 `os.path.isfile`
   剔一遍，否则"一个文件没了，整批白跑"。
8. **不删行直接重导会保留旧 flexible 属性**（只换 rowid）。这一条把 §3.4 的
   `remove + 重导` 从"可选优化"提升为"必须"：不清旧的话，插件上一轮写下的
   字段会原地留着，`status` 判定就永远看不出这轮其实没产出。
   顺带解释了为什么用 rowid 变化判断"清旧生效"是错的——两种路径都会换 id。

**负向验证（改坏 → 红 → 撤销 → 绿）**：

| 破坏 | 期望 | 结果 |
|---|---|---|
| 模板删掉 `duplicate_action: keep` | `[2]` 红 | ✅ `succeeded: 1, skipped: 5`——同标签同长的文件只有第一条进库 |
| `enrichItem` 多加一个协议外字段 | schema 校验红 | ✅ `additionalProperties: false` 直接拒 |
| `TERMINAL_EVENTS` 漏掉 `enrichDone` | Rust 单测红 | ✅ `terminal_event_set_is_the_documented_five` 失败 |
| 清旧改用不存在的 `lib.remove` | `[4]` 红 | ✅ 打进去的 `aiya_probe_mark` 原样留着，逐条打印出来 |
| `_import_paths` 不剔幽灵路径 | `[4]` 红 | ✅ `succeeded: 0, skipped: 7`——一个坏路径毁掉整批 |

第 4 行那次负向验证本身返工了三轮才成立，值得记下教训：**断言必须能被破坏**
。最初用"rowid 变了"、再改"总行数变了"，两种在坏代码下都是绿的（beets 重导
会换 id、且不产生重复行）；只有"打一个 flexible 标记、看它消没消"能分辨
"摘掉重登"与"原地更新"。断言写对之前，负向验证一直在骗人。

**没验到的（写在这里，别当成已验证）**：`_run_import` 的 `stdout=DEVNULL`。
父进程先开库，迁移日志由父进程打出（走 stderr），子进程通常无话可说，所以
"不掐会不会污染协议"在这条路径上测不出来。它仍然是必需的防线（见代码注释）。

### 11.5 批 3（宿主关联表 + 4 条命令）已落地

| 落点 | 文件 | 做法 |
|---|---|---|
| 迁移 | `migrations/0008_track_enrichment.sql` | `PRIMARY KEY (path_lower, source)`；提升列 `bpm/danceability/loudness/mood_mirex/mood_label/mood_score`；其余进 `data` JSON；失效判定用 `size_bytes/mtime`；`status CHECK IN ('ok','empty')`；不挂 `tracks` 外键 |
| 数据层 | `crates/aiya-data/src/enrichment.rs` | `path_key`（与 scanner 同口径）/ `argmax_mood`（7 类概率，严格大于、并列取先出现的）/ `stats` / `worklist` / `save` / `find` / `gc`。**10 条单测** |
| 宿主编排 | `src-tauri/src/enrichment.rs` | 清单/统计/查询/GC 的 `db.send` 包装 + `Sink`（`EventSink` 实现，`enrichItem` → 行）+ 独立落库线程（`take_batch` / `write_batch` / 10s 抢锁上限）+ `enrichPersisted` 终态事件 |
| 会话 | `beets.rs::start_enrich` | 新开 `run_session_with`（出口可替换）；`beets:enrich` 通道；`scanning` **不**提前持有 |
| 命令 | `lib.rs` | `enrichment_worklist` / `beets_enrich_start` / `enrichment_query` / `enrichment_gc`（+ `map_beets_start` 认"未安装"） |
| 契约 | `dto/enrichment.rs` | `EnrichmentWorklist` / `EnrichmentMatch` / `EnrichmentGc`（计数用 `u32`，避免 ts-rs 把它们映射成 `bigint`） |
| UI 接线 | `client.ts` + `mock/` | `enrichment` 命名空间 4 个 typed 入口；预览 mock 给确定性假数与演练事件流（`enrichItem → enrichDone → enrichPersisted`） |
| 测试 | 见下 | 数据层 10 条 + 宿主 4 条，其中 1 条是**真 sidecar 端到端**；3 条负向验证 |

**真 sidecar 端到端**（`enrichment::tests::real_sidecar_events_land_in_the_table`）：
起真进程（`AIYA_BEETS_ENRICH_PROBE=1` + 索引家模板）→ 读 stdout 的 `enrichItem`
→ 走生产的 `item_from_event` → `data::save` → 断言 `written=2`、`not_in_library=0`、
`mood_label` 落在 7 类之内。缺 venv 或测试音源时跳过（clean checkout/CI 常态）。
它存在的理由是批 2 已经付过学费的那条：**对着自己手写的事件形状写单测，永远绿**。

**与文档的偏差**：

1. **`busy_timeout` 本来就有**（P1-1，5000ms）。复核而非实现，见 §5.3 的修订。
2. **多了第 4 条命令 `enrichment_gc`**：§7② 明确把 GC 挂在批 3，§5.4 只列了
   3 条，按"欠账优先"取 4 条。
3. **§5.5 智能歌单字段整体推迟到批 4**，理由写在 §5.5 的修订块里（会牵动
   `repo.rs` 四个 FROM 变体与 `count_from`，本批无 UI 可验证）。`enrichment_query`
   已经把数据通道打通，批 4 加规则字段时不用再动数据层。
4. **落库线程独立于事件线程**（文档只要求"按批持锁"）。事件线程里同步落库会让
   sidecar 的 stdout 被磁盘 IO 反压（管道 64 KB），进而触发对面
   `SESSION_IDLE_TIMEOUT`——症状是"工具卡死"，而真因是宿主写库慢。
5. **`failed`/`skipped` 不落行**（文档只定义 `ok|empty`）。`empty` 的语义是
   "这个源对这类文件没结论"且**默认不重试**；把过程故障记成 `empty` 等于永久
   放弃那些文件，用户看到的是"进度条一次比一次短"。它们留在清单里下次重算，
   计数走 `enrichPersisted` 的 `skipped` / `notInLibrary` / `deferred`。
6. **`tool_version` 本批恒 NULL**：`enrichItem` 不带版本，而 `status` 探测与
   enrich 共用会话槽（会话中探测会被 `Busy` 拒）。批 5 接真源时随字段一起上报。
7. **probe 假源补发 7 个 `mood_*` 概率**（批 2 只发 `mood_mirex`）。不发的话
   宿主的 argmax、`mood_label` 列、批 4 的情绪筛选在没有 Essentia 的机器上
   永远为空——那不是"没数据"，是"假源没喂到该喂的地方"。冒烟新增 `[2b]` 断言。
8. **工作清单只取 `availability='online'`**：`missing` 行没有文件可分析，放进去
   只会让清单虚高（sidecar 还会 `os.path.isfile` 再剔一遍），症状是"说要算
   500 首、实际算了 480 首"。
9. **`map_beets_start` 增加"未安装"关键词**：`tool_or_err` 的文案不含
   `interpreter`，"没装组件"会被归成 `invalidArg`，UI 就不弹安装指引了。

**负向验证（改坏 → 红 → 撤销 → 绿）**：

| 破坏 | 期望 | 结果 |
|---|---|---|
| 摘掉 `write_batch` 的 `compare_exchange` 门禁 | 写锁测试红 | ✅ `write_batch_refuses_when_gate_is_taken` 失败（"应为 Busy，实际 Ok"） |
| `save` 放开状态过滤（只挡空串） | 状态测试红 | ✅ `save_skips_failures_and_unknown_paths…` 失败（`written` 由 0 变 2） |
| `item_from_event` 把 `source` 读成 `src` | 端到端红 | ✅ 真 sidecar 测试与单测同时失败——这正是"手写形状单测永远绿"的反证 |

第三条是本批最值得留的一条：它证明端到端测试**能抓住协议漂移**，而不只是
"再跑一遍成功路径"。

**没验到的（别当成已验证）**：

- 落库线程与扫描/刷新**真并发**时的退让行为（`WriteFail::Busy` → 重试 → 10s
  后计 `deferred`）。单测只覆盖了"门禁被占时立刻回 Busy"，"重试到超时"这条
  路径没有测试，只有代码阅读。批 4 的 UI 会把 `deferred` 显示出来，届时用
  真界面制造并发再验。
- 十万首级别下 `stats` 那条聚合查询的耗时（1472 首是毫秒级；`perf_50k` 尚未
  把 `track_enrichment` 纳入）。

---

## 附录 A：复现命令（探针环境）

```bash
# 隔离环境
export BEETSDIR='D:/MUSIC_Player/.probe44/home'
export PYTHONPATH='D:/MUSIC_Player/.probe44/xtpkg'   # 含 probeplug + xtractor
PY=/d/MUSIC_Player/AIYA_MUSIC_TAURI/beets-tool/.venv/Scripts/python.exe

# 1) 首次 as-is 索引（占位 directory → 全绝对路径）
rm -f lib/library.db* home/state.pickle
$PY -m beets import -A -q -s -I -R music

# 2) 二次导入被增量跳过（stage 不重跑）
$PY -m beets import -A -q music            # → Skipped 1 paths

# 3) -I 强制重跑 stage（幂等，条目数不变）
$PY -m beets import -A -q -s -I -R bulk    # 400 文件 / ~13s 首跑 / ~22s 重跑

# 4) 精确清旧 + 重登
$PY -m beets remove -f 'path::099-高音'
$PY -m beets import -A -q -s -I -R 'bulk/album099/099-高音测试 880Hz.wav'

# 5) 范围查询：typed 命中 / untyped 静默 0
$PY -m beets list 'probe_typed:0.4..0.6'
$PY -m beets list 'probe_plain:0.4..0.6'

# 6) xtractor 缺二进制：退出码 0、字段为空
$PY -m beets xtractor -f 'path::低音' ; echo $? ; $PY -m beets list -f '$bpm' 'path::低音'
```

## 附录 B：探针数字备忘

| 指标 | 值 |
|---|---|
| 400 文件首次 as-is 索引（含 1.5s 启动） | 13.0 s ≈ 28 ms/文件 |
| 400 文件强制重导（重跑 stage、幂等） | 21.6 s |
| 5 文件索引 + 5 次 DB 迁移备份打印 | 5.0 s（备份走 stderr，不污染协议流） |
| `beet list` / `remove` / `import` 冷启动 | 1.5–2 s（短进程模型的固定开销） |
