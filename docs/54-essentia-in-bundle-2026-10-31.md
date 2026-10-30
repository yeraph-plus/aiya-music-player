# Essentia 进免配置包 + xtractor 默认携带（docs/54）

> 状态：**已实现并验证**（§5 有发布物上的真机数据）。
> 日期：2026-10-31。
> 前置：docs/50（xtractor 与 Essentia 的 Windows 现状）、docs/51（安装链两路）、docs/53（外置二进制）。

这一轮用户原话：

> 「让 essentia_extractor 进免装包，让 xtractor 插件默认携带」

上一轮拍的是「三个二进制都不进免配置包」（docs/53 §5）。这一轮**把 Essentia 破例**：
它随包发，`xtractor` 也随之进默认携带。代价与补偿见 §1.6。

---

## 0. 一句话

"让 xtractor 默认携带"不是改两行 JSON。这个插件**根本不在 beets 2.14.1 里**（是 PyPI 上的
`beets-xtractor 0.4.2`），装上了还会因为一个相对路径 bug **每首失败且退出码 0**，
再配上一个默认开启的 `highlevel` 档位——那个档位在 Windows 提取器上是**整体退出 1、
一个 json 都不产出**。

所以"携带"最终是四件事一起做：**插件包 + 提取器 + 路径补丁 + 档位**。少任何一件，
用户看到的现象都一样：装好了、跑完了、bpm 一列全空。

---

## 1. 事实核查（本轮实测）

### 1.1 `xtractor` 不是 beets 的内置插件

```
$ ls beets-tool/.venv/Lib/site-packages/beetsplug/ | grep -i xtract     → 无输出
$ python -c "import beetsplug.xtractor"                                 → ModuleNotFoundError
```

它在 PyPI：`beets-xtractor 0.4.2`（`py3-none-any`，12,459 字节，2024-05-26，**MIT**，
`Requires-Dist: beets>=1.4.9, pyyaml`）。所以目录里那条 `"builtin": true` 是错的，
本轮改成 `false` + `packages: ["beets-xtractor>=0.4.2,<0.5"]` + `imports: ["beetsplug.xtractor"]`。

`pyyaml` 不是新依赖：`requirements.lock` 里本来就有 `PyYAML==6.0.3`，且 cp314/win_amd64
有轮子（实测 `pip download --only-binary=:all: --abi cp314` 成功）。

### 1.2 相对路径 bug —— 顺手订正 docs/50 §3 的一个结论

docs/50 当时写的是「`item.path` 是绝对、`item.get('path')` 是相对，所以换成 `item.path` 就行」。
**本轮实测：`item.path` 也是相对。**

```
item.path      → b'中音测试 440Hz.wav'
lib.directory  → 'D:\\MUSIC_Player\\test-music'
item_abs_path  → 'D:\\MUSIC_Player\\test-music\\中音测试 440Hz.wav'
```

所以补丁必须走 `beetslib.item_abs_path`（`os.path.join(lib.directory, item.path)`）——
那是全仓唯一的"相对基准"口径，查询与导入预览用的是同一个。只把 `get("path")` 换成 `path`
照样 FileNotFoundError（本轮第一步就是这么错的，实测报 `Input file(中音测试 440Hz.wav) not found!`）。

还有一处坑：这个方法挂在 `XtractorCommand` 上、调用点是 `self._get_input_path_for_item(item)`，
所以补丁函数的签名必须带 `self`。少一个形参是每首一条 `TypeError`，而插件只把它算作
"这首失败"——现象和 bug 本身一模一样。

### 1.3 默认档位会烧掉整次分析

插件自带 `config_default.yml`：

```yaml
extractor_profile:
  highlevel:
    compute: 1
    svm_models: [/your/path/to/svm_model.history]
```

我们下发的提取器是 AcousticBrainz 那版 **win-i686**，没编 Gaia2（docs/50 §2.5）：

```
Identifier 'GaiaTransform' not found in registry...
exit=1        # 连 json 都不生成
```

于是 `helper.extract_from_output` 拿不到文件 → 每首 `File not found` → 字段全空。
**必须在索引家模板里把 `highlevel.compute` 钉成 0**（`cfg.XTRACTOR_PROFILE`），
并且不能写 `svm_models`（我们根本没有模型文件）。

### 1.4 包里的 exe 能不能被找到 —— 靠"两个落点、一套查找"

宿主把套件包展开在 `{data_root}/beets-tool/bundle`（`beets.rs::install_via_bundle`），
而下载落点是 `{data_root}/beets-tool/bin`。两者是**兄弟目录**，所以包里带 `bin/` 时，
工具侧必须两处都看：

```
{data_root}/beets-tool/bin/          下载落点（用户点安装时下的）
{data_root}/beets-tool/bundle/bin/   包内自带（构建时塞进去的）
```

实测（按宿主布局解包 + 包内解释器）：

```
$ AIYA_BEETS_HOME=…\.bundlecheck\beets beets-tool\bundle\python.exe -m aiya_beets bins --check --only streaming_extractor_music
{"binaries":[{"name":"streaming_extractor_music","present":true,
  "path":"…\\beets-tool\\bundle\\bin\\streaming_extractor_music.exe","usable":true}],"missing":[]}
```

### 1.5 构建侧的坑：`pip --target` 不会往已有的 `beetsplug/` 里塞新子包

`build_bundle.py` 的依赖安装原来是直接 `--target <runtime>/Lib/site-packages`。实测：

```
$ pip install --target <已有 beets 的目录> beets==2.14.1        # beetsplug/ 已存在
$ pip install --target <同一目录> beets-xtractor               # 只打一条 already exists 警告
$ ls <目录>/beetsplug | grep xtract                             → 无
```

pip 对已存在的顶层目录**整目录跳过**（警告是有的，混在几十条里）。后果是
"包打出来了、插件没进去"，而构建日志一片绿——只有最后那条 import 自检会红，
红得还像是"包坏了"。

修法：**先装到空目录，再合并进 site-packages**（`shutil.copytree(dirs_exist_ok=True)`）。
uv 与 pip 走同一个实现，天然幂等，也不会留旧版本文件。

### 1.6 许可：AGPLv3 的义务现在归我们了

Essentia 是 **AGPLv3**，docs/50 §5 与 docs/53 §5 都据此定过"不要把 exe 打进安装包"。
本轮用户明确要求进包，所以这条纪律作废，改由发布物承担：

- `THIRD-PARTY.md`（包根）：每个随包二进制的来源、许可、上游地址、**本包内 sha256**；
- `LICENSES/AGPL-3.0.txt`：许可全文（仓库 `beets-tool/third-party/AGPL-3.0.txt` 自带，构建不联网）；
- `manifest.json` 的 `binaries`：同一份事实的机器可读版本。

**这不是法律意见。** AGPL §6 对"未修改二进制的源码提供方式"有要求，我们目前做的是
"给精确的上游地址 + 可核对的 sha256 + 明确声明未修改"，属于务实做法；真要大规模对外
分发，该让律师看一眼。另外：pip 包（beets-xtractor / bs4 / requests …）的许可文本
**没有**逐条收进 `THIRD-PARTY.md`，这是本次之前就存在的缺口，记在这里不装作已经解决。

---

## 2. 设计

### 2.1 数据侧：两处 `bundle: true`

```jsonc
"binaries": [
  { "name": "streaming_extractor_music", "target": "…exe", "bundle": true, "license": "AGPL-3.0-or-later", … },
  { "name": "ffmpeg",  "bundle": false, … },   // GPLv3，87MB，仍然用户机现下
  { "name": "fpcalc",  "bundle": false, … }
],
"plugins": [
  { "name": "xtractor", "packages": ["beets-xtractor>=0.4.2,<0.5"],
    "imports": ["beetsplug.xtractor"], "exe": ["streaming_extractor_music"], "bundle": true, … }
]
```

`binaries[].bundle` 是**分发决策**，所以放在表里而不是构建脚本的 if 里：一张表三个二进制，
"哪个随包发"直接决定包里要附哪些许可。派生视图 `plugins.BUNDLE_BINARIES` 与 `BUNDLE_PLUGINS`
是两个概念（一个是 exe、一个是 Python 依赖），**但带 exe 的 bundle 插件，其 exe 必须在
BUNDLE_BINARIES 里**——契约测试与 Rust 单测两头钉住，反向还钉了一条"随包发的 exe 得有
bundle 插件在用"（否则白扛体积与许可）。

### 2.2 查找侧：`bins.bin_dirs()`

```
bin_dirs() = [bin_dir(), bundle_bin_dir()]        # 下载落点优先
installed_path / inject_path / exe_path 都走它
```

- `installed_path` 命中任一处就算"有"→ `ensure()` 因此**不会重复下载**包内已有的 exe（实测 `skipped: true`）；
- `inject_path()` 把**存在的**目录都前插进 PATH（rdiff：包内 ffmpeg 也能被裸名字找到）；
- `AIYA_BEETS_BIN_DIR` 存在时不做 home 推断：显式覆盖的意思就是"只这一个目录"，否则测试
  与离线演练会意外看到真实数据根。

### 2.3 构建侧：装到空目录再合并 + 顺手带许可

`build_bundle.py` 新增三段：
`bundle_binary_plan()`（前置校验，见 2.1）→ `stage_binaries()`（走 `bins.stage`，
与用户机下载**同一套** size/sha256/成员唯一性/probe 校验）→ `write_third_party()`（§1.6 的三件套）。
打包完还回头看一眼 zip 里有没有 `bin/<target>`：路径被过滤掉就等于没带。

### 2.4 运行侧：补丁 + 一个进程内命令

- `aiya_beets/xtractor.py`：`apply()` 打补丁（幂等、不改第三方文件），`run(argv)` 进程内跑
  `beet xtractor`，`readiness()` 给排障；
- `cli.py` 新增 `xtractor` 子命令，参数**原样透传**（beets 那条命令自己有 `-f/-t/-c/-q/-w/-n` 六个开关，
  抄一份必然漂）。透传必须在 argparse **之前**分流：`nargs=REMAINDER` 遇到以 `-` 开头的参数会被
  顶层解析器抢走（实测 `aiya-beets xtractor -q` → `unrecognized arguments: -q`）。

**为什么强制索引家**：`xtractor` 只在索引家模板里启用、`essentia_extractor` 也只在那儿写；
而工作台家的 `import.write` 默认 `yes`，跑一遍就等于把 bpm 写进用户的音频文件。
所以 `run()` 在 `AIYA_BEETS_PROFILE != index` 时直接拒绝。

**为什么必须进程内**：补丁是 monkeypatch，spawn 出去的 `python -m beets` 看不到它
（`enrich._run_import` 就是 spawn 形态）。将来批 5 接线也必须走进程内这条路。

---

## 3. 实测结果（发布物上跑的，不是源码树）

在解出来的 `dist/aiya-beets-bundle-win-x64.zip` 上用**包里的**解释器跑完整链路：

| 步 | 命令 | 结果 |
|---|---|---|
| 插件在包里 | `Lib/site-packages/beetsplug/xtractor` | 在 |
| 提取器在包里 | `bin/streaming_extractor_music.exe` | 在（20,701,667 B） |
| 配置 | `aiya_beets status`（AIYA_BEETS_PROFILE=index） | `plugins: xtractor`，闸通过 |
| 档位 | config.yaml | `extractor_profile.highlevel.compute: 0`，`essentia_extractor` 指向包内 |
| 入库 | `-m beets import -A -q <3 个 wav>` | exit 0，库内 3 条 |
| 分析 | `-m aiya_beets xtractor -q` | exit 0，`Number of items to be processed: 3` |
| bpm | `items.bpm` | 120 / 110 / 120 |
| 其余字段 | `item_attributes` | `danceability` 0.4789 / 0.4842 / 0.4839；`beats_count` 9/8/9；`average_loudness` 0.8060 |
| **文件没被动** | 3 个 wav 的 sha256 + mtime | **完全一致**（`xtractor.write: no` 在"插件真的加载成功"的状态下也钉得住 —— docs/50 §8.4 的未验项） |

发布物体积：**33.7MB**（原来 28.3 → +5.4MB；解出来 97MB）。CI 的体积下限从 20MB 提到 30MB，
"提取器没进包"会当场掉到 22MB 以下。

---

## 4. 代码清单

| 文件 | 改了什么 |
|---|---|
| `data/plugins.json` | `binaries[].bundle`（3 条都写明）；xtractor：`builtin:false` + packages + imports + `bundle:true` |
| `plugins.py` | `BinarySpec.bundle`、`BUNDLE_BINARIES`、`exe_path` 改走 `bin_dirs()` |
| `bins.py` | `bundle_bin_dir()` / `bin_dirs()` / `inject_path` 多目录 / `installed_path` 搜索 / `fetch_blob` 抽出 / `install_one(dest=)` / `stage()` |
| `build_bundle.py` | 空目录安装再合并（§1.5）、`bundle_binary_plan` / `stage_binaries` / `write_third_party`、manifest `binaries`、zip 条目复核 |
| `cfg.py` | `XTRACTOR_PROFILE`、`_render_tree`（真嵌套）、`_parse` 支持任意层级、闸加一条"highlevel 必须 0" |
| `xtractor.py`（新） | 路径补丁 + 进程内 `beet xtractor` + `readiness()` |
| `cli.py` | `xtractor` 子命令（参数透传） |
| `beets.rs` | 内嵌表解析 `binaries[].bundle`；单测改成"bundle 插件的 exe 必须随包发" |
| `tests/*` | catalog：`bundle` 必填 + 两条新不变量 + 负向 9→13 连；bins：包内查找与 `dest` 落点；config：`[12]` 档位正负三向 |
| `release-beets.yml` | 自检按宿主布局解包 + `bins --check` 断言 `missing: []`；体积下限 30MB |
| UI | 体积文案 31→34MB/97MB；mock 的 xtractor 条目对齐真表（不再写"自备"） |

---

## 5. 仍未做

1. **enrich 那条线还没接**（原批 5）：`cmds/enrich.py` 里 `SourceSpec("xtractor", False, …)`
   仍是 `unwired`，所以应用里**没有任何按钮会去跑它**。本轮做的是"带上 + 能跑"，
   不是"自动跑"。接的时候要注意：必须走进程内（§2.4），并且逐首的失败要能报出来。
2. **工作台家不写 `essentia_extractor`**：`xtractor` 若在工作台家里被勾选，config 里只有
   `plugins: xtractor`，插件会拿占位符去 spawn。索引家是它唯一的正式位置，这一条
   等批 5 再决定要不要也写一份。
3. **highlevel 永远拿不到**：Gaia2 不支持 Windows，mood / gender / genre 那 12 个字段
   在这条路上没有解（docs/50 §2.5）。bpm + danceability + beats_count + average_loudness 是上限。
4. **AGPL 的"源码提供"**仍是务实做法（§1.6），不是法律意见；pip 包的许可文本没进包。
5. `beets-xtractor` 自 2024 年起没更新，补丁是我们单方面打的——它哪天发新版改了取路径逻辑，
   补丁会变成"重复修"（`apply()` 检测 `_aiya_input_path_patch` 标记，但不会检测上游是否自己修好了）。
