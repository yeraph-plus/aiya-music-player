# beets 集成重调研：双库隔离与单向查询

> 状态：**调研完成，方案待拍板（本批不改代码）**。日期：2026-10-30。
> 用户方向：beets 能力分两类（元数据扩展 / 文件路径与格式操作）；mpdupdate、m3u 生成
> 等互操作类功能在当前应用形态下无用，忽略；另有 info / random / fuzzy / missing /
> duplicates 等实用功能。通过**环境隔离让 beets 生产两份 db**：一份基于应用曲库、
> 只读文件不写；一份服务于待整理目录、可读写。应用内不做 sync，改为**针对 beets
> 数据库的单向元数据 CLI 查询**。待整理目录部分设计为可配置的插件清单（用户勾选
> 执行哪些操作）。
> 待解问题（用户原问）：beets 自己的歌曲标记模式能否与应用的入库 ID 方法互相转换。
> 前置：docs/33（两侧分离）、docs/41 §10（v2 + 终态：对账链已整体移除）、
> docs/42（进程形态不变）、docs/43（UI 重做方案，本文修订其部分设计）。

---

## 1. 结论先行

**双库隔离成立，且与已有的"零接触"终态天然互补**——DB1（曲库索引）把 beets 降级为
"可丢弃、可重建的元数据增强索引"，DB2（整理工作台）就是现有 preview/apply/维护
四件套的运行地（现状零迁移）。应用的曲库事实**仍然 100% 归扫描器**，beets 两份库
都不写应用 DB；两库与曲库之间唯一的桥是**磁盘文件 + 绝对路径**。ID 转换问题的答案：
**不需要转换**——桥是路径，beets 的 item id 是库本地 rowid（重建即变），永远不要
当跨系统 ID 用；确需跨库稳定引用时可用 flexible 字段存应用 id（可选，默认不做）。

---

## 2. 能力分类盘点（用户分类的落实）

| 类 | 能力 | 去向 |
|---|---|---|
| **元数据扩展** | 读写 flexible 字段；xtractor（Essentia：danceability/energy/mood，写入 flexible attrs）；lastgenre（Last.fm 曲风）；chroma（AcoustID 指纹查询）；lyrics；fetchart | **DB1 只读增强白名单**（xtractor/lastgenre/chroma）；写文件类（lyrics/fetchart/replaygain 的写盘行为）归 DB2 |
| **文件路径与格式操作** | import（补标签/移动/复制）、convert（转码）、replaygain（写 RG 标签）、move、edit | **DB2 整理工作台**（现有 preview/apply 流 + 操作清单勾选） |
| **实用查询** | info、random、fuzzy（模糊查询插件）、missing、duplicates | info/random/fuzzy → DB1 查询设施的能力项；duplicates → DB2（工作台内查重）；missing → 对 DB1 无意义（曲库成员资格归扫描器），对 DB2 保留 |
| **忽略** | mpdupdate（MPD 通知）、m3u/smartplaylist 生成 | 移出插件市场白名单（smartplaylist 从 MARKET_CATALOG 删除——用户拍板 m3u 无用） |

---

## 3. 实测验证（2026-10-30，beets 2.14.1 + 项目 sidecar 实跑）

| 探针 | 结果 |
|---|---|
| 双家隔离 | `BEETSDIR` 换家即完全隔离：全新 HOME_A + 手写 config → 现有 sidecar `adopt_library` 直接可用（2 文件全收）——**两份 db 不需要任何 sidecar 结构改动，只是每条命令携带不同的家** |
| 路径存储形态 | `directory` 内文件存**相对路径**、外文件存**绝对路径**（存储层混合），但 **Item API / dump 恒回绝对路径**——两种扫描根（库内/库外）在查询输出上无差别，路径桥接成立 |
| flexible 字段 | `it.mood = "happy"; it.store()` 写入 ✓；`mood:happy`（字符串）与 `mood::^hap`（正则）查询 ✓；`energy:0.7..0.9`（数值范围）**0 命中——数值型 flexible 查询需在实现期再验**（可能须精确匹配或整型范围），xtractor 的数值字段查询形态列入实现期验证项 |
| 顺手确认 | beets 首库创建时的迁移备份打印全部走 stderr（runtime.open_protocol_stream 的既有防护有效），不污染协议流 |

---

## 4. 双库设计

### 4.1 布局与命名

| | DB1 曲库索引 | DB2 整理工作台 |
|---|---|---|
| 家（BEETSDIR） | `{app_data}/beets-index` | `{app_data}/beets`（**现状零迁移**——现有 config.yaml/library.db 就是它） |
| 角色 | 应用曲库的**只读**元数据增强索引 | 待整理目录的读写工作台 |
| 写文件 | **永不**（copy/move/write 全关） | 按用户操作（apply 的 copy/move、convert、replaygain） |
| `directory:` | 第一个扫描根（实测：库外路径照样索引且查询回绝对路径） | 整理输出根（docs/43 目录卡语义收窄为只管 DB2） |
| 导入方式 | `import -A`（as-is，不联网不改标签）+ `incremental: yes` 增量；`beet update` 语义刷新已登记文件的标签变化 | 现有 preview → 决策 → apply |
| 插件 | 只读白名单（§4.2） | 操作清单（用户勾选，§4.3） |
| 生命周期 | 可随时删除重建（索引是派生物）；刷新 = 显式操作（拍板点 A） | 持久工作状态 |
| 查重 | 不做（成员资格归扫描器） | find_duplicates 保留 |

### 4.2 DB1 只读插件白名单（拍板点 B）

安全标准：**只写 beets 库（flexible attrs），不写音频文件、不联网改文件**。

- `xtractor`（Essentia：mood/energy/danceability/tempo 等，全部 flexible attrs）——
  重依赖：需要 Essentia 提取器二进制；安装轨道（A 轨 pip / B 轨套件包预置或引导
  下载）是白名单采纳的前置成本，单独评估；
- `lastgenre`（Last.fm 曲风 → flexible genre；配置为不写文件标签）；
- `chroma`（AcoustID 指纹，查询型）。

明确排除出 DB1：`fetchart` / `lyrics` / `replaygain` / `convert`（都写文件）→ 归 DB2
操作清单。**DB1 插件一律不允许出现在写路径上**，白名单在工具侧硬编码校验（DB1 的
config 渲染只接受白名单内插件，P1 不破：清单本身是 beets 知识）。

### 4.3 DB2 操作清单（用户勾选）

docs/43 的"插件卡"语义收窄为 **DB2 操作清单**：用户勾选"整理时执行哪些增强"——
`[补全元数据(autotag，内置)] [replaygain] [转码 convert] [fetchart] [lyrics]`，
勾选结果落 DB2 config 的 `plugins:` 行 + 每插件参数段（docs/43 §4.2 的描述符机制
不变，只是服务对象明确为 DB2）。执行 = 现有 preview → 决策 → apply 流，勾选的
增强作为 apply 的伴随步骤（beets 插件机制天然在 import 管线上挂载）。

### 4.4 会话与锁

两库共用现有的单会话槽与短进程模型（beets 操作全是手动的，串行足够）；
锁文件已按 `data_home` 隔离（两把锁互不干扰）。status/doctor 报两份健康
（各自 protocolVersion/schemaHash/库路径/插件三态）。

---

## 5. 单向查询设施（应用侧命令行构造）

### 5.1 形态

应用**不解析 beets 库文件、不做 sync**；查询走 sidecar 新命令（协议 v2 纯加法）：

- `query { target: index, rules: [{field, op, value}...] | raw: "mood:happy" , fields: [...] }`
  → 逐条 `{path, id, title, artist, ..., flexible:{mood:..., energy:...}}`；
  **path 恒为绝对路径**（实测保证）→ 宿主按 path 连接曲库行（`ids::track_id` 同源）；
- `fields { target: index }` → 可用字段清单（内置 + flexible，等价 `beet fields`），
  UI 的查询构造器据此渲染可选字段；
- 规则形态复用动态歌单的 `{field, op, value}` 结构（docs/40 已有先例）——**语法渲染
  归工具侧**（P1：beets 查询语法是 beets 知识，播放器不构造语法字符串）；
  原始 beets 语法（`mood::^hap`、fuzzy `~`）作为高级逃逸口提供（拍板点 C）。

### 5.2 应用侧消费

查询结果按 path 与曲库行连接后，能力形态举例：智能歌单引用增强字段
（"energy > 0.8 的歌"）、曲库页按 mood/energy 过滤、专辑页显示 xtractor 的
tempo。**增强字段是曲库行的"外挂数据"，不进应用 DB**（应用 DB 是唯一事实，
增强数据可随时随索引重建而再生成）。

---

## 6. ID 转换问题的正面回答

**不需要转换，桥是绝对路径。**

1. beets 的 item `id` 是其 library.db 的整型 rowid：库重建即变、两库各自独立、
   re-import 会换新 id——**任何情况下都不该当跨系统标识用**；
2. 应用的 `track_id = ids::track_id(path)` 是路径哈希：同一路径在 beets（两库）、
   文件系统、应用曲库之间天然对齐——查询结果回传绝对路径，连接零成本（实测 ✓）；
3. 若未来确需跨库稳定引用（例如 DB1 重建前后给增强结果做增量标记），可在
   as-is 导入时把应用 id 写进 flexible 字段（`app_track_id`）作为**可选**锚点。
   默认不做：这会让 beets 侧知道应用的 ID 方案（P2 反向耦合），而路径桥已经够用；
4. 既有的历史连续性债不变：DB2 移动文件进扫描根后，应用扫描器视为新文件
   （路径哈希变），播放统计/歌单引用会断——已登记 docs/43 §8-E 代价①，
   归扫描器侧的移动跟随启发式解决，与本调研无关。

---

## 7. 对 docs/43 计划的修订

| docs/43 原设计 | 修订 |
|---|---|
| 目录卡：整理输出根 + 待整理目录 | 保留，但**归属 DB2 工作台卡**（输出根 = DB2 的 directory） |
| 插件卡：市场 + 开关 + 配置表单 | 拆成两块：**DB1 索引增强白名单**（只读，含 xtractor 安装引导）与 **DB2 操作清单**（勾选执行） |
| YAML 编辑器 | 保留，per-DB 两份（各自的 config.yaml） |
| 协议增量 | 在 §6.2 的 5 个 config 命令之外新增 `query` / `fields`（带 target 参数区分两库）；status 增加 target 维度（两份健康） |
| 新增 UI | "曲库索引"卡：重建/刷新按钮、白名单插件、字段列表、查询构造器入口；ImportPanel 语义不变（DB2 的前端） |

批次重排（docs/43 §6 → 本文档版本）：**批 1** 双家隔离落地（sidecar 按 target 携带
不同 BEETSDIR + DB1 config 渲染 + 白名单校验）；**批 2** `query`/`fields` 命令 +
契约测试；**批 3** UI 两卡重排；**批 4** config/YAML 编辑器（docs/43 原批 4/5）；
**批 5** xtractor 安装轨道评估与接入。

---

## 8. 待拍板

| # | 问题 | 建议 |
|---|---|---|
| A | DB1 刷新策略：显式"重建索引"按钮 vs 跟随扫描自动增量 | **显式**（零接触原则的一致延伸；索引是派生物，重建成本 = 一次 as-is 扫描） |
| B | DB1 只读白名单初始集合 | **xtractor + lastgenre + chroma**；fetchart/lyrics/replaygain/convert 归 DB2。xtractor 的 Essentia 依赖单独走安装评估 |
| C | 查询接口：结构化规则为主 + 原始语法逃逸 | **是**（P1 一致性；原始语法满足高级用户） |
| D | smartplaylist/m3u 从市场白名单移除 | **是**（用户已拍板 m3u 无用） |
| E | numeric flexible 查询形态 | 实现期对 xtractor 数值字段单独实测（本次 `0.7..0.9` 未命中）；**随拍板 F 自然消解**——数值维度在关联表里是 REAL 列，查询走 SQL |
| F | **应用内新增曲库关联表回填 beets 产出**；自定义文件标签**否决**（不纳入考虑）；重型分析结果必须持久化（保留已跑完的记录）；beets 产生的 db 用后可丢弃；待整理目录完成后文件由用户移走或进入应用曲库目录；**未装 beets 时仅关联表功能不工作，其余零影响** | 2026-10-30 拍板。写入方修正：**宿主写**，流程脚本只产出 path 键的结构化结果（理由见 §9.1） |

---

## 9. 拍板 F 的修订设计：关联表 + 宿主单向回填

### 9.1 写入方修正（对用户原始设想"流程脚本向应用内写数据"的修正）

流程脚本（beets-tool 侧的 `enrich` 流程命令）**只产出数据，不执行写入**：

```text
宿主                          sidecar(enrich 流程)                beets DB1
 ├─ 算工作清单                 ├─ import -A（as-is，增量）          （计算工作台，
 │  （曲库行 LEFT JOIN         ├─ 逐文件跑 xtractor/lastgenre        可随时重建）
 │   关联表 → 缺失/过期路径）   └─ 逐文件 emit {path, fields:{…}}
 ├─ 采集事件 ←────────────────────── NDJSON 事件流（与 dump_items 同款）
 └─ enrichment_upsert(path, …) → 应用 DB 关联表（宿主持 scanning 互斥写）
```

三条硬理由：① 边界门禁（check-boundary / P2 / README 拍板 #5）禁止 beets 侧
知道应用数据模型；② 写应用 DB 必须持有宿主进程内的 scanning 互斥，sidecar
作为独立进程**物理上拿不到**；③ 打包在 sidecar 里的写行逻辑遇上老版本应用
DB 就是隐形迁移器。

### 9.2 关联表（应用 DB 侧，宿主所有）

```sql
CREATE TABLE track_enrichment (
    path_lower  TEXT PRIMARY KEY,   -- 路径桥（与扫描器/beets 两库同键）
    source      TEXT,               -- 'xtractor' | 'lastgenre' | …
    mood        TEXT,               -- 常用维度提升为可查询列（索引）
    energy      REAL,
    danceability REAL,
    -- 其余 xtractor 产出进 JSON（约 50 个低频字段不占列）
    data        TEXT,               -- JSON blob
    size_bytes  INTEGER,            -- 文件事实，失效判断用
    mtime       INTEGER,
    analyzed_at INTEGER
);
```

- **键 = path_lower**：与扫描器、beets 两库同一座桥；不以外键挂 tracks
  （扫描根被移除后关联仍存活，查询期再连接）。
- **持久层语义**：重型分析结果的家是**应用 DB**（随曲库备份）——这解掉
  §9 反驳里的持久性矛盾：DB1 从"昂贵数据的不备份容器"降级为
  "可丢弃的计算工作台"，重建不损失任何分析记录。
- **失效与续跑**：size/mtime 变了 → 行过期，宿主重算工作清单时自动重排队；
  中断重跑天然幂等（upsert）。beets DB1 删除/重建不影响关联表。

### 9.3 能力与边界

- **查询/歌单**：关联表的 mood/energy 等成为 SQL 可查列——动态歌单规则引擎
  的数据源扩展（docs/40 的规则结构加一个来源维度）与"beets 语法逃逸查询"
  都以它为底座；**运行时查询/播放零 beets 依赖**（拍板点 C/E 的数值问题
  随 SQL 化消解）。
- **未装 beets**：关联表功能不工作（查询构造器隐藏、分析按钮置灰），
  其余零影响——边界由编译期保证（核心 crate 不依赖 beets client）。
- **明确不在范围**：库内文件的原位标签修复（用户文件不动是拍板前提）；
  自定义文件标签（否决）；beets 向应用 DB 写任何行（写入方恒为宿主）。
- **已登记的既有欠账不变**：DB2 移动文件进库后的历史连续性（docs/43
  §8-E ①）；路径变化 → 关联表行按 path 失效 → 重分析（与欠账①同根）。

### 9.4 批次（取代 §7 批次表）

| 批 | 内容 |
|---|---|
| 1 | 双家隔离：sidecar 按 target 携带 BEETSDIR（index/workbench）+ DB1 config 渲染 + 只读白名单硬校验 |
| 2 | `enrich` 流程命令（import -A + 插件分析 + 逐文件 emit）+ `query`/`fields`（结构化规则 + 原始逃逸）+ 契约测试 |
| 3 | 宿主：track_enrichment 迁移 + 采集入表 + 工作清单（缺失/过期路径）+ ui 触发的分析会话 |
| 4 | UI：曲库索引卡（重建/分析/进度/字段列表）+ 工作台卡收编 docs/43 的目录卡与操作清单 |
| 5 | xtractor 安装轨道（Essentia 依赖评估）+ 数值字段查询实测 |

## 10. 落地记录（实施后追加）

（占位。）
