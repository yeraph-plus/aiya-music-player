# docs/30 — 交接前全量审查 · 批 B 接手方案（2026-10-27）

状态：**审查完成 + 第 0 步清账已落地**（业务代码未动；只改注释、类型生成物、README 与本文）。
批 B 未开工。清账明细见 §9。

审法：先跑验证套件拿实测数据，再分契约 / 后端 / 前端三层读代码；所有结论带文件行号或命令输出。

基线：`main c8a6cd0`（docs/29 批 A），工作区干净。

---

## 1. 实测验证（本次真跑，不是引用旧记录）

| 检查 | 命令 | 结果 |
|---|---|---|
| Rust 全量测试 | `cargo test --workspace` | **291 passed / 0 failed**，1 ignored（`perf_50k`）。含 `ipc_contract` 4 条装配测试 |
| 前端类型 | `npm run check`（svelte-check） | **0 errors / 0 warnings** |
| 前端构建 | `npm run build:ui` | ✓ 14.7s；`index-*.js` 436.79 kB（gzip 130.61 kB），CSS 60.65 kB |
| e2e（mock 后端 + 系统 Edge） | `npx playwright test` | **23 passed / 19.6s** |
| 类型生成一致性 | `npm run gen:types:check` | ❌ **FAIL** —— 见 P0-1 |
| 完整打包 | `npm run bundle` | ✓ 5m34s（release LTO），产出 `target/release/bundle/nsis/AIYA Music_0.1.0_x64-setup.exe` **5.74 MiB** |
| 静态检查 | `cargo clippy` | ⚠️ **跑不了**：toolchain 未装 clippy（同 rustfmt） |
| TODO/FIXME | 全仓 grep | **0 处** |

> 打包能出包这点需要写下来：这是 docs/13 之后第一次在当前 target 上复现完整 NSIS 产物，
> 5.74 MiB 与 docs/13 记录的 4.06 MiB 差异来自后续新增代码，属正常增长。

---

## 2. 契约层（UI ↔ Rust）

### 结论：**结构是健康的，两处账面失真**

**做对的部分（有机器守）**

- 62 个命令全部在 `generate_handler!`（lib.rs:2866）注册，均有 typed 入口；`src-tauri/tests/ipc_contract.rs` 四条测试分别守：UI 调了但没注册、参数名不匹配（`app_set_ui_state` 那类坑）、注册了但没人调（≤8 条容差）、client 覆盖每个注册命令。
- 类型以 Rust 为源（ts-rs → `ui/src/lib/ipc/generated/`，50 个文件）。参数侧刻意收 `Json` 手动解析，避免 Tauri 反序列化失败绕过 `ApiError` 分类 —— 这个设计是对的，且注释写清了理由。
- 浏览器预览的 mock 派发表（`mock/index.ts`）与 client **62 条一一对应，零漂移**（脚本对拍过）。
- 事件面 5 条：`player:state` / `spectrum:frame` / `scan:progress` / `beets:import` / `audio:mode`，前后端名字一致。

**问题**

| # | 级别 | 事实 |
|---|---|---|
| P0-1 | ~~阻断~~ **已修** | `gen:types:check` 红：`AlbumPage.ts` 与 Rust 源不一致。差异只有一行 doc comment —— 源里是 `` [`FACET_LIMIT`] ``，生成物是 `` [`FACET_LIMIT] ``（少一个反引号）。**功能零影响，但它是 verify 门禁**，当时 trunk 上 `npm run verify` 必红。已重跑 `npm run gen:types` 修复（52 个类型）。<br>根因值得记：docs/29 §13「遗留」写的是"ChosenRoot/LibraryRoots 两份手写契约与 ts-rs 输出格式一致"——那是**人工判断**，没有机器验证，于是一处陈旧生成物被漏过。以后"遗留：gen:types:check 没跑"不能写成"应该一致"。 |
| P1-2 | 应修 | 命令数陈述全线陈旧：`client.ts:7`「54 个命令全部覆盖」、`ipc/index.ts:8`「全部 54 个」、`schema.ts:4`「54 个命令」、`lib.rs:5`「全部 55 个」、`docs/17:483`「54 个命令」。实际 **62**。这类数字会持续腐烂，建议改成"以 `generate_handler!` 为准（当前 62）"或直接删数。 |
| P2-3 | 记录 | `scan:progress` 的载荷**没有生成类型**（`ScanProgress` 手写 `Serialize`），前端 `readScanProgress`（library.svelte.ts:191）手工逐字段取值。批 B 要加字段，两边都可能漏且**不会有编译错误**。建议批 B 顺手 `#[derive(TS)]`。 |

---

## 3. 后端（crates + 宿主）

### 结论：**分层与并发模型站得住，收尾只欠基线数据与静态检查**

**结构**

- `aiya-data`（SQLite v1 / repo / 扫描 / 封面 / beets 读模型）、`aiya-player`（纯领域状态机）、`aiya-audio`（Symphonia + WASAPI 引擎）、`aiya-audio-host`（音频 sidecar）—— 依赖方向干净，`aiya-data` 只依赖 rusqlite/lofty/ts-rs，不碰 Tauri。
- 宿主 `src-tauri/src/`：`lib.rs`（2949 行 / 62 命令）+ `beets.rs`(905) / `pump.rs`(605) / `sidecar.rs`(451) / `logging.rs`(349) / `smtc.rs`(258)。`pump.rs` 把"进度节流/过期 token 丢弃/曲尾纠偏"抽成可注入时钟的纯核心 —— 这是全套代码里质量最高的一处设计（它修的是"节流基准恒假导致进度条冻结"那类只有真机才发现的 bug）。
- 数据库并发：WAL + `busy_timeout=5000`（db.rs:124,127）；`DbHandle::send` 用 `catch_unwind` 做 panic 隔离（db.rs:177），worker 挂了不会静默挂死后续命令；扫描走**独立连接** + `BEGIN IMMEDIATE`（scanner.rs:500 注释解释了为什么 DEFERRED 会锁升级即 BUSY）。
- 扫描取消语义有三处轮询、且**取消路径绝不 mark_missing**（避免 seen 不完整把在线文件误标 missing）—— 有单测锁死。
- 迁移已压平为 `0001+0002`（db.rs:26 说明），`user_version` 与 DDL 同事务。

**问题**

| # | 级别 | 事实 |
|---|---|---|
| P1-4 | 应修 | 注释失真 3 处：`scanner.rs:5`「事务分批：每 BATCH 个文件一提交（简化为每文件一事务，批量优化见 pass 2）」—— 现在就是批事务，括号里那句是旧状态；`scanner.rs:237`「见 0003 迁移」—— 迁移已压平为 0001+0002；`docs/13 §7`「日志落盘 ❌ 全仓库没有 tracing_subscriber」—— 实际 `logging.rs` 349 行早已实现（按天滚动、保留 7 天、sidecar stderr 统一落盘）。 |
| P1-5 | ~~应修~~ **已修** | `src-tauri/bindings/` 36 个 `.ts` 是**构建残留**：`git ls-files` 为空（未跟踪），全仓无人引用，是 `cargo test` 未带 `TS_RS_EXPORT_DIR` 时 ts-rs 默认写出来的。`.gitignore:35` 的 `bindings/` 早已覆盖它 —— 所以 git 一直是干净的，只是磁盘上留着一份会误导人照着改的陈旧副本。已删除（重跑 `cargo test` 会重建，属预期）。 |
| P1-6 | 应修（未做） | **没有 CI**（`.github/` 不存在），但 `tools/gen-types.mjs` 头注释宣称"CI 里跑 `git diff --exit-code`"。已把注释改成事实陈述（"仓库目前没有 CI，门禁只在本地 `npm run verify`"）；**接入 CI 仍是 P5 待办**，需 Windows runner 跑 `verify`。 |
| P1-7 | 发布阻断 | 代码签名缺失（host + sidecar 两个 exe），docs/13 §7 已列。未签名 + 无声誉 = SmartScreen 拦。**且 `licenses/SOURCE-OFFER.txt` 里的 `https://github.com/aiya-music/aiya-music` 需要确认真实存在** —— GPL-3.0 §6(d) 的源码要约指到一个不存在的地址，是合规硬伤。 |
| P1-8 | 应修 | clippy / rustfmt 未安装，`cargo clippy -D warnings` 从未跑过。收尾阶段跑一次的收益（真 bug 检出）远高于成本。 |
| P2-9 | 记录 | `lib.rs` 2949 行 62 命令单文件。可读性成本真实，但收尾阶段重排的回归风险大于收益 —— **建议不做**，只在新批加入时继续按领域分文件（`beets.rs` 已是范例）。 |
| P2-10 | 记录 | `csp: null`（tauri.conf.json）。UI 已定稿（docs/28/29 之后形态稳定），可以开始收 CSP + capabilities 收窄，但会牵动 `aiya-artwork` 协议与 blob 图片加载，建议单独一批，不与批 B 混。 |
| P2-11 | 记录 | 扫描性能**无基线**：docs/12 只有查询性能（5 万首首屏 0.80ms、游标续页 2.2ms），没有任何"5 万文件全量扫描耗时"的记录。批 B 声称 3–4 倍收益，现在没有可比对象（见 §5 第 1 步）。 |

---

## 4. 前端（ui/）

### 结论：**分层已经收口，自动化网是真的**

- 规模：`src/` 17325 行；10 views / 16 stores / 组件分 `content|shell|ui` 三层；`App.svelte` 只剩装配 + 4 个操作 `document` 级状态的 `$effect`（主题属性、CSS 变量、媒体会话、歌词预取）—— 与注释声称的边界一致。
- docs/24 的诊断项确实修完了：P1 样式分层、P2 Tailwind preflight 白付、P3 双图标系统、P4 `ui` store 上帝对象（现只剩视图状态/设置/引导，歌单 CRUD 已归 `playlists.svelte.ts`）、P7 死代码 —— 抽查 `var(--font-mono)` 已在 `tokens.css:88` 定义，CSS 变量"使用但未定义"的差集为**空**。
- e2e 是活的：23 条覆盖一级路由可达、专辑/艺人详情、批量、搜索、设置页 scroll-spy、正在播放页四层背景。跑在 mock 后端 + 系统 Edge，不需要 Tauri。
- 扫描进度消费：`LibraryTools.svelte:117` 用 `scanned/total` 画进度条，`phase` 只做文案；`library.watchScan` → `scan.afterScan()` → 重拉 stats/playlists/list，收尾链路单一出口（scan.svelte.ts 注释解释了为什么扫描编排不能塞进 `library` —— 会成环）。

**问题（均非阻断）**

| # | 级别 | 事实 |
|---|---|---|
| P3-12 | 应修（文档） | `ui/src/lib/artwork-blob.ts:5` 头注释仍写「我们的封面走自定义 `media://`」，实际协议已是 `aiya-artwork`。同一文件下文解释"为什么不用 media://"是对的，冲突只在第一句。 |
| P3-13 | 记录 | `NowPlayingSpectrum` 与 `SpectrumPanel` 两处可视化，风格键 `spectrumStyle` 四值（mirror/radial/wave/off）—— 已验证可在设置里切换，但"关闭"档在 e2e 无断言，只能真机看。 |
| P3-14 | 记录 | 真机走查三项仍未点过（docs/23 §7）：快捷键 16 个动作逐条、主题×密度目视、960px 下队列抽屉。这三项**没有自动化替代**。 |

---

## 5. 批 B 接手方案（worker 拆分 + rayon 两段式进度）

### 5.1 文档里没写、但会挡住实现的四件事

审查 docs/29 §4 后对照代码，以下四点原文未覆盖，**必须在动手前定下来**：

**(a) 封面落盘跑不到解析线程 —— `save_artwork` 是一体的**
`artwork::save_artwork(conn, cache_dir, data, mime, now)`（artwork.rs:39）同时做三件事：算 hash、原子落盘、`INSERT INTO artworks`。解析线程没有 `conn`（SQLite 连接不能跨线程共享，这是 §4 自己写的前提），所以"封面提取归解析侧"按现状**无法落地**。
→ 拆成 `stage(cache_dir, data, mime) -> StagedArtwork{id, cache_key, mime}`（纯文件 IO，无 conn）+ `link(conn, &staged, now)`（只 upsert 行）；`save_artwork` 保留为两者的组合，不动现有调用点与测试。

**(b) 原子写的临时文件名会撞 —— 串行时无害，并行变真竞争**
`artwork.rs:59` 的 tmp 名是 `.{hash8}.{pid}.tmp`。同专辑多首共用同一张封面 → 多线程同时 `stage` 同一 hash → **同名 tmp 被两个线程同时写**，一个 rename 后另一个 rename 失败（当前代码会走"目标已存在→当作已写入"分支，看起来能兜住，但写坏的中间态仍可能先落地）。
→ tmp 名加线程内唯一后缀（线程 id 或 `AtomicU64` 计数），或 `stage` 前先判 `target.exists()` 直接返回。

**(c) channel 必须有界，否则 OOM**
`ParsedTrack` 含 `picture: Option<(Vec<u8>, Option<String>)>` —— 无损专辑封面单张几百 KB 很常见。5 万文件若无界入队，峰值可达数 GB。§4 只写了"mpsc channel"。
→ `sync_channel(capacity = batch_size * 2)`，用背压把解析池自然限速。

**(d) 没有基线就没有"收益"**
批 B 的验收标准写的是"真机量验证收益"，但现在连串行耗时都没有记录。

### 5.2 落地顺序（每步可独立验证）

| 步 | 内容 | 验收 |
|---|---|---|
| 0 | 修 P0-1 + P1-2/4/5/12（重生成类型、删 bindings、改陈旧数字与注释、README 重写） | `npm run verify` 全绿；`git status` 干净 |
| 1 | 加扫描基线工具（`aiya-data/examples/scan_bench.rs` 或 `#[ignore]` 测试）：同一目录串行扫 N 次取中位数 | 拿到串行基线（库里 1k/5k，真机 5 万）并写进 §6 |
| 2 | artwork 拆 `stage`/`link` + tmp 名唯一化 | 现有 artwork 单测全绿；新增"并发 stage 同 hash"用例 |
| 3 | scanner：rayon 解析池 + 有界 channel + 入库线程（独占 conn，攒批 200、IMMEDIATE 提交） | 新增"并行与串行结果等价"用例（同目录两种模式 DB 内容逐行一致） |
| 4 | 进度两段化：`ScanProgress` 加 `parsed`、`phase` 加 `importing`；`scanned` 语义保持"已入库" | UI 进度条不变；文案加"解析中 X/Y" |
| 5 | 契约收口：`ScanProgress` 加 `#[derive(TS)]`，前端 `ScanProgressView` 改用生成类型 | `gen:types:check` 绿 |
| 6 | 取消：walk / 每文件前 / 批边界三处查；取消后 drop sender → 入库线程 drain 丢弃未提交 | 改写 `scan_cancelled_mid_batches_keeps_committed_prefix`（现断言 `scanned == BATCH_SIZE`，两段语义下要按新口径） |
| 7 | 真机复测串行 vs 并行，数据落 §6 | 有数字才算完成 |

### 5.3 已知会被牵连的测试

- `scanner.rs` 内 5 个测试依赖 `scan_directory_ext` 签名与 `BATCH_SIZE=200`（`scan_cancelled_mid_batches_keeps_committed_prefix` 造 201 个文件切两批）→ 改批大小/签名时一起改，别只改生产代码。
- `lib.rs:991` 的 `spawn_blocking` + 宿主侧 100ms 节流保留（节流仍在宿主，别下移到 scanner —— 否则单测里没法关）。
- 前端 `readScanProgress` 缺字段按 0 处理（library.svelte.ts:191），新增字段不会让旧 UI 崩，但也意味着**漏改不报错**。

### 5.4 预期收益与风险（诚实版）

- 解析并行 = CPU 侧 3–4 倍（4 核），但 `Probe::open().read()` 是**随机 IO**：机械盘或 Windows Defender 实时扫描下，并行度超过队列深度可能反而变慢。→ 并行度按 `max(1, 物理核 - 1)` 起步，真机量出拐点后再定，必要时给设置项。
- 进度"久转"的观感问题，**两段计数比并行本身更有效**（批粒度 200 文件 → 文件粒度）。
- 不做的事：不引入 `tokio`（当前是 `spawn_blocking` + 标准库线程，够用）、不改 `mark_missing` 语义、不动 beets 对账。

---

## 6. 基线与收益实测

（批 B 第 1 步与第 7 步填这里）

| 场景 | 串行（现状） | 并行 | 备注 |
|---|---|---|---|
| 1k 文件 / 全热缓存 | — | — | |
| 5k 文件 / 混合格式 | — | — | |
| 真机曲库 5 万 | — | — | 需用户提供实际库路径 |

---

## 7. 收尾路线（批 B 之后的全局视角）

1. **批 B**：本文 §5。
2. **批 C（venv 分发）**：`beets.rs:64` 已内置开发环境 venv 候选（`../tools/beets-venv/Scripts/python.exe`），`tools/beets-requirements.lock` 已锁 20 个包 —— 批 C 的实际增量是「探测顺序改为 安装包内 venv → 开发 venv → 系统 Python」+ CI 预构建 venv 打进 resources + `install.py --from-wheels`。拍板已定：接受安装包 +50–80 MB。
3. **批 D（beets 命令面）**：sidecar 加 `modify_items / refile_items / find_duplicates / update_library`；**注意拍板 5 撤掉了 beets 高级查询**，只做应用内规划好的指令。首发档插件（replaygain / lyrics / fromfilename / duplicates / scrub / smartplaylist）写进默认 `config.yaml`，requirements.lock 需跟着加。
4. **P5 打包收尾**：签名（两 exe）、SOURCE-OFFER 地址核实、安装/卸载在干净机的闭环、4h soak、杀软误报。
5. **真机走查**（docs/23 §7 三项）：快捷键 16 动作、主题密度、960px 队列抽屉。
6. **文档收口**：README 重写（现状还停在 "Phase 0"，结构图里列着不存在的 `aiya-app`/`aiya-system`）、`docs/17` 命令数、`docs/13 §7` 的日志条目。

---

## 8. 一句话总结

契约、后端、前端三层**没有结构性债务**，291 个 Rust 测试 + 23 条 e2e + 装配测试都在守；能立刻发布阻断的只有一条（`gen:types:check` 红）和一组账实不符（命令数、README、bindings 残留、SOURCE-OFFER 地址）。
批 B 的真正难点不在 rayon，而在 §5.1 那四件文档没写的事 —— 尤其是封面落盘必须先把 `save_artwork` 拆成无 conn 的 `stage`，否则"解析并行"根本接不上。

---

## 9. 第 0 步清账记录（2026-10-27，只动注释/生成物/文档，不动业务逻辑）

### 改动清单

| 文件 | 改动 |
|---|---|
| `ui/src/lib/ipc/generated/AlbumPage.ts` | 重跑 `gen:types`（52 个类型）；仅一行 doc comment 反引号 |
| `src-tauri/bindings/`（36 个 .ts） | 删除（构建残留，`.gitignore:35` 已覆盖，git 一直是干净的） |
| `src-tauri/src/lib.rs` | 头部注释：去掉写死的 55/53 数字，改为"以注册表为准"；补历史沿革 |
| `src-tauri/src/scanner.rs` | 头部注释订正事务粒度（已是批事务）；`0003 迁移` → `0001 基线`（迁移已压平） |
| `ui/src/lib/ipc/client.ts` / `index.ts` / `schema.ts` | 去掉写死的 54 个命令 |
| `docs/17-frontend-contract-2026-10-25.md` | 同上（测试表那行） |
| `docs/13-packaging-2026-10-24.md` | §7 日志落盘 ❌ → ✅（logging.rs 早已实现）；安装包体积 4.06 → 5.74 MiB（本次复测）；CSP 备注更新 |
| `ui/src/lib/artwork-blob.ts` | 头注释 `media://` → `aiya-artwork://`（Electron 时代遗留） |
| `tools/gen-types.mjs` | "CI 里跑 git diff" → 事实陈述：仓库无 CI，门禁只在本地 verify |
| `README.md` | 整篇重写：状态（功能完成 → 收尾）、真实 crate 结构、真实依赖方向、命令表（291 测试 / e2e / verify 内容 / 23 条 e2e）、进度与剩余 |
| `src-tauri/tests/ipc_contract.rs` | `parse_registered` 由 `find` 改 `rfind`（见下） |

### 清账过程中自己踩的坑（值得记）

改 `lib.rs` 头部注释时，为了说明"事实源在哪"，写了一句带方括号的宏调用示例。
`ipc_contract` 的解析器是**文本扫描 + 首个匹配**，于是它锚到了注释里那一处，
解析出 0 条命令，`every_ui_invoked_command_is_registered` 与
`new_client_covers_every_registered_command` 同时红 —— 测试立刻抓住，没有流到后面。

处理：① 注释改成不含方括号的措辞；② 解析器改 `rfind`（注册表是文件末尾唯一真实调用），
并在测试里写明这次事故，避免同类注释再犯。**这正是装配测试的价值：它守的不是业务，是"两边能不能接上"。**

### 复验（清账后全跑）

| 检查 | 结果 |
|---|---|
| `npm run verify` | ✓ 291 Rust 测试全绿 + svelte-check 0/0 + 依赖图无环（107 模块/369 依赖）+ `gen:types:check` 52 类型一致 + beets 五场景 OK |
| `npx playwright test` | ✓ **23 passed / 25.8s** |

### 故意没做（留给后续）

- **CI 接入**：需要仓库真实托管地址（`SOURCE-OFFER.txt` 里的 `github.com/aiya-music/aiya-music` 待确认）与 Windows runner 成本 —— 归 P5。
- **`cargo clippy -D warnings` 首跑**：toolchain 未装 clippy，装组件是环境改动，等确认后做。
- **代码签名 / SOURCE-OFFER 地址**：用户确认两条都没着落，归 P5。
