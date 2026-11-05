# 67 · beets-tool Linux 提取器构建与路线验证（2026-11-06）

- 类型：调研 + 落地记录（Linux x86_64 提取器测试构建链路，含全部证据与钉扎值）
- 状态：closed（路线验证通过，两次全绿 run；转正接线待 `build_bundle.py` 平台化批次）
- 关联：docs/58（beets 便携发布链路单一定位文档——win 轨；本篇补 Linux 轨）；
  docs/50 §2.5 / 53 / 54（essentia 与外置二进制踩坑源头）；docs/65 / 66（web/server 侧为
  消费方，按 manifest entry/args spawn，宿主零改动）；ledger/beets-tool.md
- 工件：`.github/workflows/build-essentia-linux.yml`（main @ 7ef3f0f 合入）

## 0 · 结论

- **路线**：MTG 官方构建容器 `mtgupf/essentia-builds:manylinux_2_28_x86_64`（ghcr 同名
  tag 兜底）内 waf 静态构建：`waf configure --with-gaia --build-static
  --static-dependencies --with-examples`。镜像把全部静态第三方依赖（Gaia + Qt5 +
  ffmpeg + fftw + taglib + chromaprint 等）预编译进 `/usr/local`，钉扎版本
  `ESSENTIA_3RDPARTY_VERSION=v2.1_beta5-1555-g33108c70`——源码取同一 commit
  （`33108c70929684ed14091dfb671a30264cf8bb84`，2026-10-22），构建命令与 MTG 官方
  wheel CI（`travis/build_wheels.sh`）同款 + `--with-examples`。
- **验证**（2026-11-06，两次全绿）：run 42093590915（分支验证）+ run 42098420001
  （main 合入复验）。12 件 beta5 模型加载、highlevel 12 键全出
  （danceability/gender/genre_rosamerica/mood_acoustic/mood_aggressive/mood_electronic/
  mood_happy/mood_party/mood_relaxed/mood_sad/moods_mirex/voice_instrumental）、
  正弦波判 **not_danceable（概率 1）+ happy**——与 docs/58 §5 的 win 实测行为一致。
- **二进制**：17,785,272 B，sha256 `5370d6af15d7455e692ef07f48cfbe06ac68ffaf039d290c9fe4b4d281b68feb`
  （本地复验 = CI SHA256SUMS）；`metadata.version` 自报 essentia **2.1-beta6-dev**
  （与 win 包的社区构建同版本族）。**sha256 会漂移**：镜像 tag 是浮动 tag 未做 digest
  钉扎，每次以产物 SHA256SUMS 为准。
- **静态/动态边界（ldd 实测）**：ffmpeg/fftw/taglib/chromaprint/gaia **全部静态链入**；
  仅 `libQt5Core.so.5` + `libQt5Concurrent.so.5`（镜像 `/usr/local` 提供）+ libdrm/
  libzstd/libstdc++/glibc 系动态 → 转正时随包附 Qt5 两个 so（对应 win 包附
  `Qt5Core.dll` 的同款载体模式），libdrm 是否需要随包待宿主实测。
- **glibc 下限 2.28**（manylinux_2_28 底座）：Ubuntu 20.04+ / Debian 10+ / RHEL 8+。
  压到 2.17 需换 manylinux2014 底座重编依赖，转正时再评估。

## 1 · 为什么是这条路线（调研结论）

- **带 Gaia 的 Linux 提取器没有任何现成 release**（官方/社区均无，brendanerofeev 仓库
  全部 4 个 release 都是 win64）。官方 upf linux 静态包**无 Gaia**（MTG issue #1265 原文：
  配 svm_models 报 `Identifier 'GaiaTransform' not found in registry`——与 win-i686 同坑），
  且 2015 年的 2.1_beta2 linux 包在新内核上 vsyscall segfault——**旧静态二进制必须用新
  工具链重编，不能指望旧归档**。
- MTG 自己的 wheel CI 底座就是官方答案：essentia-builds 镜像 = manylinux 基座 + 预编译
  静态依赖（含 gaia），官方 wheel 每次都在里面构建 `--with-gaia` 目标，基建是活的。
- SVM 模型侧零改动：12 件 beta5 `.history` 是平台无关数据，自托管 release
  `svm-models-v2.1_beta5/svm-models.zip`（sha256 `31206754…`）原样复用；
  `cfg.py::xtractor_profile` 的 yaml 渲染平台无关（正斜杠合法）。
- 许可证：Gaia2 = **AGPL-3.0**（非 MPL）；模型 = CC BY-NC-ND 4.0。win 包三件套
  （AGPL / LGPL / MTG-Model-Notice）原样复用，无新增义务。

## 2 · 落地（workflow 的形态）

- **触发**：`workflow_dispatch`（手动测试构建工具，不挂 push/发布通道）。
- **边界**：产物只进 workflow artifact（`essentia-extractor-linux-x86_64-test`：二进制 +
  smoke.json + ldd.txt + SHA256SUMS + version.txt），**不写 release、不动 beets-portable
  通道**；零引用仓库内打包代码（冒烟逻辑按 `tools/essentia_smoke.py` 内联复刻，转正接线
  时再重构）。
- **容器内四步**：① tarball 拉 essentia（免 git，wscript 的 `get_git_version` 有
  `.git` 存在性守卫，tarball 安全）；② waf 静态构建（`PKG_CONFIG_PATH` 由镜像 ENV
  提供，脚本有哨兵）；③ 冒烟（sha256 校验载体 → 解 12 件 → stdlib 正弦波 + 内联
  profile.yaml → 断言退出 0 且 `highlevel.danceability` 在场）；④ 取数（ldd + SHA256SUMS）。
- **冒烟口径铁律**：模型缺失/不配时提取器表面跑成功但 highlevel 整段消失（docs/50 §2.5、
  docs/54 §1.3）——"退出 0"不算数，`danceability` 在场才算数，这道闸门转正后必须保留。

## 3 · 三轮踩坑（远程测试分支已删，教训只留此处）

1. **tar.gz 配了 unzip**：GitHub archive 下 `.tar.gz` 后用 `unzip` 解，exit 9
   （End-of-central-directory）。archive 要么下 `.zip`（镜像内依赖构建同款），要么 `tar xzf`。
2. **缺 `--with-examples`**：MTG wheel 命令不含它（wheel 不编 examples），照抄后 4 分钟
   编完只有 `libessentia.a`——提取器正是 examples 目标。
3. **产物名带 `essentia_` 前缀**：`src/examples/wscript` L157 给所有 examples 统一加
   `essentia_` 前缀，实际名为 `essentia_streaming_extractor_music`（构建其实已成功）；
   find 按 `*streaming_extractor_music` 尾部通配取。

## 4 · 转正清单（接 `build_bundle.py` 平台化时逐项消化）

- **build_bundle.py**：EMBED 来源换 python-build-standalone（cp314 install_only tarball，
  glibc 2.17 下限，URL 中 `+` 编码 `%2B`）；`python.exe` 硬编码 5 处；`_pth` 机制是
  Windows embeddable 独有，PBS 走标准 prefix 布局（`lib/python3.x/site-packages`）需平台
  分支；产物名 `beets-portable-linux-x64.zip`；manifest `entry` → `bin/python3`。
- **plugins.json**：schema 2 没有平台位，加 platform 维度；提取器/Qt5 so 新载体 + 钉扎；
  ffmpeg 载体 `win_amd64` wheel → manylinux wheel（member glob 跟变）；fpcalc 的 linux
  载体选型；**12 件模型与 selfhost 脚本零改动**。
- **工具侧**：`essentia_smoke.py` L31 的 exe 名平台化；`bins.py::_probe_ok` 的
  `.endswith(".exe")` 语义调整（`install_one` 已有 chmod 0o755）。
- **宿主侧**：`beets.rs` 的 `BUNDLE_ASSET` 硬编码 win-x64 → 扩档（`AIYA_BEETS_RELEASE_BASE`
  已可整体覆盖救急）；manifest 缺失 fallback `["aiya-beets.exe", "python.exe"]` 平台化；
  其余（manifest 驱动 spawn、zip 安全解包、CREATE_NO_WINDOW 门控）已平台无关。
- **workflow 侧**：体积门禁按 Linux 载荷重定；镜像 digest 钉扎；可选 manylinux2014 底座
  压 glibc 2.17。

## 5 · 边界

- 本批不触 win 轨、不触 release 通道；web/Docker 批（docs/65/66）只按 manifest
  entry/args 消费本产物，宿主零改动（docs/65 §3 拍板 1）。
