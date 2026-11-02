# ledger · beets 协议客户端（beets-protocol）

> 代码位置：crates/aiya-beets-client/、protocol/
> 相关档案：docs/08、33、45

## 条目（新→旧）

<!-- 每条一行：- YYYY-MM-DD · 代码位置 · 事实/取舍 · 来龙去脉（可选） -->

- 2026-11-03 · beets-tool/src/aiya_beets/protocol.py · 版本史：v2 升版因 E_LIBRARY_BUSY/resumedSkipped 在 v2 定为 required（生产端恒计算）+ schemaHash 运行时比对接入（闭合 docs/35 P1-4）；分包前两侧同发的 1/2/3 只是内部迭代号，冻结 v1 是两个独立发布物的第一版契约
