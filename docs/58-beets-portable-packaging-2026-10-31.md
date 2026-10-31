# 58 · Beets 便携版打包全流程与来源注记（2026-10-31）

> 本文是 beets 便携发布链路的**单一入口注记**：两条轨道的分工、发布物的每个
> 字节从哪来（含全部 sha256 钉扎）、Gaia/SVM 模型的来龙去脉、CI 自检清单。
> 排障从这篇开始，不用再翻散落的 workflow 与脚本注释。

## 1 · 定位与两轨

beets 模块 = beets helper GUI。发布物两条轨道，能力面从安装到卸载完全分开：

| | 预配置环境（bundle，普通用户） | 本机安装（installer，高级用户） |
|---|---|---|
| 载体 | `beets-portable-win-x64.zip`（自带解释器） | `beets-installer.pyz`（~100KB 引导器） |
| 用户侧前置 | 无 | 系统 Python（3.10–3.14） |
| 安装动作 | 播放器直连 HTTP 下载 + 校验 + 解压（Rust） | 脚本建 venv、按勾选装插件（Python） |
| 插件 | 固定清单（lyrics/fetchart/mbsync/xtractor），不可改 | 勾选表任意组合，可换插件重装 |
| 移除 | 「删除组件」= 目录整体移除（Rust 直删） | 「卸载组件」= 脚本过四道闸算账，宿主执行删除 |
| 数据 | `beets/` 与 `beets-index/` 两个家（用户数据）**永不**随组件删除 | 同左 |

## 2 · 通道模型（推新删旧）

- 固定下载通道：release tag `beets-portable`（prerelease）——**只有这一条**，
  没有版本分支。发布 = `git tag -f beets-portable && git push -f origin beets-portable`，
  CI 整条 release 连旧 tag 删掉重建。宿主常量指向
  `releases/download/beets-portable/<asset>`，固定 URL、零 API 查询。
- 标题用日期（`beets portable（2026-10-31）`）不用小版本——只有一版滚动打包，
  别干扰应用主体的版本线；prerelease 保证仓库 "Latest" 永远只落在播放器上。
- 触发：tag 推送或手动 dispatch（勾 publish）。构建绝不带版本号进资产名。

## 3 · 打包流程（`build_bundle.py`，CI 与本地同一条命令）

1. **embed 解释器**：官方 embeddable 下载 + sha256 钉扎（`EMBED_SHA256`）。
2. **依赖**：uv 按包内解释器解析 wheel（`--python <embed>/python.exe`，产出
   cp314 tag），lock + bundle 插件依赖一次装完。uv 只是**打包机**的工具，
   用户侧两条轨道都不碰它。
3. **bins 抽取**：`binaries[].bundle: true` 的外部件按 sha256 逐件校验落位
   `bin/`（共享载体只下载一次——提取器+Qt5Core 同一个 zip、12 件模型同一个
   zip，`bins.py::stage/ensure` 的载体缓存负责去重）。
4. **manifest**：`plugins/packages/binaries/selfCheck` 落进包根，"包里到底
   预置了什么、验过没有"必须可查。
5. **zip + 体积门禁**（CI：≥55MB——Gaia 构建 + 12 件模型是包体大头）。
6. **解包自检**（CI）：包内解释器跑 `aiya_beets version`、`bins --check`
   missing 为空、**Gaia/SVM 冒烟**（`tools/essentia_smoke.py`：正弦波 +
   svm_models 指向包内模型 → 必须产出 `highlevel.danceability`）。

## 4 · 来源与钉扎总表（全部 sha256 钉进 `data/plugins.json`）

| 工件 | 来源 | sha256（截断） | 说明 |
|---|---|---|---|
| embed 解释器 | python.org 官方 embeddable | `90905e64…`（3.14.10） | 打包脚本 `EMBED_SHA256` |
| `streaming_extractor_music.exe` | [brendanerofeev/essentia gaia-win64-2](https://github.com/brendanerofeev/essentia/releases/tag/gaia-win64-2)（源起 regorxxx 的 Actions 构建，commit 14dce99b） | `3567cd0e…`（12,096,512 B） | **带 Gaia/SVM** 的 x64 构建，替换 AB 官方 win-i686（无 Gaia2，见 §5） |
| `Qt5Core.dll` | 同上（Gaia 构建非静态链接） | `71b73ad1…`（6,130,176 B） | 运行时必须与提取器同目录；LGPL |
| Gaia 载体 zip | 同上 release 资产 | `3937f009…`（13,424,829 B） | carrier 整包钉扎 |
| `svm-models.zip`（12 件 .history） | 本仓库 release [svm-models-v2.1_beta5](https://github.com/yeraph-plus/aiya-music-player/releases/tag/svm-models-v2.1_beta5)（自托管，源起 [essentia.upf.edu/svm_models](https://essentia.upf.edu/svm_models/) beta5 归档重打包） | `31206754…`（17,753,458 B） | 逐件 sha256 亦钉在表内 |
| ffmpeg / fpcalc | PyPI imageio-ffmpeg / acoustid chromaprint（docs/53） | 表内钉扎 | 按 exe 清单按需下载 |

为什么模型要自托管：upf.edu 对家用宽带 ~17KB/s 且频繁断流（实测 beta1 归档
拉了 35 分钟、两次截断）；runner 上 3 秒拉完。所以 CI 抽 12 件重打包挂到本仓库
固定 release（`svm-models-selfhost.yml`，幂等），载体走 GitHub。

## 5 · Gaia/SVM 模型的来龙去脉（为什么换二进制）

- AB 官方 win-i686 提取器（原载体）**没编 Gaia2**：配了 svm_models 直接
  `Identifier 'GaiaTransform' not found in registry` 退出 1，一个 json 都不产出
  （docs/50 §2.5 实测，当时把 `highlevel.compute` 钉死 0）。2026-10-31 复测：
  干净环境跑通但 `highlevel` 整段缺失——danceability/mood 全空，bpm/响度/key
  不受影响（lowlevel/rhythm/tonal）。
- 社区 Gaia 构建（regorxxx Actions → brendanerofeev release）实测通过：
  **beta5 模型 exit 0、highlevel 18 键全出**（正弦波判 not_danceable/happy/
  female——合理）；**beta1 模型 exit 1**（`analysis_sample_rate could not be
  found in pool`，模型与二进制的描述符布局必须匹配——beets-xtractor 文档
  "beta5 模型配 beta5 二进制"因此成立）。
- 配置由 `cfg.py::xtractor_profile(exe)` 渲染：`compute: 1` + 12 件
  `models/*.history` 绝对路径（yaml_path 单引号转义）；配置门禁反向三连：
  compute 关、清单缺失、模型文件缺失都变红。`_render_tree`/`_parse` 因此补了
  YAML 列表支持（此前 repr 一行串是坏 YAML，反斜杠翻倍）。

## 6 · 本批遗留

- **enrich 真实提取接线（批 5）**：`PROBE_FIELDS`（bpm/danceability/mood_*）
  目前是 sha256 合成桩；模型与配置就位后，批 5 把提取 JSON 的 highlevel 映射进
  flex attrs，桩才能退役。
- 本地打包验证因网络跳过（载体重试修复已入库）；CI 解包自检 + 冒烟是发布闸门，
  下一次 `beets-portable` 发布即完整验证。
