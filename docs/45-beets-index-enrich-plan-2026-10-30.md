# beets 索引增强（关联表 + 宿主回填）实施方案

> 状态：**方案待拍板（本批只调研与实测，不改项目代码）**。日期：2026-10-30。
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
| `enrich {target:"index", want:[...], paths:[...], force:bool}` | 流式（事件） | 三段式主体，逐文件回吐 |
| `query {target, rules\|raw, fields}` | 信封 | 已在 docs/44 §5.1 定义 |
| `fields {target}` | 信封 | 同上 |

事件增量（`Event` 的 oneOf 加一条，`additionalProperties:false` 保持）：

```json
{ "type": "enrichItem", "session": "enrich_01",
  "path": "D:\\Music\\a.flac", "source": "xtractor",
  "status": "ok",              // ok | empty | failed | skipped
  "fields": { "bpm": 122.0, "danceability": 0.61, "mood_mirex": "mood_happy", "...": "..." },
  "message": null }
```

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
| 客户端 `TERMINAL_EVENTS` | 加 `enrichDone`（现有五个：done/applyDone/cancelled/sessionFailed/pluginDone） |

### 4.3 sidecar 内部三段式（`cmds/enrich.py`）

```
① 补登  import -A -q -s -I -R <分批路径>      # 幂等；已在库的会重跑 import_stage
② 清旧  仅对 force/stale：remove -f 'path::<绝对路径正则>'  # 字段清空 → 插件重新挑中
③ 分析  逐个源跑命令：
        xtractor   → beet xtractor -f <query>   （-f 才能覆盖已有值；绝不带 -w）
        lastgenre  → beet lastgenre -f <query>  （写 item.genre，不入文件）
        之后按 path 读库，emit enrichItem
```

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
- 顺手补 `db.rs` 的 `busy_timeout`（现在只有 WAL，没有 busy 等待）：
  第二个写者一旦漏拿锁就是立刻 BUSY，而不是等 100ms。

### 5.4 Tauri 命令（3 条）

`enrichment_worklist`（算清单 + 计数）、`beets_enrich_start`（起会话、推事件）、
`enrichment_query`（按维度过滤，给曲库页/智能歌单用）。`beets_enrich_cancel`
复用现有 cancel 通道。

### 5.5 动态歌单与查询（拍板 E 的落地）

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

| 批 | 内容 | 出口条件 |
|---|---|---|
| **0** | Essentia 探针：拿到 Windows 静态二进制 + beta5 模型，3 个文件跑出 `bpm/danceability/mood_mirex`；实测单曲耗时、体积、许可声明 | 出数 + 耗时表；拿不到 → 批 5 降级为"只接 lastgenre" |
| **1** | 双家隔离：`cfg.py` profile 化 + DB1 模板自检 + 白名单三元组 + `BEETSDIR` 加固 + 契约测试（两个 home 各自的 status/锁） | `npm run smoke:proto` + 新增双家契约用例全绿 |
| **2** | `enrich` / `query` / `fields`：三段式实现 + 事件 + 契约测试；**先用内置假插件**（写死字段，等价 probeplug）打通通路 | 假插件下 400 文件全链路可跑，取消/续跑可用 |
| **3** | 宿主：migration 0008 + `enrichment.rs` + 工作清单 + 批量 upsert（锁粒度）+ 3 条命令 + `busy_timeout` | 单测 + 50k 基准不退化；kill -9 后重跑不重复分析 |
| **4** | UI：曲库索引卡（状态/重建/分析/进度/字段列表/强制重算）+ 工作台卡收编 docs/43 的目录卡与操作清单 | e2e 更新 + 三主题截图 |
| **5** | `xtractor` 真接入：安装轨道（A 轨 pip / B 轨预置）+ `status` 报三态 + 数值字段实测 | 干净机装完能出数；缺二进制时 UI 报"未就绪"而非"已完成" |

批 0 必须在批 1 之前跑（它决定 §6.3 的字段契约要不要保留 mood 列）。
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
