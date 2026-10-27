# aiya-beets

beets 相关的全部能力都在这里：导入预览/应用、写标签、移动复制、库基线、
刷新标签、查重、整理文件，以及自检（运行时版本 / 插件三态）。

它是**独立发布单元**，不是播放器的子模块。播放器只做三件事：发现入口 →
起进程 → 说协议。所以这里有两条硬边界：

- 本目录**不知道播放器的数据模型**（没有 track id、没有播放历史、没有 SQLite 表）；
- 播放器**不知道这里怎么装、怎么配、开什么插件**。

唯一的耦合面是 `src/aiya_beets/protocol.py` 里那份 JSON 协议。

## 跑起来

```bash
# 源码树直接跑（不需要先 pip install）
PYTHONPATH=src python -m aiya_beets          # 进 serve：stdio NDJSON，等请求
PYTHONPATH=src python -m aiya_beets status   # 一次性探测（人看的也是它）
PYTHONPATH=src python -m aiya_beets doctor   # 人读的自检报告：运行时/配置/插件三态
PYTHONPATH=src python -m aiya_beets capabilities  # 机器读的能力三态
```

请求一行一个 JSON，应答/事件一行一个 JSON。改协议要同时改 `PROTOCOL_VERSION`
和播放器侧的镜像（`crates/aiya-beets-client/src/proto.rs`）。

## 两个发布物

| 产物 | 命令 | 大小 | 前提 |
|---|---|---|---|
| `dist/aiya-beets-installer.pyz` | `python tools/build_installer.py` | ~33 KB | 目标机器要有 Python 3.10–3.14 |
| `dist/aiya-beets-bundle-win-x64.zip` | `python tools/build_bundle.py` | ~31 MB（展开 ~112 MB） | 无。自带 embeddable 解释器 |

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
python tests/smoke_apply.py    # 5 场景：只写标签 / 复制 / 移动 / 目标冲突 / 取消
python tests/smoke_maint.py    # 15 断言：adopt / update / duplicates / move
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
