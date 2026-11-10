# ledger · beets 工具侧（beets-tool）

> 代码位置：beets-tool/
> 相关档案：docs/33、51、53、54、58、67

## 条目（新→旧）

<!-- 每条一行：- YYYY-MM-DD · 代码位置 · 事实/取舍 · 来龙去脉（可选） -->

- 2026-11-09 · crates/aiya-host-core/src/{lib.rs,beets.rs}、src-tauri/src/lib.rs、beets-tool/tools/build_bundle.py、beets-tool/src/aiya_beets/xtractor.py、Dockerfile · beets 桥双平台（win 桌面 / linux 容器）静态审查修复批：canon_path 加 cfg!(windows) 分支（unix 分隔符不动 + 去尾 / + 小写，与 aiya-data path_key 同口径；子路径前缀判断按平台分隔符——此前 Windows 形态无条件下发，unix 上靠"双方同变"碰巧自洽，`/music/` 这类带尾斜杠输入归出来的形态不对）+ src-tauri 四份重复实现（canon_path/paths_overlap/path_conflicts_with_roots/path_within_roots）收编进 host-core（canon_path_tests 双平台各 8 条，unix 半边随 2026-11-11 的 CI linux-test job 首次上线）；beets.rs manifest 缺失兜底补 linux 入口 bin/python3；build_bundle zip 打包显式 unix 权限位 + create_system=3 钉扎（zip crate 的 unix_mode() 只在 create_system=3 才读 external_attr——Windows 宿主构建 linux 包丢 +x 的路堵死；Docker 内构建本是 POSIX，此举是"任何宿主构建都对"的加固）；xtractor.py 文档串订正（win-i686 无 Gaia 的 docs/50 时代描述 → 双平台 Gaia 现状，highlevel compute 1 + 12 件 SVM）；Dockerfile 运行层补 ffmpeg + libchromaprint-tools（catalog 载体 windows-only、bins 在 linux 不下载，web 端 replaygain/convert/embedart/alternatives/chroma 五插件此前 missing 且无自助补齐路径，现系统 PATH 经 exe_path→shutil.which 兜住）

- 2026-11-06 · .github/workflows/build-essentia-linux.yml（main 7ef3f0f）· Linux x86_64 提取器测试构建链路落地并验证：MTG essentia-builds 容器（manylinux_2_28）waf --with-gaia --build-static --static-dependencies --with-examples 静态构建，两次全绿 run 冒烟与 win 实测一致（正弦波 not_danceable/happy，highlevel 12 键全出）· Linux 侧唯一风险点（带 Gaia 的提取器无现成 release，只能构建）排除；静态/动态边界 = 仅 Qt5 双 so 动态（对应 win 附 Qt5Core.dll 的载体模式）；三轮机械坑（tar.gz 配 unzip、缺 --with-examples、essentia_ 前缀）只记录在 docs/67 §3；产物仅 artifact 不碰发布通道，转正接线见 docs/67 §4

- 2026-11-05 · data/plugins.json svm-* 条目、tests/catalog_contract.py、bins.py check、release-beets.yml · beets-portable 通道首跑暴露连环 latent bug 全修：①svm 条目 license 字段塞整句出处（含 URL）→ 许可全文查找按路径穿越失败，改短 id MTG-Model-Notice（出处归 vendor）；②third-party 缺 5 份 GNU 全文（AGPL/GPL/LGPL-2.1/LGPL-3.0-or-later + LGPL-3.0）补齐；③bins --check 的 usable 口径漏走 _probe_ok——DLL/模型非可执行条目裸 probe 全报 missing，套件包自检永远过不去，对齐"在即就绪"；④release-beets.yml 还在引用已退役的 build_installer.py（.pyz 轨道），对齐单 zip 通道契约 · 三个教训：plugins.json 新增条目必须过 catalog_contract（本次加了 license 短 id + 全文存在两条门禁）；SystemExit 文案在 runner 上可能被吞（本地复现是排障正道）；通道从没建过 ≠ 被删——beets-portable tag 一直没推过线上
- 2026-11-04 · cmds/enrich.py · "提取器未就绪"报 `E_CONFIG`（用户可修的环境问题），不再用 `E_INVALID_ARG`（语义是开发者问题）· smoke_enrich 断言与 schema 错误码描述同批更新。
- 2026-11-04 · install.py · 差量卸载前按 `pip show` 的 Required-by 护栏，仍是保留插件依赖的包跳过卸载并发 progress 帧 · 防"禁 A 拆了 B 的透传依赖"。
- 2026-11-04 · cmds/enrich.py · 补登（beet import 子进程）批间检查取消代数、Popen+0.5s 轮询 kill · 与 market.install_plugin 同一标准；此前取消要拖满整个子进程（timeout 1800s）。
- 2026-11-04 · server.py · handle 对非数字请求 id 归零处理 · 乱 id 是调用方拼错，不该升级成 E_INTERNAL+traceback。


- 2026-11-03 · beets-tool/src/aiya_beets/cfg.py · Essentia xtractor 的 yml 里 svm compute 字段一度必须钉 0：AB 官方 win-i686 构建没编 Gaia2，配 svm_models 会报 Identifier GaiaTransform not found 后退出 1 且零输出（docs/50 §2.5 实测）；换社区 Gaia 构建后解除
