# 外部二进制的自动下载与自动配置（docs/53）

> 状态：**已实现并验证**（见 §5 拍板与 §6 结果）。原编号 52 与另一篇调研撞号，改 53。
> 日期：2026-10-30。
> 前置：docs/50（xtractor 与 Essentia 的 Windows 现状）、docs/51（安装链两路 + plugins.json）。

本轮要回答的那个问题：

> 「对于其他二进制，在自动化脚本中做自动依赖下载和自动配置，例如直接挂 imageio-ffmpeg 包，
> 以及子脚本方法用于直接运行 curl 下载 streaming_extractor_music，简化用户一侧的配置？」

**能做，而且必须做** —— 因为现状不是"用户少配一步"，而是"默认配置直接报错"。

---

## 0. 一句话

`plugins.json` 里三个外部二进制（`ffmpeg` / `fpcalc` / `streaming_extractor_music`）现在对
安装器是**字符串、只用来打标签**，没有任何一个会被装上。而默认 profile 里第一个就是
`replaygain`，它硬编码找 `ffmpeg`。实测：**没有 ffmpeg 时 beets 不是"少了归一化"，是整个 CLI
任何命令都起不来**。

三条取得路径都有确定答案（两个 PyPI/官方直链、一个 GitHub 直链），但**要不要替用户下发**是
产品 + 许可证问题，不是技术问题 —— 见 §4、§5。

---

## 1. 事实核查（本轮实测，命令与数字都在）

### 1.1 没有 ffmpeg，beets 全线不可用 —— 不是"某个插件降级"

临时库、`plugins: replaygain`、`PATH` 上没有 ffmpeg：

```
$ python -m beets -c cfg import -A -q D:\MUSIC_Player\test-music     # 导入完成
$ python -m beets -c cfg ls
beets.exceptions.UserError: replaygain initialization failed: could not find ffmpeg at ffmpeg
```

`ls` 也起不来 —— 因为插件在**启动时**初始化，失败即抛 `UserError`。

把同一个 ffmpeg 放进 PATH 再跑同一批命令：

```
$ PATH=/tmp/bin:$PATH python -m beets -c cfg ls          # 正常
$ PATH=/tmp/bin:$PATH python -m beets -c cfg replaygain
replaygain: Analyzing 3 tracks ~ ffmpeg backend...
$ 查库：rg_track_gain = -2.6 / -2.4 / -3.1，rg_track_peak = 0.2985
```

这条链的终点是引擎：`src-tauri/src/pump.rs:333 load_volume_carries_replaygain`（归一化开时
`SetVolume` 必须是 `0.85 × 0.501`）。**gain_db 是有人消费的真字段，不是攒着看的。**

### 1.2 replaygain 只能靠 PATH

`beetsplug/replaygain.py:289` 写死 `self._ffmpeg_path = "ffmpeg"`，**没有配置键**。
`convert` 同理（命令模板里就是 `ffmpeg -i $source …`）。所以 ffmpeg 必须"能在 PATH 上找到"，
写配置解决不了。

版本门槛（同文件 293–310 行）：`libavfilter >= 6.67.100` 或 `--enable-libebur128`。

### 1.3 imageio-ffmpeg 确实自带 ffmpeg.exe —— 你的方向是对的

`pip download imageio-ffmpeg==0.7.0 --platform win_amd64`：

```
轮子 31,247,004 B
└─ imageio_ffmpeg/binaries/ffmpeg-win-x86_64-v7.1.exe   87,638,016 B
   dist-info/licenses/LICENSE                          1,317 B（BSD-2，imageio 自己的）
   binaries/README.md                                  45 B（"Exes are dropped here by the release script."）
```

解出来直接能跑，且**满足 replaygain 的门槛**：

```
ffmpeg version 7.1-essentials_build-www.gyan.dev
configuration: --enable-gpl --enable-version3 --enable-static … 
libavfilter 10.4.100 / 10.4.100        ebur128 在 -filters 里
```

两个必须记下来的点：

1. **它是 GPLv3 二进制**（`--enable-gpl --enable-version3`，gyan.dev 的 essentials 构建）。
2. **轮子里没有 ffmpeg 的许可文本**，只有 imageio 那个 BSD-2。要分发就得自己补 NOTICE 与源码获取方式。

### 1.4 fpcalc 在 PyPI 上没有路 —— 名字是个陷阱

| 包名 | 结果 |
|---|---|
| `fpcalc` | No matching distribution |
| `pyfpcalc` | No matching distribution |
| `acoustid-fpcalc` | No matching distribution |
| `chromaprint` | 存在，但是**「彩色终端输出」**（0.5，2.7KB，2018，`print(fg=..)`），同名不同物 |

`pyacoustid` 只是 Python 绑定，不含二进制。所以 fpcalc 只能走 GitHub：

```
acoustid/chromaprint v1.6.2（2026-10-15）
chromaprint-fpcalc-1.6.2-windows-x86_64.zip   1,817,825 B
└─ chromaprint-fpcalc-1.6.2-windows-x86_64/fpcalc.exe   4,148,224 B
   $ fpcalc -version → 1.6.2 (FFmpeg Lavc62.11.100 Lavf62.3.100 SwR6.1.100)
```

### 1.5 Essentia 提取器的直链是稳的

```
HEAD https://data.metabrainz.org/pub/musicbrainz/acousticbrainz/extractors/essentia-extractor-v2.1_beta2-1-ge3940c0-win-i686.zip
→ 200，5,539,511 B，last-modified 2022-07-06
实际下载 18.9 s，sha256 = eb4e64e4334ae5c63962fdc6b8bdfbdb53ba11e4705373feeb89503c08ee2688
zip 内只有一个文件：streaming_extractor_music.exe  20,701,667 B
$ ./streaming_extractor_music.exe → "Error: wrong number of arguments / Usage: … input_audiofile output_textfile [profile]"
```

无许可文本（Essentia 是 AGPLv3，docs/50 §5 已定「不要把 exe 打进安装包」）。

### 1.6 curl 不该是脚本的下载通道

同一时刻、同一台机器：

```
$ curl -s -o /dev/null -w '%{http_code}' https://pypi.org/simple/imageio-ffmpeg/   → 000（30s 超时）
$ python -c "urllib.request.urlopen('https://pypi.org/pypi/imageio-ffmpeg/json')"  → 正常
$ python -m pip index versions imageio-ffmpeg                                      → 正常
```

原因不是网断了，是 curl 走 `127.0.0.1:54990` 代理时挂住，而 pip / urllib 用了另一条。
结论：**别在脚本里 `curl`**，用解释器自己的 `urllib`（尊重 `HTTP(S)_PROXY`、能设超时与大小上限、
不依赖系统有没有 `curl.exe`——Windows 10 1803 才有）。用户机上是 `urllib` 一条路，宿主侧
已有 `ureq` 的 `download_to`（`beets.rs:848`，带上限与形状校验）。

---

## 2. 三个二进制的取得路径（判定表）

| 二进制 | 谁的依赖 | 载体 | 下载 | 解出差 | 落盘 | 许可 |
|---|---|---|---|---|---|---|
| `ffmpeg.exe` | replaygain / convert / embedart | PyPI `imageio-ffmpeg 0.7.0` win_amd64 轮子（当信封） | 31.2 MB | `binaries/ffmpeg-*.exe` | 83.6 MB | **GPLv3** |
| `streaming_extractor_music.exe` | xtractor | MetaBrainz 直链 zip | 5.5 MB | 单文件 | 19.7 MB | **AGPLv3** |
| `fpcalc.exe` | chroma | GitHub chromaprint v1.6.2 zip | 1.8 MB | `…/fpcalc.exe` | 4.0 MB | LGPL（含 ffmpeg lavc） |

三条都能"自动下载 + 校验 + 落盘 + 配好"，差别只在**谁来分发**。

---

## 3. 机制设计

### 3.1 目录与注入点

```
{data_root}/beets-tool/
├─ venv/            路径 B 的虚拟环境（A 轨是 bundle/）
├─ bundle/          路径 A 的免配置包展开处
└─ bin/             ← 新增：ffmpeg.exe / fpcalc.exe / streaming_extractor_music.exe
```

- **PATH 注入放 Python 侧**：`cli.main()` 里、**import beets 之前**把 `…/beets-tool/bin` 前插到
  `os.environ["PATH"]`。理由：beets 的唯一入口在这里，宿主路（`aiya-beets serve`）与命令行路
  （`python -m aiya_beets`）一次覆盖；replaygain / convert 硬编码 `"ffmpeg"`，写配置文件没用。
- **xtractor 例外**：它读配置键 `xtractor.essentia_extractor`，所以 `cfg.render_config` 要写绝对路径
  （docs/50 §4.1 已定 `extractor_profile.highlevel.compute: 0`）。
- **bin/ 在 `beets-tool/` 下面** ⇒ 现有 `uninstall`（删 `--target` 目录）天然覆盖，不新增孤儿。
  数据（`beets/`、`beets-index/`）照旧不碰。

### 3.2 `binaries` 段写进 `plugins.json`（schema 2）

`plugins[].exe` 现在是裸字符串（只够打标签），改成引用顶层 `binaries` 的名字：

```jsonc
"binaries": [
  {
    "name": "ffmpeg",                       // plugins[].exe 引用它
    "target": "ffmpeg.exe",                 // 落到 bin/ 下的文件名（固定，别跟轮子内部名字走）
    "carrier": {                            // 载体 = 轮子/压缩包，exe 从里面抽出来
      "kind": "zip",                        // 轮子本身就是 zip，所以只有这一种
      "url": "https://files.pythonhosted.org/packages/e5/55/1d50cdd78d862a30c2ce44f01cea387f77b364b21fa2fecf1590258b215b/imageio_ffmpeg-0.7.0-py3-none-win_amd64.whl",
      "size": 31247004,
      "sha256": "7a22b45f60321d4172bdf2bc61f716bce4c81b85727bd59c7c7e371055a6a440",
      "member": "imageio_ffmpeg/binaries/ffmpeg-*.exe"   // glob，命中必须恰好 1 个
    },
    "size": 87638016,
    "sha256": "2ce797a0f88d7f067180338fb227f7b1928ea727bd9a4d7a1d022f7c52af71a3",
    "probe": ["-version"],                  // 落盘后跑一次，退出 0 且 stdout 非空
    "license": "GPL-3.0-or-later",
    "vendor": "gyan.dev ffmpeg 7.1 essentials（经 imageio-ffmpeg 0.7.0 分发）"
  },
  { "name": "streaming_extractor_music", "target": "streaming_extractor_music.exe",
    "carrier": { "kind": "zip", "url": "https://data.metabrainz.org/…win-i686.zip",
                 "size": 5539511, "sha256": "eb4e64e4…e2688", "member": "streaming_extractor_music.exe" },
    "size": 20701667, "sha256": "…", "probe": [], "license": "AGPL-3.0-or-later" },
  { "name": "fpcalc", "target": "fpcalc.exe", … }
]
```

为什么 `member` 用 glob 而不是写死 `ffmpeg-win-x86_64-v7.1.exe`：轮子内部文件名带版本，
升级轮子时名字会变。glob + **"必须恰好命中 1 个"** 两头都钉住（命中 0 个 = 载体换了，命中
2 个 = 有人在里面塞了第二个 exe，都要报错而不是随便挑一个）。

为什么把 `imageio-ffmpeg` 当信封而不是 `pip install` 进去：
1. 它不是 beets 的运行时依赖，装进 venv 只会让 `checkModules` 自检多一个无关模块；
2. 我们要的是一个**固定路径的 `bin/ffmpeg.exe`**（PATH 注入、状态显示、卸载清理都要靠它），
   轮子内部名字带版本，还得再 `import imageio_ffmpeg` 才知道路径；
3. 版本由我们 pin（`url` + `sha256` 就是 pin），而不是"下次装依赖时 pip 顺手升了个大版本"。

### 3.3 命令面

```bash
python aiya-beets-installer.pyz bins [--only ffmpeg,essentia,fpcalc | --from-plugins NAMES] [--dry-run] [--check]
```

- `--check`：只体检（存在 / 版本 / 路径），不下载 —— 状态页用；
- `--dry-run`：打印将下载什么、多大、落哪；
- `--from-plugins replaygain,lyrics`：**宿主用这条**（docs/55 §3.3）。传插件名而不是
  exe 名——"哪个插件要哪个工具"只有 `plugins.json` 一处知道，宿主算就是第二份派生；
- 复用 `install.py` 已有的事件流（`progress` / `done` / 错误码），UI 不用改协议。

`bins` 也可以被 `install` 串起来跑：`install --plugins replaygain` ⇒ 发现 replaygain 的 `exe`
里有 ffmpeg ⇒ 自动把 ffmpeg 加进本次下载清单。**"勾了需要它的插件就带上它"**，不给用户多一个
"还要不要再点一次"的选择。

### 3.4 校验三件套（缺一不可）

1. 载体：`size` + `sha256`（下载完立刻算，不等解压）；
2. 成员：`member` glob 恰好命中 1 个，且成员自身的 `size`/`sha256` 也对得上；
3. 可用：`probe` 跑一次（`ffmpeg -version` / `fpcalc -version`；essentia 无参调用退出码非 0
   但 stderr 有 `Usage:`，所以它的 `probe` 记成"允许非 0 退出，只要输出里含 `Usage:`"）。

第 3 条不是洁癖：**32 位 exe 在 64 位 Windows 上要靠 WoW64**，个别精简系统会缺；下完能跑才算装上。

---

## 4. 分发姿态（三档，体积与法律两条账）

> 结果：拍板落在 **F1**（三个都不预置），见 §5。下表留着是为了记住"为什么没选 F2/F3"。

| | 谁下载 | 免配置包体积 | 落盘 | 我们的合规义务 | 断网可用 |
|---|---|---|---|---|---|
| **F1 不预置** | 用户机（`bins` 现跑） | 28.3 MB（不变） | +107 MB | 无（我们不转发第三方二进制） | ❌ 首次要联网 |
| **F2 全预置** | 我们（CI 打进包） | ≈ 67 MB | +107 MB | 补 NOTICE + 许可全文 + 源码获取方式（GPLv3 + AGPLv3） | ✅ |
| **F3 混合** | ffmpeg 我们发，Essentia 用户机取 | ≈ 59 MB | +107 MB | 只需处理 GPLv3 | ffmpeg ✅ / essentia ❌ |

我的建议是 **F1 + 本地缓存**：`bins` 下完就留在 `bin/`，重装组件不重复下；再给一个
`AIYA_BEETS_BIN_DIR` 让用户指自己已有的 ffmpeg（复用 `fetch_asset` 那套"环境变量指本地文件"的
习惯，docs/51 已经在用）。理由：

- 免配置包的承诺是"不用装 Python、不用配环境"，**不是"不用联网"**——它本来就要联网下载；
- AGPL 与 GPL 的合规成本落在一个个人项目上不划算，而且 docs/50 §5 已经定过"不分发 Essentia"；
- 87.6 MB 的 ffmpeg 把免配置包从 28 MB 推到 67 MB，对"点一下就有"的体验提升不明显，
  对下载失败的容忍度反而下降（一次下 67 MB 断了要重来）。

但这是你的产品，**F2 也完全可做**，只是要加 `LICENSES/` 与 NOTICE 的活儿。

---

## 5. 已拍板（2026-10-30，用户原话）

> **后续变更（2026-10-31，docs/54）**：第 1 条对 **Essentia 提取器**破例了 —— 用户要求
> 「让 essentia_extractor 进免装包，让 xtractor 插件默认携带」。`ffmpeg` 与 `fpcalc`
> 仍然不进包，所以本文其余部分（下载/校验/落 PATH）照旧有效，只是多了"包内 `bundle/bin`
> 也算一个查找来源"。AGPLv3 的分发义务随之回到我们身上，补偿措施见 docs/54 §1.6。

1. **三个二进制都不进免配置包**：`ffmpeg` 不进，`fpcalc` 也不进（原话「ffmpeg 不进入免配包，
   fpcalc 也不进免配包」）。于是免配置包体积不变（28.3MB），全部走用户机下载 + 校验 + 落 `bin/`。
   我们不转发第三方二进制 ⇒ 不承担 GPLv3 / AGPLv3 的分发义务（docs/50 §5 的老纪律继续有效）。
2. **需要 ffmpeg 的插件不进默认集**（原话「所有需要 ffmpeg 的插件都不进入免配包」）：
   - `profiles.workbench` 摘掉 `replaygain`（现为 lyrics / fromfilename / duplicates /
     scrub / smartplaylist，全是纯 Python）；
   - `bundle: true` 保持不含任何带 `exe` 的插件（契约测试与 Rust 单测两处都钉住）；
   - **渲染侧过滤**兜底：`exe` 找不到就绝不写进 `plugins:`。老配置、手删 `bin/`、
     从别的机器拷来的 `settings.json` 都靠这一道，否则 beets 整个 CLI 起不来（§1.1）。
     用户的勾选留在 `settings.json` 里不删，`bins` 补齐后下一次启动自动恢复。
3. **fpcalc 一起做**（原话「一起做，三条都自动化」）。它只服务 `chroma`，但机制与另外两个
   完全一样，多一条目录项而已。

---

## 6. 实现结果

| 批 | 内容 | 结果 |
|---|---|---|
| C1 | `plugins.json` **schema 2**：新增顶层 `binaries`（3 条，含载体 url/size/sha256/member 与落盘物 size/sha256/license）；`profiles.workbench` 摘掉 replaygain | ✅ 契约 9 个负向逐个变红（含「exe 引用不存在的二进制」「载体不是 https」「sha256 写短」「member 留空」「声明了没人用」） |
| C2 | `bins.py`（下载 / 双哈希校验 / 抽 member / probe / 原子落盘）+ `cli bins [--only --check --dry-run --carrier]` + `install --plugins` 联带 | ✅ 三个全部真下真跑：ffmpeg 31.2MB→83.6MB（54s）、essentia 5.3MB→19.7MB、fpcalc 1.7MB→4.0MB |
| C3 | `cli.main` 里 `bins.inject_path()`（import beets 之前）+ `cfg` 过滤与 `xtractor.essentia_extractor` | ✅ 有 `bin/ffmpeg` 时 `beet ls` exit 0；PATH 抽掉 ffmpeg 立刻 `UserError`。`config_contract` 新增 [11] 三向（索引家整条不写 / 用户库只留可用的 / 闸兜底变红） |
| C4 | 宿主：`start_install` 两个轨道装完都接着跑 `bins`（`plugin_table()` 算需要哪些） | ✅ `cargo test -p aiya --lib beets::` 40 过；`clippy -D warnings` 干净 |
| C5 | UI：勾选标签「还需 X」→「另下 X」（不再是"自备"） | ✅ mock 与 e2e 文案同步 |

> 后续（docs/54，2026-10-31）：Essentia 提取器改成随免配置包发布，`xtractor` 也随之
> 默认携带（插件包 `beets-xtractor` + 提取器 + 路径补丁 + 档位四件套）。包从 28.3MB → 33.7MB。

### 顺带修掉的一个真 bug：pyz 永远退出 0

`zipapp.create_archive(main="aiya_beets.cli:main")` 生成的 `__main__.py` 是
`import …; aiya_beets.cli.main()` —— **返回值被丢掉**。实测：

```
$ aiya-beets-installer.pyz bins --only nope
{"type":"error",…,"不认识的外部工具 nope"}
$ echo $?        → 0        # 源码树同一条命令是 2
```

这和 docs/51 §0 记的「A 轨道从来没装成过，因为退出码被忽略」是同一类事故：错误发生了，
退出码说没事。修法是加一层 `entry.py`，只做 `sys.exit(cli_main())`；`build_installer.py`
里再加一条**门禁**：打完包跑一次错误路径，退出码是 0 就判打包失败。现在产物自检会打印
「错误路径退出码 2」。

### 仍未做的

- **状态页没有"外置工具"这一区**：现在只能从插件三态间接看出来（replaygain `missing_deps`
  → `missing: ["ffmpeg"]`）。补齐的办法是再点一次安装/`aiya-beets bins`（已装的会跳过）。
  独立的 `beets_bins` 命令与 UI 区块留到下一批，理由：现有安装入口已能覆盖"首次装"与
  "重装补齐"，新命令要另开一条事件流，等真的需要单独重试时再动。
- **免配置包轨道勾选 pip 插件仍然不生效**：包里预置的是固定集合，`chroma` 要的 pyacoustid
  不在里面。所以 `plugins: chroma` 在 A 轨依旧只会拿到 `fpcalc` 而没有 Python 绑定，
  渲染侧会把它当 `unavailable` 剔掉。这不是本次引入的问题，是 A 轨的既有边界。
