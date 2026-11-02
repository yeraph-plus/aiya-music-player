# ledger · Tauri 宿主（tauri-host）

> 代码位置：src-tauri/
> 相关档案：docs/03、12、13、56

## 条目（新→旧）

<!-- 每条一行：- YYYY-MM-DD · 代码位置 · 事实/取舍 · 来龙去脉（可选） -->

- 2026-11-03 · src-tauri/src/dto/beets.rs、tests/dto_wire.rs · BeetsStatus 不再携带 pythonSupported：单轨化后环境信息全部由工具自报，能力面 = plugin_health 三态；dto_wire 的陈旧断言随字段移除同步清理
