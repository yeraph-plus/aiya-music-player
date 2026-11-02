# ledger · 构建与工具链（build-tooling）

> 代码位置：tools/、ui/e2e/、CI
> 相关档案：docs/13、30

## 条目（新→旧）

<!-- 每条一行：- YYYY-MM-DD · 代码位置 · 事实/取舍 · 来龙去脉（可选） -->

- 2026-11-03 · tools/check-boundary.mjs · beets-client 词边界门禁的三条取舍：pythonVersion 例外（它是协议线上字段名，改名即换协议）；beets 不作禁用词（包名与协议字段必然带它，禁的是内部概念）；大小写敏感 + 词边界（否则 Stdio::piped 命中 pip、library_path 命中 Library，门禁变噪音就会被关掉）
