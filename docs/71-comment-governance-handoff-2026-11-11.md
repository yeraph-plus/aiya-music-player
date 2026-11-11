# docs/71 · 注释语域治理交接（2026-11-11）

> 上一会话完成：测试面对向审查（docs/70）→ 0.2.1 发布 → 本批拍板。本档是
> **下一会话的完整交接**：拍板结论不得重议，§1–§3 是全部事实依据，§4 是批次
> 计划，§5 是每批规程。读档顺序：本档 → AGENTS.md §4/§5 → docs/60 →
> tools/check-style.mjs → tools/comment-gov/。

## §0 拍板结论（已定，不重议）

1. **代码注释零外部引用**：`docs/NN`、`§`、`ADR-`、`notes/`、`AGENTS.md`
   引用在**任何注释形态**（含 `///`、`//!`、块注释）都禁。上一会话提议的
   "L2 转合法"被否——L2 延后桶取消，**253 处存量全部清理**。
2. **工作模式反向**：不是"注释指向文档"，而是"文档结构反过来服务查询"。
   AGENTS.md 改造为**结构化查询入口**：agent 遇到"这段为什么这样写/这个坑
   当年怎么回事"，先查 AGENTS 路由表找到档案，再进 docs 全文。
3. **踩坑实录归档级备查**：复杂事故链进编号文档的踩坑章节（形态见
   docs/50 §2.5、docs/67 §3——事件链 + 实测证据 + 修法）；一行式取舍进域
   ledger（docs/ledger/，一行式：日期 · 代码位置 · 事实 · 来龙去脉）。
   代码注释只留：**是什么 / 为什么（现在时）+ 仓内证据（测试名、schema、
   实测数字）**。
4. **git 历史本身就是修订记录的承接位**（AGENTS §4.1 明文）。注释里的历史
   叙事删了不丢（blame/log 可考）——收割时**不必逐字搬运**：档案里已有全文
   的直接删注释，没有的先摘要归档再删。

## §1 现状快照（截至 f455ab5）

- 提交链：68f4167（测试面 P0，docs/70）→ 330c7bd（0.2.1 收口）→ f455ab5
  （本治理批 0：阻断项清零）。本地 == origin/main。
- 线上：v0.2.1 release run 与 CI 四 job 于 2026-11-11 00:29Z 起跑。
  **新会话开工第一件事：核对这个 run 的结论**（尤其 linux-test job 首跑、
  beets 九套 smoke、e2e mock 44）。历史上 main 的 CI 曾 11-06 起连续三红
  （两个既有红已随 68f4167 修复）。
- 容器镜像发布已暂停：docker-image.yml 的 `v*` tag 触发已摘（330c7bd），
  恢复走 `container-v*` / dispatch，容器整批测试是独立待办（见 §6）。
- 门禁现状（`npm run check:style`）：**阻断 0 / L2 延后 253 / 语域 report
  94**，263 个文件；`tools/check-style-allowlist.json` 为空 `[]`；
  `check:style:strict`（阻断桶→退出码）只挂在本地 `verify`，**CI 不跑**
  ——这是治理失效的根因。
- docs/60 与现实漂移：日期戳"145→0"已回涨到 38 行；"L2 延后 210 处"
  实际 253。

## §2 语料普查（工作量依据；数字为 grep 级，分区动手时逐文件复核）

| 区域 | 日期行 | 叙事词行 | 备注 |
|---|---|---|---|
| src-tauri/src | 2 | 53 | 此前 40；logging.rs:306-320 三处日期是**测试数据**，保留 |
| crates/* | 1 | 155 | 实测 65、此前 52、旧实现 13 |
| beets-tool（src+tools+tests） | 13 | 86 | 七个 23–66 行模块 docstring 是主标的 |
| ui/src + ui/e2e | 20 | 48+ | **月戳形态为主**（"2026-11 拍板"） |
| .github/workflows | 19 | 7 | **不在治理范围**（操作性文档） |

- 叙事行三分法：**152 行"实测 NN"是 §4.2 允许的证据，保留**；~150 行
  "此前/曾经/以前 X → 现在 Y"改写为现在时；~46 行纯史（旧实现对比/事故
  链/轮次）删除或归档。
- 日期行 38 行中 ~15 行是月戳（`20\d{2}-\d{2}` 不带日），现 DATE 正则
  （要求两位日）不可见——批 B 修正则。
- 多行块 ≥15 行共 96 个；**收割标的 7 个（~210 行）**：
  1. beets-tool/tools/build_bundle.py:2-67（66 行，rationale 为主、剔历史钩）
  2. beets-tool/src/aiya_beets/cfg.py:1-63（63 行，directory 真机事故链——
     先查 docs/49/50 是否已有全文）
  3. beets-tool/src/aiya_beets/bins.py:1-48（48 行，replaygain/ffmpeg 事故）
  4. beets-tool/src/aiya_beets/uninstall.py:1-46（46 行，混合）
  5. beets-tool/src/aiya_beets/install.py:1-32（32 行，A 轨退役叙事——
     docs/58 §5 已有全文，直接删）
  6. beets-tool/tools/selfhost_svm_models.py:2-30（29 行，迁移叙事）
  7. crates/aiya-host-core/src/beets.rs:2076-2101（26 行，入库副产品事故
     + 82s 实测表）
  次级：session.rs:305-325、beets.rs:699-718 与 124-143、
  beets-tool/tests/_testmusic.py:2-16、entry.py:1-13（pyz 退出码事故——
  docs/51 §0 已有全文）。

## §3 门禁四洞与加固方案（批 B 的内容）

| 洞 | 现状 | 修法 |
|---|---|---|
| ① CI 不跑 | check:style:strict 只在本地 verify | ci.yml verify job 加一步（windows），连同 comment-gov verify |
| ② DATE 正则 | 要求两位日，月戳/`年`不可见 | 放宽为 `20\d{2}-\d{2}(-\d{2})?` + `\d{4}年` |
| ③ 叙事词不 lint | 只拦 7 个口语词（report-only） | `旧实现\|此前\|曾经\|以前\|旧版\|事故` 先进 **report 桶**跑两周看误报，再议升 L2 延后/阻断 |
| ④ Python 无常驻 | EXTS 无 .py；comment-gov/scan.py 是批次工具 | scan.py 接进 verify 链（npm script + CI）；范围 beets-tool/src+tools+tests |

**L2 棘轮**（拍板 1 的落地路径）：批 B 把 L2 桶从"延后"改为"阻断 + allowlist
豁免存量"——用 comment-gov/apply.py 或脚本机械生成 253 处豁免进
check-style-allowlist.json；此后批 C–G 每清一区烧一段白名单（净减少不净增）；
烧完把白名单清空、豁免逻辑回归裸阻断。**不允许新增豁免。**

## §4 批次计划（子代理分批）

| 批 | 范围 | 执行形态 | 验收 |
|---|---|---|---|
| A | AGENTS.md 查询入口重构 + §4.2 修订落法 | **主会话手做**（设计活，不用子代理） | 人工通读 + 引用自洽 |
| B | 门禁加固四洞 + L2 棘轮 + 253 豁免生成 | 主会话 + 只读子代理核对数字 | strict 全绿、CI 手动 dispatch 验证 |
| C | beets-tool 收割（7 大块 + 13 日期行 + 86 叙事行） | **1 个 general-purpose 子代理**（试点区，先跑精规程再复制到 D–G） | cargo test 无涉；catalog_contract + smoke:proto 必须全绿；check-style/comment-gov 阻断 0 |
| D | crates/aiya-host-core + aiya-beets-client | 1 子代理（规程复制自 C） | cargo test + clippy + fmt + strict |
| E | src-tauri | 1 子代理 | 同 D + src-tauri 测试 |
| F | crates/aiya-data + 其余 crates | 1 子代理 | 同 D |
| G | ui/src + ui/e2e（月戳 + 拍板词区） | 1 子代理 | test:e2e mock 44 + strict |
| H | 流程位收口：perf_50k.rs:3,100 命令删重（= bench:50k）、cdp-probe.mjs:9-27 启动 runbook 挪 AGENTS §2.2、probe:exclusive/probe:clock/smoke:output 补进 §2.2 表；docs/60 对账更新（数字改实测值） | 主会话 | verify 全链 |

**分区即边界**：一个子代理只写自己区的文件，区与区零交集——并行安全。
建议节奏：C 先行（试点，校准规程），D–G 可两两并行，每区一个独立提交。
主会话在每个区提交前必须亲自跑验收命令并过目 diff——子代理只产改写，
不产提交。

## §5 每批规程

### 5.1 改写规则（给子代理的硬约束）

1. 只动注释；代码、字符串字面量（UI 文案里的引用不受限，§4.3）、测试数据
   （如 logging.rs:306-320 日期串）一律不碰。
2. 三种处置，按序判定：
   - **档案已有全文**（先查 docs/ 对应编号 + ledger 检索）→ 注释删除，
     必要时在 ledger 补一行索引（不重复正文）；
   - **档案没有但值得留** → 先把叙事摘要进对应域 ledger（一行式，含日期与
     代码位置）或编号文档踩坑章节（复杂事故链），然后注释改为现在时一句
     （"为什么"保留约束来源，不带事件链）；
   - **不值得留** → 直接删。
3. "实测数字"证据保留（§4.2 允许）；"此前/曾经 X → 现在 Y"一律改写为
   "X（约束来源）"现在时。
4. **不引入新的外部引用**——改写后的注释里出现 docs/NN 即失败。
5. 逐行注释（`//`、行尾）与 L2（`///`、`//!`、块注释）同规则，本批起无
   延后桶。

### 5.2 归档路由

| 内容形态 | 去向 |
|---|---|
| 复杂事故链（多事件、多轮实测） | 编号文档踩坑章节（docs/50 §2.5 / docs/67 §3 形态）；若该事故已有编号档，只补不重写 |
| 一行式取舍/小修订 | docs/ledger/ 对应域账本（beets-tool / build-tooling / audio-engine / data-layer / player-core / tauri-host / ui-*） |
| 执行流程/命令序列 | AGENTS.md §2.2 表（不是 ledger） |

### 5.3 验收命令（每区提交前主会话跑全）

```bash
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
npm run check:style:strict          # 阻断必须 0
beets-tool 区加跑：beets-tool/.venv/Scripts/python.exe beets-tool/tests/catalog_contract.py
ui 区加跑：npm --prefix ui run test:e2e
```

### 5.4 子代理提示词模板（批 C 起复用）

> 治理区：{目录清单}。读 docs/71 §5.1 改写规则与 §5.2 归档路由后执行：
> (1) 扫描区内全部日期戳与叙事词命中（可用 `npm run check:style` 与
> `python tools/comment-gov/scan.py` 定位）；(2) 逐条按改写规则处置，产出
> ①文件编辑（直接改）②归档条目清单（拟进 ledger/编号文档的一行式文本，
> 汇总回报，由主会话落账）；(3) 报告：处置计数（删/改写/归档）、白名单
> 豁免应删除行、任何拿不准的条目列表。**不得**改代码与字符串字面量，
> **不得**引入 docs/NN 引用，**不得**提交。

### 5.5 提交纪律

每区一个提交，单行长消息（仓库惯例），注明：处置计数 + 归档去向 + 白名单
净变化。ledger 落账随区走（归档条目当批进账，不留尾巴）。

## §6 与本治理无关的挂起项（防丢失，逐项有出处）

- 容器整批测试：Dockerfile ffmpeg/chromaprint 冒烟、首启 bins 权限位、
  compose /music:ro 与工作台整理的 EROFS 提示（docs/70 §4 用户拍板"回头
  整批测试"）；恢复镜像发布走 container-v* / dispatch。
- docs/70 §4 P1/P2：e2e console.error 收集、mock↔命令面一致性闸
  （station_* 十命令不在 mock）、xtractor monkeypatch 单测、auth.rs /
  sidecar.rs 零测试面、check_client 两分支、cancel 硬钉并协议契约。
- perf_50k `#[ignore]` 性能预算零在线执行（留人工）。
- v0.2.1 release 与 CI 的线上结论核对（§1 第一件事）。
- .research_tmp/ 临时区（仓库外，linux_extractor_ci/ 证据子目录按 docs/67
  引用需保留，其余可弃）。
