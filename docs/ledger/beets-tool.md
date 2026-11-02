# ledger · beets 工具侧（beets-tool）

> 代码位置：beets-tool/
> 相关档案：docs/33、51、53、54、58

## 条目（新→旧）

<!-- 每条一行：- YYYY-MM-DD · 代码位置 · 事实/取舍 · 来龙去脉（可选） -->

- 2026-11-03 · beets-tool/src/aiya_beets/cfg.py · Essentia xtractor 的 yml 里 svm compute 字段一度必须钉 0：AB 官方 win-i686 构建没编 Gaia2，配 svm_models 会报 Identifier GaiaTransform not found 后退出 1 且零输出（docs/50 §2.5 实测）；换社区 Gaia 构建后解除
