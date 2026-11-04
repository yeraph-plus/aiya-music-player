# ledger · 音频引擎（audio-engine）

> 代码位置：crates/aiya-audio/、crates/aiya-audio-host/
> 相关档案：docs/04、09、15、21、26、46

## 条目（新→旧）

<!-- 每条一行：- YYYY-MM-DD · 代码位置 · 事实/取舍 · 来龙去脉（可选） -->
- 2026-11-04 · engine.rs `padding_ms`/`finish_stream` · 共享 autoconvert 的帧计数（GetBuffer/GetCurrentPadding/GetBufferSize）按传给 Initialize 的**源格式**走，不用 mix 率换算 · 审查按"设备 mix 帧"理解要求改算，真机探针（96k/44.1k 源）实测证伪；口径抽成纯函数并留回归测试，防后人重查一遍。
- 2026-11-04 · engine.rs `out_format_from_negotiated` · 独占协商按 wValidBitsPerSample 分流：20/24 有效位→I24In32，32→I32（新增编码路径），其他拒绝该候选回落共享 · 旧实现只看容器位深，驱动把 32/24 折成 32/32 时编码错位。
- 2026-11-04 · engine.rs 独占 attempts 循环 · `best_reason` 用 `get_or_insert` 保留首个根因 · 末次 Initialize 失败的话术会把回落原因的解释引向错误方向。
- 2026-11-04 · spectrum.rs · FFT 旋转因子改 OnceLock 预生成表（2048 点，首次 FFT 时初始化）· 乘法递推有累计舍入；生产端与 tap 关闭态不触碰表，"关闭零开销"性质保持。
- 2026-11-04 · decoder.rs · 首包对齐之后检出采样率/声道中途变化→报 `Decode` 错误，不再静默换算 · 与 HLS 跨段校验同口径：引擎侧流/EQ/斜坡/分带都按装载规格构建，静默换算会变调。

