# 整理引擎补齐 + 索引家指向曲库（docs/49）

> 状态：**两侧都已实现并真机实测**。分析侧（索引家指向曲库 / 只读入库 / 查询转歌单）
> 与整理侧（待整理目录 / 落点 / 移动或拷贝 / 应用）都跑通了，包括最容易漏的那一段：
> 整理完成后**应用曲库重扫**（`trigger=action`，151 ms）与**索引家按落点补扫**
> （1850 → 1852），两者是两套互不相干的库，必须分别处理 —— 见 §3.5。
> 页面按用户 2026-10-30 拍板拆成 **安装 / 索引 / 整理** 三个子页。  
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
| 待整理目录 | ✅ preview/apply 接目录 | ✅ `settings.json.inbox_path` | ✅ 整理子页 | — |
| 落点 | ✅ `lib.directory` | ✅ `target_root` | ✅ 整理子页（曲库目录 chip + 选子目录） | — |
| 移动 / 拷贝 | ✅ `apply_move` / `apply_copy` | ❌ 不落盘（按次选） | ✅ 动作下拉，默认拷贝 | — |
| 索引家指向曲库 | ✅ | — | ✅ | — |
| 整理后同步曲库 | ⚠️ 只有整库 refresh | — | — | 收窄成落点（未做） |

### 2.1 实现时撞出来的两个真问题

1. **`library_choose_root` 不能拿来选待整理目录**（旧 `ImportPanel` 就是这么用的）。
   它会把选中的目录**追加进 `library.roots`**：选完待整理目录，它自己就成了曲库
   根——copy 模式下原件留在里面，扫描器扫一遍就是重复曲目；而且紧接着的
   `beets_set_inbox` 重叠校验会拒绝这次选择，用户看到"不能与曲库目录相同"，
   完全猜不到是自己刚才那一下加进去的。新增 `pick_folder`（只开对话框、只回
   路径、不写设置）供 inbox 与落点共用。
2. **inbox 的重叠校验少一个方向**。原来只判"相等 / 在曲库目录内部"，漏了
   "inbox 是曲库目录的**父目录**"——把 `D:\` 当待整理目录，预览会递归扫到整个
   曲库，同一批文件再导一遍。新增 `paths_overlap`（三方向）与 `path_within_roots`
   （只判"在内或相等"，给落点用），四个单测钉住含分隔符边界。


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

### 3.5 整理之后谁需要重扫（两套体系，别混为一谈）

整理完有新文件落到曲库目录里，这件事要通知**两个互不相干的库**：

| 谁 | 谁在管 | 整理后要不要补 |
|---|---|---|
| 应用曲库（`library.sqlite`） | `refresh` 管线 | **不用，已经有了** |
| beets 索引家（`beets-index/library.db`） | 只有 `index_import` | **必须补** |

**应用侧什么都不用做。** `beets::start_apply` 的 `on_exit` 早就在推
`refresh::Trigger::Action`，跑的是完整刷新管线（整库增量扫描）。扫描器的
`unchanged` 预筛按 mtime/size 跳过没变的文件，整库重扫是几十毫秒级；
而且它的对账（`mark_missing`）WHERE 严格限定在**传入 roots 的前缀**内
（`path_lower LIKE ? ESCAPE`），所以"扫哪一段"本身也不会误伤别的目录。

> 我最初写的是"整理后只扫落点（应用侧）"，还打算加 `refresh::spawn_scoped`。
> 这是**多余的**：已有的整库增量既更简单也够快，再加一条收窄分支等于给同一条
> 链子接两个开关。已删除。

**索引家必须补，而且必须只扫落点。** 它是独立的库文件，`apply` 写的是整理家，
不补的话"刚整理完的歌在查询页永远 0 命中"。这里"只扫落点"不是优化，是必需：

| 扫什么 | 批数 | 实测耗时 |
|---|---|---|
| 曲库根全长 1846 首 | 96 | **82 s** |
| 一个落点目录 | 1 | **1.0 s** |

82 s 里几乎全是**进程启动开销**——`index_import` 每批起一个
`python -m beets import`，固定约 0.85 s（实测：1 个已入库文件 0.88 s、
20 个 1.25 s），跟批内文件多少关系很小。所以"整理完顺手整根重扫"是
不能接受的 82 秒，而落点通常只有一两批。

传子目录不会被 `-R` 误跳过：增量记忆是 `statefile: state.pickle` 里的
`taghistory`，它记的是「一个导入任务的那批 paths 元组」，新文件组成新元组。
实测：新建一个目录只扫它 → 索引库 1849 → 1850。

**实现位置**：`start_apply` 的 `on_exit`，在推完 `Trigger::Action` 之后调
`start_index_import(exit_app, dest)`。落点在**发命令前**取（`roots_for_tools`），
因为结束后用户可能已经改掉，补索引就会扫错目录。补索引失败只记 `warn`——
文件已经搬好了，整理结果不受影响，用户还能手动「重建索引」兜底。

已知退化：落点 = 曲库根本身时（用户没单独设落点），新文件散在根下多个新目录，
这时只能整根扫 → 82 s 后台跑。真正的解法是让 `index_import` 能直接传**目录**
（`beet import <dir>` 一个进程走完整棵树），而不是现在这样把文件列表切 96 批。
未做，见 §8。

#### 3.5.1 真机实测（2026-10-30 22:11，走手搓 decisions 绕开联网匹配）

预览要连 MusicBrainz（本机单请求 6 s，一张专辑要发很多次），为了不把验证挂死在
网络上，这一轮直接用 `ipc.beets.apply()` 传手工 decisions（`candidate: {}`，
apply 会 `Item.from_path` 读文件自身标签，合并后非空即过 `blankIdentity` 门）。

结果三条日志/三条事实彼此印证：

```
22:11:57.401  INFO tool-session-reader aiya_lib::beets:
              整理完成：应用曲库交给 refresh(action)，索引家按落点补扫
              dest="D:/CkMsr/Music/_整理测试"
22:11:57.552  INFO library-refresh-action aiya_lib::refresh:
              library refresh trigger="action" scanned=1848 imported=2 changed=true elapsed_ms=151
```

| 断言 | 结果 |
|---|---|
| 文件真的落到配置的子目录 | ✅ `_整理测试/Non-Album/<艺人>/<曲>.mp3` ×2 |
| 应用曲库自己重扫 | ✅ `trigger=action`，`imported=2`，**151 ms** |
| 索引家按落点补扫 | ✅ 索引库 1850 → 1852，2 条带落点路径 |

`imported=2 / 151 ms` 这组数字顺便证实了 §3.5 的判断：**应用侧的重扫根本不需要
优化**——整库 1848 首走一遍 unchanged 预筛只要 151 毫秒（定时刷新那几轮是 90–230 ms）。
把它"收窄成只扫落点"省不下任何可观时间，只会多一条分支。

### 3.6 规则：落点必须落在某个扫描根之内

整理完的文件要是应用扫不到，"存为歌单 / 播放"全落空。所以：

* 落点 = 某个扫描根，**或是它的子目录**（子目录更实用：`曲库/整理/`）；
* 不满足 → `beets_set_target` 直接拒绝，提示「先把它加为扫描目录，或改选曲库内的目录」；
* 判定用 `path_within_roots`（`canon_path` 前缀 + 路径分隔符边界，避免 `D:\Music2`
  误判成 `D:\Music` 的子目录）。

**这条判据有三处消费者，必须同源**（2026-10-30 一次性踩全）：

| 消费者 | 作用 | 漏改的症状 |
|---|---|---|
| `beets_set_target` | 校验用户的选择 | —（最先放宽的一处） |
| `roots_for_tools` → `pick_dest` | 决定下发给工具的 `AIYA_BEETS_DEST` | 落点静默回退成曲库根 |
| `config_view_of` | 决定界面上显示哪个"生效落点" | 界面显示 A、实际写 B |

现在三处都调 `pick_dest`，由 `pick_dest_*` 与 `config_view_shows_subdirectory_target`
两组单测钉住。**教训**：一个"目录是否合法"的判据被复制三遍，放宽时必定漏一处。

### 3.6.1 落点为什么一开始根本没生效（工具侧，最隐蔽的一个）

即使 `AIYA_BEETS_DEST` 正确下发，整理出来的文件仍然全落到 `~/Music/Non-Album/…`。
真机第一次跑整理的落点目录是**空的**，两首歌躺在 `C:/Users/<me>/Music/` 下面。

根因在 `cfg.ensure_config` 的 workbench 分支：

```python
if not path.is_file():            # ← 只在文件不存在时写一次
    path.write_text(render_config(base, dest_root() or scan_root()), ...)
return path
```

这条"用户的家只写一次"是刻意的（他的 `plugins` / `path_formats` 不能被覆盖），
但它把 `directory` 一起带进去了 —— 而 `directory` **不是用户偏好，是应用事实**：
它由曲库根 / 落点决定，用户没有编辑入口。

于是必然的坏结局：用户第一次进设置页时还没有曲库目录 → 生成 config 时省略
`directory` 行（"瞎猜一个目录比不写更糟"，见 cfg.py 头部）→ **此后永远不补** →
`beets move/copy` 用内置默认 `~/Music`。

修法是把它从那条规则里摘出来，只同步这一行：

```python
_DIRECTORY_LINE = re.compile(r"^directory:[^\n]*$", re.MULTILINE)

def _sync_directory(text, root):
    want = f"directory: {yaml_path(root)}"
    if _DIRECTORY_LINE.search(text):
        return _DIRECTORY_LINE.sub(lambda _m: want, text, count=1)  # 函数替换，反斜杠不被当转义
    ...插在 library 行之后...
```

其余行**逐字节保留**。契约测试 `[9]` 守两件事：插入后去掉那一行必须与用户原文
逐字节相同；换落点只改那一行。两个方向都做过负向（不同步 → 红；整份重写 → 红）。

---

## 4. UI 形状：整理库页拆成三个子页

原计划是「查询面板下面再加一张整理卡」。落地时改成**三个子页**（2026-10-30 用户拍板），
因为三件事的心智模型是分开的，挤在一屏里必然互相污染：

| 子页 | 回答的问题 | 读写 | 落地组件 |
|---|---|---|---|
| **安装** | 组件在不在、哪一版、插件能不能干活 | 只读 | `components/beets/InstallPanel.svelte` |
| **索引** | 索引库建了没 → 查 → 挑歌 → 存歌单 | 读 | `components/beets/IndexPanel.svelte` |
| **整理** | 新文件从哪来、落到哪、动不动原文件 | **写** | `components/beets/OrganizePanel.svelte` |

壳（`views/BeetsView.svelte`）只做三件事：标题、tab 条（复用 `content/Toolbar`）、
"组件未就绪"的门。门放在壳里只写一次，两个子页因此可以假设前提成立。

tab **永远可点**，不因为没装而禁用——用户要能看见那一页在讲什么；点进去是
一句人话 + 一个「去安装」按钮，而不是一个点不动的灰块。

### 4.1 整理子页

```
┌ 待整理目录 ──────────────────────────────────────────┐
│ [📥] C:\Users\me\Music\待整理      [打开] [更换] [清除] │
│ 未设置时：说明这里放"还没进曲库的新文件"，给 [选择]      │
├ 整理落点 ───────────────────────────────────────────┤
│ D:\Music\整理                      [选择子目录]        │
│ 或直接用曲库目录：[D:\Music] [E:\Audio\FLAC]           │
├ 整理 ───────────────────────────────────────────────┤
│ [开始整理（预览）]  文件动作：[复制进来 ▾]              │
│                        [应用所选（3 张）]              │
└─────────────────────────────────────────────────────┘
```

1. **预览**（`beets_preview`，源 = 待整理目录递归出的音频文件，自动排除已整理过的）；
2. 每张专辑一张卡：候选下拉（默认项是「跳过这张」）、证据徽标、逐字段 diff、
   逐曲目对齐、`只应用这张`；
3. 执行（`beets_apply`，决策里带 `apply_move` / `apply_copy`），进度走既有 `beets:import` 事件；
4. 完成：宿主触发一次曲库刷新（结果走 `beetsSession.syncVersion`）。

**动作是整批一个下拉**（拷贝 / 移动），不是每张专辑一个。理由是"两套选择并存时
到底哪个生效"会让这张卡变成谜题；需要不同动作时分两次整理，比猜规则便宜。

**默认动作**取「拷贝」而不是「移动」：移动是不可逆的（源文件没了），第一次用的人应该能反悔。
这是安全默认，不是性能选择。

**落点选择**给两条路：曲库目录 chip（落点通常就是某个根）与「选择子目录」（走
`pick_folder`，校验在后端：必须在某个根之内）。


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

### 6.1 实现期撞到的两个"只在预览里坏"的坑

记在这里是因为它们**不报错**，只表现成"卡住"，真机上看不出来：

1. **会话 store 不能在模块加载时订阅事件**。`beets-session` 在静态 import 链上，
   求值早于 `main.ts` 装 mock transport，那一次 `listen` 落到 Tauri 通道、被 `sub()`
   的 catch 吞掉 —— 浏览器预览里点「开始整理」永远停在「匹配中 …」，一条事件都收不到。
   改成第一次发命令时懒订阅（`ensureListening`）。真宿主没这个问题，所以只有预览坏，
   而预览是唯一能快速改版式的地方，坏了就得靠想象。
2. **阶段要先置、终态交给事件**。`preview()` 原来是 `await ipc.beets.preview()` 之后
   才把 phase 设成 `previewing`。真宿主里命令立刻返回，没问题；mock 会把整轮事件在
   命令返回前发完，于是 `done` 先把 phase 归 idle，然后这一行又把它写回 `previewing`
   ——按钮永久锁死。同理 `apply()`。

### 6.2 真机才看得见的两个：忙 ≠ 没装，配置不补就白配

这两个都在真窗口里才暴露，mock 与单测都照不到。

1. **会话忙被报成"未安装"**。`query_status` 也要抢会话槽（它要起一个 beets 进程），
   `probe` 把 `ToolError::Busy` 统一翻成 `E_BEETS_UNREACHABLE` → `found=false` →
   整理库页判定"组件没在"，**自己跳到安装页**。真机表现：预览在联网匹配（MusicBrainz
   单请求 6 s），用户什么都没做，页面自己变成"未安装 beets，点这里安装"。
   修法：`busy_status()` —— `found: true` + `E_BEETS_BUSY` + `library_lock: "held"`，
   `schema_matched` 仍为 false（没拿到报告就谈不上契约匹配，UI 正好该禁用写操作）。
   `query_sidecar` 的签名从 `Result<_, String>` 改回 `Result<_, ToolError>`，
   就是为了让调用方能分开这两件事。
2. **落点配了但不生效**，见 §3.6.1（`config.yaml` 只在首次生成时写，`directory` 永不补）。

第 2 条的代价很实在：界面显示落点 `D:\Music\整理`、实际写 `~/Music/Non-Album/`，
而且**没有任何报错**——`files moved: 2` 看起来完全成功。


---

## 7. 验证计划

标记：**✅ 已验** / ⬜ 未做。分析侧与整理侧的代码都已落地，`npm run verify` 全绿；
整理链路已在真窗口跑到"文件落地 + 应用曲库重扫 + 索引补扫"三条都成立。

### 7.1 单测（工具侧）
* ✅ `render_config(index)` 的 `directory` = 传入的曲库根，不再是 scratch（`config_contract [1]`）；
* ✅ 配置闸：`directory` 不等于曲库根 → 必须红（`[2]` 的「给了曲库根却仍指占位目录」）；
* ✅ `layout.json` 变化 → 成对删四个文件；不变 → 一个文件都不动（`[5b]`）；
* ✅ 落点外科式修改（`[9]`）：给一份带用户注释、自定义 `plugins` 与 `path_formats`
  的 workbench 配置，同步 `directory` 之后**其余行逐字节相同**；换落点只改那一行。
  两个负向都验过（不同步 → 红在"directory 行必须存在"；整份重写 → 红在"逐字节保留"）。

### 7.2 单测（宿主侧）
* ✅ `env_pairs`：`index` 收到 `SCAN_ROOT`，`workbench` 收到 `DEST`
  （`env_separates_library_root_from_destination`）；
* ✅ `beets_set_target`：落点是扫描根子目录 → 接受；假兄弟 `D:\Music2` → 拒绝；
  父目录 `D:\` → 拒绝（`target_accepts_root_and_subdirectory` /
  `target_rejects_outside_and_fake_sibling`）;
* ✅ **落点下发口径与校验同一条**：`pick_dest` 接受子目录、曲库外回退第一个根
  （`pick_dest_accepts_subdirectory` / `pick_dest_falls_back_when_outside`）。
  这条是补一个静默 bug——`beets_set_target` 放宽到子目录后，`roots_for_tools`
  还写着严格相等，于是"设置里存着 `D:\Music\整理`"会被静默回退成 `D:\Music`；
* ✅ inbox 三方向重叠：`parent_of_root_conflicts`。

### 7.3 e2e
* ✅ 三个子页可切换、各自只放自己的东西（`docs/49：整理库拆成 安装/索引/整理`）；
* ✅ 整理链路：未设待整理目录 → 「开始整理」禁用 → 选目录（`pick_folder`）→ 预览出两张
  专辑卡（高置信默认选中、低置信默认跳过）→ 应用所选 → 汇总「已应用 1 张 · 跳过 1 张」;
* ✅ 真窗口全链路（分析侧，CDP 驱动）：建索引 1849 条 / 重复入库幂等 / 查询命中 1849 /
  全选 197 首且跳过 3 首未入库 / 存为歌单后点开有曲目 / console 零告警；
* ✅ 真窗口整理链路（CDP + 手搓 decisions 绕开联网匹配）：文件落到配置的子目录 ×2 /
  应用曲库 `trigger=action imported=2 elapsed_ms=151` / 索引库 1850→1852（详见 §3.5.1）。

### 7.4 负向验证（必须）

不信"绿"，只信"破坏后必须红"。

1. ✅ `layout` 比对改恒等（永判"相同"）→ `[5b]` 红在 `口径变了必须重建：旧库要删`；
   **反向**（永判"变了"）也验了 → 红在 `state.pickle` 的"同口径不该重建"。
   两向都红 ⇒ 这条断言不恒绿。撤销后 ALL PASS。
2. ✅ 落点子目录判定改成裸 `starts_with`（不加分隔符边界）→ 假兄弟测试必须红。
   实测：`target_rejects_outside_and_fake_sibling` 红在
   `assertion failed: !path_within_roots("D:\\Music2", ...)`；撤销后 20 个 test
   result 全 ok。
3. ✅ 索引家 `directory` 退回 scratch → 配置闸必须红（`[2]` 的专项负向，已绿）。
4. ✅ `pick_dest` 的判据退回严格相等 → `pick_dest_accepts_subdirectory` 必须红。
   实测红在 `left: Some("D:\\Music") / right: Some("D:\\Music\\整理")` —— 正是
   "子目录落点被静默回退成曲库根"这个症状本身；撤销后回绿。
5. ✅ **`directory` 同步两个方向都验**（契约测试 `[9]`）：
   * 不同步（改回"文件已存在就什么都不做"）→ 红在「`directory: 'D:/Music/整理'` 必须存在」；
   * 整份重写（丢掉用户的行）→ 红在「除 directory 外必须逐字节保留」。
   两向都红，说明这条测试守的是"只改一行"，不是"文件里恰好有这行"。

探针脚本（判断 beets 行为用，不入仓）：`%TEMP%\path-probe2.py`、`path-probe3.py`。

---

## 8. 没验到的

1. **联网匹配这一段没验通**：预览要对 MusicBrainz 发很多请求（本机单请求 6.2 s），
   真机上挂了 3 分钟还停在「匹配中 0/1」，只能取消。所以 §3.5.1 的验证是绕开预览、
   直接用 `ipc.beets.apply()` 传手工 decisions 做的 —— **"预览出的候选 → 用户挑 →
   应用"这条完整产品链路，还没在真机上走通过一次**。编辑器/候选卡/证据徽标的渲染
   只在 mock 的 e2e 里验过。
2. **`index_import` 的分批方式是下一个瓶颈**：它把文件列表切 96 批、每批一个进程，
   整根 82 s 几乎全是启动开销。正解是让这条命令能直接收**目录**（`beet import <dir>`
   一个进程走完整棵树，`taghistory` 按专辑分组命中，已导入的照旧跳过）。改之前要
   先实测 `taghistory` 在"传目录"和"传文件列表"两种形态下的命中是否一致——
   改错的表现是"整根重扫永远被跳过，新文件进不来"，比慢更糟。
3. "路径跟随"：beets move 改了文件名 → 应用库里旧 path 失效，靠扫描段重建。
   若撞上"同一位艺人同专辑换了曲序"，可能被当成新曲目。（本轮 copy 模式验过：
   `trigger=action` 把 2 首新文件按 151 ms 增量补进了应用曲库。）
4. 落点在 NAS / 别的盘时 `copy` 的耗时与中断恢复；
5. 无标签 wav 的撞名概率与用户实际接受度；
6. 索引家的库**只读到 API 层**：`Item.path` 是绝对、`PathQuery` 两口径都命中，已在
   1849 首真机上验过（docs/49 §1）；但**绕过 API 直读 sqlite 的代码**（若将来加）仍会
   撞上相对路径——见 §1 的适用边界。顺带一条实测印证：索引库 `items.path` 列里存的
   确实是**相对**路径（`_整理测试/Non-Album/…`），只有走 API 才拿到绝对路径；
7. 索引家「曲库根换了」的**自动重建**只在契约测试里验过；真机上从 A 目录换到 B 目录
   还没跑过一次（换目录要重新扫 1849 首，约 86 秒）。
