# ledger · beets 协议客户端（beets-protocol）

> 代码位置：crates/aiya-beets-client/、protocol/
> 相关档案：docs/08、33、45

## 条目（新→旧）

<!-- 每条一行：- YYYY-MM-DD · 代码位置 · 事实/取舍 · 来龙去脉（可选） -->
- 2026-11-04 · protocol/v2.schema.json · InstallerEvent/InstallerStage 按单轨化后 install.py 实际帧重写（stage 增 `bin`、type 增 `warning`/`dryRun`、done 双形态），protocol_contract 补 `install --dry-run` 实线 · schema 里"给另一条进程形态用的词汇表"没有实线覆盖时，腐化速度与普通文档一样快。
- 2026-11-04 · tools/proto-sync.mjs · `proto:sync` 只写镜像与 meta，**不**更新 protocol.py 的 `SCHEMA_SHA256`（需手动同步，contract 测试 [1] 自证兜底）· 哈希真源三处（schema 文件/meta/常量），两处自动一处手动。


- 2026-11-03 · beets-tool/src/aiya_beets/protocol.py · 版本史：v2 升版因 E_LIBRARY_BUSY/resumedSkipped 在 v2 定为 required（生产端恒计算）+ schemaHash 运行时比对接入（闭合 docs/35 P1-4）；分包前两侧同发的 1/2/3 只是内部迭代号，冻结 v1 是两个独立发布物的第一版契约
