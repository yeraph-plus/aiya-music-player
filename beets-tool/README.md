# aiya-beets

beets 相关的全部能力都在这里：导入预览/应用、写标签、移动复制、库基线、
刷新标签、查重、整理文件、索引增强（enrich / query / fields），以及自检
（运行时版本 / 插件三态）。

它是**独立发布单元**，不是播放器的子模块。播放器只做三件事：发现入口 →
起进程 → 说协议。所以这里有两条硬边界：

- 本目录**不知道播放器的数据模型**（没有 track id、没有播放历史、没有 SQLite 表）；
- 播放器**不知道这里怎么装、怎么配、开什么插件**。

唯一的耦合面是**协议**：`protocol/v2.schema.json`（契约真源，随本包发版）
+ `src/aiya_beets/protocol.py`（命令名/错误码常量）。播放器仓根镜像一份
（`protocol/aiya-beets-v2.json`，字节副本），`npm run proto:check` 守字节一致。

## 跑起来

```bash
# 源码树直接跑（不需要先 pip install，beets 在 .venv 里）
PYTHONPATH=src python -m aiya_beets          # 进 serve：stdio NDJSON，等请求
PYTHONPATH=src python -m aiya_beets status   # 一次性探测（人看的也是它）
PYTHONPATH=src python -m aiya_beets doctor   # 人读的自检报告：运行时/配置/插件三态
PYTHONPATH=src python -m aiya_beets capabilities  # 机器读的能力三态
```

请求一行一个 JSON，应答/事件一行一个 JSON。serve 之外还有几条子命令：
`install`（在便携包内变更插件与配置，`--dry-run` 只打印计划）、`uninstall`
（删安装产物，数据不动）、`bins`（补外部工具）、`xtractor`（手动算音频特征）、
`version`。

## 协议（v2）

| 文件 | 角色 |
|---|---|
| `protocol/v2.schema.json` | **真源**。JSON Schema 2020-12，`$defs`：`Request`(17 条) / `Response` / `Event`(20 种会话事件 + cancelAck 回执) / `InstallerEvent`（install 子命令的 stdout 帧，与 serve 是两套词汇表） |
| `src/aiya_beets/protocol.py` | 命令名、`PROTOCOL_VERSION`、11 个错误码、`SCHEMA_SHA256` |
| `tests/protocol_contract.py` | 用**真实会话**逐行验 schema（离线 serve 全链路 + 取消 + 安装器 dry-run 帧） |

改协议的顺序（顺序错了会被测试拦住）：

1. 改 `protocol/v2.schema.json`（真源）；
2. 改 `protocol.py` 的常量——**改名才 +1**（`PROTOCOL_VERSION`），加可选字段
   不加；只改 schema 文本也要同步 `SCHEMA_SHA256`；
3. 仓根 `npm run proto:sync`（镜像 + meta 一条龙），`npm run proto:check` 确认绿。

第 2、3 步的"忘了"都会被测试拦住（`proto:check` 与 `tests/protocol_contract.py`
各自持一份 sha256），这是刻意的：协议变化必须在 diff 里看得见，不允许静默跟随。

`status` 里还会自报 `schemaHash`（同一份 sha256）。这是唯一的**运行时**凭据——
播放器与组件分别升级时，靠它一眼看出"那不是同一版契约"。

## 发布物（单轨）

唯一发布物是**免安装便携套件包** `dist/beets-portable-win-x64.zip`：

```bash
`python tools/selfhost_svm_models.py [--publish]`  # SVM 模型自托管（换模型版本才跑；upf 慢源 → 12 件重打包 → 挂固定 release `svm-models-<版本>`，plugins.json 的 svm-* 条目从这里下载）
npm run beets:bundle    # = beets-tool/.venv 的解释器跑 tools/build_bundle.py
```

自带 embeddable 解释器 + 依赖 + 预置插件包（`plugins.json` 里 `bundle: true`
的那些）+ 外置工具（`bin/`，含 Essentia 提取器与 SVM 模型），解压即全功能。
根目录 `manifest.json` 写明启动入口（`entry` / `args`）与预置清单，播放器据此
记下怎么起。「换插件」同样不出这套动作：宿主用包内解释器 spawn
`install --plugins <新清单> --previous <旧清单>`（差量 pip + 补外部工具 + 同步
配置），播放器不建 venv、不装 pip 包、不碰 Python 工具链。

曾经的"A 轨安装器"（`tools/build_installer.py` 产出的 `.pyz`，在目标机建 venv）
随单轨化退役，构建脚本已删除——应用不再探测、不再构造包外的任何 Python 环境。

**套件包为什么能搬来搬去**：embeddable 解释器把「解释器 + stdlib + DLL」全放在
包内，`sys.prefix` 由 exe 位置推出，所以整个目录复制到任意路径都能跑。这和
venv 相反——venv 的 `pyvenv.cfg` 里 `home` 是指向系统 Python 的硬指针，
换个路径就废。

**踩过的坑**：embeddable 自带 `python3xx._pth` 时，Python **忽略
`PYTHONPATH`**。所以本包必须装进 `Lib/site-packages`，靠环境变量喂路径是
无效的（`build_bundle.py` 已按此实现）。

## 测试

```bash
# 在仓根跑；全绿是发版门槛（verify 里逐一列了）
npm run smoke:beets      # 5 场景：只写标签 / 复制 / 移动 / 目标冲突 / 取消
npm run smoke:maint      # adopt / update / duplicates / move
npm run smoke:proto      # 契约：真实会话逐行验 schema（要 jsonschema）
npm run smoke:config     # 双家隔离契约（模板 / 闸 / 外科式同步）
npm run smoke:catalog    # 插件目录三态
npm run smoke:bins       # 外置工具下载与校验
npm run smoke:uninstall  # 卸载安全闸
npm run smoke:lock       # 库文件锁
npm run smoke:enrich     # 索引增强 / 查询
```

契约类测试需要 `jsonschema`（只有它们需要，工具本身不依赖）：

```bash
python -m pip install -e ".[dev]"    # 或者只装 jsonschema
AIYA_BEETS_ONLINE=1 npm run smoke:proto   # 连 album/albumError/done 一起验
```

测试都用临时家（`AIYA_BEETS_HOME` 指到临时目录）+ 临时库目录，不碰真实曲库。
解释器默认取 `beets-tool/.venv/Scripts/python.exe`，可用 `AIYA_BEETS_PYTHON`
覆盖；测试音源目录用 `AIYA_TEST_MUSIC` 覆盖（都找不到就现场合成 wav，
见 `tests/_testmusic.py`）。

`tests/fixtures/beets-home/` 是 dev 库（3 条，含 `config.yaml`），给**播放器侧**
的 `dump_parse_and_sync_end_to_end` 用：它要验证「工具 dump 出来的形状 →
播放器解析 → 对齐进自己的库」这条链，所以需要一个真实存在过的 beets 库。
里面是二进制 sqlite 且含绝对路径，属于机器本地数据，**不入库**；缺失时那条
测试自动跳过。

## 环境变量

| 变量 | 作用 |
|---|---|
| `AIYA_BEETS_HOME` | **数据根**。工具把它翻译成 `BEETSDIR`（必须在 `import beets` **之前**，beets 导入时就把 config_dir 定死了）；锁文件、幂等登记簿、断点、外置工具落点都从它推导。与外部 `BEETSDIR` 不一致时拒绝启动 |
| `AIYA_BEETS_PROFILE` | 跑在哪个家：`workbench`（缺省，用户库）或 `index`（索引库）。未知值直接拒绝启动——双家隔离靠它，静默回落会让索引会话写进用户库 |
| `AIYA_BEETS_SCAN_ROOT` | **曲库根**（播放器传的事实）：索引家的分析对象；没给时索引家退回 `{home}/scratch` 占位目录 |
| `AIYA_BEETS_DEST` | **整理落点**：workbench `directory` 行的来源；没给回落 `AIYA_BEETS_SCAN_ROOT`。与曲库根拆成两个变量，防"改落点把索引范围一起改掉" |
| `AIYA_BEETS_BIN_DIR` | 外置工具目录覆盖（ffmpeg / Essentia 提取器 / SVM 模型）。缺省按 `AIYA_BEETS_HOME` 的上一级推 `{data_root}/beets-tool/bin` |
| `AIYA_BEETS_PYTHON` | 指定解释器：tests/ 与插件安装器（`cmds/market.py`）用它覆盖 `sys.executable` |
| `AIYA_BEETS_ENRICH_PROBE` | `1` = 开 enrich 的自测假源（按路径哈希造确定性字段）。默认关：假数据进真库是最难排查的一类污染 |
| `AIYA_BEETS_ONLINE` | `1` = 契约测试补验联网匹配场景 |
| `AIYA_TEST_MUSIC` | 测试音源目录覆盖（仅 tests/） |

## 插件能力为什么要自己探

`plugins.load_plugins()` 对导入异常**只记日志、不抛错**，而 `config['plugins']`
只是一份"愿望清单"。照配置名单报能力必然假绿（实测：缺 `bs4` 时 `lyrics`
仍然 load 成功）。所以这里用 `importlib.util.find_spec` 逐个试依赖，上报
三态 `ready` / `missing_deps` / `unavailable`，并列出缺什么。只看包在不在，
**不 import 插件模块**——插件导入有注册钩子副作用。

## 许可

GPL-3.0-or-later，全文见 [`LICENSE`](LICENSE)（与仓库根 `LICENSE` 同一份）。

2026-10-27 统一过一次：此前这里的 `pyproject.toml` 写的是 MIT，而根 README 与
`licenses/SOURCE-OFFER.txt` 一直说 GPL-3.0-or-later——两处声明打架。法律上 MIT
也站得住（上游 beets 2.14.1 本身就是 MIT），选 GPL 是为了"一个仓库一个许可"。

套件包内附许可全文：zip 根目录一份（解压即见），`Lib/site-packages/aiya_beets/data/`
一份。它是独立发布单元，用户机器上没有仓库可指。
