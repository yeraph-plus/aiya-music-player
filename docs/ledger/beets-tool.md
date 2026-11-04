# ledger · beets 工具侧（beets-tool）

> 代码位置：beets-tool/
> 相关档案：docs/33、51、53、54、58

## 条目（新→旧）

<!-- 每条一行：- YYYY-MM-DD · 代码位置 · 事实/取舍 · 来龙去脉（可选） -->
- 2026-11-04 · cmds/enrich.py · "提取器未就绪"报 `E_CONFIG`（用户可修的环境问题），不再用 `E_INVALID_ARG`（语义是开发者问题）· smoke_enrich 断言与 schema 错误码描述同批更新。
- 2026-11-04 · install.py · 差量卸载前按 `pip show` 的 Required-by 护栏，仍是保留插件依赖的包跳过卸载并发 progress 帧 · 防"禁 A 拆了 B 的透传依赖"。
- 2026-11-04 · cmds/enrich.py · 补登（beet import 子进程）批间检查取消代数、Popen+0.5s 轮询 kill · 与 market.install_plugin 同一标准；此前取消要拖满整个子进程（timeout 1800s）。
- 2026-11-04 · server.py · handle 对非数字请求 id 归零处理 · 乱 id 是调用方拼错，不该升级成 E_INTERNAL+traceback。


- 2026-11-03 · beets-tool/src/aiya_beets/cfg.py · Essentia xtractor 的 yml 里 svm compute 字段一度必须钉 0：AB 官方 win-i686 构建没编 Gaia2，配 svm_models 会报 Identifier GaiaTransform not found 后退出 1 且零输出（docs/50 §2.5 实测）；换社区 Gaia 构建后解除
