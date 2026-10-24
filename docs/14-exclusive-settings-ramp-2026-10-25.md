# 收尾批（四）：独占模式、设置四项、音量斜坡

> 日期：2026-10-25　范围：日志落盘之后的第二到第四刀——WASAPI 独占模式真正可用、
> 设置四项接线、音量逐样本斜坡。均在真机（Realtek 板载）验证。
> 不在本轮：代码签名、4h soak、CSP/capabilities、UI（将整体重做）。

## 1. 独占模式：从"只试 f32"到"可解释的可用功能"

### 1.1 上一版的判断错在哪

上一版 `open_exclusive` 只请求源格式（f32@源采样率），被拒就回落共享，理由是
"独占的意义是 bit-perfect，位深转换/重采样不划算"。这条理由对**重采样**成立，
对**位深**不成立。实测把设备真实能力摊开看：

```text
$ cargo run -p aiya-audio --example exclusive_probe
默认输出设备：扬声器 (Realtek(R) Audio)
共享混音格式：48000 Hz / 2 ch / 32 bit / Some(Float)

格式          采样率       声道    独占
f32         44100..192000  1/2   全部 ✗
i32         44100..192000  1/2   全部 ✗
i24-in-32   44100..192000  1/2   全部 ✓ 精确
i16         44100..192000  1/2   全部 ✓ 精确
```

结论：**消费级板载声卡的独占接口只接受整数格式**，f32 一律拒绝。只试 f32
等于让"独占"在大量机器上形同虚设——而且用户看到的只是"开了没反应"。

### 1.2 现在的策略

候选按音质优先级依次试，第一个被设备接受的胜出：

| 顺序 | 格式 | wBits / wValid | 适用 |
| --- | --- | --- | --- |
| 1 | f32 | 32 / 32 | 专业卡、部分 USB DAC（无转换） |
| 2 | i24-in-32 | 32 / 24 | 24bit 源无损；16bit 源也将就无损 |
| 3 | i16 | 16 / 16 | 16bit 源（绝大多数流行乐）无损 |

**仍然不做重采样**：采样率/声道数必须与源精确匹配，否则回落共享并说明原因。
位深转换只是一次乘加取整（`encode_frames`），没有 SRC 的复杂度与音质代价。

24bit 有效位按 WAVEFORMATEXTENSIBLE 约定**左对齐**在 32bit 容器（`v << 8`），
有单测锁死（`encode_i24_is_left_aligned_in_32` 断言第 4 字节为 `0x7F`）——
写错边就是"整轨 -48 dB"。

### 1.3 切模式必须先彻底释放设备

播放中从独占切回共享，上一版报：

```text
切换失败：wasapi: Windows returned an error: 0x8889000A   // AUDCLNT_E_DEVICE_IN_USE
```

原因是顺序：旧流还持有 `IAudioClient`，新流就在同一设备上 `Initialize`。把顺序
改成"先停再开"仍然失败——**`stop_stream()` 不够，必须 drop 掉句柄**。

于是 `Output` 改成 `Output(Option<OutputInner>)` 并实现 `Deref/DerefMut`：
所有读取点（`out.client` / `out.mode` / `out.fmt` …）一行都不用改，
只有切模式处调用 `out.release()` 真正释放。切模式的三种结局分开表达：

```rust
enum SwitchOutcome {
    Switched(Option<String>),   // 切成（Some 是回落原因）
    KeptPlaying(String),        // 目标模式打不开，已退回原模式继续播
    Dead(AudioError),           // 两次都打不开 → 交给设备恢复路径
}
```

真机冒烟（`npm run smoke:mode`，3 秒 440Hz 会出声）：

```text
独占：拿到 ✓
切回共享：OK
再切独占：OK（exclusive=true）
MODE SMOKE OK
```

### 1.4 崩溃重启要记得独占偏好

sidecar 崩溃后是新进程，引擎内部 `pending_exclusive` 归零。`sidecar.rs` 的
`TrackState` 增加 `exclusive`，重连时**无条件**回放进恢复序列（不依赖当前是否
装载——未装载时引擎只暂存，首曲 Load 生效）。漏这条的表现是：设置页显示独占开着，
实际早已退回共享。

## 2. 设置四项接线

### 2.1 为什么先加设置内存缓存

事件泵 25ms 一拍，`preventSleep` / `normalizeVolume` 每拍都要读。原来的
`setting_flag` 每次都是 `db().send(...)`——DB 单 worker 串行，泵会被自己的读库
拖慢。改为 `AppState.settings: Mutex<Value>`：启动时灌一次，`app_set_settings`
落库成功后整体替换。写失败不刷新缓存（缓存里的值与实际生效行为保持一致）。

### 2.2 四项各自的行为

| 设置 | 之前 | 现在 |
| --- | --- | --- |
| `preventSleep` | 只要在播就 `SetThreadExecutionState`，关掉开关无效 | 泵里与开关取与：`setting && playing` |
| `rememberPosition` | 恒记进度 | 关 → 恢复起点 0，且**落库写 0**（库里值与行为永远一致，不靠读取时再判断） |
| `autoplayOnStart` | 恢复恒为暂停 | 开 → hydrate 后补 `Resume` + `TogglePause`，只对"有当前曲且暂停"生效 |
| `normalizeVolume` | 后端零实现（UI 侧 `setNormalization` 是 no-op，注释写着"已在后端"——并非事实） | Actor 持有开关，`engine_volume()` 唯一计算点：`音量 × 10^(gain_db/20)`，夹到 [0,1] |

归一化放在 `PlayerActor` 而不是宿主，因为 `gain_db` 本来就在 Actor 手里
（`current`），分两处算必然出现第二套口径。`gain_db` 限幅 ±12 dB：beets 的
`rg_track_gain` 来自标签，坏标签能写出 −40 dB 这种值，直接乘会把声音搞没。
上限 +12 dB 实际会被引擎音量上限（1.0）夹住——我们不承诺"响度拉平"，
只承诺"按标签补偿"。

## 3. 音量逐样本斜坡

音量是逐样本乘上去的，目标值瞬间跳变等于给波形加阶跃 → 宽带"咔哒"。
`VOLUME_RAMP_MS = 20`，`resample_take` 接受 `vol_start` / `vol_step`，块内线性
走完；`volume_now` 在 chunk 之间保持（不保持的话每块首样本重新跳变，斜坡等于没做）。

没有做抖动（dither）：f32→i24 的量化噪声在 −144 dBFS，远低于任何设备底噪。
取整用 `round` 而不是截断——截断会给所有样本引入 −0.5 LSB 直流偏置，i16 上可测。

## 4. 验证

```text
cargo test --workspace       140 passed, 0 failed, 1 ignored（perf_50k）
cargo build --all-targets    0 warning
npm run smoke:mode           独占→共享→独占 全通
```

新增单测（15 条）：`encode_*` 4 条（f32 位精确 / i16 缩放进位与削顶 /
i24 左对齐 / 缓冲区不足不越界）、`volume_ramp_*` 2 条、归一化 5 条
（关/开/无 gain/坏标签限幅/静音与上限）、pump 装载音量 2 条（归一化生效、
静音下发 0）。

## 5. 留给后续

- 设备只支持 i16 而源是 24bit 时**静默降位深**：目前只在日志里记一行
  `独占模式已启用 format=...`，设置页要能展示"My设备实际跑了 i16"（等 UI 重做）
- 独占失败原因已随 `AudioMode.reason` 到 UI，但 UI 目前没有展示位
- 采样率不匹配（96k 源 + 48k 设备）仍直接回落共享；要不要做高质量 SRC 是
  独立决策，见 docs/09
