# ledger · 播放核心（player-core）

> 代码位置：crates/aiya-player/
> 相关档案：docs/05、21、22、28

## 条目（新→旧）

<!-- 每条一行：- YYYY-MM-DD · 代码位置 · 事实/取舍 · 来龙去脉（可选） -->
- 2026-11-04 · actor.rs · Loading 态 TogglePause 是**翻转**语义：意图翻转入 `pending_pause`，Loaded 按它落终态；命令路径归约后读 `pending_pause()` 同步发引擎 Pause/Resume · 只记不发的版本在 autoplay 装载窗口按暂停会"UI 已暂停、声音继续放"，且按两次意图卡死。
- 2026-11-04 · actor.rs · PlayNext 在 Idle 且队列非空时自动起播，与 Append 对齐 · 插队定位才依赖当前曲，空队列时插入曲就是播放起点。

