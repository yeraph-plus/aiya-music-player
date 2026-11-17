# ledger · UI 与 IPC 契约（ui-ipc）

> 代码位置：ui/src/lib/ipc/、ui/src/lib/settings*.ts
> 相关档案：docs/17、18、24

## 条目（新→旧）

- 2026-11-18 · StationProgramIssue / program.rs / station-manager · 节目可用性改为带 kind 的结构化原因，来源使用 playlistId，周一为 0；UI 翻译页面与开播错误条，TZ 缺省返回 null · 推流中英文复核见 docs/75。

- 2026-11-18 · 频道 WebSocket 仅发送连接频道的 track 事件，拒绝其他频道及无身份事件 · docs/74、75 整批审查确认共享总线转发缺少过滤。

<!-- 每条一行：- YYYY-MM-DD · 代码位置 · 事实/取舍 · 来龙去脉（可选） -->
- 2026-11-17 · station DTO / aiya-server/program.rs / ipc.rs / client.ts · 频道节目改为带版本的系统歌单引用文档，整队列原子保存与运行时应用分离；来源指纹检测成员变化，开停播和应用串行化，旧频道歌单命令与 DTO 删除 · 用户明确本地为测试环境，本轮不加迁移、旧接口兼容或歌单删除拦截；约定见 docs/75。
- 2026-11-17 · aiya-server/ipc.rs app_get/set_ui_state · HTTP 保存客户端 state 参数的内容，读取兼容含 state 包装的存量数据，页面刷新可恢复到原路由 · 推流拆页的真实 HTTP E2E 暴露读写层级不一致，修复与回归见 docs/75。
- 2026-11-17 · http.ts / aiya-server · HTTP 播放命令与引擎回报串行执行，SSE 解绑移除原生 handler，Web 收听接通原生 HLS / hls.js 与私有频道分段鉴权 · 双宿主审查修复与验证见 docs/74。
- 2026-11-04 · dto/app.rs、dto/output.rs、`app_diagnostics`/`app_info` · Diagnostics 压平到四个有消费方的字段（version/dataDir/recentErrors/mediaKeys），AppInfo 只剩 dataDir；ListTiming/AudioModeInfo/PowerInfo/StreamsInfo 结构体连同后端耗时环、`sleep_prevented` 诊断镜像一并退役；MediaKeysInfo 砍 `failed`（旧实现设置关闭时误报"初始化失败"），enabled 改为设置值、active 为会话存活，"开着没生效"由 SMTC 初始化失败走 `record_background` 报进最近错误台账 · 压平审计逐字段清点消费面：20 个字段里 15 个没有任何 UI 读过，每次拉取白跑一轮 COUNT；gen:types 不清理退役生成物（index 按目录扫描，死 .ts 反向污染 index），删类型后须手动删除 generated/ 旧文件再重生
- 2026-11-04 · lib/ipc/client.ts + lib/errors.ts · `isRetryable` 只在 errors.ts（client.ts→errors.ts 单向，反向会成环，check:deps 守着）· 同一逻辑曾双份实现。
- 2026-11-04 · 仓库根 bindings/ · 已删除；ts-rs 生成唯一出口是 `ui/src/lib/ipc/generated/`（gen-types.mjs 的 TS_RS_EXPORT_DIR）· 该目录是过期快照（未入 git、缺 62 个类型），按它核对 DTO 会得出错误结论。
- 2026-11-04 · stores/scan.svelte.ts + stores/ui.svelte.ts · RefreshSnapshot/pluginHealth/pluginSummary 动态事件逐字段 typeof 收口，坏载荷整条丢弃 · 对齐 readBeetsStatus 既有风格，裸 as 断言清零。
- 2026-11-04 · lib/ipc/events.ts · beetsMaintenance/beetsEnrich 注明当前无消费者 · 恢复维护四件套 UI 时订阅要连挂 boot 清理链一起回来。

