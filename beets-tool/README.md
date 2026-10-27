# aiya-beets

beets 相关的全部能力都在这里：导入预览/应用、写标签、移动复制、库基线、
刷新标签、查重、整理文件，以及自检（运行时版本 / 插件三态）。

它是**独立发布单元**，不是播放器的子模块。播放器只做三件事：发现入口 →
起进程 → 说协议。所以这里有两条硬边界：

- 本目录**不知道播放器的数据模型**（没有 track id、没有播放历史、没有 SQLite 表）；
- 播放器**不知道这里怎么装、怎么配、开什么插件**。

唯一的耦合面是**协议**：`protocol/v1.schema.json`（契约真源，随本包发版）
+ `src/aiya_beets/protocol.py`（命令名/错误码常量）。播放器仓根镜像一份
（`protocol/aiya-beets-v1.json`，字节副本），`npm run proto:check` 守字节一致。

## 跑起来

```bash
# 源码树直接跑（不需要先 pip install）
PYTHONPATH=src python -m aiya_beets          # 进 serve：stdio NDJSON，等请求
PYTHONPATH=src python -m aiya_beets status   # 一次性探测（人看的也是它）
PYTHONPATH=src python -m aiya_beets doctor   # 人读的自检报告：运行时/配置/插件三态
PYTHONPATH=src python -m aiya_beets capabilities  # 机器读的能力三态
```

请求一行一个 JSON，应答/事件一行一个 JSON。

## 协议（v1 已冻结）

| 文件 | 角色 |
|---|---|
| `protocol/v1.schema.json` | **真源**。JSON Schema 2020-12，四组 `$defs`：`Request`(11) / `Response` / `Event`(16) / `InstallerEvent` |
| `src/aiya_beets/protocol.py` | 命令名、`PROTOCOL_VERSION`、10 个错误码、`SCHEMA_SHA256` |
| `tests/protocol_contract.py` | 用**真实会话**逐行验 schema（离线 59 条消息 + 取消场景） |

改协议的顺序（顺序错了会被测试拦住）：

1. 改 `protocol/v1.schema.json`（真源）；
2. 改 `protocol.py` 的常量（`PROTOCOL_VERSION` / `ERROR_CODES`）——**改名才 +1**，
   加可选字段不加；
3. `python tests/protocol_contract.py` 前先更新 `SCHEMA_SHA256`（测试会告诉你正确值）；
4. 播放器侧 `npm run proto:sync` 同步镜像，然后 `cargo test -p aiya-beets-client`。

第 3、4 步的"忘了"都会被测试拦住，这是刻意的：协议变化必须在 diff 里看得见，
不允许静默跟随。

`status` 里还会自报 `schemaHash`（同一份 sha256）。这是唯一的**运行时**凭据——
将来播放器与组件分别升级时，靠它一眼看出"那不是同一版契约"。

## 两个发布物

| 产物 | 命令 | 大小 | 前提 |
|---|---|---|---|
| `dist/aiya-beets-installer.pyz` | `python tools/build_installer.py` | ~33 KB | 目标机器要有 Python 3.10–3.14 |
| `dist/aiya-beets-bundle-win-x64.zip` | `python tools/build_bundle.py` | ~28 MB（展开 ~72 MB） | 无。自带 embeddable 解释器 |

`build_bundle.py` **自己准备运行时**（下载官方 embeddable → 写 `_pth` → 按
`requirements.lock` 装依赖），缓存在 `.runtime/`（不入库、可删）。以前这步在播放器
仓里做，M2 删掉那份脚本之后套件包就再也打不出来了——所以责任搬回本目录。

`.pyz` 是 zipapp：`python aiya-beets-installer.pyz install --target <dir>` 在那里
建 venv 并装 `requirements.lock`。套件包解压即用，根目录有 `manifest.json`
写明启动入口（`entry` / `args`），播放器据此记下怎么起。

**套件包为什么能搬来搬去**：embeddable 解释器把「解释器 + stdlib + DLL」全放在
包内，`sys.prefix` 由 exe 位置推出，所以整个目录复制到任意路径都能跑（实测过
复制到 `%TEMP%` 后 `sys.prefix` 跟着走）。这和 venv 相反——venv 的
`pyvenv.cfg` 里 `home` 是指向系统 Python 的硬指针，换个路径就废。

**踩过的坑**：embeddable 自带 `python3xx._pth` 时，Python **忽略
`PYTHONPATH`**。所以本包必须装进 `Lib/site-packages`，靠环境变量喂路径是
无效的（`build_bundle.py` 已按此实现）。

## 测试

```bash
python tests/smoke_apply.py         # 5 场景：只写标签 / 复制 / 移动 / 目标冲突 / 取消
python tests/smoke_maint.py         # 15 断言：adopt / update / duplicates / move
python tests/protocol_contract.py   # 契约：真实会话逐行验 schema（要 jsonschema）
```

契约测试需要 `jsonschema`（只有它需要，工具本身不依赖）：

```bash
python -m pip install -e ".[dev]"    # 或者只装 jsonschema
AIYA_BEETS_ONLINE=1 python tests/protocol_contract.py   # 连 album/albumError/done 一起验
```

两条都用临时 `BEETSDIR` + 临时库目录，不碰真实曲库。解释器默认取
`beets-tool/.venv/Scripts/python.exe`，可用 `AIYA_BEETS_PYTHON` 覆盖；
测试音源目录用 `AIYA_TEST_MUSIC` 覆盖。

`tests/fixtures/beets-home/` 是 dev 库（3 条，含 `config.yaml`），给**播放器侧**
的 `dump_parse_and_sync_end_to_end` 用：它要验证「工具 dump 出来的形状 →
播放器解析 → 对齐进自己的库」这条链，所以需要一个真实存在过的 beets 库。
里面是二进制 sqlite 且含绝对路径，属于机器本地数据，**不入库**；缺失时那条
测试自动跳过。

## 环境变量

| 变量 | 作用 |
|---|---|
| `AIYA_BEETS_HOME` | 数据根。本工具把它翻译成 `BEETSDIR`（必须在 `import beets` **之前**，beets 导入时就把 config_dir 定死了） |
| `AIYA_BEETS_SCAN_ROOT` | 生成默认 `config.yaml` 时写的 `directory`（整理目标根） |
| `AIYA_BEETS_PYTHON` | 仅测试用：指定解释器 |

## 插件能力为什么要自己探

`plugins.load_plugins()` 对导入异常**只记日志、不抛错**，而 `config['plugins']`
只是一份"愿望清单"。照配置名单报能力必然假绿（实测：缺 `bs4` 时 `lyrics`
仍然 load 成功）。所以这里用 `importlib.util.find_spec` 逐个试依赖，上报
三态 `ready` / `missing_deps` / `unavailable`，并列出缺什么。只看包在不在，
**不 import 插件模块**——插件导入有注册钩子副作用。
