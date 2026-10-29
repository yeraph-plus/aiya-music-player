# docs/40 — 动态歌单 / beets 整理识别 / 插件市场 调研

> 状态：**调研完成，待拍板**。日期：2026-10-28。
> 前置：docs/33（两侧分离，M1–M4 完成）、docs/31（集成测试计划）、协议 v1（schema 冻结）。
> 本文回答三个问题：① 能不能做"给歌单设标签、动态生成曲目"的动态歌单；
> ② beets 实跑整理（move/import）后应用还能不能正确识别，差什么；
> ③ beets 插件做成"插件市场"形态怎么落地。

---

## 0. 事实底座（本轮核查）

| 事实 | 位置 | 对结论的影响 |
|---|---|---|
| 协议 `DumpItem` **有 `genre` 字段**（required） | `protocol/v1.schema.json:688` | beets 侧标签数据本来就会过网线 |
| 播放器 client 的 `BeetsItem` **没有 genre**，dump 入库时丢弃 | `aiya-beets-client/src/proto.rs`；`aiya-data/src/beets_sync.rs:14` 注释明说"genre 暂不迁移" | 动态歌单要补的第一块数据，协议零改动 |
| `tracks` 表**没有 genre 列**；`beets_item_id` / `path_lower` 对账键已就位 | `migrations/0001_init.sql:33-65` | 加 genre 需要 0003 迁移 |
| `playlists` 表只有 name + 手动 items，无规则字段 | `0001_init.sql:87-106` | 智能歌单要加 kind + 规则列 |
| beets 整理后的对账链路**已实现**：`path_lower` 命中优先 → `beets_item_id` 兜底 → `relocate_track` 跟随移动，冲突（扫描器先建了新行）单独告警不中断 | `aiya-data/src/beets_sync.rs:321-380` | 主题②是**验证缺口**不是实现缺口 |
| 刷新触发器已覆盖：beets 命令终态强制刷新 / 磁盘变化 / 上次失败重试 / 定时 / 聚焦 | `src-tauri/src/refresh.rs:360-376` | 同上 |
| 插件三态健康检查已实现且**不 import 插件**（`find_spec` 零副作用）：ready / missing_deps / unavailable | `beets-tool/src/aiya_beets/plugins.py` | 插件市场的"健康检查"地基已打好 |
| config.yaml 的 `plugins:` 列表由 beets-tool 侧生成（`render_config`），播放器不知道 YAML 语法 | `beets-tool/src/aiya_beets/cfg.py:77-90` | P1 边界成立：启停插件必须走 beets-tool |
| beets 官方插件全部**随 beets 内置**（`beetsplug` 命名空间），第三方插件是**普通 pip 包**（装进同一 venv 后命名空间自动可见），beets 没有官方市场 API | beets 官方文档（dev/plugins，2026-10 核查） | "市场"只能是我们自维护的目录清单 + pip 安装，无现成商店可接 |
| `smartplaylist` 已在默认插件清单（六件套之一） | `plugins.py:45` | 反向导出（m3u 给别的设备）零新增依赖 |
| smartplaylist 能力（官方文档核查）：beets 查询语法（artist:/genre:/year:、排序、多查询并集）、**库变化后自动再生**（auto 默认 yes）、extm3u 可带 id/genre 字段、`beet splupdate` 手动再生 | beets.readthedocs.io smartplaylist | 见 §1 方案 B 评估 |

---

## 1. 动态歌单（标签歌单）

### 1.1 需求原话的拆解

"为歌单设置指定的标签 → beets 识别歌单后入库"。拆成两半：
**规则从哪来**（用户在播放器里选标签）和**曲目列表在哪算**（beets 还是播放器）。

### 1.2 方案 A（推荐）：播放器侧智能歌单，beets 只供数据

- **规则存播放器**：`playlists` 加 `kind`（manual/smart）与 `rule_json`
  （如 `{"all":[{"field":"genre","op":"eq","value":"摇滚"}],"sort":"added_desc","limit":null}`）。
  曲目列表**不物化**，打开歌单时对本地 SQLite 实时查询——推荐页的"最近添加/常听榜"
  已是同款动态查询先例，无新机制。
- **标签数据链路**（本方案的 beets 角色）：
  1. client `BeetsItem` 补 `genre` 字段（协议里本来就有，纯客户端补齐）；
  2. `tracks` 迁移 0003 加 `genre TEXT` + `genre_norm`（LIKE 检索用，同 title_norm 惯例）；
  3. `sync_items` 的 `differs_from`/`insert_track`/`update_track` 纳入 genre
     （注意 beets_sync.rs:606 既有约定：beets 侧变 None 不清已有值——genre 沿用）。
- **规则字段首期**：genre（beets 供）、艺人/专辑（本地）、年份（本地）、
  播放次数与最近播放（本地统计）。全部有列，不用 beets 在线。
- **没装 beets 时**：genre 为空的曲目不出现在按 genre 过滤的智能歌单里，
  其余规则照常工作——符合 docs/33 §2.2"beets 是增强不是依赖"。

**为什么不是 beets 算**：方案 B（下）要绕一圈
播放器规则 → 写 config.yaml → beets 导 m3u → 播放器解析 m3u → 按路径回导
（还要 path 对账）。四跳里两跳是文件 IO，两跳是配置写，每一跳都能静默错位；
而且 beets 没装时功能整个消失，直接违反边界原则。beets 的查询语法比 SQL 弱
（且解析 shell-like 引号），把规则源放 beets 没有任何收益。

### 1.3 方案 B（记录备查）：smartplaylist 导出 m3u 反向入库

保留为**后续增强**而非实现路径：若用户想把智能歌单导出给手机/mpd 等别的设备，
beets-tool 新增 `export_playlists` 命令（配置 smartplaylist 段 → `beet splupdate`
→ 读 m3u 回传路径列表），播放器端只做"导出到文件夹"。这是输出方向，不承担
播放器内的动态歌单职责。

### 1.4 需要拍板的点

- 规则 UI 的形态：首期建议"歌单编辑弹层：条件行（字段/操作符/值）+ 排序 + 上限"，
  与队列抽屉同款弹层风格。
- 智能歌单与手动歌单互不转换；"从智能歌单固定一份手动副本"（snapshot）可后补。

---

## 2. beets 实跑整理后的识别保障

**结论：实现已齐，缺的是闭环验证。** docs/31 的集成测试计划覆盖了这条链，
但"真 beets 进程 import → move → 应用自动识别"的端到端实跑至今没在真机跑过
（工作日志欠账里同为待验项）。

### 2.1 已就位的保障（代码层）

- 双键对账：path_lower 命中优先、beets_item_id 兜底，"同一首被移动"不会裂成两行
  （这正是 beets_move 后最容易翻车的形态，代码注释里写明了动机）；
- relocate_track 跟随移动；目标路径被扫描器先占时放弃跟随但继续更新字段；
- 触发时序：beets move 终态（applyDone/moveDone）→ 强制刷新 → dump → sync，
  不依赖磁盘 mtime（adopt 只登记不写文件，mtime 不变的坑已在 refresh.rs:100 注明）。

### 2.2 本批要补的验证（可执行清单）

1. **集成测试**（`tests/` 或 CI job，`PYTHONUTF8=1` 已有约定）：
   临时目录建 beets 库 → import 3 个文件 → 应用同步 → `move_items` 移动 1 首 →
   断言：曲目数不变、路径已跟随、播放计数/歌单引用仍在同一 track_id 上、
   其余两首不受影响。
2. **次序对抗**：先手动把文件挪了再让应用扫描（扫描器按新路径建行）→ beets 同步
   到达 → 断言走"目标被占"分支后仍收敛为一行（现有告警路径）。
3. **真机手动验收**：`test-music/` 三个 wav 实跑一轮，作为发布前验收项
   （与 4h soak、干净机装卸同单）。

### 2.3 风险备注

- `beets move` 若开了 `copy` 而非 `move`（config 由 beets-tool 管，默认 import
  行为是 copy），路径不漂移、两行并存风险为零——集成测试里把 move 模式显式固定。
- Windows 路径大小写：path_lower 已内联处理，测试加一条"仅大小写变化"的 move。

---

## 3. beets 插件市场

### 3.1 生态事实（决定形态）

- beets 没有官方市场/注册表。官方插件 100+ 全部**随 beets 内置**，装 beets 即有；
  第三方插件是普通 pip 包，`pip install` 进同一 venv 后 `beetsplug` 命名空间
  自动可见（Python namespace package 机制，无需注册）。
- **启用**与**安装**是两回事：装了不等于开着，`config.yaml` 的 `plugins:` 列表
  才是开关——而 config 归 beets-tool 管（docs/33 P1：播放器不知道 YAML 语法）。

### 3.2 形态：目录清单驱动的"市场"（三处小改动 + 一个 UI）

**目录（真源在 beets-tool）**：`plugins.py` 的 `DEFAULT_PLUGINS` 扩展为完整
`CATALOG`——每项 `{name, title, note, py_deps, exe_deps, source}`，`source`
三档：`builtin`（beets 自带，免安装只开关）/ `pypi:<包名>`（需 pip）/
`wheels`（离线 wheelhouse 预置）。首批目录建议 12–20 个实用项
（replaygain、lyrics、fetchart、embedart、convert、duplicates、scrub、
smartplaylist、fromfilename、edit、mbsync、chroma…），不做全量 100+。

**协议（纯加法，不破坏 v1）**：
- 新增 `plugin_catalog`（一次性应答：目录 + 每项三态 + 是否已启用）；
- 新增 `plugin_set_enabled {name, enabled}`（改 config plugins 列表 + 回写）；
- 新增 `plugin_install {name}`（走现有 InstallerEvent 帧流：pip install → 复检
  三态 → done）；`Session` 前缀枚举加 `plug_`（schema 注明新增前缀属于扩展）。
- 宿主对旧版工具发新命令会得 `E_UNKNOWN_CMD`：宿主按 `capabilities` 先判
  （capabilities 里加 `pluginsMarket: true` 标志位），UI 没拿到标志就隐藏市场入口。

**beets-tool 内部**：
- `install_plugin` = 复用 install.py 的 pip 管道（`_NO_WINDOW`、逐行透传、
  `--no-index --find-links` 离线档）装进现有 venv；
- `plugin_set_enabled` = 读 config.yaml（YAML 只在 beets-tool 侧解析）→
  改 `plugins:` → `render_config` 重写 → 返回新三态报告；
- `capability_report` 从"只看 DEFAULT_PLUGINS"改为按目录全量三态。

**播放器 UI**（扫描入库页新增"插件"分区，不新开页面）：
- 插件卡列表：名称、一句话说明、三态徽标、启用开关；
- `pypi:` 档位显示"未安装"，按钮走安装流（复用 beets 组件安装的进度 UI）；
- `missing_deps` 显示缺什么（py/exe 分开，exe 如 ffmpeg 给指引）；
- 没装 beets 工具时整个分区显示引导安装，不挡其他内容。

### 3.3 风险与限制（如实）

- **B 轨道（离线套件包）**：`build_bundle.py` 自写 `_pth` 固定 site-packages，
  装 PyPI 插件需要 wheelhouse 预置或放开 pip 联网。首版市场在 B 轨道下只保证
  `builtin` 档（官方内置插件开关），`pypi:` 档标注"需要联网 + A 轨道"。
- **插件质量**：第三方插件可能拖慢 serve 启动或写 stderr 杂音——stdout 只有
  JSON 的硬规则已有 reader 过滤兜底，但市场目录首版只收录经过本地实测的插件。
- **离线**：无网时 `pypi:` 档安装直接失败，UI 给 E 类错误码，不重试不假死。

---

## 4. 分期与协议影响汇总

| 期 | 内容 | 协议 | 涉及 |
|---|---|---|---|
| P1 | genre 数据链路（client 补字段 + tracks 0003 + sync 迁移） | 无（DumpItem.genre 已在 schema） | client / aiya-data / 迁移 |
| P2 | 智能歌单（playlists.kind + rule_json + 查询引擎 + 编辑 UI） | 无 | aiya-data / src-tauri / UI |
| P3 | beets 集成测试三件套（move 跟随 / 次序对抗 / 真机验收单） | 无 | tests / CI |
| P4 | 插件市场（目录 + 3 命令 + 市场开关标志 + UI 分区） | 加法：plugin_catalog / plugin_set_enabled / plugin_install / capabilities 标志 | beets-tool / client / src-tauri / UI |

P1+P2 是"动态歌单"的完整交付；P3 独立可先行；P4 依赖 P1 无、依赖前三者无，
可并行。**协议 v1 不升版**：全部为新增命令与新增 capabilities 字段，现有命令
一字节不动；`proto:check` 镜像同步照旧。

## 5. 待用户拍板

1. 方案 A（播放器侧智能歌单）是否认可？规则字段首期四类（genre/艺人专辑/年份/播放统计）够不够。
2. 智能歌单编辑 UI 形态（条件行弹层）是否接受。
3. 插件市场首版范围：仅 builtin 档还是含 pypi 档（含则标注联网限制）。
4. 首批目录插件清单（§3.2 的 12–20 个）是否有必收项。
5. P3 集成测试是否提前到 P1 之前跑（它不依赖任何新代码，随时可做）。

---

## 6. 拍板记录（2026-10-28，全部已落地）

对 §5 五个待拍板项的用户结论与实施结果：

1. **动态歌单**：方案 A（播放器内置）认可，**特征字段（能量/效价/舞曲性）有意不加**
   ——beets 侧没有现成特征数据（要靠 essentia 等重依赖离线分析，收益配不上体积）；
   `rule_json` 的 field 枚举是加法扩展点，未来真要加特征列，迁移加列 + smart.rs
   加枚举变体即可，协议不动。落地：迁移 0003（genre 列 + kind/rule_json）+
   `aiya-data/src/smart.rs` 编译器 + repo 的 `TrackSource::SmartPlaylist` 路由 +
   统一模态框规则编辑器。纵切测试 `vertical_smart_playlist_rule_query` 绿。
2. **编辑/创建同一模态框**：`playlistEdit` 单一 Dialog（创建时选手动/智能 +
   规则编辑器；智能歌单"重命名"入口变"编辑规则"）。picker 过滤智能歌单
   （智能歌单不接受手动加曲目——它是查询，不是容器）。
3. **插件市场 = 白名单 + 安装命令**：不做全量表。`MARKET_CATALOG` 12 项
   （replaygain/lyrics/fetchart/embedart/convert/scrub/fromfilename/duplicates/
   smartplaylist/edit/mbsync/chroma）；协议加法扩展 `plugin_catalog`（一次性
   应答）+ `plugin_install`（pluginProgress → pluginDone 事件流），v1 不升版，
   镜像已 `proto:sync`（Event 18 / Request 13）。安装目标恒为 serve 进程自己的
   解释器；pip → `uv pip` 按序探测；B 轨道（_pth 无 pip）明确报错指引。
4. **监听根可移除缺陷修复**：根因有二——曲目残留只标 missing 不清除 +
   删光后 `roots_from_settings` 回落旧键导致目录复活。修复 = `library.roots`
   键**存在即权威**（空数组也是权威）+ `rootsCount`/`rootsPrune` 命令
   （substr 前缀匹配，只删索引不动文件）+ UI 内联确认条（删除曲目并保存 /
   仅停止监听）。lib_tests 7 条、refresh 5 条、e2e 全绿。
5. **P3 集成测试**：未提前，维持原批次。

### 实测证据

- dev venv 真跑 `plugin_catalog`：12 项，三态与来源档如实上报
  （missing_deps 的 ffmpeg/bs4/acoustid 缺口逐项报出）。
- `cargo test -p aiya-beets-client` 16+5 绿（含 TERMINAL_EVENTS 镜像断言）；
  python 契约 `ALL PASS`（59 消息 11 事件离线全链路）。
- e2e 29/29（新增用例：市场展开懒加载 → 白名单三态 → 安装事件流翻绿）。

### 二轮修订（2026-10-28 晚，用户试用反馈）

1. 模态框创建描述行删除；类型切换从分段控件改为 **Switch「动态歌单」**（仅创建时出现——后端不支持手动↔智能转换，编辑态不出开关避免假可供性）。
2. 条件行改「模式 select + 动态值控件」两列：流派=曲库流派下拉（新命令 `library_genres`，distinct genre_norm + MIN(genre) 显示，store 缓存 60s + 扫描后失效）、艺人/专辑=关键词 input、年份/播放次数=数字 input（方向折进模式：不早于/不晚于、至少/至多）。`modeOfCond` 反推保证旧规则无损归位。
3. **排序键从编辑器移除**（含"入库时间"）：排序由歌单视图列头处理——repo build_head 的 smart 分支改为吃 `q.sort`（UI 默认 title/asc），规则里的 sort/direction 沦为兼容字段（保存恒写 title/asc）；smart 换 JOIN 后必须同步丢弃 Playlist 来源的 pi.position 位置序（否则 no such column）。智能歌单视图开列头排序（canSort），手动歌单保持拖拽排序（顺序是语义）。
4. 拖拽排序：歌单视图**本来就有**（TrackList onmove → startRowDrag → moveItem），本轮只是限制为仅手动歌单。
5. 上限独立一行。

**坑**：模态框打开 effect 若读 `editKind`（流派拉取写在里面），点开关 → effect 重跑 → 草稿被无条件重置回打开时快照，开关"点了没反应"。流派拉取必须独立 effect（读 editKind 是有意的，重跑无害）。

### 三轮修订（2026-10-28 晚，表格拆分 + 最近播放周期 + 筛选器字段条件）

1. **表格组件三分支**（解 TrackList 40-prop 开关耦合）：
   - `TrackTable`：纯榜单（首页常听榜，本就不动）；
   - `TrackList`：**管理列表**（曲库/搜索/艺人/专辑，拖拽代码整体移除）；
   - `PlaylistList`（新）：**拖放排序列表**（歌单视图）。
   接线仍只有 `TrackListPanel` 一份：面板 Props 用 `dragSort?` 选表——
   传了渲染 PlaylistList（手动歌单），不传渲染 TrackList；智能歌单是
   查询结果不可拖，判断留在 LibraryView。两文件行结构/CSS 类名刻意
   一致，头注释互相提醒同步改。
2. **最近播放保留周期收敛到设置**（防 history 无限膨胀）：
   - 设置项 `historyDays`（默认 90，0=永久），UI 在设置页「最近播放保留」；
   - 迁移 0004 只加 `idx_history_played_at` 索引（周期是设置不是 schema）；
   - `history::prune(cutoff)` 在**启动 / 每次写历史 / 设置变更**三处触发；
   - 裁剪只删 history 行，`play_count`/`last_played_at` 统计列是累计语义
     不动；「最近播放」tab 与常听榜聚合 history 行，自动遵循周期。
3. **筛选器去掉「上次播放」档**（与最近播放 tab 功能重叠），加**字段
   条件** `conds`：与智能歌单同一批键（genre/artist/album/year/playCount），
   UI 直接复用 COND_MODES 模式行；后端 `smart::conds_from_value` 解析
   （与 rule_json 共用 parse_conds，未知键/类型错配明确报错），落
   `TrackQueryInput.conds` → repo build_where，**只在本次查询生效不落库**。
   TrackRow DTO 顺路补 `genre`（筛选与未来标签列用）。
4. 配套：筛选到 0 行的空态文案与"空库"分开（"筛选后没有匹配的曲目" +
   一键清除）；空文本条件应用前 UI 拦下（后端也会拒绝，但不该把列表页
   打进错误态）。

**坑**：e2e 里 0 行时 TrackList 整个不渲染（工具条在组件内部），筛选角标
断言必须放在非空结果视图；Dialog 的定位器是 `getByRole('dialog')` 而非
臆造的 `.overlay-dialog`。

**验证**：aiya-data 111+2（conds/prune 新测）、aiya lib 94、dto_wire 20
（TrackRow.genre 键断言）、gen:types 58、svelte-check 0/0、e2e **32/32**
（新增筛选器条件链路 + 空值拦截两条）。KitView 的 Snippet 双类型误报
为 svelte-check 环境怪病（基线 10 错、工作区 5、复跑 0），与改动无关。

## 四轮修订（2026-10-28 晚，用户实测反馈）

1. **select 原生框体**（用户："筛选表单的 select 没有配套黑色外观"）：
   根因是 Chromium 里给 select 设 `background/color` **不会**去掉原生控件
   外观——边框、高亮、箭头仍是浏览器画的。修法是 base.css 加全局
   `select` 元素默认：`appearance: none` + tokens 配色 + 内联 SVG chevron
   （中性灰，深浅主题都可读；data-URI 用不了 CSS 变量）。ImportPanel 的
   两个原生 select 一并被治好；OverlayHost 的 `.rule-select` 只覆写
   padding/字号，流派下拉加 `select.rule-value` 让字别压箭头。
2. **筛选器不复刻歌单的条件行**（用户："不是完全复刻歌单的模式，而是
   用相同的可筛选字段，做成 tab 或者别的方式"）：动态「+ 添加条件」行
   是规则引擎的编辑形态，筛选是临时意图，改成**固定字段表单**——每字段
   一行（流派下拉 / 艺人 / 专辑 / 年份从–到 / 播放次数至少–至多），
   **留空 = 该字段不过滤**，不再需要"空值拦截"报错。应用时把非空字段
   组装成 conds（与智能歌单同键同解析），打开时用 `modeOfCond` 把已生效
   条件回填到字段（旧 UI 写出的条件都能落位）。后端协议零改动。
3. e2e 两条筛选用例随表单改写（字段回填断言替代"+添加条件"流程；空值
   拦截用例改为"全空应用 = 无条件生效"）。

**验证**：svelte-check 0/0、e2e **32/32**（先 build 再跑——直接
`npx playwright test` 会吃旧 dist，快照里还是旧模板，白查一轮）；Edge
截图目检筛选对话框与歌单规则编辑器，select 全部深色统一、箭头自绘。

**为什么筛选器不该复刻条件行**：歌单规则是持久语义（要表达任意组合、
可增删），筛选是当前列表的临时意图（最常见就是"看某个流派/某个年代"）
——固定表单零学习成本，留空即跳过；真要复杂组合，走智能歌单。

## 五轮修订（2026-10-28 晚二批，用户实测反馈）

1. **下拉弹层仍是原生亮色条**（用户："select 的下拉表还是浏览器原生"）：
   上一轮只治了触发按钮的外观，`<select>` 的**弹出选项列表根本不受 CSS
   管**。根治 = 新建 kit 零件 `ui/Select.svelte`（bits-ui `Select`）：
   触发按钮与 Field 输入框同族外观（--control-h/--radius-md/surface-input
   + focus 描边 + 旋转 chevron），选项列表 portal 到 body、沿用 Menu 的
   深色面板参数（surface-4 + 毛玻璃 + 10px 圆角），白送键盘导航/typeahead/
   碰撞翻转。e2e 定位锚点：trigger 是 `role=combobox`、选项 `role=option`。
2. **输入框别手搓**（用户："kit 应该有配套的外观"）：筛选器/规则编辑器
   的文本、数字、上限输入全部换成 `Field`（新增 `type:'date'`）；
   OverlayHost 里手写的 `.rule-value`/`.filter-value` 输入样式整套删除。
   `base.css` 的全局 select 默认保留（ImportPanel 两个原生 select 仍在）。
3. **入库时间档 → 日期区间**（用户："从 tab 固定值改成区间输入"）：
   `TrackTimeFilter.added: AddedPreset` 改为 `addedFrom/addedTo: string|null`
   （YYYY-MM-DD 本地时区；`localDayMs` 解析并防 2 月 30 日进位）；上界含
   当天，查询时 +1 天换算成次日 0 点配后端开区间语义。ADDED_PRESETS/
   presetRange 整套删除。后端协议零改动（始终是 addedFromMs/addedToMs）。
4. **播放次数从筛选器移除**（用户："我不需要播放次数相关的筛选"）：
   表单删行；conds 协议保留 playCount（智能歌单规则还用）；回填时旧会话
   遗留的 playsMin/Max 直接丢弃。

**验证**：svelte-check 0/0、e2e **32/32**（歌单智能创建用例改为 combobox/
option 交互；筛选用例改为流派下拉 + 年份区间制造 0 命中）；Edge 截图目检
筛选对话框（下拉展开态）与歌单规则编辑器，弹层、触发器、输入框全部统一
深色。

## 六轮修订（2026-10-28 晚三批，入库引擎检修）

**后端**
1. `library_roots_set` 报「缺少 roots 数组」：参数错位——客户端传的就是数组本身
   （Tauri 按参数名 `roots` 取值），后端却用 `Json` 再 `.get("roots")` 找包装对象，
   每次都落空。改成 `Vec<String>` 强类型（与 roots_count/roots_prune 同款），归一化照旧。
2. 新增 `library_clear` 清空曲库：`DELETE FROM tracks`，外键级联清歌单条目/队列/历史，
   歌单壳保留，磁盘文件不动。返回 `RemoveTracks`（复用现有 DTO，零协议新增）。

**界面（ScanToolsView + LibraryTools）**
3. Header 单标题（big 形态，去 kicker）；删掉两组 section 的大段描述文案。
4. 统计小卡片：图标座（accent 弱底）+ 21px 数字 + 标签；「库大小」改名「索引大小」
   （它是 SQLite 文件体积，不是音乐文件体积）；位置缺失卡走 warn 色。
5. 曲库目录 textarea → 条目列表：一行一目录（等宽字体 + 行内移除钮），移除走内联
   确认条（删曲目并停止监听 / 仅停止监听）。三动作（添加目录/立即索引/清空曲库）
   在卡片头部右侧；清空曲库二次确认内联。
6. 执行状态并入同一卡片（分隔线隔开）：进行中（进度/取消）或上次索引记账 + 自动刷新
   状态行。报错内置：scan store 从全局 toast 改 `lastError` 状态，目录操作报错走
   `rootsError`，都在卡片内渲染。

**七轮微调**：统计数字不加粗（font-weight 400，靠字号立住）；执行状态块并入卡后
e2e 锚点从 `.task-status` 改 `.status-block`。

**验证**：cargo check 通过、svelte-check 0/0、e2e **33/33**（新增"立即索引跑完→执行
状态留记录"用例；目录增删用例按条目列表重写）。

## 八轮修订（2026-10-28 晚四批，执行状态整理）

用户实测反馈：状态区右侧"遍历中"和"曲库索引"转圈是两行重复表达阶段；自动刷新
文本没用；取消按钮悬在半空（flex 列拉伸成居中）且点了没反应；曲库页还有另一套
进度条 + 取消。

1. **状态块重排**：删掉"执行状态"标题行——阶段合并进任务行一行表达
   （`⟳ 曲库索引 · 遍历中`），取消按钮就在**同一行右端**（不再悬空）；
   进度条、数字明细不变；结束态只剩一行记账。
2. **自动刷新行删除**：静默管线不占版面。设置页"曲库自动刷新"的提示语
   同步去掉"最近一次结果看扫描入库页"的指引。
3. **取消入口收敛为唯一**：曲库页 `.scan-bar` 精简成纯指示（进度 + 计数 +
   "到扫描入库页可取消"提示），删掉第二个取消按钮。
4. **取消真的没用（预览侧）**：mock 的 `library_scan_cancel` 此前恒回
   `{cancelled:false}` 且 scan() 一路播完无视取消——预览里点取消自然无效
   （真后端置位 → walk/worker/入库三处轮询 → 发 `cancelled` 终态，链路是
   完整的）。mock 补上等价物：置位标志 + 每步轮询 + 发 `cancelled` 事件 +
   提前返回（已完成批次保留语义对齐），步长 250ms→400ms 给 e2e 点击留窗口。
5. **取消后也要收尾**：`watchScan` 此前只在 `done` 触发 afterScan——取消路径
   同样可能已提交批次（统计/歌单/列表过期），现在 `cancelled` 也收尾。

**验证**：svelte-check 0/0、e2e **34/34**（删自动刷新用例，新增"取消停在
cancelled 终态"与"曲库页扫描条无按钮"两条；取消用例连跑 3 遍稳定）。

## 九轮修订（2026-10-28 晚五批，识别信息归位）

1. **「高级与诊断」折叠区整体迁去设置页新「关于」tab**：版本/平台/Schema/
   播放列表/数据目录 + 重新读取/打开数据目录 + 最近错误。理由：这些信息
   描述的是整个应用，不是"入库"这件事；挂在扫描入库页底部折叠区一直是
   过渡形态。设置页 tab 从四个变五个；diag-grid/diag-block/error-list 样式
   本体在全局 overlays.css，SettingsPanel 直接复用（含 user-select 豁免）。
   顺手把快捷键分支的 `{:else}` 兜底改成显式 `{:else if}`（新增分支后
   duplicate-clause 编译错误暴露了这个隐患）。
2. **清空播放历史并入曲库卡动作行**：它与清空曲库是同族的"清掉一类记录"
   动作，放同一行；不加确认（历史只是播放记录，toast 即刻反馈，与
   HomeView 同款语义）。
3. 扫描入库页现在只剩曲库 + 整理库两组卡片，无折叠区。

**验证**：svelte-check 0/0、e2e **34/34**（诊断折叠用例改写为"扫描页无
diag 区 + 设置页关于 tab 有"；smoke 用例 tab 数 4→5）；Edge 截图目检
「关于」tab 与曲库卡动作行。

## 十轮修订（2026-10-28 晚六批，设置页改回单页 + 关于瘦身 + 爱发电）

1. **设置页改回单页平铺，tab 只做跳转**（用户拍板）：五个区块全部渲染、
   各带小标题；顶部 tab 点击 scrollIntoView 平滑滚动，高亮只跟点击走、
   **不做 scroll-spy**——旧版"拉扯抖动"的病根是 spy 滚动中写回高亮，
   只要不写回，平铺与跳转可以共存。大标题固定「设置」。
2. **「关于」改一行一项**（与设置项同款 setting-row）：平台 / Schema /
   播放列表计数三行删掉（内部信息或恒 0）；「重新读取」删掉（诊断随页面
   加载自动拉）；版本行带「获取更新」占位按钮（功能未做，恒禁用，title
   注明）；数据目录与「打开目录」并成一行（完整路径等宽字体 + 省略 +
   可复制）；「最近错误」补回上边距。
3. **爱发电二维码进「关于」**：`afdian.com.png`（工作区根）拷入
   `ui/src/assets/afdian-qr.png`，vite 资源管线 import（CSP img-src 'self'
   天然放行）。「支持开发」行 = 文案 + 104px 圆角二维码。刻意不做外链
   ——Tauri 开浏览器要走 shell open，等"获取更新"真做时一起搭基建。

**验证**：svelte-check 0/0、e2e **34/34**（smoke 用例改写为"单页平铺 +
tab 跳转"；route 关于用例断言一行一项 + 获取更新禁用 + 二维码可见 +
三项不复活）；Edge 截图目检关于区块。

## 十一轮微调（2026-10-28 晚七批，支持开发区块）

用户拍板：二维码行不要描述文本；二维码放到「关于」最下面、放大、居中；
区块底部一行"支持创作，用爱发电"；为后续别的平台二维码预留并排结构。

- 删掉原 setting-row（标题 + 提示文案 + 右侧 104px 图）。
- 新 `.support-block`（居中）→ `.support-items`（flex 并排容器，gap 32）
  → `.support-item`（figure：图 + figcaption）。爱发电二维码 104→160px。
- 以后加别的平台：在 `.support-items` 里并排多一个 `.support-item` 即可。
- img 保留 `afdian-qr` 类（e2e 锚点不变），最近错误块 margin-top 语义不变。

**验证**：svelte-check 0/0、e2e 34/34。

## 十二轮微调（2026-10-28 晚八批，快捷键须知盒）

- 「Esc 取消录制…」描述 + 「恢复默认」按钮：从快捷键列表**底部**移到**组顶**
  （小标题之下、冲突提示之上），`.setting-foot` 改成带边框圆角的独立条
  （surface-2 底、上下留白）——用户点下任何绑定键之前先读到规则。
- 类名保留 `setting-foot`（全局样式仅此一处使用，改名收益为零）。
  user-select 豁免选择器同步改为 `.shortcut-foot .dim`。

**验证**：svelte-check 0/0、e2e 34/34。

## 十三轮（2026-10-28 晚九批，入库模型定型）

用户在 beets 练习场（工作区 beets-playground/，隔离 BEETSDIR + dev venv）实玩后拍板：
新手不该面对裸 beets，应用按"待整理目录 → beets 整理 → 唯一输出根"定型。

**拍板三点**：① 待整理目录必须是**用户自选的外部目录**，后端校验不得与任何曲库
目录相同或在内部（copy 留原件会重复入库）；② 整理输出根在曲库目录里**单选**
（radio）；③ 保留原文件与否（copy/move）沿用导入面板既有的 moveMode 三选
（只写标签 / 复制 / 移动），不是配置项。

**实现**
- 后端：`beets_config_get` / `beets_set_inbox`（重叠校验 invalid_arg）/
  `beets_set_target`（必须在 roots 内）/ `beets_open_inbox` /
  `beets_inbox_imported_add`（FIFO 2000 排除表）。键：`beets.inboxPath` /
  `beets.targetRoot` / `beets.inboxImported`。roots_set/choose_root 之后
  `clear_target_if_invalid`（输出根被移除→清掉回退首根）。
- `scan_root`（beets.rs）改为 targetRoot 优先、回退首根——老配置零迁移。
- **防重复导入**：inbox 固定 + copy 模式 = 原件常驻 → 二次整理重复入库
  （beets 重复条目 → dump → 曲库离线重复行）。方案：applyDone 后 UI 把成功
  路径记进 `beets.inboxImported`；`start_preview` 在宿主侧注入
  `options.excludePaths`（协议 options 本就是预留自由对象，schema 补文档
  不升版）；beets-tool preview_import 按 normcase 过滤。UI 全程不搬运列表。
- UI：新 store `beets-config.svelte.ts`；ImportPanel 顶部「待整理目录」行
  （打开/更换/整理入库，旧「选择文件夹」降级为「浏览其他文件夹」次要入口，
  重叠报错内置）；曲库目录行加「整理输出」radio（回退首根时首根显示选中）。
- mock 全套接线（同语义校验）。

**验证**：cargo check + lib 100（新增 5 条路径重叠单测：相等/内部/兄弟目录/
空串/归一）+ dto_wire 20（BeetsConfig 键）；协议契约 ALL PASS（SHA 同步）；
镜像一致；svelte-check 0/0；e2e **35/35**（新增 radio+inbox 引导用例）。

**真机待验**：inbox 选择对话框真跑一遍 + copy 应用后二次整理确认排除表生效。
