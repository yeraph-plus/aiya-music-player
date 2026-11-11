# ledger · beets 协议客户端（beets-protocol）

> 代码位置：crates/aiya-beets-client/、protocol/
> 相关档案：docs/08、33、45

## 条目（新→旧）

<!-- 每条一行：- YYYY-MM-DD · 代码位置 · 事实/取舍 · 来龙去脉（可选） -->
- 2026-11-12 · crates/aiya-beets-client/src/session.rs（TERMINAL_EVENTS）· 终态集合必须逐命令完备（每条会话式命令至少一个 *Done/cancelled/sessionFailed 终态）· 维护四件套 + dump + index_import 的终态不在表里：命令成功后客户端等不到终态，会话空转到 600 s 被看门狗误杀，期间会话槽占死、on_exit 对账（整理后修正库路径的唯一时机）被推迟
- 2026-11-12 · crates/aiya-beets-client/src/proto.rs（EnvelopeError）· 线上错误信封字段是 snake 的 `beets_message`，serde 不能挂 `rename_all = "camelCase"`（schema EnvelopeError required 一字不差）· 接 v2 时误挂 camelCase：ok:false 信封全部解析失败、工具诊断被当成"协议解析失败"；status 主路径恒 ok:true 从未暴露，v2 拒旧组件路径一跑现形
- 2026-11-12 · crates/aiya-beets-client/src/session.rs（reserve/acquire 时序）· 会话位必须在 spawn 之前占用 · 并发第二请求曾先起进程后占位：请求行进对方队列照常执行（写标签、动数据），却没有 reader 转发事件——既脏又泄漏

- 2026-11-04 · protocol/v2.schema.json · InstallerEvent/InstallerStage 按单轨化后 install.py 实际帧重写（stage 增 `bin`、type 增 `warning`/`dryRun`、done 双形态），protocol_contract 补 `install --dry-run` 实线 · schema 里"给另一条进程形态用的词汇表"没有实线覆盖时，腐化速度与普通文档一样快。
- 2026-11-04 · tools/proto-sync.mjs · `proto:sync` 只写镜像与 meta，**不**更新 protocol.py 的 `SCHEMA_SHA256`（需手动同步，contract 测试 [1] 自证兜底）· 哈希真源三处（schema 文件/meta/常量），两处自动一处手动。


- 2026-11-03 · beets-tool/src/aiya_beets/protocol.py · 版本史：v2 升版因 E_LIBRARY_BUSY/resumedSkipped 在 v2 定为 required（生产端恒计算）+ schemaHash 运行时比对接入（闭合 docs/35 P1-4）；分包前两侧同发的 1/2/3 只是内部迭代号，冻结 v1 是两个独立发布物的第一版契约
