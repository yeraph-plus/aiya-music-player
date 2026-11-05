# ledger · 构建与工具链（build-tooling）

> 代码位置：tools/、ui/e2e/、CI
> 相关档案：docs/13、30

## 条目（新→旧）

<!-- 每条一行：- YYYY-MM-DD · 代码位置 · 事实/取舍 · 来龙去脉（可选） -->

- 2026-11-06 · .github/workflows/{ci,release,release-beets,build-essentia-linux}.yml、Cargo.toml、ui/package.json + 双 lock · 依赖盘点落地批：CI actions 升当前 major（checkout v7 / setup-node v7 / setup-python v7 / setup-uv v10 / upload-artifact v7，原 v4–v6 落后 2–4 个 major）；ci.yml 新增 audit job（taiki-e install-action 装 cargo-audit，ubuntu 独立 job——全仓此前从未跑过 advisory 扫描，bundled SQLite 是周期性 CVE 源）；rusqlite 0.40.2→0.41.1（libsqlite3-sys 0.39.1 = bundled SQLite 上游更新，升级主因）、base64 0.22→0.23、cargo update 吸收 zip 9.4.0；npm three 0.186→0.187（caret 0.x 不跨 minor，manifest 必须动，@types/three 同步）+ npm update 吸收 svelte-check 4.7.8 / lucide 1.69 / tailwind 4.3.4 / vite 8.3.6 / bits-ui 2.20.6 · 暂缓清单（各需专门批次）：ureq 2→3（API 重写，HLS 拉流/下载器/SUMS 三处用户面）、sha2 0.10→0.11、symphonia 0.5→0.6（音频核心，docs/22 feature 集需对照）、typescript 5.9→7（svelte 生态兼容未验证）；tower-http 0.6→0.7 随 web 拆分落地时直接写 0.7（origin/main 尚无该依赖，aiya-server 未合入）



- 2026-11-04 · .github/workflows/ci.yml · 9 个 beets smoke 全量进 CI（原 6 个）· 两个真实破坏（uninstall 的 ImportError、bins 的过期断言）恰好都发生在缺席的套件里，本地 verify 红而 CI 绿；tools/check-style-allowlist.json 同批落位（AGENTS.md 早已引用它）。


- 2026-11-03 · tools/check-boundary.mjs · beets-client 词边界门禁的三条取舍：pythonVersion 例外（它是协议线上字段名，改名即换协议）；beets 不作禁用词（包名与协议字段必然带它，禁的是内部概念）；大小写敏感 + 词边界（否则 Stdio::piped 命中 pip、library_path 命中 Library，门禁变噪音就会被关掉）
