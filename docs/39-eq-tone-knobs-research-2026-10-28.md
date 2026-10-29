# docs/39 — EQ 低音/高音旋钮方案调研（2026-10-28）

> **已按方案 B 落地**（用户确认：前置增益完整保留，前端做成旋钮外观）。
> 实施记录见文末。

## 背景与问题

用户反馈：单独的「前置增益」旋钮意义不明（它是抗削顶的工程预留，不是用户想调的
东西）；「单独调整低音和高音」才是主流播放器的实用形态（Poweramp 的 Tone、
VLC 的低音/高音条）。

要求：调研实现方案，给出取舍与推荐，确认后开工。

## 事实核查（引擎与协议现状）

- 引擎链路（`crates/aiya-audio/src/eq.rs`）：
  `pregain → 10 × RBJ biquad（低架 / 8 × 峰值 / 高架）→ 音量`。
- **首段（31.25Hz）已经是 lowShelf、末段（16kHz）已经是 highShelf**——
  低音/高音旋钮的 DSP 原语在引擎里早已存在，`Coeffs::design` 的 shelf 分支
  是现成公式（S=1 斜率）。
- wire：`EqParams { bands: [f32;10], pregain, enabled }`，`#[serde(default)]`，
  NDJSON 传输，协议握手 v7（EQ 在 v6 引入）。持久化 = DB `settings` 表 `"eq"`
  键整体存 JSON。
- 旁路条件：`!enabled || (pregain==0 && 全段==0)` → 逐样本直通。

## 方案对比

### 方案 A：纯前端映射——低音/高音旋钮绑定到既有首末两段

低音旋钮 = `bands[0]`（31Hz 低架），高音旋钮 = `bands[9]`（16kHz 高架），
10 段行保留或收缩为 8 段。零后端改动。

- 优点：引擎/协议/持久化全不动；两段本来就是货架滤波器，语义天然成立；
  旋钮与段滑杆共享同一状态，显示永远一致。
- 缺点：**31Hz 低架在笔记本外放上几乎听不见**（外放在 <100Hz 已滚降），
  「低音」旋钮拧了没反应是最差 UX；16kHz 高架同理偏「空气感」而非「高音」。
  咬牙把低音映射到 31+62+125 三段复合联动，则旋钮显示值与手动改段立刻
  分叉（两个真源），语义混乱。

### 方案 B：引擎加独立音色段（推荐）

`EqParams` 增 `bass_db` / `treble_db` 两个独立旋钮，引擎在 10 段之后再串
**两个独立 shelf biquad**（低架 ~90Hz、高架 ~7.5kHz，落在外放/耳机都有效
的频段），UI 出「低音 / 高音」两根横滑杆，撤掉前置增益旋钮。

- 优点：**音色与图示 EQ 完全正交**——互不覆盖、显示无歧义； shelf 公式
  现成（泛化 f0 即可），引擎改动约 40 行 + 单测；「低音拧了立刻有感觉」。
- 缺点：动引擎与 wire。但属**加法变更**：`serde(default)` 让旧持久化行
  （无这两个字段）自动补 0；host 与 sidecar 同包发布、握手永远同版本，
  不需要 bump `PROTOCOL_VERSION`（无删除/改名，旧对端也能解析）。
- 配套：`is_bypass()` 补上两旋钮 == 0；DTO（`dto::EqSettings`）同步加字段；
  UI store 加 bass/treble 状态与下发；预设不受影响（预设只定义 bands，
  音色旋钮独立于预设身份判定）。
- 前置增益的去留：**UI 撤旋钮，引擎/wire 保留字段**（抗削顶预留的工程
  价值仍在，默认 0；旧持久化值继续生效，只是不再暴露给用户直接调）。

### 方案 C：只撤前置增益、不加旋钮

最小改动，但不满足「单独调低音/高音」的诉求，仅作为退路。

## 推荐

**方案 B**。理由：

1. 方案 A 的致命伤不是实现成本而是听感——31Hz 低架在外放设备上无效，
   旋钮做了等于没做；复合映射又会制造双真源。
2. B 的引擎改动是既有 shelf 公式的泛化复用 + 两条系数槽位，风险面小；
   wire 是纯加法，不触碰冻结协议的兼容性红线。
3. 与主流播放器的「Tone」形态对齐，用户心智零学习成本。

## 实施清单（B，待确认后执行）

1. `crates/aiya-audio/src/eq.rs`：`Coeffs::design` 泛化出任意 f0 的
   lowShelf/highShelf 分支；`EqParams` 加 `bass_db`/`treble_db`（默认 0，
   clamp ±12）；`Eq` 加两条系数槽与状态、`redesign`/`process_frame` 串接；
   `is_bypass` 补条件；单测：50Hz 正弦随 bass 增益放大、7kHz 随 treble
   放大、旁路位级一致、20 次热切无爆音。
2. `src-tauri`：`dto::EqSettings` 加 `bassDb`/`trebleDb`；`player_set_eq`
   透传；`run()` 启动回填补两字段。ts-rs 类型重新生成。
3. UI `stores/eq.svelte.ts`：加 `bass`/`treble` 状态、`setBass`/`setTreble`、
   apply 回写（沿用 clamp 差异回写约定）；`reset()` 归零含两旋钮。
4. UI `EqPanel.svelte`：「前置增益」分区替换为「音色」分区（低音 / 高音
   两根横滑杆，行尾挂当前 dB 值）；预设菜单、10 段行不动。
5. e2e：EQ 无既有用例，跑全量回归确认无破损。

## 涉及的坑（来自本仓实测记忆）

- 系数热切不动延迟线（切参数无爆音的前提），新槽位同样只在 `redesign`
  换系数。
- 超 Nyquist 跳段逻辑对自定义 f0 同样适用（32kHz 采样率下 16k 段已跳，
  新高架 7.5kHz 在 32kHz 下仍低于 0.45×fs=14.4kHz，安全）。
- DTO 是 `rename_all=camelCase`，UI 侧手写 JSON 的单测发现不了字段名错——
  以 ts-rs 生成类型为准。

## 实施记录（2026-10-28 当天完成）

- 引擎 `eq.rs`：`Shape` 枚举泛化 `Coeffs::design`（低架/高架/峰值三形状）；
  槽位 10→12（SLOT_BASS=10 低架 90Hz、SLOT_TREBLE=11 高架 7.5kHz）；
  `EqParams` 加 `bass_db`/`treble_db`（serde default，clamp ±12）；
  `is_bypass` 补两字段 ==0。新增测试：50Hz 随 bass +6dB ≈2×（1kHz 不动）、
  10kHz 随 treble +6dB ≈2×（500Hz 不动）、仅音色非零不旁路、20 次热切含
  音色变化无爆音。proto 精确 JSON 断言同步（bassDb/trebleDb 进 wire）。
- `dto::EqSettings` 加 `bassDb`/`trebleDb`，**字段级 `#[serde(default)]`**
  ——旧持久化行缺这两字段，不加会解析失败静默回退默认值、丢用户已存
  bands。`player_set_eq` 入口 clamp + 透传；`player_get_eq` 与 `run()`
  启动回读补字段。ts-rs 重生成（56 类型）。
- UI store：`bass`/`treble` 状态 + `setBass`/`setTreble`，apply 沿用
  "clamp 差异回写"约定；mock 后端同语义 + 旧 localStorage 行兜底。
- UI `Knob.svelte`（新）：270° 扫角 SVG 弧线旋钮，竖向拖动（150px 全
  量程）/滚轮微调/键盘（方向键±step、Home/End），emit 前吸附 step 网格
  （与 Slider 同约定），role=slider 无障碍。
- UI `EqPanel`：分区定稿「预设 → 音色（低音/高音横杆）→ 频段（10 段）→
  前置增益（旋钮居中）」；预设命中判定不变（只看 bands，音色独立）。
- 验证：cargo test 全仓全绿（aiya-audio 64+11）；svelte-check 0/0；
  e2e 28/28（EQ 用例同步：k-slider 12 根 + knob 1 个、预设按钮选择器
  `.preset-btn`）；无头浏览器实测四区依序、零溢出、面板 507px < 抽屉 632px。
