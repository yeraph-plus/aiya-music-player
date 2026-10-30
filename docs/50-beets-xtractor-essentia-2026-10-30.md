# xtractor 与 Essentia 的 Windows 现状（docs/50）

> 状态：**调研完成，未改任何代码**。结论是「现在这条路在 Windows 上只能拿到 4 个 lowlevel
> 字段，而且还要先修第三方插件的 bug」，highlevel（mood/gender/genre/voice）在 Windows
> 上没有任何可分发路径。  
> 日期：2026-10-30。  
> 前置：docs/45 §6.2（索引家插件白名单＝名字+强制参数+写盘标志）、
> docs/49 §8（缺 xtractor 是已知项）。

---

## 0. 一句话

索引家模板里 `plugins: xtractor` 是**假开**——运行环境里根本没有这个插件，每次 `beet import`
都会往 stderr 吐一段 traceback 然后照常跑（退出码 0）。而即使装上，Windows 上也还有两道
拦路的：Essentia 的 highlevel 分类器（Gaia）没有 Windows 构建；`beets-xtractor 0.4.2`
用 `item.get("path")` 取路径，在 beets 2.14 的「相对路径」存储下每首都会 `FileNotFoundError`。

能拿到的只有 lowlevel 四项：`bpm` / `danceability` / `beats_count` / `average_loudness`。

---

## 1. 现状实测（三处，都是硬证据）

### 1.1 模板开了，环境里没有

`{app_data}/beets-index/config.yaml`（生成物）里是 `plugins: xtractor`，
但 `beets-tool/.runtime/.../site-packages` 与 `.venv` 里都只有 numpy，没有
`beetsplug/xtractor`，也没有 `essentia`。

跑一次索引家配置：

```
$ python -m beets -c <beets-index>/config.yaml version
Traceback ... beets/plugins.py:522 in _get_plugin
beets.plugins.PluginImportError: Could not import plugin xtractor
beets version 2.14.1 / Python version 3.14.7
no plugins loaded
exit=0
```

**退出码 0**。也就是说每次索引都会打一段 traceback，但我们没在看 stderr。这正是
docs/31 P0-1 说过的「beets 对插件导入异常只写日志不抛错」——只不过 2.14 连 traceback
一起打了。

### 1.2 市场里没有这个插件

`plugins.py::MARKET_CATALOG` 的 13 个条目里没有 xtractor（只有 `INDEX_PLUGINS` 白名单
里提到它）。所以 UI 上**没有任何安装入口**——「缺 xtractor」缺的是这一段。

`beets-xtractor` 在 PyPI 是纯 Python 包（`py3-none-any`，12KB，0.4.2，2024-05-26），
实测 `pip download` 一次成功。它的 `requires_dist` 只有 `beets>=1.4.9` 和 `pyyaml`——
**不声明 essentia 依赖**，所以装完不会提示「你还差个 20MB 的 exe」。

### 1.3 索引库的 MBID 命中率是 0

```
items 1849 / mb_trackid 0 / mb_albumid 0 / mb_releasetrackid 0
```

这条数据很重要：它直接否掉了「不本地算、改查 AcousticBrainz 历史计算结果」的替代方案
（见 §4.3）——AB 是按 MBID 索引的，我们库里一个都没有。

---

## 2. Essentia 到底怎么装（Windows，四条路逐条判）

先把一个常见误解拆掉：**xtractor 不 import essentia 的 Python 绑定**。
它调的是外部可执行文件（`command.py:250`）：

```python
cmd_and_args = [extractor_path, input_path, output_path, profile_path]
```

`extractor_path` 来自配置键 `xtractor.essentia_extractor`，默认值是占位符
`/your/path/to/streaming_extractor_music`。所以**只要有一个能跑的 exe 就行，不需要 Python 包**。

### 2.1 pip 装 essentia / essentia-tensorflow —— ❌ Windows 无发行版

```
$ pip install --dry-run --only-binary=:all: essentia
ERROR: Could not find a version that satisfies the requirement essentia (from versions: none)
```

PyPI 的 `essentia` 索引里，全平台文件只有 `manylinux*` 与 `macosx*`；
`essentia-tensorflow` 的 75 个 wheel 同样是 Linux/macOS。最新的 `2.1b6.dev1438`
已经编到 cp314，仍然没有 win。

第三方轮子也查了：`essentia-win` / `essentia-wheel` / `pyessentia` / `essentia-bin` /
`essentia-python` 全部 `No matching distribution`。

### 2.2 conda-forge —— ❌ 包不存在

```
GET https://api.anaconda.org/package/conda-forge/essentia
{"error":"\"essentia\" could not be found"}
```

### 2.3 官网 static binaries —— ⚠️ 本机不可达，且已迁走

`https://essentia.upf.edu/` 在本机连不上（`curl` 报 `CONNECT tunnel failed, response 502`，
`WebFetch` 也是 fetch failed）。不是站点挂了，是这台机器到该站的路由/代理不通。

而官网 2015 年放的那批 static binaries，在 AcousticBrainz 下线时被镜像到了 MetaBrainz
的存储上（见下表）。官网本身是否还挂着新版本，本机验不了。

### 2.4 MetaBrainz 镜像 —— ✅ 实测可用

AcousticBrainz 停止收数据时把工具留在 `data.metabrainz.org`：

| 资产 | 地址 |
|---|---|
| Windows 提取器（5.5MB zip） | `.../acousticbrainz/extractors/essentia-extractor-v2.1_beta2-1-ge3940c0-win-i686.zip` |
| SVM 模型全集（约 37MB） | `.../acousticbrainz/svm_models/`（`danceability.history`、`mood_*.history` 等 12 个 + `.param`） |

实测记录：

```
http=200  下载 5539511 字节（240s 超时，需 -C - 续传一次）
解压 → streaming_extractor_music.exe  20701667 字节  2014-11-19
```

跑一首 440Hz wav：

```
$ ./streaming_extractor_music.exe "中音测试 440Hz.wav" out.json
Process step: Compute aggregation / All done / Writing results to file
real 1.135s   exit=0
top keys: ['lowlevel','metadata','rhythm','tonal']
bpm: 120.185   danceability: 0.4789   average_loudness: 0.806
```

**32 位 exe 在 64 位 Windows 上能跑**（WoW64），不需要任何运行库。

### 2.5 但 highlevel 不行 —— Gaia 没编进去

按 README 配 `svm_models` 后：

```
$ ./streaming_extractor_music.exe in.wav out3.json prof2.yml
Identifier 'GaiaTransform' not found in registry...
exit=1        # 连 json 都不生成
```

官方文档原话：SVM 模型需要 `Install Gaia2 library (supported on Linux/OSX)`。
所以 mood / gender / genre_rosamerica / voice_instrumental / moods_mirex 这 12 个
highlevel 字段，在这个 Windows 构建上**拿不到**，而且配了模型会让整个 exe 退出 1。

自编译也没戏：gaia2 本身不支持 Windows。TensorFlow 那条路（Essentia-TF 的 MusiCNN）
同样没有 Windows wheel。

---

## 3. 第三个阻断：插件自己的相对路径 bug

> **订正（2026-10-31，docs/54 §1.2）**：下面这段当时写「`item.path` 是绝对」——**实测不成立**，
> `item.path` 在 beets 2.14.1 上同样是**相对曲库根**。所以修法不是"换成 `item.path`"，
> 而是 `os.path.join(lib.directory, item.path)`（全仓已有 `beetslib.item_abs_path`）。
> §4.1 的 A 方案已按此落地，见 docs/54。

把插件装到隔离目录（`pip install --target`）后，拿一首已有 item 的临时库跑：

```
FileNotFoundError: Input file(中音测试 440Hz.wav) not found!
  beetsplug/xtractor/command.py:294
```

原因在 293 行：

```python
input_path = item.get("path").decode("utf-8")
```

beets 2.14 起 `items.path` 列存的是**相对曲库根的路径**，`item.path`（属性）才会合成绝对
路径。实测同一个 item：

```
item.path = b'D:\\MUSIC_Player\\test-music\\中音测试 440Hz.wav'   # 绝对
item.get('path') = b'中音测试 440Hz.wav'                          # 相对（插件用的是这个）
```

所以哪怕 Essentia 装好了，插件的每一首都会失败。修法只能是**在 `aiya_beets` 侧打
monkeypatch**（把它换成 `item.path`），不改第三方包——否则一升级就回退。

---

## 4. 三种方案

### 4.1 A：外部工具 + 只开 lowlevel（推荐）

- `pip install beets-xtractor`（走现有 market 机制）；
- 用户自己下 `streaming_extractor_music.exe`（应用给 §2.4 的地址 + 文件选择 + 存在性校验）；
- 索引家配置写 `xtractor.essentia_extractor: <path>`，`extractor_profile.highlevel.compute: 0`；
- `aiya_beets` 启动时 monkeypatch `_get_input_path_for_item`；
- 索引库多出 `bpm` / `danceability` / `beats_count` / `average_loudness`。

代价：用户要下一个 5.5MB 的包（网络慢的话得续传）。收益：BPM 是推荐/歌单/排序都要用的
基础字段，现在库里是空的。

### 4.2 B：自编译带 Gaia 的 Windows 版 —— ❌ 不建议

gaia2 不支持 Windows，要改它的源码 + MSVC 工具链，产出一个我们不敢分发的二进制。

### 4.3 C：不本地算，改查 AcousticBrainz —— ❌ 现在不可行

AB 有 756 万条 recording 的 lowlevel + highlevel JSON（还在线，API 实测返回 404 而不是
连不上，说明服务活着）。但它按 MBID 索引，而**索引库 1849 首的 `mb_trackid` 是 0 条**。
要拿 MBID 就得先跑 chroma 指纹 + 联网匹配，而那正是 docs/49 §8.1 里还没通的那条路。

只有当「指纹 → MBID」这条链修好后，C 才有意义——而且它能补的恰好是 A 拿不到的 highlevel。

---

## 5. 许可证

Essentia 是 **AGPLv3**。A 方案里我们只调用用户自己下载的 exe（进程边界、不分发二进制），
和现在 ffmpeg / fpcalc 的处理一致，不构成传染。**不要把 exe 打进安装包**。

> **已作废（2026-10-31）**：用户拍板把提取器打进免配置包（docs/54）。AGPLv3 的分发义务
> 现在由发布物承担：包内 `THIRD-PARTY.md` + `LICENSES/AGPL-3.0.txt` + manifest 的
> `binaries`。这不是法律意见，见 docs/54 §1.6。

---

## 6. 建议删掉的那一行

索引家模板无条件写 `plugins: xtractor`，在 Windows 上等于每次 import 都吐 traceback。
在 A 落地之前，建议改成按「插件可导入 + exe 存在 + 模型不需要」三条同时成立才写进
`plugins`，否则不写——`cfg.render_config` 里 `INDEX_PLUGINS` 是常量，需要加一个
「按环境过滤」的入口。

---

## 7. 验证记录

| 项 | 结果 |
|---|---|
| `pip install essentia`（Windows） | `from versions: none` |
| conda-forge essentia | 包不存在 |
| `essentia.upf.edu` 可达性 | proxy 502（本机） |
| MetaBrainz Windows exe 下载 | 200，5,539,511 字节（续传一次） |
| exe 跑 440Hz wav | exit 0，1.135s，bpm 120.19 / danceability 0.479 |
| highlevel + svm_models | `GaiaTransform not found`，exit 1 |
| `beets-xtractor` pip 下载 | 成功（py3-none-any，0.4.2） |
| 索引家配置加载 | `PluginImportError: Could not import plugin xtractor`，exit 0 |
| 索引库 mb_trackid 非空数 | 0 / 1849 |
| 插件跑临时库 | `Input file(...) not found!`（相对路径 bug） |

临时产物都在 `%TEMP%\ae`、`%TEMP%\xtest`、`%TEMP%\xtsite`，未进仓库、未动真库。

## 8. 没验到的

1. 走完 A 全流程后在真索引库上的耗时与字段落库（要 patch 之后才能跑）；
2. 32 位 exe 对 mp3/flac/m4a 的解码覆盖（只测了 wav）；
3. 中文路径/超长路径下 exe 的参数传递（实测那首文件名是中文，正常）；
4. 索引家「写盘开关」在装了 xtractor 之后是否仍然钉得住——`write: no` 的 forced 已有
   契约测试，但没在「插件真的加载成功」的状态下再跑一次。
