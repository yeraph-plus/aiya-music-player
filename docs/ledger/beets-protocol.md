# ledger · beets 协议客户端（beets-protocol）

> 代码位置：crates/aiya-beets-client/、protocol/
> 相关档案：docs/08、33、45

## 条目（新→旧）

<!-- 每条一行：- YYYY-MM-DD · 代码位置 · 事实/取舍 · 来龙去脉（可选） -->
- 2026-11-13 · beets-tool/src/aiya_beets/{cli.py,cfg.py,plugins.py,data/plugins.json} · 刮削 0 候选根因闭合（拍板全删遗留的收官）：beets 2.14 把插件加载改为显式 load_plugins（只挂在 beets 自己的 ui 入口），aiya-beets 的 serve 不经过 ui → 加载从未发生 → 元数据源（musicbrainz 匹配）恒空。修复 = serve 分支显式 load_plugins + REQUIRED_PLUGINS 并集（plugins.json required 字段识别：musicbrainz 恒入启用行，存量 config 下一次会话启动/换插件自动补齐）｜直连工具进程验证：MusicBrainz 源加载 → 5 候选 → apply_import 全通（写标签+拷贝落点+入库 3s failed 0）｜拍板记录：代理方案 A 延后一批（GitHub 访问不稳可能连便携包都拉不下来，方案先想清楚）；xtractor 分析已实证出值、不跑全库（用户自理）｜观察项：混合目录组 recommendation none + 候选路径 Non-Album 分段（候选 track.album 为 null 时 beets 的分段名）——整专辑目录导入不受影响
- 2026-11-13 · beets-tool/protocol/v2.schema.json + crates/aiya-beets-client/{proto.rs,session.rs,tests/proto_mirror.rs} + beets-tool/src/aiya_beets/{server.py,env.py,uninstall.py} · 协议 v2→v3：删 plugin_catalog / plugin_install / pluginProgress / pluginDone 与 StatusData.pythonSupported，PROTOCOL_VERSION +1（schema 自带治理规则：删字段算破坏兼容必须 +1）· 三个缺口的出身（git/docs 追溯）：市场链生于 docs/40 §3 调研（beets 无官方市场 → 自维护目录 + pip）、429557c 落宿主侧、cc7b323「市场实现回收」撤了 UI 留了线、docs/58 单轨化让「换插件」闭环后整条链零调用方；uninstall 的 venv-only 分支是 c3c848d 两轨时代的产物（A 轨 venv 安装需要只卸 venv 的模式），25c7e99 单轨化后宿主恒传 --all、分支只服务旧装；pythonSupported 是 c3c848d 拆两路时给宿主判环境用的，25c7e99 只删了宿主消费侧（docs/58 §6 说"一并退役"未兑现）——三个都是"上层设计换代、下层没删干净"的同一模式｜拍板全删（单轨预打包 + 无更新路径下不留惰性字段）；副作用 = 旧装 bundle 与新宿主 schemaHash/版本不符会被拒，恢复 = 重新下载安装（即无更新路径哲学的预期行为）｜卸载同步收口为单形态（--all 旗标删除，宿主不再传；v/bundle/bin 标记保留兼容旧装目录形状）｜SCHEMA_SHA256 常量随 schema 字节手工更新（proto:sync 只管镜像与 meta）；CapabilitiesData.runtimeSupported 为活面（capabilities 命令 + CLI 在用）不动｜验证：proto:check ✓ + smoke:proto ALL PASS + protocol/catalog/uninstall 三契约 ALL PASS + cargo test client/host-core/data 绿
- 2026-11-12 · crates/aiya-beets-client/src/session.rs（TERMINAL_EVENTS）· 终态集合必须逐命令完备（每条会话式命令至少一个 *Done/cancelled/sessionFailed 终态）· 维护四件套 + dump + index_import 的终态不在表里：命令成功后客户端等不到终态，会话空转到 600 s 被看门狗误杀，期间会话槽占死、on_exit 对账（整理后修正库路径的唯一时机）被推迟
- 2026-11-12 · crates/aiya-beets-client/src/proto.rs（EnvelopeError）· 线上错误信封字段是 snake 的 `beets_message`，serde 不能挂 `rename_all = "camelCase"`（schema EnvelopeError required 一字不差）· 接 v2 时误挂 camelCase：ok:false 信封全部解析失败、工具诊断被当成"协议解析失败"；status 主路径恒 ok:true 从未暴露，v2 拒旧组件路径一跑现形
- 2026-11-12 · crates/aiya-beets-client/src/session.rs（reserve/acquire 时序）· 会话位必须在 spawn 之前占用 · 并发第二请求曾先起进程后占位：请求行进对方队列照常执行（写标签、动数据），却没有 reader 转发事件——既脏又泄漏

- 2026-11-04 · protocol/v2.schema.json · InstallerEvent/InstallerStage 按单轨化后 install.py 实际帧重写（stage 增 `bin`、type 增 `warning`/`dryRun`、done 双形态），protocol_contract 补 `install --dry-run` 实线 · schema 里"给另一条进程形态用的词汇表"没有实线覆盖时，腐化速度与普通文档一样快。
- 2026-11-04 · tools/proto-sync.mjs · `proto:sync` 只写镜像与 meta，**不**更新 protocol.py 的 `SCHEMA_SHA256`（需手动同步，contract 测试 [1] 自证兜底）· 哈希真源三处（schema 文件/meta/常量），两处自动一处手动。


- 2026-11-03 · beets-tool/src/aiya_beets/protocol.py · 版本史：v2 升版因 E_LIBRARY_BUSY/resumedSkipped 在 v2 定为 required（生产端恒计算）+ schemaHash 运行时比对接入（闭合 docs/35 P1-4）；分包前两侧同发的 1/2/3 只是内部迭代号，冻结 v1 是两个独立发布物的第一版契约
