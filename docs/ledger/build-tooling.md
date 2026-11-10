# ledger · 构建与工具链（build-tooling）

> 代码位置：tools/、ui/e2e/、CI
> 相关档案：docs/13、30

## 条目（新→旧）

<!-- 每条一行：- YYYY-MM-DD · 代码位置 · 事实/取舍 · 来龙去脉（可选） -->

- 2026-11-11 · .github/workflows/{ci,release-beets,build-essentia-linux}.yml、beets-tool/{tests/catalog_contract.py,tools/build_bundle.py,tools/essentia_smoke.py}、crates/aiya-server/tests/contract.rs、src-tauri/tests/ipc_contract.rs、crates/aiya-data/src/ids.rs · 测试面对向审查 P0 批（docs/70）：ci.yml 新增 linux-test job（aiya-server 依赖图 7 crate 全量 + cargo fmt --check 首次进 CI——生产平台是 Linux 而 CI 只有 windows，cfg(unix) 用例此前零在线执行；aiya-audio 因 wasapi 无 cfg 门被排除，进来的前提是平台解耦）；catalog_contract platforms 门禁修复（item_errors 未定义 → 坏值进来 NameError 伪装门禁，负向十五连补两用例本地实证变红）；contract.rs client 解析加下限守卫（实测 101 条、阈值 60——"UI⊆ROUTED" 的 missing 恒空静默空转路被堵）；build_bundle 与 release-beets 自检补 `import beets`（`aiya_beets version` 只打印常量，beets 坏包此前能过全部发布闸）；essentia 冒烟两份手写脚本同步补 highlevel ≥12 键下限（只盯 danceability 漏"部分模型缺席"）；收尾撞出并修掉 main 两个既有红（stash 实证 527cbe2 即红，推断线上同红）：ipc_contract 桌面侧漏 station 十命令的 SERVER_ONLY 对称豁免（2451486 起红，补豁免表 + 双向钉扎：桌面注册 web 专属命令 = 越界要报）、ids.rs 三个 station 函数在 test 模块之后触发新 toolchain 的 items_after_test_module（CI 门禁 -D warnings 必红，函数上移）· P1/P2 待办清单见 docs/70 §4（e2e console.error 收集、mock↔命令面一致性闸、xtractor monkeypatch 单测、auth.rs/sidecar.rs 零测试面）

- 2026-11-06 · .github/workflows/{ci,release,release-beets,build-essentia-linux}.yml、Cargo.toml、ui/package.json + 双 lock · 依赖盘点落地批：CI actions 升当前 major（checkout v7 / setup-node v7 / setup-python v7 / setup-uv v7 / upload-artifact v7，原 v4–v6 落后 2–4 个 major）；ci.yml 新增 audit job（taiki-e install-action 装 cargo-audit，ubuntu 独立 job——全仓此前从未跑过 advisory 扫描，bundled SQLite 是周期性 CVE 源）；rusqlite 0.40.2→0.41.1（libsqlite3-sys 0.39.1 = bundled SQLite 上游更新，升级主因）、base64 0.22→0.23、cargo update 吸收 zip 9.4.0；npm three 0.186→0.187（caret 0.x 不跨 minor，manifest 必须动，@types/three 同步）+ npm update 吸收 svelte-check 4.7.8 / lucide 1.69 / tailwind 4.3.4 / vite 8.3.6 / bits-ui 2.20.6 · 暂缓清单（各需专门批次）：ureq 2→3（API 重写，HLS 拉流/下载器/SUMS 三处用户面）、sha2 0.10→0.11、symphonia 0.5→0.6（音频核心，docs/22 feature 集需对照）、typescript 5.9→7（svelte 生态兼容未验证）；tower-http 0.6→0.7 随 web 拆分落地时直接写 0.7（origin/main 尚无该依赖，aiya-server 未合入）· 坑：action 的 release 与浮动 major tag 不同步——setup-uv releases/latest 报 v10.3.3，但 git tag 只到 v7 浮动 major（v8.x/v9.0.0 有精确 tag 无浮动），dispatch 秒挂 "unable to find version v10"；升 action 前用 git/refs/tags 核对可用 major tag，别信 releases/latest



- 2026-11-04 · .github/workflows/ci.yml · 9 个 beets smoke 全量进 CI（原 6 个）· 两个真实破坏（uninstall 的 ImportError、bins 的过期断言）恰好都发生在缺席的套件里，本地 verify 红而 CI 绿；tools/check-style-allowlist.json 同批落位（AGENTS.md 早已引用它）。


- 2026-11-03 · tools/check-boundary.mjs · beets-client 词边界门禁的三条取舍：pythonVersion 例外（它是协议线上字段名，改名即换协议）；beets 不作禁用词（包名与协议字段必然带它，禁的是内部概念）；大小写敏感 + 词边界（否则 Stdio::piped 命中 pip、library_path 命中 Library，门禁变噪音就会被关掉）
