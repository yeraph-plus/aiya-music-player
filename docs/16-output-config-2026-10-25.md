# 16 · 输出配置模型落地（2026-10-25）

> 承接 `15-output-modes-eval`。15 号文档给的是**评估与设计**：四组合方向对、维度选错
> （不该拆成"共享/独占"两个开关）、"Tauri 原生后端"要否决。本文是**实现与验收**：
> 引擎里到底加了什么、每一项的实测证据、边界在哪。
>
> 一句话结论：输出侧的「模式 × 时钟 × 缓冲」现在是一组配置（`OutputConfig`），
> 走协议 v4 从宿主下发到 sidecar 再到引擎；同时补齐了四个主流播放器都有、
> 而此前缺失的基础件——**MMCSS 线程优先级、declick 淡入淡出、独占暂停释放设备、
> Null 静音输出**。151 项测试全绿，真机九项冒烟全过。

---

## 1. 配置模型

```rust
pub struct OutputConfig {
    pub mode: ModePref,       // shared | exclusive | exclusiveRequired | null
    pub buffer_ms: u32,       // 0 = 设备默认；显式值时只试这一个，不妥协
    pub clock: ClockPref,     // auto | events | polling
    pub release_on_pause: bool,
}
```

四个维度各解决一个真问题：

| 维度 | 取值 | 为什么需要 |
|---|---|---|
| `mode` | `shared` / `exclusive` / `exclusiveRequired` / `null` | `exclusive` 是**优先**独占、被拒回落；`exclusiveRequired` 是**必须**独占、被拒报错。两者语义不同，不能合成一个布尔 |
| `clock` | `auto` / `events` / `polling` | 事件驱动省 CPU、延迟低；轮询在部分驱动上反而更稳。`auto` = 先事件后轮询 |
| `buffer_ms` | 0 或显式值 | 0 交给设备；显式值用于"宁可不出声也不接受 200ms 延迟"的高级用户 |
| `release_on_pause` | bool | 独占会霸占设备（别的程序没声音）。暂停即释放，恢复再抢 |

**为什么不做"Tauri 原生后端"**（15 号文档已定，此处只记结论）：Tauri 没有音频 API。
唯一的"原生"路径是 WebView 的 `<audio>`，那会复活第二条音频路径——ADR-0003/0006
明确排除。所以"不启用 WASAPI"这个需求的正确形态是 `mode = null`：**解码、进度、
歌词、频谱照常推进，只是不打开设备**。它有两个真实用途：无声卡/远程桌面机器上应用
不假死；排查"爆音是音频路径还是解码/调度引起的"时可一键摘掉输出段做对照。

配置入口只有一条命令，取代了早期的 `SetExclusive(bool)`：

```rust
EngineCommand::SetOutputConfig(OutputConfig)
```

一次性下发整组参数的原因：分开下发会出现"模式已切、缓冲还是旧的"中间态，
且每次都要重建流两次。`matches_config(out, cfg)` 负责判断"新配置与当前流是否等价"，
等价就不重建——避免用户只是调了缓冲、却被打断播放。

---

## 2. 四组合实测：两个必须写进代码的反直觉结论

测量工具：`npm run probe:clock`（`examples/clock_probe.rs`）与 `npm run smoke:mode`。
本机：Realtek 板载，mix format 48 kHz / 2ch / 32bit，设备默认周期 10 ms。

### 2.1 结论一：独占模式下 `GetCurrentPadding()` 完全不可信

48 kHz、200 ms 缓冲，每栏 2 秒：

| 模式 | `pad` 取值 | 写入帧数 | 判定 |
|---|---|---|---|
| 事件驱动独占 | 恒 9600（= 满缓冲，从不变化） | 96000 | 正常 |
| 轮询独占 | 恒 9728 | 9728 | **假开** |
| 轮询独占（50 ms 缓冲） | 70 种不同取值 | 98228 | 正常 |

事件驱动独占下 padding 恒等于 buffer，`可写 = buffer − padding` 会算出 0 帧，
流立刻饿死。正确做法是**每个事件写满一个缓冲**——这也是 WASAPI 事件驱动独占的
标准写法，只是被 padding 的"看起来合理"掩盖了。

```rust
fn writable_frames(clock: Clock, mode: OutputMode, buffer_frames: u32, padding: u32) -> u32 {
    match (mode, clock) {
        // 事件驱动独占：padding 恒等于 buffer，问了也没用，按契约写满一缓冲
        (OutputMode::Exclusive, Clock::Events) => buffer_frames,
        // 其余（共享两种 + 独占轮询）：padding 可信，按可写空间写
        _ => buffer_frames.saturating_sub(padding),
    }
}
```

三条单元测试把这个分叉钉住：`writable_exclusive_events_writes_full_buffer`、
`writable_exclusive_polling_trusts_padding`、`writable_shared_tracks_padding`。

### 2.2 结论二：200 ms 的轮询独占会"假开"

上表第二行：`Initialize` 成功、`AudioMode` 报 `exclusive`、`IAudioClock` 一路推进、
错误码一个没有——但设备一帧都不消费，实际是静音。同机的**事件驱动独占 200 ms 正常**，
**轮询独占 50 ms 正常**。也就是说这个坑不能靠"枚举格式/周期"预测，只能看运行期
有没有数据真的写进去。

### 2.3 曾走过又回退的弯路：用 `IAudioClock` 记账

试过放弃 padding、改用 `IAudioClock::GetPosition` 自己记账（`可写 = 已消费 + buffer − 已写`）。
结果事件模式的写入量反转成只剩 9600 帧——`GetPosition` 比实际写入超前约 5%，
把它当"设备已消费"会系统性高估可写空间。已回退，`IAudioClock` 现在不参与可写空间判定。

---

## 3. 哑流自愈（`DUMB_STREAM_MS = 600`）

因为 §2.2 那类组合无法预测，运行期必须有兜底：非暂停状态下 **600 ms 没有任何成功写入**
即判定为哑流，按下面的顺序自救（`dumb_retry_cfg`）：

1. **先换时钟**（`events`↔`polling`）。时钟是同一个独占流内部的实现细节，换它不改变
   用户的音质选择。
2. **再降模式**（仅 `ModePref::Exclusive` → `Shared`）。用户只是"优先独占"，就回落共享
   继续放；`exclusiveRequired` 的语义是宁可停也不要共享，所以**不降**，交给调用方报错。
3. 没有第三步——两种时钟都哑了、又不许降级，就停在错误上并如实上报。

真机日志（`smoke:mode` 第 4 项，请求 200 ms 轮询独占）：

```
独占轮询哑火 → 自动切 events（polling · 202ms 缓冲下设备不消费数据（600ms 无写入））
独占/轮询: 写入 58688 帧 / 1500ms（期望 ≈72000）✓
```

注意括号里的原因文本会随 `AudioMode.reason` 一起上到 UI——**降级必须是可解释的**。

---

## 4. MMCSS：Pro Audio 线程优先级

`crates/aiya-audio/src/mmcss.rs`，渲染线程启动时
`AvSetMmThreadCharacteristicsW("Pro Audio")`，`Drop` 时 `AvRevertMmThreadCharacteristics`。

为什么这不是"优化项"：Windows 把普通线程压得很低，后台有编译任务或杀毒扫描时，
渲染线程没 CPU 就是**欠载 → 爆音**。foobar2000、WASAPI 官方示例、所有 ASIO 宿主都注册
这个任务。

两点实现约束：
- 函数在 `windows_sys::Win32::System::Threading`（不在 `Win32::Media::Audio`），
  feature 需 `Win32_System_Threading` + `Win32_Foundation`）；
- 失败**不是致命错**：精简版系统可能关掉 MMCSS 服务，此时退化成普通线程照常出声，
  只记日志。测试 `register_is_fallible_not_fatal` 只断言"不 panic"。

---

## 5. declick：20 ms 增益斜坡

暂停、seek、停止、切流都是"波形突然截断"，不做淡出就是可闻的咔哒声。

实现是逐样本增益逼近：`gain_now`（当前实际增益）向 `declick_target`（1 或 0）每样本
最多走 `ramp_step = 1/VOLUME_RAMP_MS 样本数`，跨 chunk 保持 `gain_now`——否则每块首样本
重新跳变，斜坡等于没做。

关键设计是**"先静音再动手"**：暂停/seek/停止/切流都不直接执行，而是置
`pending_pause` / `pending_seek` / `pending_stop` / `pending_switch`，等 `declick_target`
真正走到 0 才动手。淡入也是同一条路径（目标回 1，自然斜坡）。音量本身共用这套机制，
所以调音量也不会爆。

实测（`smoke:mode` 第 5 项）：

```
暂停：淡出期写 9600 帧，之后 800ms 写 0 帧 ✓
Resume 后 AudioMode=Some("exclusive")
恢复: 写入 76800 帧 / 1200ms ✓
```

断言特意写成"淡出 400ms 后 quiet 800ms 的写入量 < 采样率/20"，而不是"暂停后写入 0 帧"——
淡出期本来就该还写，断言 0 帧会把正确的实现判成失败。

---

## 6. 独占暂停释放设备

独占模式会霸占设备，别的程序（浏览器、会议软件）一点声音都发不出来；用户暂停去开会，
设备等于被空占。`release_on_pause = true` 时暂停会 drop 整个 `IAudioClient`，恢复时重开。

实现细节：`Output` 内部改成 `Option<OutputInner> + Deref` 实现"可释放"，读取点零改动。
踩到并修掉的三个 panic 都是同一个模式——**先 `is_live()` 再碰字段**：

- `if cfg.release_on_pause && out.mode == ... && out.is_live()` → 条件顺序反了，
  `out.mode` 已经 Deref 了；
- `if let Some(ev) = out.event.as_ref()` → 改 `out.0.as_ref().and_then(|i| i.event.as_ref())`；
- 暂停分支的 `out.client.get_current_padding()` → 外包 `if out.is_live()`。

`AudioMode` 事件在流已释放时统一回报 `mode = "null"` + `reason = "输出流未打开"`——
UI 不需要区分"用户选了 null"和"暂停释放了设备"，看 `mode` 就知道现在没声卡占用。

---

## 7. 协议 v4

`PROTOCOL_VERSION = 4`，两处变更：

| 变更 | v3 | v4 |
|---|---|---|
| 输出配置命令 | `setExclusive(bool)` | `setOutputConfig{ mode, bufferMs, clock, releaseOnPause }` |
| 模式回报 | `audioMode{ exclusive, note }` | `audioMode{ token, requested, mode, clock, bufferMs, format, reason }` |

`audioMode` 从"是个布尔"扩成**完整快照**：`requested` 是用户要的，`mode/clock/bufferMs/format`
是设备真给的，`reason` 是回落原因。`wire_shapes_are_stable` 测试直接断言 JSON 字面量：

```json
{"k":"cmd","m":{"t":"setOutputConfig","v":{"mode":"exclusiveRequired","bufferMs":50,"clock":"polling","releaseOnPause":true}}}
{"k":"evt","m":{"t":"audioMode","token":4,"requested":"exclusive","mode":"shared","clock":"events","bufferMs":200,"format":"f32","reason":"设备拒了"}}
```

> **踩坑记录**：改完引擎只 `cargo build -p aiya` 就启动，应用直接 panic——
> `协议不兼容：host v4 != sidecar v3，拒绝启动`。原因是 `target/debug/aiya-audio-host.exe`
> 还是旧二进制，它不是 `aiya` 的依赖，cargo 不会顺带重建。**改协议必须同时
> `cargo build -p aiya-audio-host`**。为防这类接缝再出事，新增了管道层冒烟
> `npm run smoke:output`（见 §9）。

---

## 8. 宿主侧接线

- **设置项**：`DEFAULT_SETTINGS.output = { mode: "shared", bufferMs: 0, clock: "auto", releaseOnPause: true }`。
  `AppState::setting_output()` 先读 `output{...}`，回落老的 `exclusiveMode: bool`，最后 Default——
  老配置文件不会因缺字段而丢设置。
- **生效路径**：`app_set_settings` 比较 `prev != next` 才发 `SetOutputConfig`；`setup()` 启动即下发一次
  （引擎未装载时只暂存，首曲 Load 才开流）。
- **崩溃恢复**：sidecar `TrackState.output: OutputConfig` 记账，重启恢复序列**第一条**就是回放
  `SetOutputConfig`，且必须在 `writer_slot` 发布前写入——漏了这条，崩溃重启后会静默退回共享。
- **诊断页**：`app_diagnostics` 的 `audioMode` 返回 `{requested, mode, clock, bufferMs, format, note, desired{...}}`，
  `desired` 是设置里的期望值，两者并排就能看出"设置没生效"还是"设备拒了"。

---

## 9. 验收记录（2026-10-25）

```
cargo test --workspace            151 passed / 0 failed（+1 ignored：perf_50k）
cargo build --workspace --all-targets   0 warning
npm run check                     svelte-check 0 errors / 0 warnings
```

引擎级九项（`npm run smoke:mode`，真机出声）：

| 项 | 结果 |
|---|---|
| [1] 共享 / 事件 | 63840 帧 / 1200ms ✓ |
| [2] 共享 / 轮询 | 64320 帧 / 1200ms ✓ |
| [3] 独占 / 事件 / 自动缓冲 | `clock=events buffer=10ms format=i24-in-32`，67680/1500ms、换曲 69600/1500ms ✓ |
| [4] 独占 / 轮询 / 200ms | 哑火自愈切 events，58688/1500ms ✓ |
| [5] 暂停 / 恢复 | 淡出期 9600 帧，之后 800ms 0 帧；恢复 76800/1200ms ✓ |
| [6] seek | 57600/1000ms ✓ |
| [7] 独占（必须）/ 事件 | 设备支持 → 独占 ✓ |
| [8] null | 70272/1500ms（不出声，进度照走）✓ |
| [9] null → 共享 | 75840/1500ms ✓ |

管道层六项（`npm run smoke:output`，真起 sidecar 子进程走 v4）：

```
握手 OK: v4
  共享/轮询 120ms 生效: ✓ requested=shared mode=shared clock=polling buffer=120ms
  独占请求被如实回报: ✓ mode=exclusive clock=events reason=""
  切独占后仍在推进: ✓ pos=1036ms
  切 Null 回报 mode=null: ✓
  Null 下进度仍推进: ✓ 1036ms → 2088ms
  Null → 共享 恢复出流: ✓ mode=shared
  恢复后进度继续: ✓ 2088ms → 2964ms
```

这一条专验宿主↔sidecar 接缝（配置 wire 形状、sidecar 记账、事件回流、播放中切 Null
不断解码）——就是 §7 出过事的地方。断言只要求"配置被正确理解并如实回报"，
不与设备能力挂钩（独占被拒＝合法结果，只要 `reason` 说清楚）。

---

## 10. 已知边界（明确不做的）

1. **200 ms / 100 ms 的轮询独占在本机是假开**。不特判、不屏蔽：靠 §3 的哑流自愈兜底
   （600 ms 无写入即自救）。特判会把一个驱动相关问题固化成代码里的设备黑名单。
2. **独占不做重采样**。采样率/声道必须精确匹配，否则回落；只允许位深下调
   f32→i24→i16（实测板载独占接口普遍只接受整数格式，24 bit 有效位在 32 bit 容器里左对齐 `v<<8`）。
3. **gapless 还没做**。同专辑连续播放仍有间隙，属于下一批。
4. **UI 未接**。当前 UI 不引用 `exclusiveMode` / `audio:mode`，将随整体重做一起接；
   后端已把 `audioMode` 快照与诊断页数据准备好。
5. **快捷键设置页、主题密度/主色逐项核对、E2E** 仍未做（P4 遗留小项）。
