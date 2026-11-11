# 60 · 注释治理 v2：引用单向化与 docs/ledger（2026-11-03）

> 类型：治理方案　状态：已收口（2026-11-12，对账见 §7）
> 关联：取代 docs/61 的执行模型（61 未入库即删：其取代对象 59/60 的链条断裂，
> 属悬空记录）；docs/59 的批次执行方式（G4b 后的语义重写分片）停用，
> 已落地部分不受影响。

## 0. 一句话

代码注释只回答"现在是什么、为什么这样写"；历史叙事、设计取舍、踩坑过程
一律落 `docs/ledger/`（按架构域的流水账）；**代码内禁止出现指向 docs 的
引用**（`docs/NN`、`§`、`ADR-`、`notes/`、`AGENTS.md`）——引用方向从此单向：
ledger → 代码。

## 1. 为什么改

v1（docs/59 的 G0–G7 批次与三层模型）有两个与目标冲突的赌注，均被实践证伪：

1. **"注释保留指针"**。L1 模板把 docs 链接设为义务，注释累计出现
   620 行 / 548 块 docs 引用。AI 代理读代码时顺引用进入编号文档翻旧实现，
   是"跑偏、反复思考"的主要来源——注释治理的原始动机正是压掉这个来源，
   v1 却把引用制度化成了义务。
2. **语义重写分片**。试跑批把 27 个 `#[test]` 属性行覆盖成注释行
   （cargo test 假绿，工作区整体回退）；重写路径成本高、风险大、
   单位收益低。

替代路径：机械批先清结构（日期戳 145→17、内部多行块 233 个压到 ≤2 行、
分隔线/空块清零，`tools/comment-gov/` 三重门 + 剥注释零差异验收，已收口）；
剩余引用类内容按"纯指针直接删、混合块先 harvest 进 ledger 再删"两档清零。

## 2. docs/ledger/ 制度

- 位置 `docs/ledger/`，一主题一篇；首版主题沿用十主题划分
  （audio-engine / data-layer / player-core / beets-protocol / tauri-host /
  beets-tool / ui-shell / ui-render / ui-ipc / build-tooling）。
- 每篇头部固定两行：`> 代码位置：<路径列表>` 与 `> 相关档案：docs/NN…`。
  docs → docs 的引用合法，这是引用唯一允许存在的方向。
- 条目 schema（append-only，新 → 旧）：

      - YYYY-MM-DD · 代码位置 · 事实/取舍（现在时一句话）· 来龙去脉（可选，≤2 句）

- 纪律：
  - 同一事实只在一处写全；落不进一行事实的内容交给 git 历史，不硬写。
  - 不从编号文档搬家——docs/NN 是历史档案；ledger 只承接"从代码里删掉的"
    与"今后的新取舍"。
  - 新取舍默认进 ledger；跨进程契约、难回退决定等仍按 AGENTS §5.3 开
    编号文档，ledger 留一行指针（docs → docs）。
  - 单篇 ≤400 行，超出即拆题。

## 3. 注释规范（v2 修订点）

| 项 | v1 | v2 |
|---|---|---|
| L1 文件头 | ≤8/16 行，四要素，docs 链接义务 | ≤2 行（职责+边界），无链接义务 |
| L2 契约（`///`、JSDoc、docstring） | 逐条成条 ≤3 行 | 不变；行内 docs 引用随批次收口（见 §5） |
| L3 行内 | 单行 ≤100 字符 | 不变 |
| 证据 | 注释里指 docs/NN | 不进注释：测试名/schema/实测数字可留，docs 指针删 |
| 历史叙事 | 改写保留指针 | 直接删；有保留价值 → ledger |
| Python docstring | 按注释计 | 运行时字符串，按代码对待，永不机械触碰 |

例外保留：`契约：`/`场景：` 标记块（wire 表与单测场景说明，≤8 行）。

## 4. 门禁与工具

- `tools/check-style.mjs` 增加第三类违例 **DOCREF**（`docs/\d`、`notes/`、
  `ADR-`、`AGENTS.md`；裸 `§` 为 GPL 条款/源码定位等合法引用，不拦），
  并按层分桶接入 `verify` 阻断（`check:style:strict`）：**阻断** = 非 L2
  注释的日期/引用；**L2 延后** = `///` 与块注释内的同类命中，随收口批
  并入阻断；禁词维持语域 report。`bindings/` 生成物不扫；beets-tool 的
  Python 侧由 `tools/comment-gov/scan.py` 覆盖（口径一致）。
- `tools/comment-gov/`：scan（盘点）/ apply（三重门变换：diff 行级白名单、
  属性行计数、剥注释+丢空行零差异）/ verify（对 HEAD 终验）/
  test_scan（状态机用例）。验收判据沿用：剥离注释后代码逐行一致、
  `#[test]` 计数不变。
- 收口判据：非 L2 的 DOCREF 命中归零（确属合理的保留项走白名单）；
  ledger 条目覆盖全部被删的混合块。

## 5. 执行

- 2026-11-03：删 docs/61；建 ledger 骨架；check-style 加 DOCREF 并接入阻断；
  引用类注释分拣（排除 L2：剥引用留理由 / 纯指针删 / 混合块 harvest 后删），
  日期 parenthetical 同批剥离。L1 整体压缩 ≤2 行未执行（带引用的 L1 已随批
  清零），列为待办；L2 行内引用的收口另批处理。

## 6. 执行结果

2026-11-03 三批机械治理（`tools/comment-gov/`，逐批三重门 + 剥注释零差异验收）：

- 结构批：日期戳 145 → 0（余 1 行为 JSDoc 开符行，随 L2 收口）、内部多行块
  233 个压到 ≤2 行、纯分隔线/空注释块清零。
- 引用批：剥离引用 411+ 行、删纯指针/历史段 9 行；非契约注释（L1/L3/测试/tools）
  的 docs 引用清零。ledger 收录 4 条（beets-tool、beets-protocol、ui-render、
  build-tooling 各一）。
- 余量：docs 引用 223 处全部位于 Rust `///` 契约注释（§5 延后收口）与
  `bindings/` 生成物（12 处，C4 不手改）；日期戳余 1 行（JSDoc 开符行，同上）。
- 验证：cargo test 489 全绿（dto_wire 一处断言引用已删除的
  `pythonSupported` 字段，随单轨化决策同步清理，ledger tauri-host 有记）、
  svelte-check 0/0、gen:types ✓、cargo fmt 零 diff（含 beets.rs 四处存量
  重排一并收口）、`#[test]` 422 基线不变、剥注释+丢空行 vs HEAD 零差异
  （唯一例外 `tools/check-style.mjs` 为本批的功能性改动：新增 DOCREF 检测）。
- 待办：`///` 行内引用收口（L2 批）——收口后 L2 延后桶（当前 210 处）
  并入阻断。`check:style:strict` 已于 2026-11-03 接入 `verify`，负向验证
  通过（注入 `//` 行引用 → 红；注入 `///` 行引用 → 豁免；还原 → 绿）。
  另：L1 文件头整体压缩 ≤2 行（129 块/1,685 行，现内容已无引用，纯体量
  优化）；beets-tool 的 `#` 注释常驻门禁（现靠 scan.py 盘点，无常驻拦截）。

## 7. 收口对账（2026-11-12）

拍板与执行按 docs/71（本档 §5/§6 的执行模型被其取代：L2 延后桶取消、
分区收割子代理分治）。本档留存的数字以实测口径修正如下：

- **L2 延后桶已清零**：拍板时存量 253 处（§6 待办所记 210 为旧口径）。
  日期/引用在任何注释形态都是阻断，存量走 allowlist 棘轮——峰值 355 条
  （JS/Rust 255 + Python 100），批 C–G 分区收割烧至 **1 条**（beets-tool
  protocol_contract.py:55 的 JSON Schema "2020-12" 技术标识，正则误伤豁免，
  非治理延后项）。烧完回归裸阻断。
- **门禁四洞全闭**（docs/71 §3）：CI verify job 跑 strict + check:style:py
  + comment-gov verify；DATE 正则放宽（月戳与 "YYYY 年" 可见）；语域叙事词
  （旧实现/此前/曾经/以前/旧版/事故）进 report 桶——收割后**全仓命中 0**
  （含 tools 自区 4 处），观察期提前收口；scan.py --check 接进 verify 链
  （beets-tool 的 # 注释与 docstring 同为注释面，共用 allowlist）。
- **扫描面扩 .css/.sql**：此前两形态不扫属盲区；批 G/F 实测 styles/*.css 与
  migrations/*.sql 清零后纳入门禁。
- **验收清单缺口补记**：ts-rs 把 `///` 导出进生成物，注释治理批的验收必须
  带 `gen:types:check`（run 50 实证：漏跑则 CI 契约生成物一致步红）。
- 分区执行记录：批 C beets-tool 36 文件、D host-core/beets-client 10、
  E src-tauri 11、F 其余 crates 26+生成物、G ui 76；归档条目逐区入
  docs/ledger/（本档 §2 制度承接）。
