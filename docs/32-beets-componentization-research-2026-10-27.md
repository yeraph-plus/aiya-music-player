# beets 组件化分发 + 变更刷新调研

> 状态：**调研结论，待拍板**。日期：2026-10-27。
> 触发：把 beets 从"内置运行时"改成"当前播放器内集成、但独立运行的整理工具"，
> 用类似插件商店的方式按需安装；放弃"随包内置 venv/embeddable"的旧决策。
> 关联：docs/31（验收计划）§5 与 §10 的一部分被本文取代；docs/29 §15（批 C）的分发决策被本文推翻。
>
> **⚠ 本文的分发方案已被 docs/33 取代**（2026-10-27 晚，用户要求"beets 与播放器分两侧维护"）：
> §5（五方案对比）、§7（组件注册表 + 双后端推荐）由
> `docs/33-beets-split-architecture-2026-10-27.md` 取代。
> **仍然有效并继续被引用**：§2 现状事实、§3 的体积实测（venv 89 MB vs
> 内置 112 MB，numpy 55 MB 不可避免）、§4 的刷新成本实测（全量 dump 1 s、
> 增量重扫 60 ms、beets items 无 `updated_at`）、§6 的两条设计结论
> （replaygain 读写拆分、web 入口建议删）。

---

## 0. 一句话结论

**可以直接做，而且比现在的形态更好——但"省体积"不是理由（实测省不到 25 MB），真正的理由是"按需装、独立升级、能力可选"。**

推荐形态：**组件注册表（清单驱动）+ 双安装后端（系统 Python 建 venv 为默认，uv 为可选自动挡）**，
默认不随包；刷新靠**"操作后触发 + 定时增量扫描 + 全量 dump 对账"**，不上 FS watcher（成本数据见 §5.4）。

---

## 1. 先把定位说清楚（它决定了后面每一个取舍）

现状的实际形态是：beets 被当成**应用的运行时依赖**——随包 112 MB、内置清单钉死六个插件、
`status` 一挂就整个维护区不可用。

你要的形态是：beets 是一个**外挂的整理工具**，可以装、可以卸、可以单独升级，
应用只做三件事：

1. **发现**它能用（版本、能力、健康）
2. **驱动**它干活（维护命令，参数固定）
3. **跟随**它的结果（路径/元数据变了，应用内立刻是新的）

这三条里，第 3 条现在是**完全缺失**的——全仓没有任何文件监听、没有窗口焦点刷新、
没有定时重扫，只有用户手点"同步"或"扫描"。这才是"变更不会自动刷新"的根因，
和 beets 怎么装**没有任何关系**。这一点必须先分开，否则会误以为换成组件化就自动解决了。

---

## 2. 七个决定性事实（全部实测，会改变判断）

| # | 事实 | 证据 | 它推翻了什么 |
|---|---|---|---|
| **F1** | **本机 venv = 89 MB，内置运行时 = 112 MB，只差 23 MB** | `du -sh tools/beets-venv` → 89M；`du -sh tools/beets-runtime` → 112M | 推翻"不内置就能省体积"的直觉。差额只有解释器+DLL+stdlib 那部分 |
| **F2** | **numpy 占 55 MB（34 MB + 21 MB numpy.libs），两个形态都跑不掉** | 两处 site-packages 的 numpy 目录一致；`beets` 元数据里 numpy 是**核心依赖**（`numpy>=2.3.5; python_version>='3.13'`） | 推翻"精简依赖"的幻想。别在依赖裁剪上花时间 |
| **F3** | 内置运行时里另有 **13 MB 是无用的 pip** | `du -sh .../site-packages/pip` → 13M | 唯一真的能裁掉的大件 |
| **F4** | **beets 要求 Python `>=3.10,<3.15`** | `importlib.metadata.metadata('beets')['Requires-Python']` | "要求用户装 Python"必须处理版本窗口：装了 3.15 的用户会装不上，且报错难懂 |
| **F5** | 本机 Python 探测就是活教材：`python` = **3.13.12**（WorkBuddy 自带的），`C:\Python314` 存在，`WindowsApps\python.exe` 是 **Store 别名桩** | `where python` 四条结果 | 裸 `python` 不可信。必须走 `py -0` / 显式路径 / 注册表 |
| **F6** | **全量 dump 1845 首 ≈ 1 秒**（进程启动 0.79 s，3 条数据 1.5 KB / 4 行） | `time (echo dump_items \| python sidecar.py)` → real 0.787s | 推翻"需要变更水位表"的想法：全量 dump 比维护水位更简单也更快 |
| **F7** | **beets 的 items 表 97 列，没有 `updated_at`** | `pragma table_info(items)` | 无法按时间戳取增量；「全量 dump + 差分入库」是唯一可靠解 |

补充两条已知数据（来自 docs/29 批 B 实测，本文直接复用）：

- **增量重扫 1846 首 = 60 ms**（无封面路径）。这是"定时重扫"可行的全部依据。
- 扫描器的 `ON CONFLICT(path_lower) DO UPDATE` **不写 `beets_item_id` / `gain_db`**（实测 SQL 里没有这两列）→ 扫描不会冲掉同步结果，两条管线可以任意顺序跑。

---

## 3. 分发形态：五个方案对比

| 方案 | 安装包增量 | 磁盘占用 | 离线首用 | 升级 | 失败模式 |
|---|---|---|---|---|---|
| **O1 现状：内置 embeddable** | 112 MB | 112 MB | ✅ | 重发安装包 | 版本钉死、插件钉死、无 beets 需求用户白付 112 MB |
| **O2 用户自备 Python + 应用建 venv** | 0 | 89 MB | ✅（若已装） | 重装 venv | 无 Python / 3.15 / Store 别名 / 权限 / 公司策略 |
| **O3 随包 uv + uv 管 Python** | ~31 MB | 63 MB（Python）+ 89 MB（venv） | ❌（要下载） | `uv pip install -U` | 首次要联网；uv 自己的信任问题 |
| **O4 纯外置（用户 pipx 装 beets）** | 0 | 用户自管 | ✅ | 用户自管 | 版本漂移 → sidecar 进程内 API 直接炸；路径不可知 |
| **O5 组件注册表（清单 + 脚本安装）** | 0（可选离线包） | 同 O2/O3 | 取决于后端 | 清单升版本 | 见下 |

**关键判读**：

- O2 和 O3 的**磁盘占用几乎一样**，因为大头是 numpy。差别只在"谁来提供解释器"。
- O3 的 31 MB 换来的是"用户不用自己装 Python"，这是**唯一**它能做的事；但它把"离线可用"换掉了。
- O4 不能选：sidecar 是**进程内 `import beets`**，不是调 CLI。版本必须是应用验证过的那个，
  否则 `Library` / `Item` / `autotag` 的签名一变就崩在运行时。这条是硬约束，写在 docs/29 §16 里。
- **O5 不是第六种安装方式，它是 O2/O3 的元数据层**——清单描述"装什么、怎么验、装哪、失败怎么退"，
  后端可以是系统 Python，也可以是 uv。这才是"插件商店"的正确理解。

---

## 4. 推荐方案：组件注册表 + 双后端

### 4.1 形态

```
{app_data}/components/
  registry.json          已安装组件与状态（版本/extras/健康/安装时间）
  beets-venv/            该组件的隔离环境（默认后端产物）
config.yaml.tmpl         组件声明的配置片段（首次安装时渲染）
```

默认**不随包任何 Python 产物**。用户第一次点"安装 beets 整理工具"时：

1. 探测可用解释器（`AIYA_BEETS_PYTHON` → `py -0` 列出的候选 → `C:\Python3xx` → PATH）
2. 在 `components/beets-venv` 建 venv（**本机创建**，不是搬运）
3. 按清单装包（`pip install -r lock`，或 `uv pip sync`）
4. 跑健康检查（见 4.5）
5. 写 registry，推事件 → UI 立刻可用，**不重启**

> 这里有一条容易被误读的历史：docs/29 §15 说"venv 不能随包分发"是**对的**（`pyvenv.cfg` 的
> `home` 是硬指针，搬到没装同版本 Python 的机器上报 `did not find executable`）。
> 但它**不否定**"在目标机器上用用户的 Python 建 venv"。批 C 当时的困境是"想零依赖开箱即用"，
> 一旦接受"首次安装需要环境"，venv 就是正确形态。**这两个结论不矛盾，别被旧文档带偏。**

### 4.2 为什么还要 uv（它不是必需品，是"没有 Python 时的救生圈"）

| 维度 | 系统 Python 建 venv | 随包 uv |
|---|---|---|
| 安装包增量 | 0 | ~31 MB（uv.exe，静态 Rust，Apache-2.0/MIT） |
| 用户需要装 Python | ✅ 必须 | ❌ `uv python install 3.14` 自己下（21 MiB 压缩 → 63 MB 解出） |
| 版本窗口控制 | 难（用户可能只有 3.15） | 易（直接钉 3.14） |
| 装包速度 | pip 单线程 | 并行 + 全局缓存，实测快 3–4× |
| 国内镜像 | `-i` 参数 | `UV_DEFAULT_INDEX` 环境变量 |
| 离线 | wheelhouse（`--find-links`） | `--offline` + 缓存，或 wheelhouse |

**建议**：uv **不随包**，做成"安装向导里的第二个选项"——用户机器上找不到合规 Python 时，
应用提示"要不要让应用自己下一个 Python（约 21 MB 下载）"，点同意才去下载 uv。
这样默认体积是 0，需要的人也能一键解决，不必逼用户去 python.org 手动装。

### 4.3 组件清单（manifest）草案

```json
{
  "id": "beets",
  "name": "beets 曲库整理",
  "version": "2.14.1",
  "kind": "python-tool",
  "requires": { "python": ">=3.10,<3.15", "diskMb": 90, "network": true },
  "install": {
    "backends": ["venv", "uv"],
    "packages": ["beets==2.14.1"],
    "extras": {
      "lyrics":  ["beautifulsoup4", "langdetect"],
      "coverart":["pillow"],
      "chroma":  ["pyacoustid"]
    },
    "index": "https://pypi.org/simple"
  },
  "health": {
    "cmd": "status",
    "expect": { "protocolVersion": 3, "beetsVersion": ">=2.14,<2.15" },
    "plugins": ["duplicates", "scrub", "fromfilename"]
  },
  "config": { "fragment": "beets.config.tmpl" },
  "uninstall": { "paths": ["components/beets-venv", "beets"] }
}
```

三条设计原则：

1. **版本是清单里钉死的**，不是"装最新的"。sidecar 是进程内 API 调用，升版本必须走清单，
   并且 `protocolVersion` 不匹配时**拒绝使用**（现状宿主只读不校验，是 docs/31 §6.3 的缺口）。
2. **extras 是组件的能力开关**，不是一个字符串数组。选 `lyrics` 才装 bs4；
   这也顺手解决 docs/31 的 P0-1（现在 `lyrics` 在清单里但没依赖）。
3. **`uninstall.paths` 必须完整**：装了什么、删什么，写在清单里，不散落在代码里。

### 4.4 安装器契约（沿用现有形态，不新造）

现有 `sidecar/beets_install.py`（145 行）已经是"脚本 + 行帧 JSON 事件流"的形态，
宿主 `start_install` 转发到 `beets:install` 通道。**组件化不需要改这套协议**，只需要：

- 输入从"硬编码 requirements"变成"清单里的 packages + 选中的 extras"
- 输出事件里带 `componentId`（未来多个组件时区分）
- 结束前必须跑一次健康检查，把结果放进 `finished` 事件（现状已经这么做了）

### 4.5 健康检查：替代现在那张"愿望清单"

现状 `_status_payload().plugins` 取自 `config['plugins']`，是**用户想要什么**，不是**实际加载成功什么**。
实测：配 `replaygain lyrics duplicates scrub`，`load_plugins()` 返回成功，但 `lyrics` 实际抛
`PluginImportError`（缺 bs4）——UI 上六项全绿，功能一半是死的。

**改法**：健康检查逐个 `importlib.import_module("beetsplug." + name)` 并报告三态：

```
plugins: [
  { "name": "duplicates",   "state": "ok" },
  { "name": "lyrics",       "state": "missing-dep", "reason": "No module named 'bs4'" },
  { "name": "replaygain",   "state": "missing-binary", "reason": "ffmpeg not found" },
  { "name": "smartplaylist","state": "ok-but-unconfigured" }
]
```

`ok-but-unconfigured` 这一态是必须的：`fromfilename` / `smartplaylist` 能加载但缺子配置，
跑起来什么都不做。区分"能加载"和"会干活"，否则又是一轮"看着绿、实际红"。

### 4.6 版本锁 / 回滚 / 卸载

- **锁**：`beets-requirements.lock` 已有（21 包精确版本），清单引用它，不重复描述。
- **回滚**：升级前把 `beets-venv` 改名为 `beets-venv.prev`，新环境健康检查失败则换回。
  一次只保留上一版（磁盘 +89 MB 的代价，可接受；也可在切换成功后删）。
- **卸载**：按清单 `uninstall.paths` 删；**默认不删 `components/beets/`（用户库）**，单独确认。

### 4.7 extras 与"外部二进制"的定位（重要）

插件分两类，不能混：

| 类型 | 例子 | 处置 |
|---|---|---|
| 纯 Python 依赖 | `bs4`、`pillow`、`pyacoustid` | 进 extras，pip 装 |
| 需要外部二进制 | `replaygain`→ffmpeg（~80 MB）、`chroma`→fpcalc | **不随包**，检测到就用，检测不到明确告知并禁用 |

**replaygain 要拆成两件事**（这是本文最实用的一条建议）：

- **读**：扫描器直接读文件里已有的 `REPLAYGAIN_TRACK_GAIN` 标签 → 进 `gain_db`，**零依赖**。
  多数有整理习惯的用户，标签早就被 foobar/Mp3tag 打好了。
- **算**：用 ffmpeg 分析音频算增益 → 这是重活，做成"可选组件"，检测不到就不提供。

现状是把"读"和"算"都绑在 beets 的 replaygain 插件上，而插件因为缺 ffmpeg 完全不加载，
结果连用户**已经有的**增益标签也用不上。这是纯粹的设计浪费。

> 待验证（M1 的一项）：symphonia 0.5.5 默认不映射 ID3 的 `TXXX:replaygain_track_gain`，
> 需要自己读 tag frame。flac 的 Vorbis comment 更容易。这条不确定，别写进承诺。

---

## 5. 变更刷新（第二个真问题）

### 5.1 三个变更来源，机制完全不同

| 来源 | 例子 | 谁负责 |
|---|---|---|
| **S1 应用内操作** | adopt / update / move / apply 改完文件 | 应用自己知道，操作后立刻刷新 |
| **S2 外部工具改文件** | Mp3tag 改标签、foobar 改名、别的机器同步 | 只能**检测**：mtime/size 变了 |
| **S3 beets 侧路径变化** | beets 自己 move 过，或外部 beets CLI 动过 | 靠 `beets_item_id` 双键对账（已有） |

现状：S1 只覆盖了 adopt/move（`run_sync`），**update 和 apply 之后没有对账**；
S2 完全没有机制；S3 有逻辑但只在你手点同步时跑。

### 5.2 刷新管线（顺序有讲究）

```
触发
 └─ A. 增量扫描（scan_roots_ext：stat → mtime/size 预筛 → 只解析变了的）
     └─ B. 全量 dump + sync_items（beets → 自有：补 item_id、跟随路径、搬 gain_db）
         └─ C. 列表版本 bump（list.version++）→ 只有可见页重取
```

**为什么 A 在 B 前面**：

- A 负责"磁盘是事实"（标签、存在性、时长）；
- B 负责"beets 是事实"（item id、beets 侧的路径）。
- 若 B 先跑，sync 会拿 beets 里的旧标签写进库，紧接着 A 又按磁盘改一遍，**两次 bump + 一次回退**。
- 反过来的代价：A 先把 `path` 更新到磁盘现状，B 再按 `path` 命中就不需要 relocate 分支——顺带少走一次路径迁移。

**为什么 C 必须单独一步**：`scan` 自己会 bump 列表版本，但 `sync` 改的是同一批行，
没有统一的"结束信号"就会看到列表闪两次（批 D 的 e2e 已经踩过类似的心跳问题）。

### 5.3 触发策略（三条，全部低成本）

| 触发器 | 时机 | 代价（实测） | 覆盖 |
|---|---|---|---|
| 操作后 | beets 命令 done 事件 | dump 1 s + sync <1 s | S1、S3 |
| **定时增量** | 空闲时每 **30–60 s** 一次 | 全量 stat 1846 首 = **60 ms** → CPU 占用 <0.2% | S2 |
| 窗口聚焦 | `WindowEvent::Focused(true)` | 同上 | S2（用户刚在外部工具改完切回来） |

补充：**扫描进行中不要叠加**——用一个 `refresh_inflight` 标志合并，宁可漏一次也不要并发跑两条扫描
（批 B 的入库线程是单写者，两条扫描同时进来会互相抢事务）。

### 5.4 为什么不上一开始就上 FS watcher

- 17 GB / 1846 个文件的删除+重建场景（beets move 会**先搬走再清理空目录**）会产生大量事件，
  需要 debounce + 事件合并 + 忽略自身操作（否则自己 move 完触发自己重扫，形成回环）。
- 而**轮询 60 ms 就能拿到同样的结果**。以 30 s 为周期，成本可以忽略。
- 结论：**P1 用轮询，P2 再考虑 watcher**（watcher 的价值是"即时"而不是"能发现"，
  对音乐库这种低频变更，1 分钟内发现完全够用）。

> 顺带说明：`notify` crate 目前**不在任何 Cargo.toml 里**，全仓没有任何文件监听代码。
> 所以这不是"改造 watcher"，是"从零加 watcher"。

### 5.5 并发与安全

- **写者互斥**：扫描（自有库）与 beets 维护（beets 库）写的是两个库，但 `sync` 会串起来 →
  用一把应用级 `maintenance_lock`，同一时刻只允许一个"会改变库的作业"。
- **播放不受影响**：beets 只读写 SQLite 和文件，不碰音频线程；但 `move` 会移动**正在播放的文件**，
  播放器读的是已打开的句柄——Windows 上会因文件被占用而 move 失败。**必须在 move 前检查当前播放路径**
  （这是 docs/31 没覆盖的一个真场景，建议进 A5）。
- **取消**：轮询触发的刷新要可取消（复用扫描的 `should_cancel`），否则大库变更时会拖住。

### 5.6 已知边界（写清楚，别到时候当 bug 报）

| 边界 | 现象 | 处置 |
|---|---|---|
| 内容变了但 mtime 没变（`copy2` 保留时间戳） | 增量扫描发现不了 | 提供"强制重扫"；或在设置里说明 |
| 跨盘移动 | path 变了、item_id 不变 → relocate 命中 | 已支持 |
| beets 侧删了条目但文件还在 | 自有库仍在（online），`beets_item_id` 置空 | 符合预期，不判 orphan |
| 两个根下同名文件 | `path_lower` 唯一，不会冲突 | 已支持（多根） |

---

## 6. 对现有资产的处置

| 资产 | 处置 | 说明 |
|---|---|---|
| `src-tauri/src/beets.rs` 探测链（`interpreter_candidates` / `probe` / `beets_paths`） | **保留** | 已经是多候选形态，加一个"组件目录"候选即可 |
| `sidecar/beets_install.py` | **改造**（不是重写） | 参数从硬编码 requirements 换成清单 §4.4 |
| `beets_install` 命令 + `beets:install` 事件 + UI 进度条 | **保留** | 通道与 UI 都够用 |
| `tools/pack-beets-runtime.mjs` + `tools/beets-runtime/`（112 MB） | **废弃** | 改为可选的离线包（`pack:offline`，只在"给无网用户"时构建） |
| `tools/beets-requirements.lock` | **保留** | 变成清单引用的版本锁 |
| `FIRST_WAVE_PLUGINS` 硬编码 | **改造** | 变成清单里的 `health.plugins` + config 模板 |
| `beets_web_open`（spawn `beet web`） | **删掉** | 实测 `beet web` → `unknown command 'web'`（缺 flask，且 web 在 2.14 是插件），现在点了必失败且无提示 |
| `beets_sync.rs`（618 行，双键对账） | **保留** | 这是 S1/S3 的全部价值，别动 |
| 扫描器 `upsert` | **保留** | 实测不碰 `beets_item_id`/`gain_db`，与 sync 可以任意顺序 |
| 新增：组件注册表 + 健康检查 + 刷新调度 | **新增** | 约 3 个模块：`components.rs`、`refresh.rs`、sidecar `health` 命令 |

---

## 7. 被推翻的决策（要同步改旧文档）

| 旧决策 | 出处 | 新结论 |
|---|---|---|
| 预构建 venv 随包分发 | docs/29 §5 路线 B | 已实测不成立（批 C），保留 |
| 改走 embeddable 随包 112 MB | docs/29 §15 | **推翻**：改为清单驱动按需安装，112 MB 只在可选离线包里 |
| 六个插件写死在 `FIRST_WAVE_PLUGINS` | `beets.rs:129` | **推翻**：改为清单 `extras` + `health.plugins` |
| docs/31 §5 的 10 项"内置包验收" | docs/31 | **降级**：只在构建可选离线包时适用（C1/C2/C5/C7 仍要） |
| docs/31 §10 三问（ffmpeg/lyrics/web） | docs/31 | **合并为一问**：哪些进默认、哪些进 extras、哪些不做（见 §10） |

---

## 8. 风险清单（按"会不会真的挡住用户"排序）

| 风险 | 现实概率 | 缓解 |
|---|---|---|
| 用户没有 Python | 中 | 安装向导给 uv 自动挡（§4.2）；或离线包 |
| 用户只有 Python 3.15+ | 中低 | 探测时按 `<3.15` 过滤并明确提示；uv 挡可装 3.14 |
| 企业环境禁装 / 杀软拦 pip | 中低 | 离线 wheelhouse + 明确错误文案；不静默失败 |
| 首次安装要联网且慢 | 高 | 显示真实进度（已有事件流）；支持镜像 |
| 用户以为"点了就装好"但 venv 建失败 | 中 | 健康检查必须跑，失败要显示原因（不是"未知错误"） |
| 升级 beets 后 sidecar 崩 | 中 | `protocolVersion` 硬校验 + 版本窗口 |
| 磁盘残留（89 MB × N 个旧版本） | 低 | 回滚只留一版，切换成功后删旧的 |
| 轮询刷新与用户操作打架 | 低 | `refresh_inflight` 合并 + 焦点触发代替高频轮询 |

---

## 9. 分阶段落地

**M1 组件注册表骨架（不动 beets 逻辑）**
清单文件 + registry.json 读写 + `component_install/uninstall/health` 三条命令 + UI 卡片
验收：能装/能卸/能报三态健康；旧的内置运行时仍可用（双轨并存，不破坏现状）

**M2 健康检查 + 清单化插件**
sidecar 加 `health` 命令（逐插件三态）；`FIRST_WAVE_PLUGINS` → 清单；extras 可选装
验收：`lyrics` 缺依赖时 UI 明确显示"缺 bs4，点此补装"，不再假绿

**M3 刷新管线**
`refresh.rs`：扫描 → dump+sync → bump 一条链；操作后/定时/聚焦三个触发器；`maintenance_lock`
验收：外部改 10 个文件标签，30 s 内应用内可见；改 10 个文件名，30 s 内播放不报错

**M4 收口**
删 `beets_web_open`；内置运行时降级为可选离线包；`protocolVersion` 硬校验；README/docs 同步
验收：全量 verify 绿；无网环境用离线包也能装

---

## 10. 待拍板（我给了倾向，你改）

1. **默认后端**：系统 Python 建 venv（0 体积）**+ 找不到时引导装 uv（31 MB）**。倾向这个。
   备选：直接随包 uv（省事，但白送 31 MB）。
2. **默认插件集**：`duplicates / scrub / fromfilename / smartplaylist`（纯本地、能干活）。
   `replaygain` 从默认集拿掉，改为"读标签内置 + 算增益可选"。倾向这个。
3. **`beets_web_open` 删还是修**：倾向**删**（flask 只为看库不值得，应用内维护卡片已覆盖）。
4. **离线包做不做**：倾向**做但后置**（M4），构建脚本保留，不进常规安装包。
5. **刷新周期**：倾向 30 s 定时 + 聚焦触发；不做 FS watcher（P2 再说）。

---

## 附：本文关键数字的复现命令

```bash
du -sh tools/beets-venv tools/beets-runtime                 # 89M / 112M
du -sh tools/beets-runtime/Lib/site-packages/pip            # 13M（唯一可裁大件）
python -c "import importlib.metadata as m;print(m.metadata('beets')['Requires-Python'])"  # <3.15,>=3.10
time (echo '{"id":1,"cmd":"dump_items"}' | tools/beets-venv/Scripts/python.exe sidecar/beets_sidecar.py > /dev/null)  # ~0.8s
where python                                                # Store 别名陷阱实证
grep -rn "notify" Cargo.toml crates/*/Cargo.toml src-tauri/Cargo.toml   # 空：无 FS watcher
```
