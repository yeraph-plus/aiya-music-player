# docs/70 · 测试面对向审查（2026-11-11）

> 对向口径：不问"测试是否全绿"，问"哪里全绿但什么都没验证"。方法：三路并行
> 扫描（Rust 面 / beets-tool Python 面 / 前端+e2e+契约面）+ 头号发现逐处源码
> 抽查（6/6 属实）。P0 批当日落地，P1/P2 见 §4。

## §0 结论

测试面规模：Rust 448 条（audio 103 / data 130 / src-tauri 75 / host-core 48 /
player 45 / beets-client 32 / store 9 / server 4 / dto 2 / audio-host 0）、
beets-tool 9 套契约 smoke、e2e 44 条 mock（CI）+ 6 条真 server + 44 条 Firefox
（均 manual）。接线质量异常扎实：Python 9/9 进 CI、负向"必须变红"套件、
`dto_wire` 锁 serde 真实 wire 形状、`proto:check` 字节镜像。

两大结构性盲区：

1. **CI = windows 单平台，生产 = Linux**（Docker 部署 aiya-server）。所有
   `cfg(unix)` 测试从未在线执行——canon_path unix 8 条、extract_safely
   unix_mode 还原、bins/lockfile 的 unix 分支全部零在线验证。2026-11-09
   canon_path 的 Windows 语义漏进 Linux（本仓审查实录）恰是这一类的真实事故。
2. **守卫自身会失效**（假绿三类，§2 逐条列）。

## §2 假绿清单

**A. 任何环境不可证伪**

- `aiya-audio/src/mmcss.rs:58` `register_is_fallible_not_fatal`——Windows 分支
  `let _ = r;`，unix 分支断言由构造保证的 None。
- `src-tauri/src/lib.rs:284` `snake_case_mapping_matches_tauri_convention`——
  camel→snake 在测试里重写一遍再断言硬编码对；Tauri 真实映射变了它照样绿
  （注释自认是"锚定规则"tripwire，但名实不符）。
- `aiya-beets-client/src/session.rs:1010` `collect_sink_records_channel`——
  测试自己的 sink 记了测试自己 emit 的东西，测的是 harness。

**B. CI 上恒为空跑**

- `aiya-host-core/src/enrichment.rs:476` 真 sidecar→store 端到端：双重 skip
  （无 venv/音源 return；有目录无 wav 再 return）——enrichItem 管线唯一端到端
  守卫，单测自认"测的是手写形状"。
- `aiya-host-core/src/beets.rs:3048` `dump_parse_end_to_end`：Rust 解析真
  sidecar 输出的唯一测试，CI 恒 skip。
- `aiya-host-core/src/beets.rs:3001` `probe_finds_bundle_runtime`：B 轨自洽 +
  协议版本对齐前提，CI 恒 skip。
- `beets-tool/tests/catalog_contract.py:150`（已修）platforms 校验分支引用
  未定义的 `item_errors`：坏值进来 = NameError 崩溃而非变红；负向套件无
  platforms 用例 → 这条门禁从未被证明能触发。
- `aiya-server/tests/contract.rs:121`（已修）"UI 调用 ⊆ ROUTED" 的 client 侧
  解析无下限守卫：解析漂移成空集 → missing 恒空 → 判定静默空转（routed 侧
  有 `>=50` 守卫，反向会响，正向不会）。
- `beets-tool/tests/protocol_contract.py:402` `AIYA_BEETS_ONLINE=1` 的
  album/albumError/done 帧校验 CI 恒 off（有打印，属"知情缺角"）。
- `beets-tool/tests/smoke_enrich.py:368` XTR_OK 双分支：CI 无提取器只测
  E_CONFIG 拒绝路，xtractor 经协议真跑成功的路径 CI 永远看不见。
- `aiya-data/tests/perf_50k.rs:100` `#[ignore]`：5 万行性能预算零在线执行。

**C. 结构性覆盖错觉**

- 真实后端 e2e（登录闸/真 `<audio>`/station 生命周期 6 用例）与 Firefox 矩阵
  manual-only；CI 的 e2e 绿只证明 mock 套件对假数据工作。
- mock 覆盖 94/105 命令且无一致性闸：`station_*` 10 条 + `app_open_url` 在
  mock dispatch 缺席——新增命令过所有 CI 闸却在 CI 跑的套件里不可测。
- 27/44 的 route.spec 与全部 http spec 不收集 console.error/pageerror，与
  `playwright.config.ts` 注释宣称的语义不符。
- 零测试高价值模块：`aiya-server/auth.rs`（is_public 白名单一处笔误=全端点
  泄漏；token 比较非常量时间）、`src-tauri/sidecar.rs`（613 行命名管道传输）、
  `aiya-audio-host/main.rs`（405 行）、`aiya-host-core/proc.rs`（reveal 的
  explorer 引号 bug 有前科无钉）、`beets-tool xtractor.py` 的 `_input_path`
  monkeypatch（模块存在的全部理由零测试，其文档自述的失败模式恰是"每首静默
  跳过、退出 0、bpm 全空"）。
- **发布闸从不 `import beets`**（已修）：build_bundle 与 release-beets 的自检
  只跑 `aiya_beets version`（cli 打印常量）+ 插件模块 import——beets 本体在
  包里坏掉能通过全部发布闸；依赖装齐 ≠ 能导入。
- `build-essentia-linux.yml` 与 `essentia_smoke.py` 双份手写口径（12 模型检查
  在 workflow 的 svm_prep.py 里其实有；真正缺口是两处都只把 danceability 做
  金丝雀，"部分模型缺席"会漏——已补 ≥12 键下限）。

## §3 P0 落地批（当日）

1. `catalog_contract.py`：`item_errors`→`errs` 修复；负向十三连→十五连，
   补 "platforms 写了未知平台"/"platforms 是空数组" 两用例（本地 ALL PASS
   实证两闸变红）。
2. `aiya-server/tests/contract.rs`：client 侧下限守卫（实测 101 条，阈值 60，
   注释写明漂移模式）。
3. `build_bundle.py` 自检 + `release-beets.yml` 解包自检补 `import beets`
   （Docker beets-bundle 构建走 build_bundle 同一函数，一并受益）。
4. `ci.yml` 新增 `linux-test` job：`cargo test -p aiya-server -p aiya-host-core
   -p aiya-data -p aiya-player -p aiya-dto -p aiya-beets-client
   -p aiya-beets-store` + `cargo fmt --check`（fmt 首次进 CI）。crate 集 =
   aiya-server 完整依赖图 = Dockerfile `cargo build -p aiya-server` 的同一
   编译面（Linux 可编译性有 Docker 实证）；刻意排除 src-tauri（miow/windows
   系）与 aiya-audio（wasapi 无 cfg 门，实测 crate 源 0 处 cfg(windows)，
   非 Windows 编不过——进来的前提是先平台解耦）。
5. `assert_smoke.py`（workflow heredoc）与 `essentia_smoke.py` 同步补
   highlevel 键数 ≥12 下限 + 互引注释。
6. 收尾时撞出 **main 上两个既有红**（与审查无关但挡住全绿提交，本地 stash
   到 clean HEAD 527cbe2 复现，推断线上同红）：
   - `src-tauri/tests/ipc_contract.rs` `every_ui_invoked_command_is_registered`
     自 2451486（推流频道 UI 接线）起红：10 条 `station_*` 在 client.ts 静态
     文本里，桌面壳不注册；"豁免清空"清的是 **web 侧** contract 的
     SERVER_ONLY（station 已进 web ROUTED），桌面侧的对称豁免漏了。修法 =
     桌面契约补 `SERVER_ONLY` 豁免表 + 双向钉扎（缺席不报错；桌面注册了
     web 专属命令反而报"命令面越界"）。
   - `aiya-data/src/ids.rs` `items_after_test_module`（新 toolchain lint，
     `cargo clippy --workspace --all-targets -D warnings` 是 CI 门禁）：
     station 批把三个函数追加在 test 模块之后。修法 = 函数上移。

验证：catalog_contract ALL PASS（15 负向全红）；`cargo test --workspace`
510 绿 0 失败（2 ignored 为性能预算）；`cargo clippy --workspace
--all-targets -D warnings` 0 警告；fmt clean；三份 workflow YAML 解析通过；
heredoc Python 语法校验通过。

## §4 P1/P2 待办（后续批次）

**P1**：route.spec/http.spec 统一收 console.error/pageerror；mock↔命令面一致性
闸（station_* 补进 mock 或显式豁免登记）；xtractor monkeypatch 最小单测；
http-e2e 至少进 nightly cron；mmcss/snake_case 改真调用或改名如实。

**P2**：auth.rs 白名单表钉死测试；sidecar 握手抽纯逻辑层再测；
`protocol.py check_client` 补两分支（非整数 id、"update the player"）；
cancel 硬钉（现只在 smoke_apply）并入协议契约；enrichment Sink flush 错误
分支测试。
