# docs/29 — 曲库工具页拆分 · 扫描 worker 拆分 · beets 集成调研（2026-10-26）

状态：**调研稿，未开工**。按流程先审后做——文末「§9 待确认问题」逐条拍板后才进实现批。

---

## 1. 目标（用户需求原文归纳）

把音乐库扫描、beets、日志从设置页拆出成独立页面；顺带解决六件事：

1. 扫描目录支持多个文件夹（textarea，一行一个）；
2. 扫描进度条"入库快、进度久转"——疑似标签解析阻塞，拆 worker：入库只管入库、解析只管解析；
3. beets 用仓库内 `install.py` 配置 venv 安装 beets + 插件，发布时打包携带；
4. 调研 beets GUI 壳的常见设计；希望 beets 维护曲库（元数据修正/文件组织/完整性/重复扫描），
   并评估"用 beets 查询指令（标签/范围/正则）替代内置索引"是否可行、与现有数据表如何结合
   （beets 直接当 worker + 唯一事实源？）；
5. 候选插件调研：chroma、lyrics、fromfilename、spotify、replaygain、duplicates、scrub、
   smartplaylist、convert、vibenet、xtractor、acousticbrainz——哪些能集成本项目；
6. 评估直接用 beets 官方 web 插件（应用内不设计 GUI，放链接跳浏览器）。

---

## 2. 现状模型摸底（代码证据）

### 2.1 扫描管线（单线程串行，这是进度问题的根源）

```
library_scan 命令（lib.rs:893）
  └─ spawn_blocking（独立连接，不占主 DB 线程）
      └─ scanner::scan_directory_ext（scanner.rs:367）
          ├─ walk()          全量遍历收集文件列表进内存（walkdir，剪 .git 等）
          └─ for batch in chunks(200):
              ├─ 批内逐文件 parse_file()   ← lofty 同步读标签 + 封面提取（含 sha/落盘）
              ├─ 一个 IMMEDIATE 事务批量 upsert
              └─ on_progress 回调（批边界才动一次）→ 宿主 100ms 节流 emit scan:progress
```

- **进度"久转"的实情**：进度粒度是**批边界**（200 个文件），一批内解析 200 个文件
  （无损封面提取 + 哈希落盘是大头）期间进度完全不动；解析与入库**同线程串行**，
  DB 写得快但必须等批内全部解析完才提交——与用户"入库快、解析阻塞"的判断一致。
- 取消：`AtomicBool`，walk 每条目 + 每批事务边界轮询；取消路径不跑 mark_missing。
- 扫描根：单一 `library.root`（settings 字符串，`pickRootAndScan` 选目录后持久化）。
- UI：`LibraryTools.svelte`（扫描按钮 + 进度 + beets 任务条）挂在设置页
  `#settings-library` 区块；`ImportPanel.svelte` 是 beets 导入预览/应用面板（389 行）。
- 设置页结构（SettingsPanel）：六分组滚动单页，左栏 scroll-spy。**注释里明确写了**
  "所有区块始终渲染：扫描/beets 任务必须挂常驻区块，条件切换会让任务进度卸载"——
  拆页时这条约束要一起搬走（新页面成为任务进度的常驻宿主）。

### 2.2 beets 侧现状（docs/08 ADR：beets 写权威、自有 DB 读模型）

- sidecar：`sidecar/beets_sidecar.py`（564 行，include_str! 打进宿主二进制，落盘 app_data/sidecar/），
  行 JSON 协议，命令：`status / ping / preview_import / apply_import / dump_items / cancel`。
- 宿主 `beets.rs`：解释器探测（系统 Python）→ spawn sidecar → 行协议收发。
- 读模型 `beets_sync.rs`：`dump_items` 全量 item → 按 path（大小写不敏感）+ beets_item_id
  对账 upsert 自有 tracks 表；移动跟随、orphan 只记账。**分块 500 提交防写锁抢占播放历史**。
- 自有表独有数据：play_count / last_played_at / 队列 / 歌单 / 播放历史——beets 完全不管。

---

## 3. 多扫描目录（需求 1）

- settings 新键 **`library.roots: string[]`**；读取兼容旧 `library.root`（字符串 → 单元素数组），
  首次写入后新键为唯一事实（沿用 spectrumEngine→spectrumStyle 的"新键取代旧键"惯例）。
- UI：textarea 一行一个路径（空行忽略、去重、去引号——Windows"复制文件地址"带引号是常踩坑）；
  另保留"添加目录"按钮（文件夹选择器逐个追加），textarea 直接可编辑。
- Rust：`scan_directory_ext` 外层循环多根：先对全部根做 walk 合并文件列表（或逐根扫描），
  `seen` 集合 = 所有根的并集，missing 对账语义不变（不在任何根 → missing）。
- 校验：不存在的路径标红不阻塞其余；两个根互为父子目录时会重复扫同一文件——
  path 对账（path_lower 唯一约束）天然幂等，不算 bug，但 UI 给一行提示更好。

## 4. 扫描 worker 拆分（需求 2）

目标形态：**生产者（解析）与消费者（入库）分离，SQLite 单写者**。

```
walk 线程 ──路径──▶ 解析线程池（rayon，N=物理核-1）──ParsedTrack──▶ mpsc channel
                                                                        │
              进度事件（parsed/imported 两段计数）◀── 入库线程（独占连接，批事务提交）
```

- **SQLite 约束**：连接不能跨线程共享 → 入库线程独占一个连接；解析线程不碰 DB。
- **封面提取归解析侧**（hash + 原子落盘是 IO/CPU 混合活），artwork_cache 目录传给解析 worker。
- **进度改两段**：`phase: parsing | importing`，`parsed/scanned` 双计数——进度条按
  scanned（已入库）走，同时显示"解析中 X"就能解释"入库快解析慢"的观感；事件仍走
  100ms 节流，但从入库线程发（批边界 + 解析计数转发）。
- **有序性**：不保证跨文件顺序无所谓（upsert 按主键幂等）；但**批事务语义要保留**
  （一批失败回滚整批记账 failed），channel 攒批由入库线程做。
- 取消：`should_cancel` 查 walk、解析（每文件前）、入库（批边界）三处；取消后 channel
  drain 收尾，仍不跑 mark_missing。
- 预期收益：解析并行化（4 核 ≈ 3-4 倍），且进度条从"批粒度"变"文件粒度"，
  "久转不动"的观感直接消失。风险：rayon 引入新依赖（或手写线程池，工作量不大）；
  Windows Defender 实时扫描可能才是真瓶颈——并行后 IO 争抢，收益要真机量。

## 5. beets 环境分发（需求 3）

三种路线对比：

| 路线 | 做法 | 体积 | 离线 | 评价 |
|---|---|---|---|---|
| A. install.py 首启建 venv | 检测系统 Python → `python -m venv` → `pip install beets[...]`（PyPI 联网） | 0 | ✗ 需联网 | 最简单；受系统 Python 版本/网络影响，装机体验差 |
| B. 发布携带预构建 venv | CI（Windows runner）上建 venv 装好 beets + 插件，整个目录打进安装包；install.py 退化为校验/修复脚本 | ~40-80 MB | ✓ | **推荐**。与 sidecar 现有"include_str! 落盘"模式同思路，只是从单文件变目录；beets 纯 Python 依赖少，venv 可移植性好 |
| C. Python embeddable + wheels | 携带 embeddable python（~11 MB）+ 离线 wheelhouse，install.py 解包安装 | 类似 B | ✓ | embeddable 官方不支持 pip（需手工 get-pip）；比 B 多一层脆弱性，只在"系统完全无 Python"时才需要 |

- **推荐 B + A 兜底**：发布带 venv（B），`install.py` 同时支持 `--from-wheels` 离线修复
  与在线安装（A）；宿主 `beets.rs` 探测顺序：自带 venv → 系统 beets（现行为）→
  触发安装引导 UI。venv 的 python.exe 直接当 sidecar 解释器，`sidecar_env()` 加
  `VIRTUAL_ENV`/`BEETSDIR` 指向 app_data。
- 插件装哪些进 venv：见 §7 矩阵的"首发档"。ffmpeg / fpcalc / Essentia extractor 是
  **独立二进制**，不进 venv，走"可选组件"路径（检测到才启用对应插件功能）。

## 6. beets GUI 壳调研（需求 4a）

四种模式 + 代表项目：

| 模式 | 代表 | 特点 | 对本项目的启示 |
|---|---|---|---|
| 官方 web 插件 | `beet web`（Flask REST + Backbone 单页） | **后端 REST 是资产**（/item/query/<beets 查询串> 直接暴露全部查询能力），前端官方自评"very basic... clunky" | 不要用它的前端；API 可当查询通道（见 §8） |
| 第三方 web 壳 | Beetiful（命令构建器+配置编辑器+库浏览器）、beets-flask（导入前预览/自动导入/撤销/Web 终端/多收件箱）、Beetle（Node 前端消费 web API） | 共同点：**导入预览 UI 是核心价值**（beets-flask 的动机原话："beets 自动打标大部分时候对，但想控制的人不方便"）、配置编辑器、库浏览 | 我们的 ImportPanel 已经覆盖"预览+候选+应用"，方向与社区演进一致，不必引外部壳 |
| CLI 包一层 | Beetiful 的命令构建器 | 把 `beet import/list/modify` 做成交互式构建 | 可作为高级功能的降级出口（"运行命令"文本框），不作为主交互 |
| 应用内 sidecar 协议 | **本项目现状**（docs/08） | 行 JSON 协议、进程隔离、取消代数 | 维持。新需求（元数据修正/文件组织）按同一协议加命令即可 |

**结论**：社区没有"嵌入式桌面 GUI"的现成方案，全部是 web 壳或 CLI 包装；本项目已有的
sidecar + 预览面板在架构上不落后于任何代表项目。缺的是命令面（modify/move/update/
duplicates 等，见 §7 功能映射）。

## 7. beets 查询替代内置索引？（需求 4b，核心决策）

### 7.1 事实

- beets 的 `library.db` **就是 SQLite**（items / albums / item_attributes / album_attributes
  四表 + 弹性字段列）；Rust 理论上可以只读直连，但：
  - schema 由 beets 版本控制（弹性字段 ALTER TABLE 动态加列），直连=耦合其内部模型；
  - beets 注册了自定义函数 `regexp()`/`bytelower()`，外部连接查正则查询会报函数不存在；
  - docs/08 已定的 ADR（自有读模型）与 54 条命令契约全部建立在自有表上。
- beets 查询语法（`field:term`、`::正则`、`:start..end` 数值范围、`=精确`、`-field:` 负向、
  逗号 OR）是**解析层**能力，不在 SQL 里，直连 DB 拿不到——要么经 sidecar（`lib.items(query)`），
  要么经 web REST（`/item/query/...`）。

### 7.2 三个选项

| 选项 | 说明 | 判定 |
|---|---|---|
| ✗ beets 当唯一事实源（去掉自有 tracks） | 播放计数/历史/队列/歌单无处安放（beets 有 mpdstats 字段但语义/更新路径完全不同）；54 命令契约、游标分页、虚拟列表全要重写到 beets schema 上；beets 升级即 breaking | 否决。播放行为数据必须自有 |
| △ beets.db 只读直连当查询引擎 | 绕过 sidecar 延迟；但要复制查询翻译、处理弹性字段、函数缺失 | 不推荐。复杂度高、收益仅省一次进程往返 |
| ✓ **beets 当写权威 + 自有读模型（现状 ADR），查询能力按需下推** | UI 高级筛选 → beets 查询串 → sidecar 新命令 `query_items`（流式返回，复用 dump_items 通道）→ 映射成 TrackRow 展示。普通浏览/播放仍走自有表 | **推荐**。能力与隔离兼得 |

### 7.3 结合方案（推荐选项的具体形态）

- **元数据修正**：`beet modify`/edit 语义 → sidecar 新命令 `modify_items`（写标签 + 更新
  beets 库）→ 完成后自动触发一次 `dump_items` 对账，读模型自动收敛（现有 sync 机制白拿）。
- **文件组织**：`apply_import` 已支持 move/copy；`beet move` 补一个 `refile_items` 命令。
- **完整性/重复**：`beet duplicates`（插件）→ `find_duplicates` 命令 → 结果列表复用
  TrackList 展示（去留决策走 remove/modify）。
- **扫描对账**：beets 库为准的全量 `dump_items` 已有；"重复扫描"（beets update 语义）→
  `update_library` 命令。
- **高级查询**：曲库页筛选器加"beets 查询"输入（示例提示 `genre:pop year:2020..2024
  artist::^周`），命中结果以临时列表展示（不替换虚拟列表主查询——分页/排序契约不动）。

## 8. 候选插件集成矩阵（需求 5）

| 插件 | 来源 | 外部依赖 | 与本项目契合 | 建议 |
|---|---|---|---|---|
| replaygain | 内置 | ffmpeg 或 mp3rgain | **极高**：自有表 `gain_db` 列已在消费（引擎链路预留） | 首发档 |
| lyrics | 内置 | 纯 Python（lrclib/genius 源；musixmatch 已被官方移除） | **极高**：LRC 是规划内能力，lyrics 字段可直供 | 首发档 |
| fromfilename | 内置 | 无 | 高：无标签文件导入兜底 | 首发档 |
| duplicates | 内置 | 无 | 高：完整性维护核心 | 首发档 |
| scrub | 内置 | 无 | 中：导入时清脏标签 | 首发档（低配置） |
| smartplaylist | 内置 | 无 | 高：基于 beets 查询生成 m3u，与"猜你喜欢"规划衔接 | 首发档 |
| chroma | 内置 | fpcalc（chromaprint）+ AcoustID API key | 中高：无标签/错标签文件的指纹纠错 | 二档（可选组件检测启用） |
| spotify | 内置 | 内置公共凭据（Client Credentials，可覆盖） | 中：元数据源补充；国内网络可达性存疑 | 二档 |
| convert | 内置 | ffmpeg | 中：转码导出需求未出现 | 观察档 |
| xtractor | 第三方（官方推荐替代 acousticbrainz） | Essentia `streaming_extractor_music` 二进制 + SVM 模型包 | 中：bpm/mood 等特征字段——"猜你喜欢"的潜在特征源，但部署配置重 | 后期档 |
| vibenet | 第三方 | pip 装 + ML 推理（模型大） | 中低：valence/energy 等情绪特征，同上 | 后期档 |
| acousticbrainz | 内置（已弃用） | 服务已关停（需自建镜像才可用） | ✗ | **不集成**，官方替代即 xtractor |

## 9. web 插件方案评估（需求 6）

- `beet web`：REST+JSON（`/item/`、`/item/query/<查询串>`、`/item/<id>/file`、`/stats`），
  默认 readonly，端口 8337，CORS 可配。**API 有价值，前端不值得嵌入**（官方自评 clunky，
  社区三个壳项目都是因为嫌它简陋才出现的）。
- 两条可行用法：
  a) **外链浏览器**：设置/工具页放"打开 beets Web"按钮，sidecar 起 `beet web`（venv 解释器
     运行 `python -m beets web`）→ `shell.open`。成本极低，作为"高级用户出口"合理；
  b) 应用内消费 REST：与 sidecar 命令重复，且多一个端口生命周期要管，**不采用**。
- 结论：**本期不做 web GUI**；查询能力走 sidecar `query_items`（§7.3），
  web 外链作为零成本可选项放进工具页。

## 10. 推荐落地路线（分批）

1. **批 A（页面拆分 + 多目录）**：新路由页（AppPage +1，「曲库工具」），从设置页搬
   LibraryTools + ImportPanel + 日志；`library.roots` 多目录（textarea + 校验 + 兼容旧键）。
2. **批 B（worker 拆分）**：§4 的解析池/入库线程/两段进度/取消三处轮询；真机量验证收益。
3. **批 C（venv 分发）**：install.py（在线 + `--from-wheels` 离线）、CI 预构建 venv、
   beets.rs 探测顺序改造。
4. **批 D（beets 命令面扩展）**：sidecar 加 query_items / modify_items / refile_items /
   find_duplicates / update_library；UI：高级查询输入、重复管理、元数据编辑入口；
   首发档插件写入默认 config.yaml。
5. **批 E（观察档）**：chroma/spotify 可选组件检测；xtractor/vibenet 待"猜你喜欢"定特征
   需求后再评估。

## 11. 待确认问题（开工前逐条拍板）

1. 新页面入口放哪：侧栏第七项「工具」，还是保持侧栏六项、从设置页/曲库页跳转？
2. 多目录语义：多个根的并集为一库（现状单库模型不变）——确认没有"多库切换"诉求？
3. 批 B 是否引入 rayon（接受新依赖）还是手写小线程池？
4. 批 C 首选路线 B（CI 预构建 venv）是否接受安装包 +50-80 MB 体积？
5. 批 D 的 beets 查询入口形态：曲库筛选器加"beets 查询"高级输入，还是单独的查询页？
6. web 外链（§9a）要不要顺手做？

## 12. 拍板记录（2026-10-26，§11 六问全落定）

1. 新页面入口：**侧栏第七项「扫描入库」**。
2. 多目录：**并集单库**（无多库切换诉求）。
3. 批 B 解析池：**引入 rayon**。
4. 批 C 路线 B（CI 预构建 venv）：**接受安装包 +50-80 MB**。
5. 批 D beets 高级查询：**不纳入计划**（沿用当前数据库模型）；应用内只执行规划好的
   扫描指令（import/update/duplicates 等按需加）。
6. web 外链：**做**——工具页放一个"在浏览器中打开 beets Web"按钮即止，应用内不设计
   GUI / 命令行转换。

### 路径漂移问题（用户追问：beets 整理目录后应用内文件路径怎么办）

结论：**读模型已内建跟随机制，无需新设计**，批 D 只需把触发时机接对：

- `tracks.beets_item_id` 存 beets item id；`sync_items` 对账是**双键**的：
  path 命中优先；未命中再看 beets_item_id——命中后者即为"文件被移走"，
  `relocate_track` 把该行 `path/path_lower` 更新为新路径，**行 id 保持不变**
  （`beets_sync.rs::sync_items` 的注释即为此场景而写）。
- 行 id 不变 ⇒ 播放计数/历史/歌单项/队列状态全部稳定；引擎下次开播读到的已是新路径。
- 扫描器 upsert 冲突键是 `path_lower`（`scanner.rs`），beets 整理后再扫描
  不会把同一文件裂成两行。
- 边缘已在机制内：目标路径恰好被扫描器先建行占用 → 放弃跟随、继续更新字段
  并 warn，不让单行失败影响整次对账。
- **唯一要做的接线**：refile/move 属应用内触发的规划指令（拍板 5），
  指令完成 → sidecar `dump_items` → `sync_items` 即时对账，不等用户下次扫描。
  使用方约定：beets 整理与库扫描不同时跑（对账幂等，乱序最坏结果是
  "跟随失败 warn + 下次对账收敛"）。

## 附：调研来源

- beets 官方文档：plugins/index、web、replaygain（v2.14.0）、acousticbrainz（deprecated 声明）、
  API（Library/Transaction）；DeepWiki beets Database System（schema 四表 + regexp/bytelower）。
- GUI 壳：beetbox/beets#6209（Beetle 讨论，2025-12 活跃）、ProfPrice/beets-flask（v1.0.0 RC1，
  2025-12 活跃）、Vansmak/beetiful（2025-12 活跃）。
- 插件：pypi beets-xtractor 0.4.2（2024-05；Essentia beta5 + SVM models）、
  adrian-fuertes/vibenet、ArchWiki Beets。
- Python 打包：CPython 官方 Using on Windows（embeddable 限制）、PortablePythonMaker、
  venvdrop（离线 wheelhouse 模式参考）。
