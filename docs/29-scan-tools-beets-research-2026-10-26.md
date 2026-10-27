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

## 13. 批 A 落地记录（2026-10-26）

### Rust 侧

- **scanner 多根化**：`scan_roots_ext(conn, roots: &[PathBuf], …)` 为新主入口，
  `scan_directory_ext` 单根版委托（兼容既有调用与测试）。逐根 walk 后**并集**
  进同一批解析/入库；`mark_missing` 改收根切片，OR 拼接各根前缀（前缀小写 +
  分隔符收尾 + LIKE 转义规则不变）。**根列表为空时不做对账**（配置为空是
  "用户还没填"，把整库标 missing 是灾难）。
- **为什么不能逐根调单根版**：missing 对账以"本轮 walk 到的路径"为准，逐根
  调用时第 1 根的对账会把第 2 根下的曲目全部误标 missing。新单测
  `multi_roots_union_scan_and_missing_scope` 专门锁这个语义（根内 missing /
  他根 online / 根外 online 三方断言）。
- **lib.rs**：`library_scan` 根来源优先级 = `options.roots`（数组）>
  settings `library.roots`（数组）> 旧 `library.root`（单串）> 系统 Music。
  逐根验证 `is_dir`：无效根**跳过并 warn**，全部无效才报 `invalid_arg`
  （信息带上每个无效根，方便对照 textarea 修）。
- **`library_choose_root` 追加语义**：选完目录追加进 `library.roots`
  （大小写不敏感去重），返回 `ChosenRoot { path, roots }`（UI 直接回填，
  省一次 roots_get 往返）。
- **新命令** `library_roots_get` / `library_roots_set`：读/覆盖写根列表。
  set 侧服务端再归一化（trim / 去空行 / 去引号 / 大小写不敏感去重）——
  客户端可能绕过校验。
- **`beets_web_open`**（拍板 6）：复用 probe 找到的解释器起 `python -m beets web`
  （分离进程，不进会话通道），1.5s 后 `explorer <url>` 拉系统浏览器开
  `http://localhost:8337`。beets 环境报错属用户侧排障（页面有状态块对照）。

### UI 侧

- **路由**：`AppPage` + `'tools'`（12 值），schema 枚举收编；侧栏第七项
  「扫描入库」（`DatabaseZap` 图标），nav/history 的诊断刷新触发点扩到 tools。
- **`ScanToolsView`**（新）：「音乐库与任务」（LibraryTools）+「诊断与日志」
  （整段自 SettingsPanel 迁入，diag 样式走全局 overlays.css 不复制）。
  数据源同设置页（`ui.diagnostics` / `ui.beets`），快捷键冲突块改只读展示。
- **`LibraryTools`**：新增**多目录 textarea**——草稿态绑定本地文本，失焦
  有效变更才发 `roots_set`；保存成功后草稿回填为后端归一化结果（textarea
  与实际生效值一致）；beets 状态块新增「在浏览器中打开 beets Web」按钮
  （仅 `beets.found` 时显示）。
- **SettingsPanel 收窄**：撤「音乐库与任务」「诊断」两分区（props 只剩
  settings/onsave），左栏六组 → 四组；快捷键区保留录制时的即时冲突提示。
- **mock**：`library_roots_get/set`（默认根 `D:/MUSIC_Player/test-music`），
  `beets_web_open` 抛 unsupported（预览不真拉浏览器）。

### 验证

- `cargo test -p aiya-data`：**98 passed / 0 failed**（新增多根并集单测）；
  `cargo check -p aiya` ✓。
- `svelte-check`：**0 errors / 0 warnings**；`vite build` ✓ 5.58s。
- Playwright e2e：**23 passed / 22.2s**——route.spec 新增「扫描入库页」用例
  （textarea 归一化回填断言 + 设置页分区撤离断言），「一级路由逐个可达」
  加第七项；smoke.spec 设置页 rail 6 组 → 4 组。
- 首跑抓到一个真 bug：textarea 保存后草稿未回填归一化结果（e2e 先于手测
  拦住）。
- **遗留**：`gen:types:check` 因应用运行中锁构建目录未能跑（ChosenRoot/
  LibraryRoots 两份手写契约与 ts-rs 输出格式一致），应用关闭后补跑。

## 14. 批 B 落地记录（2026-10-27）

基线：`main 46220f8`（清账后 `npm run verify` 全绿）。实测曲库：
`D:\CkMsr\Music`，**1846 首 / 17 GB**（flac + mp3，1393 张嵌入封面）。

### 14.1 先量基线，再动手

§4 写着"预期收益 4 核 ≈ 3-4 倍"，但**没有可比对象**。补了
`crates/aiya-data/examples/scan_bench.rs`（`cargo run --release -p aiya-data
--example scan_bench -- --root <dir> --db <file> --runs N [--artwork <dir>]`），
同一 DB 连跑 3 轮：第 1 轮是全量入库，第 2/3 轮是增量重扫，正好两种场景一次量完。

串行基线（改造前，同一台机器同一目录）：

| 场景 | run1 全量 | run2/3 重扫 |
|---|---|---|
| 无封面 | 8815 ms | 4992 / 4851 ms |
| 带封面 | 18118 ms | 4473 / 4918 ms |

**量基线时发现的真问题**：重扫明明 100% `unchanged`，却几乎没比全量快 ——
因为 `unchanged` 判定写在 `upsert` 里，等轮到它时 `parse_file` 早就把整份标签
和嵌入封面读完了。模块头注释写着"unchanged 判定：mtime+size 都没变就跳过解析"，
**实际跳过的是"重新写库"**。这条比"并行"本身收益还大，所以批 B 把它一起做了。

### 14.2 三个前置缺口（审查时挖出来的）

1. **`save_artwork` 必须拆**：它是 `hash + 原子落盘 + upsert 行` 一体的，
   而解析线程没有 `conn`。拆成 `artwork::stage(dir, data, mime)`（无 conn，
   hash + 落盘，返回 `StagedArtwork`）+ `artwork::link(conn, staged, now)`
   （登记一行）。`save_artwork` 保留为两者的组合（测试与 beets 导入仍走它）。
2. **原子写 tmp 名并行会撞**：旧名 `.{hash8}.{pid}.tmp` 在串行时无害，
   并行下"同专辑多首共用一张封面"是常态 → 两个 worker 同写一个 tmp，
   rename 拿到半张图。加进程内序号（`STAGE_SEQ`）后唯一，跨进程仍靠 pid 区分。
   新单测 `concurrent_stage_same_bytes_does_not_corrupt_target`（8 线程同字节）
   断言目标文件完整 + 无 tmp 残留。
3. **channel 必须有界**：`sync_channel(CHANNEL_CAP = 256)`。无界会把
   "解析比入库快"直接翻译成"整库结果堆内存"。有界才有背压 —— 入库慢了
   解析自然跟着慢。

### 14.3 流水线形态

```
walk（主线程）→ 全量路径 + seen
      ↓
load_index：一次 SELECT path_lower/mtime/size/availability 全表 → HashMap
      ↓
rayon 局部池（N = 逻辑核-1）每文件：stat → 预筛 →（未命中才）读标签 + 封面 stage
      ↓ sync_channel(256)   ← 有界 = 背压
入库线程（= 调用线程，独占 conn）：攒 BATCH_SIZE → 一个 IMMEDIATE 事务提交
```

- **为什么用局部池**（`ThreadPoolBuilder` 而不是 rayon 全局池）：扫描是偶发批任务，
  不该长期占着全局池；而且要精确控制 N。留一个核给入库线程 —— 它是 SQLite
  单写者，多一个解析 worker 只会在 channel 上堵着。
- **为什么预筛放进 worker**：`stat` 也是 IO（Windows 上还要过 Defender 一道），
  摊到池里比串行 stat 1846 次快得多。`metadata` 只 stat 一次，同一个 `Metadata`
  传给 `parse_file_with_meta`，不再重复 stat。
- **连接不跨线程**：`process_file` 全程不碰 DB，`index` 是只读快照。
  "解析线程拿不到连接"这件事由函数签名保证，不是靠自觉。

### 14.4 两段式进度

`ScanProgress` 新增 `parsed` 字段（解析完成数，由各 worker 用 `AtomicUsize`
递增，入库线程发事件时读）：

- 进度条按 `scanned`（已入库/已记账）走 —— 与旧的观感一致；
- 详情行显示「`scanned`/`total` 已入库 · 解析 `parsed`」，`parsed > scanned`
  时额外显示「（排队 N）」；
- phase 追加 `importing`：`parsed >= total && scanned < total` 时如实说"正在入库"，
  否则用户在"解析快、入库慢"那段看到进度条不动会以为卡死；
- **删掉了 `artwork` phase**：封面落盘已并进解析 worker，不会再单独出现
  （mock 同步删，不让"只在 mock 里存在"的状态多一个）。

实测事件序列（带封面全量，1846 首）：

```
parsing scanned=600  parsed=600
parsing scanned=800  parsed=803     ← 解析略超前，流水线在动
parsing scanned=1400 parsed=1415
parsing scanned=1800 parsed=1800
importing scanned=1800 parsed=1846  ← 解析全完、最后一批没写库
done    scanned=1846 parsed=1846
```

### 14.5 实测收益（同一台机器、同一目录、同一 DB 冷启动）

| 场景 | 改造前 | 改造后 | 倍数 |
|---|---|---|---|
| 全量·无封面 | 8815 ms | **953 / 1012 / 1048 ms** | **≈9×** |
| 全量·带封面 | 18118 ms | **5123 ms** | **≈3.5×** |
| 重扫·无封面 | 4992 ms | **64 / 60 ms** | **≈80×** |
| 重扫·带封面 | 4918 ms | **75 / 68 ms** | **≈68×** |

带封面的全量 3.5 倍落在 §4 预估的 3-4 倍区间；无封面 9 倍是纯解析并行+
少了重复 stat。重扫两个数量级来自预筛（本来只需 stat，不该读标签）。

**正确性对照**（不能只看快）：并行版封面缓存 `1393 张 / 1.4 GB`，与串行版
**完全一致**；`imported=1846 / failed=0`，两版一致；`artwork/originals` 下
tmp 残留 `0` 个。

### 14.6 过程记录：一个真死锁

首跑测试卡在 `case_variant_paths_do_not_duplicate_rows`（一个只扫描 1 个文件的
用例）。原因是我们自己写的：生产者闭包写成**非 `move`**，`tx` 只是被借用、
活到函数末尾，于是消费者 `rx.recv()` 在生产者早已跑完之后还在等一个永远不会
到来的 `Disconnected`。改成 `move` 闭包（提取 `&pool`/`&files`/`&index`/`&cancel`
四个引用进闭包）让 `tx` 跟着闭包 drop 就好。

值得记的是"1 个文件的用例卡死"这个形态：如果是卡在 1846 个文件的基准上，
很容易被误判成"慢"。**测试套件里的最小用例才是死锁最好的探测器。**

同一轮还修了两处自己的毛病：批提交后又按步长判断重复报了一次完全相同的事件
（`next_report` 没跟着批边界前推）；以及 `importing` 在正常时序里几乎不露面
（尾部批提交前补一次如实上报）。

### 14.7 验证

- `cargo test --workspace`：**291 passed / 0 failed**（1 ignored = perf_50k），
  含新单测 `concurrent_stage_same_bytes_does_not_corrupt_target`。
- `svelte-check`：**0 errors / 0 warnings**。
- Playwright e2e：23 passed。
- 真实曲库基准：见 14.5（工具入库为 `examples/scan_bench.rs`，可随时复现）。

### 14.7.1 并行带来的一个 flaky（已修，教训留下）

`scan_cancelled_mid_batches_keeps_committed_prefix` 在批 C 的验证里红了一次，
再跑又绿 —— 典型的竞态。根因不在产品代码：

它靠进度回调置位取消，条件是 `p.phase == "parsing" && p.scanned > 0`。并行化
之后解析常常**先于入库跑完**，于是第一批提交时 `parsed` 已经等于 `total`，
这一条事件的 phase 是 `importing` 而不是 `parsing` —— 取消永远不置位，扫描
正常跑完，断言以"取消没生效"的形式失败。

修法：判定条件只留 `p.scanned > 0`（**不要用 phase 名当钩子**，它是内部状态
的派生，会随实现变），并连跑 5 次确认稳定。

顺带记一条同源教训：`14.6` 的死锁也是这套测试抓到的。**最小用例 + 可重复跑**
比"跑一次通过"值钱得多 —— 这两次失败如果只跑一次都是绿的。

### 14.8 遗留

- **取消延迟**：取消后要等当前在解析的几个文件跑完（每 worker 一个文件），
  实测 1846 首的曲库在毫秒级；万首 + 大无损文件时最长约一次解析耗时。
- **`updated` 字段仍是 0**：语义上"更新了多少首"与 `imported` 混在一起，
  批 B 没动（改它要连 `ScanResult` 契约与 UI 一起改，不在本批范围）。
- **rayon 可能不是瓶颈**：Windows Defender 实时扫描在并行后参与 IO 争抢，
  实测仍有 3.5-9 倍说明不是主要瓶颈，但换机器（尤其无排除项的机器）需要复量。

## 15. 批 C 落地记录（2026-10-27）

> **⚠ 本节的分发决策已作废（2026-10-27 晚）**：随包 embeddable 运行时
> （`resources/beets-runtime`、`tools/pack-beets-runtime.mjs`）不再采用，
> 改为 beets 作为独立工具分发、播放器只留 client。见
> `docs/33-beets-split-architecture-2026-10-27.md`。
>
> 本节仍然有效的部分：①「venv 预构建不可重定位」的实测结论（`pyvenv.cfg`
> 的 `home` 是硬指针）——它否定的是"预构建后打包"，不否定"目标机器现场建 venv"；
> ②embeddable 与 `_pth` 对 `sys.path` 的影响；③首发插件清单的三处踩坑。
> 作废的只有"把运行时打进安装包"这一个决定。

### 15.1 先证伪：§5 的路线 B 不成立

§11 第 4 问拍板"接受安装包 +50-80MB，走路线 B（CI 预构建 venv，整个目录打进
安装包）"。**这个前提是错的**，动手前先实测：

```text
tools/beets-venv/pyvenv.cfg:
  home = C:\Python314
  executable = C:\Python314\python.exe
  include-system-site-packages = false

把整个 venv 复制到别处、把 home 改成不存在的路径：
  python.exe -c "print(1)"
  → did not find executable at 'C:\NoSuchPython314\python.exe'
```

结论：**Windows 的 venv 只是一层指针**，标准库和 `python314.dll` 都在建库时的
那个系统 Python 里（`sys.base_prefix = C:\Python314`）。目标机器没装同版本
Python，venv 就启动不了。把它打进安装包只是把 89MB 一起发出去，用户机器上
照样跑不起来。

（顺带记一条：`sys.executable` 报告的是 venv 里的路径，`sys.base_prefix` 才
暴露真相 —— 只看前者会以为 venv 自包含。）

### 15.2 改走"embeddable + site-packages"

真正能随包分发的形态是 **embeddable Python 发行版**：自带解释器、标准库
（`python314.zip`）和 DLL，靠解压目录里的 `python314._pth` 决定 `sys.path`。

实测（本机，先手工拼一遍验证可行）：

```text
1. 下载 python-3.14.7-embed-amd64.zip（12.1MB）→ 解压
2. 把 beets 的全部依赖装进 <runtime>/Lib/site-packages
3. python314._pth 写：
      python314.zip
      .
      Lib\site-packages

      import site
4. <runtime>/python.exe -c "import beets, numpy"
   → beets 2.14.1 | numpy 2.5.4
5. echo '{"id":1,"cmd":"status"}' | <runtime>/python.exe sidecar/beets_sidecar.py
   → {"ok":true,"data":{"beetsVersion":"2.14.1","pythonVersion":"3.14.7",
      "plugins":["replaygain","fromfilename","duplicates","scrub","smartplaylist"]}}
```

两个必须踩过的坑，都写进脚本注释了：

1. **`_pth` 的文件名只带主次版本**（`python314._pth`）。写成 `python3147._pth`
   解释器根本不读，症状是"pip 装成功了但 import 不到"。
2. **`import site` 必须取消注释**。默认 `_pth` 把 `site.main()` 关掉，关了之后
   连 `Lib\site-packages` 都不会被扫 —— `sys.path` 里明明有目录却 import 失败。
3. pip 装出来的 wheel 是按**跑 pip 的那个解释器**的 ABI 选的（`cp314`）。用系统
   Python 的 pip 往 embeddable 的 site-packages 里装，版本一不一致就装进
   `cp312` 的 `.pyd`，运行期炸。所以构建脚本先 `get-pip.py` 给 embeddable
   装上自己的 pip，再让它自己装依赖。

### 15.3 探测顺序（beets.rs）

```
AIYA_BEETS_PYTHON           排障/开发时显式指定
  → {app_data}/beets-venv   用户点"安装运行时"装的（install.py 建 venv）
  → 随包 beets-runtime      embeddable + site-packages（构建期产物，绝大多数用户）
  → tools/beets-venv        开发机
  → python                  PATH 兜底（用户自己装过 beets）
```

**BEETSDIR 恒定指向 `{app_data}/beets`**（§5 拍板）。不再"只有设了
`AIYA_BEETS_DIR` 才隔离"，因为默认落点是 `%APPDATA%\beets` —— 那是用户系统
beets 的库，本应用去写它属于越界。`AIYA_BEETS_DIR` 仍可覆盖（dev 冒烟）。

顺带修了一个真 bug：`beets_web_open` 起 `python -m beets web` 时**没带**
sidecar 那套 env，于是 web 界面读的是 `%APPDATA%\beets`、sidecar 写的是
`{app_data}/beets` —— 用户在浏览器里看到一份空库，只会以为"同步坏了"。
现在两处共用 `sidecar_env()`。

### 15.4 首次启动生成 config.yaml

BEETSDIR 换到 app_data 之后那里是空的，beets 用内置默认 ⇒ **plugins 一个都不开**，
§8 的首发档能力全部落空。所以首次探测时写一份：

```yaml
# AIYA Music 首次启动生成；改过之后应用不会再覆盖
library: '<app_data>\beets\library.db'
directory: 'D:\CkMsr\Music'        # 取第一个扫描根；读不到就省略这一行
plugins: replaygain lyrics fromfilename duplicates scrub smartplaylist
```

- `directory` **读不到扫描根时省略**，让 beets 用它自己的默认。猜一个目录的
  后果是把用户的文件 move 到他没预期的地方，比不写糟得多。
- 路径用**单引号**：YAML 双引号里 `\M` 会被当转义序列，是这类配置最经典的坑。
- 已存在就**不动**（用户手改过不能被每次启动覆盖）。
- 首发档插件全是 beets 内置，不增加 pip 依赖、不影响离线可用性。

### 15.5 运行时安装脚本（sidecar/beets_install.py）

覆盖"随包运行时缺失/损坏，但机器有 Python"这条路：用系统 Python 建 venv 到
`{app_data}/beets-venv`，装完 `import beets` 自检一次（只看 pip 退出码不够，
装到一半的依赖也会返回 0）。

- `--from-wheels <dir>`：离线 wheelhouse，同时加 `--no-index` ——
  给了 wheelhouse 就**绝不**回落 PyPI，否则离线场景会卡在网络超时而不是
  干净地报"缺 wheel"。
- 输出**行帧 JSON**（与 sidecar 同风格），宿主逐行转发成 `beets:install` 事件；
  装完由宿主再 `probe` 一次推 `{"type":"finished","status"}` ——
  能不能用以探测结果为准，不以退出码为准。

宿主侧命令 `beets_install`：立刻返回 `JobStarted`，进度走事件。没有系统 Python
时**同步报错**（`invalidArg`），错误文案说明"随包运行时应随安装包分发"。

### 15.6 打包集成

- `tools/pack-beets-runtime.mjs`：下载 embeddable → 装 pip → 按
  `tools/beets-requirements.lock` 装依赖 → 自检 `import beets, numpy`。
  锁文件 sha256 存成 `.lock-sha256`，没变就跳过 pip（`npm run bundle` 每次
  都带上这一步，每次都连 PyPI 核对是不可接受的）。
- `npm run bundle` 改为先跑它再 `tauri build`：发布包**必须**带运行时。
- `tauri.conf.json` 的 `bundle.resources` 加 `"../tools/beets-runtime": "beets-runtime"`。
- `.gitignore` 排除 `tools/beets-runtime/` 与 `tools/.cache/`（约 100MB，可重建）。

### 15.7 验证

- `tools/beets-runtime` 构建产物 **103.7 MB**，自检 `beets 2.14.1 / numpy 2.5.4`。
- 新单测 `probe_finds_bundled_runtime_beets`：随包运行时能被 probe 命中
  （真 spawn sidecar + 收 status，不是查文件存在）。
- 新单测 `render_config_covers_plugins_and_paths`：插件、单引号路径、
  "无扫描根不写 directory" 三条断言。
- `beets_install` 实测：`python sidecar/beets_install.py --target <tmp>` 建 venv +
  装依赖 + 自检通过。
- `cargo test -p aiya --test ipc_contract` 5 passed（新命令双向覆盖）；
  `svelte-check` 0/0。

### 15.8 遗留

- **体积超出预估**：实际 103.7 MB（§5 估 40-80MB）。差在 numpy（beets 的
  `replaygain` 依赖它）+ pip 自身。可考虑发行时裁剪 `pip` 与 `*.dist-info`
  （约 10MB），但那样 `pip` 就不能用于现场修复，留到 P5 权衡。
- **没有 CI 预构建**：脚本本身是 CI-ready 的（不依赖系统 Python，自动下载
  embeddable + get-pip），但仓库还没有 CI（docs/30 P1-6）。
- **离线 wheelhouse 没造**：`--from-wheels` 分支实现完整，但仓库里没有
  wheelhouse 产物（`pip download` 需要一次联网准备），P5 若要做离线段再补。

## 16. 批 D 落地记录（2026-10-27）

### 16.1 开工前的事实核查（推翻了两个想当然）

§10 第 4 条写的批 D 是"sidecar 加 query_items / modify_items / refile_items /
find_duplicates / update_library；UI 高级查询、重复管理、元数据编辑入口"。
动手前先核了三条事实，范围因此变了两次：

**事实 1：beets 库是空的。** `%APPDATA%\dev.aiya.music\beets` 目录都不存在，
dev 库 `tools/beets-home/library.db` 只有 3 条测试残留。原因是整条链路**只有
`apply_import` 会写 beets 库**——用户用自有扫描器入库的 1846 首从没进过 beets。
所以 update / duplicates / move 三条命令在真机上首跑全是"无对象"。

> 结论：批 D 必须补一条 §10 没写的命令——**adopt（建库基线）**，
> 也就是 beets 的 as-is 入库：读标签入库，不联网、不改标签、不移动文件。
> 没有它，后面三条是空转的。

**事实 2：beets 2.14 的 `items.path` 存的是相对路径。** 直接查 DB：

```
sqlite> SELECT id, path FROM items;
1|Non-Album/冒烟艺人/曲目A.wav
```

绝对路径靠 `Library.directory` 现场拼。推论很硬：**move 的目标根和 dump 的
解析根必须同一个**，否则 `beets_sync` 会拿错位路径去对账（整库误判
orphan/imported）。

**事实 3：`config['directory']` 恒有值。** config.yaml 不存在的库也返回
`~/Music`（来自 `config_default.yaml`），无法区分"用户配过"与"用的默认"。
拿它去 move 等于把用户曲库搬进他真实的音乐文件夹。

> 结论（事实 2+3 的合解）：**move 的库根只认 config.yaml 里显式写的
> `directory`，调用方传的值只作校验**。不一致 → 拒绝执行；没配置 → 拒绝执行。
> 判断"显式配置过"只能读 config.yaml 原文（`_configured_directory`），
> 问 beets 问不出来。

**事实 4：duplicates 默认 keys 是 `mb_trackid/mb_albumid`。** as-is 入库的曲目
这两个字段全是 NULL，用默认 key 查重必然零命中——看上去像"查重坏了"，实际是
条件不适用。所以 keys 做成参数，默认给 `title + artist`。

**范围结论（相对 §10 收窄）**：做 `adopt_library` / `update_library` /
`find_duplicates` / `move_items` 四条；`query_items` 按拍板 5 排除（不是
"规划好的扫描指令"）；`modify_items` 一并后置——它对应"元数据修正"需求，
但手改标签的 UI 是另一个交互尺度（字段编辑器 + 校验 + 撤销），塞进这批会
让"命令面"和"编辑器"两个东西互相拖。

### 16.2 sidecar：四条命令的实现取舍

全部走**进程内 beets API**，不 spawn `python -m beets <cmd>`：CLI 的输出是给
人看的文本，转结构化事件既脆弱又拿不到逐曲粒度；而 `Library`/`Item` 这套对象
本来就够用（`beet update` 与 `beet move` 的核心也是同一批调用）。代价是要自己
复刻命令的判定逻辑——每处复刻在注释里标了 beets 源码位置，升级版本照着比对。

| 命令 | 照抄的 beets 语义 | 复刻要点 |
|---|---|---|
| `adopt_library` | `import -A`（as-is） | `Item.from_path` + `lib.add`，已在库跳过；一个大事务（1846 条逐条 fsync 太慢），取消是 `return` 非回滚（幂等，重跑即可） |
| `update_library` | `update.py::update_items` | ①文件没了→删库行 ②**mtime 没变→跳过不重读**（这是 update 快的全部原因）③逐字段 diff 有变化才 `store`，没变化只落新 mtime |
| `find_duplicates` | `duplicates.py::_group_by` | 非 strict 分支：**全空才跳过**，部分空仍参与分组；分组 key 用小写归一值，**展示用首条原值**（否则 UI 上看到"曲目a"，用户对着库里的"曲目A"找不到） |
| `move_items` | `move.py` + `Item.move` | 目标==源→same（`normcase` 比，Windows 盘符/分隔符大小写不同）；目标已存在→conflict 不改名不覆盖（beets 的 `move_file` 会静默改成 `name.1.ext`） |

三条细节：

- **`_item_brief` 的 `size` 要现读磁盘**：`size` 不是 Item 字段（`getattr` 为
  None），读不到给 0（文件已不在时照样要能显示这一行）。
- **上限三件套**：错误 50 条（`MAINT_MAX_ERRORS`）、明细 500 条
  （`MAINT_MAX_DETAIL`）、进度步长 20 文件（`MAINT_PROGRESS_STEP`）。几千条
  明细会让一次会话的 stdout 涨到几十 MB，而用户根本看不过来。
- **协议版本 2 → 3**：加了四条命令，`status` 报的 `protocolVersion` 要跟上。
  `status` 同时新增 `libraryDirectory`（= `_configured_directory()`），
  UI 拿它显示"库根"并在 None 时禁用整理文件。

### 16.3 宿主：一条入口 + 一个事件通道

四条命令的公共部分（占会话槽 → 起 sidecar → 转发事件 → 按需对账）合成
`start_maintenance`。**`spawn_reader` 加了 `event` 参数**：导入链路仍走
`beets:import`（ImportPanel 在听），维护走 `beets:maintenance`——混在一个通道
里会让"导入进度条"突然收到"整理文件"的事件。

对账（`run_sync` → `librarySynced`）只有 adopt 与 move 给：

- **adopt 后必须对账**：`tracks.beets_item_id` 要靠对账才填上，而后面 move 的
  路径跟随恰恰依赖这个双键（§12 的结论）。不做这一步，move 之后 dump 出来的
  新路径在自有库里 path 不匹配、item_id 为空 → 判定 orphan 而不是 relocate。
- **move 后必须对账**：路径漂移要立刻跟随，不等用户下次扫描。
- update 改的是标签（路径不变）、duplicates 纯只读，都不自动对账。

`beets_adopt` 的 `paths` 参数：**UI 永远传空数组**，空 = 宿主取自有库全部
在线路径（`beets_sync::online_paths`）。让前端把 1846 条路径经 IPC 传一遍纯属
浪费，而且"哪些行可播"是 DB 的事实，不是 UI 的。

新增 `beets_reveal(path)`：查重结果的「定位」。不复用
`library_reveal_track`——那条按 track id 查自有库拿路径，而查重结果来自
beets 库，可能有自有库没有的条目（用户在 beets 侧导入过、还没对账的）。

### 16.4 顺手修掉一个批 A 的 bug（批次外，但挡路）

**从侧栏进「扫描入库」页，诊断区永远"正在读取…"、beets 块永远"正在检测…"。**
根因：`ui.openPage` 只在 `page === 'settings'` 时触发 `refreshDiagnostics`，
而 tools 页的诊断/beets 数据靠 `nav.apply`（返回栈还原路径）才有——侧栏
点击走的是 `openPage`。批 A 的 e2e 只断言了目录 textarea 与设置页分区，
没看这两块内容，所以没拦住。

批 D 的「曲库维护」直接依赖 beets 状态（是否可用、库根是什么），这个洞必须
一起补：`openPage` 与 bootstrap 的恢复路径都扩到 `settings | tools`。

### 16.5 验证

- `cargo test --workspace`：**295 passed / 0 failed**（1 ignored = perf_50k）。
  新增 `online_paths_skips_missing_and_orders_by_path`（只取 online + 按
  path_lower 排序）；`dto_wire` 补 `libraryDirectory` 的 wire 断言。
- `tools/smoke_maint.py`（新，纳入 `npm run verify`）：隔离 BEETSDIR + 独立库
  目录，14 个断言全过——adopt 幂等、update 的 mtime 预筛与 pretend 不写库、
  查重分组与空 key 跳过、move 的"库根不一致拒绝/未配置拒绝/预览不动文件/
  执行后源目录搬空/再跑全 same/冲突不覆盖"。
- `svelte-check` 0/0；`gen-types` 52 类型一致；依赖图 108 模块 / 375 依赖无环。
- Playwright **24 passed / 23.3s**（新增一条：四个按钮的事件流渲染——基线
  汇总、diff 预览→应用→预览按钮消失、查重分组+定位、整理清单→确认移动）。
  这条 e2e 正是抓 16.4 那个 bug 的地方（先红后绿）。

### 16.6 遗留

- **`modify_items`（手改标签）没做**：见 16.1 的范围结论，它需要字段编辑器
  那套交互，单独一批更合适。届时可以复用 `update_library` 的 diff 结构
  反向（把"目标值"写成用户输入）。
- **查重只有报告**：按拍板"只读报告 + 定位"，应用内不删文件。真要清理走
  `beet dup -d` 或资源管理器。若以后要加应用内删除，必须走回收站并且
  逐项确认（personal_files_safety 那套规矩）。
- **adopt 不认 beets 侧已有条目**：它以自有库为准单向登记。反向（beets 有、
  自有没有 → 让用户决定是否纳入曲库）目前还是 `sync_items` 的 `imported`
  计数，没做成 UI 流程。
- **move 的 path_formats 是 beets 默认**：`$albumartist/$album/$track $title`。
  用户改 config 才会变，应用内不做模板编辑（那是 beets 的地盘）。预览里
  能逐条看到"从哪到哪"，是这条命令的安全阀。
