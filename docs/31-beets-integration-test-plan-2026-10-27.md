# beets 集成验收计划（集成效果 / 插件适配度 / 内置整合包 / 固定调用命令）

> 状态：**计划待审**（未开工）。日期：2026-10-27。
> 前置：docs/29 批 A–D 已交付（扫描入库页、beets 运行时随包、曲库维护四件套）。
> 本文只定"测什么、怎么判、改什么"，不含实现。
>
> **⚠ 已被 docs/32（组件化调研）部分取代**：§5「内置整合包 10 项验收」降级为"仅构建可选离线包时适用"；
> §10 三个待拍板项合并进 docs/32 §10。本文 §2 的事实基线、§3 的测试分层、
> §6 的固定调用命令仍然有效（且是 docs/32 的输入）。
>
> **⚠ 更进一步被 docs/33 取代**：`docs/33-beets-split-architecture-2026-10-27.md`
> 定下 beets 与播放器分两侧维护。本文的定位随之变化：
> - §3 测试分层、§6 固定调用命令 → **继续有效**，且成为 beets 侧仓库的测试基线
> - §5 内置整合包验收 → 只在做可选离线包时适用（docs/33 §8.2）
> - §10 待拍板 → 全部并入 docs/33 §10
> - §4 插件适配度矩阵 → **继续有效**，但结论要改成 `capabilities` 三态上报
>   （而不是静态清单），因为"能不能干活"只有 beets 侧知道

---

## 1. 这份计划要回答的四个问题

1. **集成效果**：从应用点下去，beets 这条链路（探测 → 预览导入 → 应用 → 对账 → 维护）在真机、真曲库上到底能不能跑通？跑通的定义是"数据对得上"，不是"没报错"。
2. **插件适配度**：随包运行时里 73 个 beets 插件，哪些能加载、哪些缺依赖、哪些与本地播放器定位冲突、哪些值得进内置启用清单。
3. **内置整合包**：`tools/beets-runtime`（112 MB）作为可分发产物的验收标准——可重定位、依赖完整、体积可控、许可齐全、离线可用。
4. **固定调用命令**：把"应用如何调用 beets"冻结成一封闭集合（进程内 sidecar 协议 + 极少数 CLI 逃生口），并让版本漂移、隐式默认值无法悄悄改变行为。

---

## 2. 事实基线（均已实测，带证据）

做计划前先核了一遍，**结论与 docs/29 的假设有四处不一致**。以下全部是命令输出，不是推断。

### 2.1 自有库与 beets 库的真实状态

| # | 事实 | 证据 |
|---|---|---|
| F8 | 应用数据目录有 `library.sqlite`（1472 行，全部 online，`beets_item_id` 全为 NULL），**但没有 `beets/` 子目录** | `ls %APPDATA%/dev.aiya.music` → cache / library.sqlite / logs / sidecar |
| F8b | 也就是说 `ensure_config` 从未执行过，beets home 尚未建立 | 同上；`render_config` 只在 beets 命令触发时跑 |
| F9 | 库来源是三处混杂：1381 行来自 `D:\FM_Stations\CloudMusic1`、88 行来自 `D:\CkMsr\Music`、3 行来自 `test-music` | 按路径前缀聚合 `select path from tracks` |
| F9b | settings 表里只有**旧键** `library.root = "D:\\FM_Stations\\CloudMusic1"`，**没有新键 `library.roots`** | `select * from settings`；多根键缺失 → 走单根兜底 |
| F10 | `D:\CkMsr\Music` 磁盘上 1845 个 mp3 + 1366 个 lrc，库里只收了 88 首 | `find` 计数 vs DB 前缀计数 |

> **F9/F10 的含义**：现在这份库是"多根残留"，不能直接拿 1845 做基准。测前必须先定根（改成 `D:\CkMsr\Music`）并跑一次全量扫描，否则"覆盖率对账"这项一开始就是错的。

### 2.2 内置整合包

| # | 事实 | 证据 |
|---|---|---|
| F1 | 112 MB。构成：`Lib` 88M（numpy 34M + numpy.libs 21M + **pip 13M** + beetsplug 3.3M + beets 2.1M + unidecode 2.2M + mutagen 1.7M + lap 1.6M）；顶层 python314.zip 4M / python314.dll 6.5M / libcrypto 6M / sqlite3 1.6M / libssl 1.3M | `du -sh` 逐层 |
| F2 | 依赖锁 = 21 个包（`beets==2.14.1`、`numpy==2.5.4`、`mutagen==1.48.1`、`lap==0.5.13`…），解释器 = embeddable Python 3.14.7 | `tools/beets-requirements.lock`、`python -m beets version` |
| F3 | **numpy 是 beets 的核心依赖**（`numpy>=2.3.5; python_version>='3.13'`），不是 extra → 55 MB 不可能裁掉；**pip 13 MB 是纯运行期无用** | `importlib.metadata.requires('beets')` |
| F11 | `tools/beets-runtime/.lock-sha256` **不存在** → 打包脚本里"锁没变就跳过 pip"的增量判断从未生效，每次构建都重装一遍 | `find tools -name .lock-sha256` 无结果；脚本 167/185 行确有写戳逻辑 |

### 2.3 插件可加载性（73 个模块，实测 import）

**62 个可导入，11 个失败**，失败原因全部是缺 extras 依赖：

| 插件 | 缺什么 | 备注 |
|---|---|---|
| `lyrics` | `bs4`（beautifulsoup4） | **在首发档清单里** |
| `chroma` | `acoustid`（pyacoustid） | docs/29 批 E 观察档 |
| `aura` | `flask` | 另一个 Web 前端，与本地播放器定位冲突 |
| `autobpm` | `librosa` / numba | 体量极大 |
| `beatport` / `bpsync` | `requests_oauthlib` | 需凭据 |
| `lastimport` | `pylast` | 需凭据 |
| `mpdstats` | `mpd`（python-mpd2） | 对接 MPD，与自有引擎冲突 |
| `sonosupdate` | `soco` | 硬件桥接 |
| `thumbnails` | `xdg`（pyxdg） | |
| `titlecase` | `titlecase` | |

**两个必须记住的判读规则**（否则评估会得出错误结论）：

1. **"import 成功" ≠ "功能可用"**。`fetchart` 在没有 `bs4`／`pillow`／`langdetect` 的情况下照样 import 通过（重依赖写在函数体内懒加载），真调用时才炸。所以适配度评估必须做**功能级冒烟**，不能只看 import。
2. **插件加载失败不会中断 beets**。`plugins.load_plugins()` 捕获 `PluginImportError` 只记日志，进程继续跑（实测：plugins 配 `replaygain lyrics duplicates scrub`，返回 `loaded ok`，但 traceback 里明确 `Could not import plugin lyrics`）。→ 见 F7。

### 2.4 首发档 6 插件的实际可用性（最重的一组发现）

`FIRST_WAVE_PLUGINS`（`src-tauri/src/beets.rs:129`）= replaygain / lyrics / fromfilename / duplicates / scrub / smartplaylist。逐个实测：

| 插件 | 现状 | 根因 |
|---|---|---|
| `replaygain` | **不可用** | 默认后端是 `ffmpeg`（`replaygain.py:289` 硬编码 `self._ffmpeg_path = "ffmpeg"`），初始化时 `ffmpeg -version` 探活失败即抛错；本机 `which ffmpeg` **NOT FOUND**，`bs1770gain` 也没有 → `gain_db` 永远为空，而架构里"gain_db 已在引擎链路消费"这条等于空转 |
| `lyrics` | **不可用** | 缺 `bs4`，import 直接失败（见上表） |
| `fromfilename` | **配了等于没配** | 纯 Python 可加载，但只在 `import` 且 `fromfilename.auto: yes` 时起作用；生成的 config.yaml 没有这段 |
| `smartplaylist` | **配了等于没配** | 可加载，但需要 `smartplaylist:` 段列 playlist 定义，没有就一个 m3u 都不产出 |
| `duplicates` | 可用 | 批 D 已接线（且默认 keys 必须是 `title+artist`，否则零命中） |
| `scrub` | 可用（有条件） | `mutagen` 在锁内；但只有 `import.write: yes` 时才有意义，而 config 没有 `import` 段 |

**F7（契约缺口）**：`_status_payload()` 的 `plugins` 字段取自 `config['plugins']` 这张**愿望清单**，不是"实际加载成功的清单"。所以 UI 上六个插件全显示"已启用"，其中两个根本没加载。这是"看着绿、实际红"的典型，评估必须先补这一层探针。

### 2.5 Web 入口是死的

| # | 事实 |
|---|---|
| F5 | `beetsplug/web` 是包目录，`flask` 缺失 → 插件导入失败 → `python -m beets web` 直接报 `error: unknown command 'web'`（实测） |
| F5b | 而 `beets::web_open`（`beets.rs:922`）正是 spawn 这条命令，且 **stdout/stderr 全设 null、不探活、不检查退出码**，1.5 s 后无条件 `explorer http://localhost:8337` |
| F5c | 用户看到的是浏览器 `ERR_CONNECTION_REFUSED`，应用里没有任何提示。而且即使装上 flask，config 里也没把 `web` 加进 plugins 列表 |

### 2.6 一个"现在安全但很脆"的隐式依赖

| # | 事实 |
|---|---|
| F12 | 生成的 config.yaml **没有 `import:` 段** → beets 默认 `import.copy: yes` + `import.write: yes`（`config_default.yaml`） |
| F12b | 当前安全：`apply_import` 不走 beets 的 import 命令，而是手工 `Item.from_path` + `FileOperation(MOVE/COPY)`，所以复制/写标签都由 sidecar 显式下发 |
| F12c | 但这是**隐式**的——将来谁图省事调一次 `beets.ui.commands.import_files`，就会静默复制整个曲库（1845 首 / 17 GB）。需要一条断言把这个前提钉住 |

---

## 3. 工作流 A：集成效果测试

### 3.1 测试分层与安全边界（先立规矩）

| 层 | 数据 | 允许的操作 | 说明 |
|---|---|---|---|
| L1 只读 | 真库 `D:\CkMsr\Music` | status / ping / dump_items / find_duplicates / update(pretend) / move(pretend) | 不写 beets 库、不动文件 |
| L2 可回滚写 | 真库 + 独立 `BEETSDIR` | adopt（只写 beets 库，不动文件） | 回滚 = 删掉临时 beets home |
| L3 破坏性 | **临时副本**（30 首，复制到 temp） | update(应用) / move(应用) / 冲突场景 | **禁止在真库上跑** |

> 硬规则：L3 一律用副本。真库上只跑"看得见清单、不动文件"的 pretend 模式。move 真执行前先 `--pretend` 出清单，清单条数与预期不符即中止。

### 3.2 六个场景组（A1–A6）

**A1 环境探测与降级**

| 用例 | 操作 | 判定 |
|---|---|---|
| A1.1 | 删掉 `beets-venv` 与 `beets-runtime`，点"检测" | 报 `E_BEETS_NOT_FOUND`，UI 出"安装 beets 运行时"按钮，不崩、不卡"正在检测…" |
| A1.2 | 恢复运行时，重启应用 | `status` 报 beets 2.14.1 / py 3.14.7 / protocolVersion 3；`libraryDirectory` 正确回显 config 原文 |
| A1.3 | `AIYA_BEETS_PYTHON` 指向不存在的路径 | 回退到随包运行时，不报错退出 |
| A1.4 | 首次进入「扫描入库」页 | `beets/` 目录被创建、config.yaml 生成、六个插件写进去（防批 A 那个"永不加载诊断"的回归） |
| A1.5 | **插件健康检查**（新增探针） | 六个首发插件逐个报"加载成功/失败+原因"；断言 `lyrics` 当前为失败（fail-loud，不许静默） |

**A2 预览导入（只读，联网）**

| 用例 | 操作 | 判定 |
|---|---|---|
| A2.1 | 对 20 首正常专辑跑预览 | 组数 = 目录分组数；候选非空；**文件 mtime/size 全不变**（前后各拍一次快照对比） |
| A2.2 | 断网跑预览 | 每组一条 error 事件，不中断整批，会话正常收尾（不是超时被看门狗杀） |
| A2.3 | 中途取消 | `cancelled` 事件及时到，槽位释放，可立刻再开一次 |
| A2.4 | 50 首跨专辑 | 进度按文件粒度推进，10 min 看门狗不误杀 |

**A3 应用导入（写标签，L3 副本）**

| 用例 | 操作 | 判定 |
|---|---|---|
| A3.1 | apply（不移动） | 标签写入后 `mutagen` 读回一致；**没有新文件产生**（验证 F12 的隐式依赖）；beets 库行数 +N |
| A3.2 | apply_copy | 副本目录多出 N 个文件，源文件不动 |
| A3.3 | apply_move | 源目录变空（beets 清理空目录），目标路径符合 path_formats |
| A3.4 | 目标同名冲突 | 报 conflict，**不覆盖、不改名**，源文件留在原地 |
| A3.5 | 取消于写入中途 | 已写文件不回滚（文档已声明幂等），重跑能把剩余补上 |

**A4 对账（sync_items）**

| 用例 | 操作 | 判定 |
|---|---|---|
| A4.1 | adopt(1845) → sync | `beets_item_id` 全部回填（当前 0/NN）；`imported=0 orphaned=0` |
| A4.2 | 手工把 10 个文件改名 → dump → sync | 判定为 relocate，**行 id 不变**（不是删旧建新） |
| A4.3 | 删 5 个文件 → scan → sync | 判 orphaned，不误删行 |
| A4.4 | beets 侧删 3 条 → dump → sync | 对应行 `beets_item_id` 置空，不判 orphan |

**A5 曲库维护四件套（真库 L1/L2 + 副本 L3）**

| 用例 | 操作 | 判定 |
|---|---|---|
| A5.1 | adopt 重跑 | 幂等：全 skipped，不重复插入 |
| A5.2 | update pretend → 看 diff | 只出真实差异；无差异时"应用"按钮不出现 |
| A5.3 | update 真应用（副本） | 磁盘标签与库一致；**mtime 未变的文件被跳过**（用耗时/日志证明，不是"看起来快"） |
| A5.4 | find_duplicates(keys=title,artist) | 分组数与 beets CLI `beet dup -k title -k artist` 一致 |
| A5.5 | find_duplicates 默认 keys（MBID） | 明确零命中并在 UI 说明原因（不能让人以为功能坏了） |
| A5.6 | move pretend → move | 清单条数 = 待移动数；执行后幂等（再跑全 same） |
| A5.7 | 库根不一致 / 未配置 / 写 `~/x` | 三种都必须在动手前拒绝（已有冒烟覆盖，需扩到真库） |

**A6 端到端与性能**

| 用例 | 操作 | 判定 |
|---|---|---|
| A6.1 | 扫描入库 → beets adopt → 维护 → 播放 | 全链路不重启应用；播放不受影响（beets 只是读写库，不碰音频线程） |
| A6.2 | adopt 1845 首耗时 | 记录基线（目标：单事务 < 30 s）；与批 B 的扫描 953 ms 对比，说明瓶颈在哪 |
| A6.3 | 内存 | sidecar 进程 RSS 峰值（numpy 已加载）记录，判断 112 MB 运行时是否带来过多常驻 |
| A6.4 | 并发 | 扫描进行中触发 beets 维护 → 要么排队、要么明确拒绝，不许两边同时写（一个写自有库、一个写 beets 库，看似无关，但 sync 会串起来） |

### 3.3 证据留存

每条用例留三样：命令/操作、关键输出、判定结论。写入 `docs/31-appendix-beets-test-log.md`（跑的时候生成，一次一份）。**不写"通过"两个字就完事**——必须有可比对的数字（行数、耗时、字节数、清单条数）。

---

## 4. 工作流 B：插件适配度评估

### 4.1 评估维度（六维）

| 维度 | 取值 | 为什么要这一维 |
|---|---|---|
| 可导入 | 是/否（实测） | 模块级门槛 |
| 功能级可用 | 是/否/未测（跑一次真调用） | import 通过不代表能用（fetchart 反例） |
| 外部依赖 | 无 / Python 包 / 外部二进制 / 凭据 | 决定能否随包 |
| 联网 | 否 / 必须 / 可选 | 离线用户是主场景 |
| 写盘 | 无 / 写标签 / 搬文件 / 删文件 | 危险度，决定是否要两段式确认 |
| 定位冲突 | 无 / 与自有引擎重复 | `play`/`bpd`/`web`/`aura`/`mpd*` 都是"另一个播放器" |

### 4.2 分档结论（先给建议，跑完再定稿）

**A 档：内置启用（进固定命令面，随包必须能跑）**

| 插件 | 理由 | 前置 |
|---|---|---|
| `duplicates` | 已接线，纯本地 | 无 |
| `scrub` | 导入清脏标签，纯本地 | 需 config 补 `import.write` 语义说明 |
| `fromfilename` | 无标签文件兜底，纯本地 | 需 `fromfilename.auto: yes` |
| `smartplaylist` | beets 查询 → m3u，接"猜你喜欢" | 需 `smartplaylist:` 段（可空模板） |
| `missing` | 找出库里有、磁盘没的条目 | 纯本地，值得补进 A 档 |
| `badfiles` | 校验文件完整性 | 纯本地，值得补进 A 档 |
| `info` | 字段自检 | 纯本地 |

**B 档：可选启用（用户勾选后装依赖）**

| 插件 | 缺什么 | 备注 |
|---|---|---|
| `lyrics` | `bs4` + `langdetect`（+ 网络） | 应随包（体积小），否则"LRC 能力"是空话 |
| `replaygain` | `ffmpeg` 二进制（约 80 MB） | 要么随包 ffmpeg，要么明确"检测不到就关掉并告知"，**不能假装已启用** |
| `fetchart` / `embedart` | `bs4` / `pillow` / `langdetect`（+ 网络） | 与自有封面管线重复，需评估是否让 beets 抢活 |
| `lastgenre` | `pylast`（+ 凭据） | 可选 |
| `chroma` | `pyacoustid` + `fpcalc`（外部二进制）+ 凭据 | docs/29 批 E 观察档 |
| `bpm` / `autobpm` | `librosa`（体量巨大） | 只考虑 `bpm`（读标签），`autobpm` 放弃 |
| `zero` / `ftintitle` / `replace` / `rewrite` | 无 | 纯文本改写类，成本低，按需 |

**C 档：明确不整合（写进文档，避免反复讨论）**

- **定位冲突（另一个播放器/前端）**：`play`、`bpd`、`web`、`aura`、`mpdstats`、`mpdupdate`、`subsonicplaylist`、`subsonicupdate`
- **硬件/第三方服务推送**：`sonosupdate`、`kodiupdate`、`embyupdate`、`plexupdate`、`ipfs`
- **需要商业凭据且与本地库无关**：`spotify`、`tidal`、`deezer`、`beatport`、`bpsync`、`discogs`
- **转码/格式转换**：`convert`（自有引擎不做转码，且会复制文件）
- **云端提交**：`absubmit`、`mbsubmit`、`mbcollection`、`acousticbrainz`、`listenbrainz`、`lastimport`

> 判据：凡是"要另起一个服务/要账号/要搬文件"的，一律 C 档。A 档只留"纯本地、只读或只写标签、一行命令能验证"的。

### 4.3 评估产出

1. `tools/probe_beets_plugins.py`——逐插件探测：import 结果、缺失依赖、是否需要网络/凭据、是否找到外部二进制。输出 CSV + 汇总表，**可重复跑**（不是一次性手工表）。
2. `docs/31-appendix-plugin-matrix.md`——73 行 × 六维矩阵，随包版本升级时重跑即可 diff。
3. 结论落到 config 生成器：A 档进 `plugins:`，B 档进"可选",C 档进文档黑名单。

---

## 5. 工作流 C：内置整合包验收

### 5.1 十项验收检查

| # | 检查 | 判定标准 | 现状 |
|---|---|---|---|
| C1 | 可重定位 | 整个 `beets-runtime` 目录改名/挪盘后仍能 `python -m beets version` | 未验（venv 死在这条，embeddable 应过） |
| C2 | 无外部 Python | 在**没装 Python** 的路径下（改 PATH 屏蔽系统 python）能跑 | 未验 |
| C3 | 依赖完整 | `beets_requirements.lock` 21 包全部 `import` 成功 | 部分（beets 核心 OK） |
| C4 | 插件完整 | A 档插件全部加载成功（**不是 import 通过**） | **不达标**：lyrics 失败（F4/F6） |
| C5 | 体积可控 | 裁掉 pip 后 ≤ 100 MB；列出体积 Top 10 并逐个说明必要性 | **不达标**：112 MB，含 13 MB 无用 pip（F1/F3） |
| C6 | 离线可用 | 断网跑 status / adopt / duplicates / move 全通过 | 未验（这几条本就不联网） |
| C7 | 增量构建 | 锁文件未变时跳过 pip（`.lock-sha256` 戳生效） | **不达标**：戳文件不存在（F11） |
| C8 | 自检 | 打包结束时自动跑"版本 + A 档插件 + 一条真查询"，失败即构建失败 | 部分（有自检，没检插件） |
| C9 | 许可与声明 | beets（MIT）+ numpy（BSD）+ 其余 21 包的 LICENSE 收进 `resources/notices/`，生成一份汇总 | 未做 |
| C10 | 契约一致 | `PROTOCOL_VERSION`（3）与宿主期望一致；运行时版本变化能被应用识别并提示 | 未验（宿主没有版本下限校验） |

### 5.2 包体裁剪与增补（方向，不含实现）

- **裁**：`site-packages/pip`（13 MB，运行期无用；pip 只在打包阶段需要，装完即删）；`__pycache__`（可留，影响启动速度，需实测取舍）。
- **不裁**：numpy（核心依赖，55 MB）。
- **增**（若定 B 档随包）：`beautifulsoup4` + `langdetect`（lyrics/fetchart，约 3 MB）——这是"花 3 MB 买个能用的歌词能力"，性价比明显。
- **ffmpeg**：单独决策。要么随包（+80 MB，但 replaygain 才成立），要么从首发能力里摘掉 replaygain 并在 UI 说清。**现状是最差的一种：配了、显示了、永远不工作。**

---

## 6. 工作流 D：固定调用命令

### 6.1 冻结"应用如何调用 beets"

**规则一：应用内不 spawn 任意 `beet` 子命令。**
理由：CLI 输出是给人读的文本，转结构化事件既脆弱又拿不到逐曲粒度；且 CLI 行为随版本漂移（`beet web` 在 2.14 就从内置命令变成了插件）。
**唯一例外**：`python -m beets web`（当前是死的，见 P0-3）。要么修好并纳入白名单，要么删掉这个入口。

**规则二：所有文件操作必须显式下发 operation。**
不许依赖 `config['import'].copy/write` 的默认值（默认是 `yes`，F12）。代码层现状已满足，缺一条**断言**把它钉住：

> 建议加一条 Rust 测试或脚本检查：`grep -rn "import_files" sidecar/ src-tauri/` 必须为空；`beets_sidecar.py` 中不得出现 `config['import'] =` 的写入。

**规则三：库根只认 config.yaml 原文。**
已落实（`_configured_directory`），本计划只把它写进"固定契约"清单，防后人改回 `config['directory']`。

### 6.2 命令面清单（v3 现状，冻结）

| 命令 | 必填 | 选填 | 默认 | 禁止 |
|---|---|---|---|---|
| `status` | — | — | — | — |
| `ping` | — | — | — | — |
| `preview_import` | `paths[]` | `options{}` | — | 空 paths |
| `apply_import` | `decisions[]` | — | 不移动、不复制 | 空 decisions |
| `dump_items` | — | — | — | — |
| `adopt_library` | `paths[]` | — | 宿主填 online_paths | 空 paths |
| `update_library` | — | `pretend` | `pretend=false` | — |
| `find_duplicates` | — | `keys[]` | `[title, artist]` | — |
| `move_items` | — | `directory`, `pretend` | 库根取 config | 库根缺失/不一致 |

### 6.3 版本钉（三处必须同源）

| 项 | 当前值 | 谁来钉 |
|---|---|---|
| beets | 2.14.1 | `tools/beets-requirements.lock` |
| Python | 3.14.7 embeddable | `tools/pack-beets-runtime.mjs` 的 `PY_VERSION` |
| sidecar 协议 | 3 | `PROTOCOL_VERSION` + 宿主校验 |

**缺口**：宿主 `probe()` 目前只读 `protocolVersion`，**没有做下限/上限校验**（`ping` 也不校验）。若运行时被换成一个老版本，行为差异会以"命令不存在"的形式在运行时暴露。建议：`protocolVersion != 3` 直接判不可用并提示重装运行时。

### 6.4 命令面收敛的方向（评估项，非本次实现）

9 条命令里，`preview_import` / `apply_import` 与 `adopt_library` / `update_library` 有语义重叠（都是"把标签弄进库"）。评估完插件矩阵后可以考虑合并成"一个 import 族 + 一个 maintain 族"，但那属于 P2 重构，不进本轮。

---

## 7. 缺陷与修复优先级

| ID | 缺陷 | 严重度 | 归属 |
|---|---|---|---|
| P0-1 | `lyrics` 插件在内置清单里但**根本没加载**（缺 bs4），UI 显示"已启用" | **P0** | B + C（补依赖 or 从清单摘掉，二选一，不许保持现状） |
| P0-2 | `replaygain` 配了但后端探活失败（无 ffmpeg）→ `gain_db` 永远空，而它已被引擎链路消费 | **P0** | C（随包 ffmpeg 或摘掉能力） |
| P0-3 | `beets_web_open` 静默失败：spawn `beet web` 报 unknown command，stdout/stderr 全 null，1.5 s 后无条件开浏览器 | **P0** | A（先探活/校验退出码；web 插件也不在 plugins 列表） |
| P1-1 | `status.plugins` 报的是愿望清单，不是加载结果；`load_plugins` 失败不抛错 | **P1** | A（插件健康检查并入 status） |
| P1-2 | `fromfilename` / `smartplaylist` 配了但缺子配置 → 空转 | **P1** | A（config 生成器补段） |
| P1-3 | 运行时 112 MB 含 13 MB 无用 pip；增量戳缺失导致每次全量装 | **P1** | C |
| P1-4 | 宿主不校验 `protocolVersion` 上下限 | **P1** | D |
| P2-1 | 库来源混杂（三处根、旧键 `library.root`、新键缺失），覆盖率无法对账 | **P2** | 测试前先整理，不视为代码 bug |
| P2-2 | 命令面 9 → 可收敛（import 族/maintain 族） | **P2** | 重构，不进本轮 |

> P0-1/P0-2/P0-3 的共同点：**功能没工作，但界面显示它工作了**。这类问题比"报错"严重，因为它们消耗用户的信任额度。本计划把它们排在最前面，其余都得等这三条定案。

---

## 8. 执行顺序（四个里程碑）

### M1 打地基：插件探针 + 现状快照（必须先做，后三个都依赖它）

- 产出 `tools/probe_beets_plugins.py`（逐插件六维探测，可重复跑）
- 产出插件矩阵附录（73 行）
- 产出"当前状态快照"：自有库行数、beets 库行数、运行时体积、首发插件真实可用性
- **验收**：矩阵里每一行都有实测结论，没有"应该可以"

### M2 修 P0 三条 + 定 config

- P0-1/P0-2/P0-3 定案（补依赖还是摘能力，逐条拍）
- config 生成器补齐 `import` / `fromfilename` / `smartplaylist` 段，并把 A 档插件写全
- `render_config` 的单测同步扩（现在只断言了五个插件名存在）
- **验收**：六个首发插件逐个"功能级可用"或"明确不可用且有提示"，没有中间态

### M3 集成效果测试（A1–A6 全跑）

- 先整理库根（改成 `D:\CkMsr\Music`，全量扫描，与磁盘 1845 对账）
- 按 A1 → A6 顺序跑，L3 只在副本上
- 产出测试记录附录（带数字）
- **验收**：A 组全部有结论；失败项进缺陷表；性能数字可与批 B 的扫描基线并列

### M4 整合包与固定命令收口

- 裁 pip、补 `bs4/langdetect`（若拍）、补 `.lock-sha256` 增量、补许可汇总
- 宿主加 `protocolVersion` 校验
- 把 6.1/6.2/6.3 三张表写进 `docs/08-beets.md`（契约文档），并加"禁止事项"一节的自动化断言
- **验收**：C1–C10 全过；`npm run verify` 里新增 `probe:plugins` 与 `smoke:maint` 并列

---

## 9. 不做什么（本轮明确排除）

- **不做 `modify_items`（手改标签）**：需要字段编辑器那套交互，与"命令面"是两个尺度（docs/29 §16.6 已定）。
- **不做应用内删除重复文件**：查重只读 + 定位（你已拍板）。
- **不做 `query_items` 工具化查询**：拍板 5 已排除。
- **不做 beets 侧独有条目"纳入曲库"的 UI 流程**：`sync_items` 的 `imported` 计数先留着。
- **不引入 `convert` / `play` / `web` 这类"另一套播放/转码"能力**。
- **不改 path_formats 模板编辑**：beets 的领地，应用只做预览。
- **不动音频引擎**：本计划全程不碰 Rust 音频侧（beets 只写库，不回放）。

---

## 10. 需要你拍板的三处

1. **replaygain 怎么办**：① 随包 ffmpeg（+约 80 MB，能力成立）② 检测不到就自动从 config 摘掉并在 UI 说清（省体积，牺牲能力）③ 保持现状（不推荐，等于假功能）。
2. **lyrics 怎么办**：① 随包补 `bs4 + langdetect`（+约 3 MB，强烈建议）② 从首发清单摘掉（等 LRC 批再装）。
3. **web 入口怎么办**：① 修好（随包 flask + config 加 `web` 插件 + 探活）② 删掉这个按钮（应用内自带的维护卡片已覆盖同类需求，flask 只为看库不值得）。倾向 ②。

---

## 附：与本计划相关的事实核查命令（可复现）

```bash
# 插件可导入性（73 → 62/11）
tools/beets-runtime/python.exe -c "import importlib,os; d=r'tools\beets-runtime\Lib\site-packages\beetsplug'; ..."

# 插件加载失败不抛错（lyrics 反例）
tools/beets-runtime/python.exe -c "from beets import config; config['plugins']=['lyrics']; from beets import plugins; plugins.load_plugins()"

# web 命令是死的
tools/beets-runtime/python.exe -m beets web --help      # → error: unknown command 'web'

# replaygain 默认后端与 ffmpeg 缺失
grep -n "_ffmpeg_path" tools/beets-runtime/Lib/site-packages/beetsplug/replaygain.py
which ffmpeg        # → NOT FOUND

# 库状态
sqlite3 %APPDATA%/dev.aiya.music/library.sqlite "select count(*) from tracks;"
du -sh tools/beets-runtime/*  | sort -h | tail
```
