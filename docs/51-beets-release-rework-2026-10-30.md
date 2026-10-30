# beets 侧发布与安装重构（2026-10-30）

本轮四件事，全部围绕「组件怎么装、装什么、怎么发」：

1. beets 的打包流程从播放器 release 里独立出来，手动触发，单独 tag；
2. 免安装（免配置）包改用 uv 打，脚本独立，预置插件读同一份白名单；
3. `install.py` 改入参模式：给插件名，脚本自己按内置表拼依赖命令；插件表落成 JSON；
4. 用户机只有两条路：下免配置包，或「检测到 Python → 勾插件 → 装」；补 `uninstall.py`。

先给事实，再给设计，最后是要拍板的地方。

---

## 0. 事实核查（都是本轮实测，不是推断）

| 事实 | 证据 |
|---|---|
| `build_bundle.py` 硬编码 `VERSION = "0.1.0"`，包内 `__version__` 是 `0.2.0` | 产物 `dist/bundle/manifest.json` 写 `"version": "0.1.0"`；`aiya_beets/__init__.py:7` 写 `0.2.0` |
| 现有 bundle 里**一个插件依赖都没有** | `dist/bundle/Lib/site-packages/` 无 `bs4` / `beautifulsoup4` / `acoustid` |
| 现有 `plugins: xtractor` 是假开 | `docs/50`：`PluginImportError: Could not import plugin xtractor`，退出码 0 所以一直没看见 |
| 播放器与 beets 抢 `releases/latest` | `release.yml` 注释自己写的：单独发 beets 会让 `latest` 指过去，所以两边都得带资产 |
| **A 轨道（`.pyz`）在真机上根本装不成** | 实测 `python aiya-beets-installer.pyz install --target …` → `缺少依赖清单：…/beets-tool/dist/requirements.lock`。`Path(__file__).parent/'data'` 在 zip 里 `is_file()` 恒 False，回落到仓库根的 lock（用户机器上没有），然后退出 2。**路径 B 的载体本身是坏的** |
| 仓库零 release | `api.github.com/.../releases` → `[]` |
| **uv 能对 embeddable 解释器装** | `uv 0.13.6`，`uv pip install --python <embed>/python.exe --target <site> -r requirements.lock` 成功；装出 `numpy-2.5.4` 的 wheel tag 是 `cp314-cp314-win_amd64` —— 按 **embeddable 的 3.14** 解析，不是本机 3.13 |
| 装出来的包能被 embeddable 导入 | `<embed>/python.exe -c "import beets"` → `beets 2.14.1` |
| GitHub release 下载在本机被挡 | `curl` → `CONNECT tunnel failed, response 502`；uv 改从 PyPI 装（`uv` 在 PyPI 有 win_amd64 wheel） |
| `find_system_python` 只试 `python` / `python3` | `beets.rs:932`；Windows 上更可靠的是 `py -0p`（本机 `py -0p` 正常列出 `3.14 * C:\Python314\python.exe`） |

结论：**打包链路的可复现性现在是假的**（版本号对不上、插件没预置、CI 只在打 tag 时才知道坏没坏）；安装链路的能力边界也是假的（config 写着的插件装不上）。

---

## 1. 单一事实源：`plugins.json`

现在插件信息散在四处：`plugins.py` 的 `MARKET_CATALOG`（12 条，带 pypi 包名）、`DEFAULT_PLUGINS`（6 条）、`INDEX_PLUGINS`（3 条，带 forced/writes）、`env.REQUIRED_EXTRAS`（1 条，bs4 的说明）。`install.py` 完全不知道插件，`build_bundle.py` 也不知道。

改成一份 JSON，五个消费方都读它：

```
beets-tool/src/aiya_beets/data/plugins.json
   ├─ plugins.py        → MARKET_CATALOG / DEFAULT_PLUGINS / INDEX_PLUGINS 由它派生
   ├─ install.py        → --plugins a,b 拼 pip 命令
   ├─ uninstall.py      → 列出"装过什么"以便报告
   ├─ build_bundle.py   → bundle:true 的包预置进免配置包
   └─ src-tauri/build.rs → include_str! 进二进制，UI 没装组件时也能勾选
```

Rust 侧这份不是抄一份，是编译期 `include_str!("../../beets-tool/src/aiya_beets/data/plugins.json")` + `rerun-if-changed`。**五个消费方读同一个字节序列**，改一处全生效。

### schema（v1）

```jsonc
{
  "schema": 1,
  "beets": { "min": "2.14.0", "max": "3.0.0" },
  "profiles": {
    // 首次生成 config.yaml 时启用哪些（workbench = 用户整理家）
    "workbench": ["replaygain", "lyrics", "fromfilename", "duplicates", "scrub", "smartplaylist"],
    // 索引家白名单：显式列举，不由布尔推导（"默认开什么"是产品决策）
    "index": ["xtractor"]
  },
  "plugins": [
    {
      "name": "lyrics",
      "title": "歌词抓取",
      "note": "抓取歌词，LRC 字段可直供",
      "builtin": true,                  // 插件随 beets 提供
      "packages": ["beautifulsoup4>=4.12,<5"],   // pip 目标（插件包或依赖）
      "imports": ["bs4"],               // 三态自检用的模块名
      "exe": [],                        // 外部二进制（只提示，不代装）
      "writes": false,                  // 会写音频文件 → 索引家结构性拒绝
      "network": false,
      "indexForced": {},                // 仅索引家插件：渲染时强制写入的键
      "bundle": true                    // 免配置包预置
    }
  ]
}
```

为什么用 array 而不是 object：UI 展示顺序需要稳定，object 无序（解析器恰好保序不算契约）。

为什么没有条目级的 `default` 键：「默认开哪些」是**产品决策、按家不同**（用户整理家 6 个、
索引家 1 个），只能在 `profiles` 里表达。初版我在条目上加了 `default: true`，结果和
`profiles.workbench` 打架 —— 同一件事两个地方写，删掉条目级那个，`ui_catalog()` 按
profile 集合注入 `default` 字段。

为什么 `packages` 可以带约束：`requirements.lock` 只 pin beets 及其传递依赖；插件依赖是"用户勾了才装"的可选集，用范围约束 + CI 冻结记录（见 §3.3）比维护第二份 lock 现实。

### 迁移

`plugins.py` 的三个常量改为从 JSON 构造，行为不变（同名字段），契约测试加三条：
- 每个 `name` 唯一；
- `profiles.index` 里的每个名字都在表里，且 `writes == false`（结构上拒绝写盘插件进索引家，这是 docs/45 §6.2 那条纪律的延续）；
- 每个 `imports` 里的模块在装了 `packages` 之后真的存在（CI 里跑，见 §3.3）。

---

## 2. `install.py` → 入参模式 + `uninstall`

### 2.1 新签名

```
python aiya-beets-installer.pyz install   --target DIR [--plugins a,b] [--requirements F] [--from-wheels D] [--dry-run]
python aiya-beets-installer.pyz uninstall --target DIR [--dry-run]
```

`--plugins` 的语义：
- 省略 / 空 → 只装 beets 基础（= 现在行为，向后兼容）
- `a,b` → 基础 + 这些插件的 `packages`（去重后一次 pip 命令装完，不拆多次解析）
- `all` → 表里所有不需要外部二进制的插件（UI 的「全选」）
- 未知名字 → **报错退出**（`2`），不静默跳过。静默跳过会让用户"勾了 A 装了 B"，比报错难查十倍
- `--dry-run` → 只打印解析出的包清单，不落盘（CI 与用户排障都用得上）

输出事件补三个字段（宿主已有解析器，纯加法）：
```jsonc
{"type":"done","stage":"done","python":"…","beetsVersion":"2.14.1",
 "installedPlugins":["lyrics"],"packages":["beautifulsoup4>=4.12,<5"],"checkModules":["bs4"]}
```

装完还要**逐个 import 一遍** `checkModules`：pip 退出码 0 不等于依赖齐
（beets 自己也只在调用时才发现缺依赖）。缺一个就报错退出 6。

### 2.2 卸载的边界（2026-10-30 拍板）

> 「卸载脚本只管安装脚本的反向操作，即将依赖、beets 本体和插件再删除；产生的数据因为造到了应用目录内，由应用管理。」

安装往 `--target` 里只放四类东西：venv 解释器、beets 本体、pip 依赖、插件。
全在同一个目录里，所以**反向操作 = 删这个目录**。

**数据不归它管**：`beets/`（整理家）与 `beets-index/`（分析库）在同一个数据根下，
但它们是应用数据、由应用自己的清理入口管。卸载组件顺手删掉 1850 首的分析结果，
是用户没让你做的事。`library.sqlite` 更不沾（根本不是 beets 的东西）。

| 目标 | 谁负责 |
|---|---|
| `{data_root}/beets-tool/venv`（install 的产物） | `uninstall.py --target …` 或宿主的「卸载组件」 |
| `{data_root}/beets-tool/bundle`（宿主展开的套件包） | 宿主的「卸载组件」（Python 侧不知道这个目录） |
| `{data_root}/beets/`、`{data_root}/beets-index/` | 应用自己的清理入口，**不随组件卸载** |

安全闸（两边都要有，参数来自外部，而它会 `rmtree`）：
- 必须是已存在的目录，且自身不是符号链接/junction；
- 必须看起来是 venv（有 `pyvenv.cfg`）——否则拒绝，防止把 `D:\Music` 传进来；
- 拒绝盘根 / 用户主目录 / Windows 与 Program Files；
- 先统计再删，把"删了什么、多少字节"报给 UI。

**为什么卸载的主力不放在 Python**：路径 A（免配置包）的用户机器上可能根本没有
Python。所以宿主侧必须有纯 Rust 的 `beets_uninstall`（删目录而已），
`uninstall.py` 是同一件事的命令行版本。两边只共用一个事实：删哪几个目录。

### 2.3 顺手修的两处

- `.pyz` 里目前只打 `data/requirements.txt` 与 `data/LICENSE`，要加 `data/plugins.json`；`pyproject.toml` 的 `package-data` 从 `data/*.txt` 扩到 `["data/*.txt", "data/*.json"]`。
- `build_bundle.py` 的 `VERSION = "0.1.0"` 改成从 `aiya_beets.__version__` 读 —— 已经漂了（manifest 0.1.0 vs 包内 0.2.0）。

---

## 3. 免配置包：uv 重打

### 3.1 目标形态

现在的 bundle 是「beets + 依赖，零插件」，用户装完还是要联网 `pip install` 才有歌词/封面。免配置包的意思是：**解压即全功能**，勾选的插件依赖已经在 `site-packages` 里。

### 3.2 用 uv 而不是 pip

pip 走 `--target` 要自己解析、自己下载、自己解包；uv 同一件事实测快一个量级，且 CI 有官方 action。

已验证的组合（本机实测）：

```
uv pip install --python <runtime>/python.exe --target <runtime>/Lib/site-packages \
    -r requirements.lock  beautifulsoup4>=4.12,<5  pyacoustid
```

关键一点：`--python` 指 embeddable 的 `python.exe`，uv 会真的去问那个解释器要 sysconfig，所以解析出来的是 **cp314** wheel（实测 `numpy-2.5.4` tag = `cp314-cp314-win_amd64`），而不是打包机上的 3.13。这正好消掉了「打出来的包在用户机 import 失败」这一类事故。

**不引入中间 venv**：venv 的作用是"能运行"，而 embeddable 本身就能跑（`_pth` 决定搜索路径）。中间建 venv 再拷 `site-packages` 只会多一次拷贝、并把 `pyvenv.cfg` 里那条绝对路径带进来 —— 那正是 venv 不可重定位的原因，与「免安装包」的需求相反。所以流程是：**embeddable 目录就是包根，uv 直接往里装**。

### 3.3 脚本与 CI

`beets-tool/tools/build_bundle.py` 改造（仍是独立脚本，`--uv` 可指路径，缺 uv 时回落 pip 并打警告 —— 开发者机器不一定有 uv）：

1. `ensure_runtime()`：下 embeddable、写 `_pth`（现状保留）
2. `install_deps()`：读 `plugins.json` 里 `bundle:true` 的 `packages`，**一次** uv 命令装完
3. 拷 `aiya_beets` 源码 + `LICENSE` + `plugins.json`
4. `manifest.json` 增加 `"plugins": ["lyrics", …]`、`"pluginsJsonSchema": 1`、`"packages": {…冻结版本…}"`
5. 自检两步（都要过）：
   - `<bundle>/python.exe -m aiya_beets version`
   - `<bundle>/python.exe -c "import bs4"` —— 预置插件的依赖真的能导入，这是"免配置"的验收条件

CI 侧 `.github/workflows/release-beets.yml`：

```yaml
on:
  workflow_dispatch:          # 唯一常规入口（手动）
  push:
    tags: ["beets-v*"]        # 也允许打 tag 发版
jobs:
  build:
    runs-on: windows-latest
    steps:
      - checkout
      - astral-sh/setup-uv@v6
      - setup-python 3.12      # 只为跑 build 脚本本身
      - run: python beets-tool/tools/build_bundle.py --uv uv
      - run: python beets-tool/tools/build_installer.py
      - 校验产物名 + 自检（解压后跑 version / import bs4）
      - 上传：tag 上 → beets-v0.2.0 release；同时 `gh release upload beets-latest --clobber`
```

`release.yml` 里 beets 的两步整段删掉，只留播放器。

### 3.4 下载地址与 latest 的冲突

现在宿主写死 `releases/latest/download/<asset>`。beets 一旦单独发版，`latest` 就指到 beets release；之后发播放器 release（不带 beets 资产）又会让它 404 —— 这是两个发布节奏互相踩。

解法：**浮动 tag `beets-latest`**。

- 每次 beets 发版，除了 `beets-v0.2.0`，再把同样两个资产 `--clobber` 上传到永久 tag `beets-latest` 的 release；
- 该 release 标 `--prerelease`，这样 `releases/latest` 的语义（最新正式版）不被污染；
- 宿主常量改成 `.../releases/download/beets-latest`，从此与播放器发布完全解耦；
- `AIYA_BEETS_RELEASE_BASE` 环境变量保留（本地演练、镜像源）。

为什么不用「查 API 找最新 beets-v\* release」：多一次未认证请求（限流 60/h）、多一层 JSON 解析、多一种失败模式，换来的只是"没有幽灵 tag"。浮动 tag 是一行 `gh` 命令，出问题时 `git ls-remote --tags` 就能看见。

---

## 4. 用户机的两条路

### 4.1 路径 A：免配置包

链接下载（`beets-latest/aiya-beets-bundle-win-x64.zip`）→ 展开到 `{data_root}/beets-tool/bundle` → 读 `manifest.json` 记入口。现状不改，只是包内容变成"开箱即用"。

UI 上写清三件事：不需要本机 Python、预置了哪些插件、多大。

### 4.2 路径 B：本机 Python + 勾选

1. **探测**：`py -0p`（Windows 首选）→ `python -V` → `python3 -V`，逐个取版本；输出 `{found, path, version, supported}`。现在只试后两个，Windows 上 py launcher 是最常见的安装形态，漏了会误报"没有 Python"。
2. **展示**：从内嵌 `plugins.json` 渲染可勾选清单（不需要先装组件、不需要起 sidecar）。带 `exe` 依赖的项标注「还需 ffmpeg / fpcalc」，但不阻止勾选（装完照样能跑，只是那个插件不 ready）。
3. **安装**：`beets_install(track="installer", plugins=[...])` → 宿主跑 `.pyz install --plugins … --target {data_root}/beets-tool/venv`。
4. **落地到 config**：装完把清单存进 `{data_root}/beets/settings.json` 的新键 `plugins`，`cfg.ensure_config` 的 workbench 分支用 `_sync_plugins()`（与 `_sync_directory()` 同款外科式同步）改写 `plugins:` 那一行。

第 4 步是现在缺的一环：`render_config` 只能吃 `DEFAULT_PLUGINS`，用户在 UI 勾了 lyrics 也只会装包、不会启用。**装了不启用 = 白装**。

`_sync_plugins` 与 `_sync_directory` 同样要过两向负向验证（不同步 → 红；整份重写 → 红「除 plugins 外逐字节保留」）。

### 4.3 宿主新增命令

| 命令 | 作用 | 备注 |
|---|---|---|
| `beets_plugin_table()` | 内嵌 JSON 转 UI（注入 `default` / `indexDefault`） | 不依赖 sidecar，组件未装可用 |
| `beets_python_probe()` | 探测本机 Python（`py -0p` 优先） | 取代 `find_system_python` 的位置 |
| `beets_install(track, plugins?)` | 现有命令扩参数 | `plugins` 省略 / 空 = 只装基础 |
| `beets_uninstall()` | 删 `beets-tool/`（venv + bundle）、清 `toolPath`/`toolArgs` | 纯 Rust，事件流同 install；**不碰数据目录** |

---

## 5. 已拍板（2026-10-30，用户原话）

1. **卸载只做安装的反向操作** —— 删依赖 / beets 本体 / 插件（即 `--target` 那个目录）；
   数据因为造在应用目录内，由应用管理。所以没有 `--purge`。
2. **beets release 发两个资产**：`.pyz` + 免配置包。路径 B 需要一个给本机 Python
   执行的东西，砍掉 `.pyz` 等于砍掉路径 B。
3. **下载地址用浮动 tag `beets-latest`**，不查 API。
4. **免配置包预置哪些插件：下一步再计划。** 机制先做好（`bundle: true` 读 JSON），
   当前的预置集是 `lyrics` / `fetchart` / `mbsync`，改集合只改 JSON，不改代码。

---

## 6. 批次与结果

| 批 | 内容 | 结果 |
|---|---|---|
| B1 | `plugins.json` 唯一事实源 + `plugins.py` 三视图派生 + `catalog_contract.py` | ✅ 14 条；6 组断言（[1]–[5] 正向 + [6] 负向五连：名字重复 / 引用不存在 / 缺 write:no / bundle 带 exe / 打错键），负向逐个实测变红 |
| B2 | `install.py` 入参化 + `uninstall.py` + `.pyz` 打 JSON | ✅ 用新 pyz 真装一次 venv：`beets 2.14.1 + beautifulsoup4 4.15.0`，插件自检通过；未知插件退出 2；卸载的四组安全闸（盘根 / 主目录 / 非 venv / 幂等）实测 |
| B3 | `build_bundle.py` 改 uv + 预置插件 + manifest 补版本与清单 | ✅ uv 0.13.6 只补 2 个包（57ms 缓存命中）；产物 28.3MB；包内 `import bs4` = 4.15.0；manifest 版本 0.2.0（不再是硬编码的 0.1.0） |
| B4 | 拆 `release-beets.yml`、`release.yml` 删 beets 段 | ✅ 12 步 / 9 步，YAML 解析通过。**未在 CI 实跑**（要推送后才有 runner） |
| B5 | 宿主：`beets-latest`、`beets_plugin_table`、`beets_python_probe`、`beets_uninstall`、install 扩参 | ✅ `cargo test` 40 过（含内嵌表解析、Python 探测形状）；`dto_wire` 锁死三个新 DTO 的键集合 |
| B6 | 安装页两卡重构 + 插件勾选 + 卸载两段确认 | ✅ `svelte-check` 0/0；e2e 40 过（新增「安装页给出插件勾选与卸载入口」） |
| B7 | `settings.plugins` → `_sync_plugins` 闭环 | ✅ 三向断言（选中 / 缺键 / 空数组）+ 两向负向验证（不同步 → 红；整份重写 → 红） |
| B8 | 全量门禁 `npm run verify` | ✅ 一次过：`cargo test` 全 workspace 绿（135+128+71+45+24+20+11+9+5+5+3）、`svelte-check` 0/0、e2e 40 passed、`check:fmt` / `check:clippy`（`-D warnings`）/ `gen:types:check` 绿、六个 smoke 全 `ALL PASS`（含新增 `smoke:catalog`） |

### 仍未验的（下一轮）

- **CI 没跑过**：两个 workflow 只是本地 YAML 解析通过，第一次 `workflow_dispatch` 才是真验证。
- **`beets-latest` 浮动通道没建过**：第一次发布时才会创建那个 prerelease。
- **真机装一遍免配置包**：本地打完是在开发机上展开验的，没走「下载 → 解压到 `{data_root}` → 宿主记入口」整链。
- **卸载真跑**：只在临时目录验过；没在装了 1850 首的库上点过一次（要确认删完状态页真的变"未启用"）。
- **预置插件集合**：等下一步计划（改 JSON 即可，`bundle: true` 的开关已经生效）。

依赖顺序：B1 → (B2 ∥ B3) → B4 → B5 → B6 → B7。B1 必须最先，其余都读它。

---

## 7. 不做的

- **不做 macOS / Linux 的免配置包**：应用当前只规划 Windows（用户已拍板），脚本留 `--platform` 口子但不进 CI。
- **不签名**：2026-10-27 已拍板，不因这次重构改变。
- **不把 Essentia 打进包**：AGPLv3 + 20MB + 只支持 32 位，docs/50 已定「引导用户自备」。
- **不做插件热卸载**：`--plugins` 只增不减。减插件 = 重装 venv（删目录重来），避免维护"哪些包只被谁依赖"的图。
