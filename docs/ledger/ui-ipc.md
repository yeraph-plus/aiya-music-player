# ledger · UI 与 IPC 契约（ui-ipc）

> 代码位置：ui/src/lib/ipc/、ui/src/lib/settings*.ts
> 相关档案：docs/17、18、24

## 条目（新→旧）

<!-- 每条一行：- YYYY-MM-DD · 代码位置 · 事实/取舍 · 来龙去脉（可选） -->
- 2026-11-04 · dto/app.rs、dto/output.rs、`app_diagnostics`/`app_info` · Diagnostics 压平到四个有消费方的字段（version/dataDir/recentErrors/mediaKeys），AppInfo 只剩 dataDir；ListTiming/AudioModeInfo/PowerInfo/StreamsInfo 结构体连同后端耗时环、`sleep_prevented` 诊断镜像一并退役；MediaKeysInfo 砍 `failed`（旧实现设置关闭时误报"初始化失败"），enabled 改为设置值、active 为会话存活，"开着没生效"由 SMTC 初始化失败走 `record_background` 报进最近错误台账 · 压平审计逐字段清点消费面：20 个字段里 15 个没有任何 UI 读过，每次拉取白跑一轮 COUNT；gen:types 不清理退役生成物（index 按目录扫描，死 .ts 反向污染 index），删类型后须手动删除 generated/ 旧文件再重生
- 2026-11-04 · lib/ipc/client.ts + lib/errors.ts · `isRetryable` 只在 errors.ts（client.ts→errors.ts 单向，反向会成环，check:deps 守着）· 同一逻辑曾双份实现。
- 2026-11-04 · 仓库根 bindings/ · 已删除；ts-rs 生成唯一出口是 `ui/src/lib/ipc/generated/`（gen-types.mjs 的 TS_RS_EXPORT_DIR）· 该目录是过期快照（未入 git、缺 62 个类型），按它核对 DTO 会得出错误结论。
- 2026-11-04 · stores/scan.svelte.ts + stores/ui.svelte.ts · RefreshSnapshot/pluginHealth/pluginSummary 动态事件逐字段 typeof 收口，坏载荷整条丢弃 · 对齐 readBeetsStatus 既有风格，裸 as 断言清零。
- 2026-11-04 · lib/ipc/events.ts · beetsMaintenance/beetsEnrich 注明当前无消费者 · 恢复维护四件套 UI 时订阅要连挂 boot 清理链一起回来。

