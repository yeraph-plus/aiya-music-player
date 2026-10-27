# 收尾台账（2026-10-27 盘点）

本文是**唯一一份"还剩什么"的总表**。它不解释方案，只登记状态与判据——
每条的为什么写在出处那篇里。

盘点方法（可复现，不是凭印象）：

```bash
# 代码里有没有藏着未完成标记
grep -rn "TODO\|FIXME\|unimplemented!" --include="*.rs" --include="*.ts" \
  --include="*.svelte" src-tauri/src crates ui/src        # → 0 处
# 各文档的遗留章节
grep -n "^#\+.*\(未完成\|遗留\|剩余\|仍欠\|后续\|待办\)" docs/*.md
# 发布材料的真实状态（下面 P0 各项都是这样核出来的）
grep -n "signingIdentity\|csp" src-tauri/tauri.conf.json
head -20 licenses/SOURCE-OFFER.txt
ls .github/workflows                                      # 2026-10-27 之前不存在
cargo clippy --version                                   # → 组件未安装
```

**读法**：`[ ]` 未做 / `[x]` 已做。每条末尾的「出处」指向写下这条理由的文档，
判断"要不要现在做"时先读那一节，别只看这一行。

---

## P0 发布阻断（不解决不能发版）

- [ ] **P0-1 代码签名**：host 与 audio-host 两个 exe 都没签，`tauri.conf.json`
      无 `signingIdentity` / `certificateThumbprint`。
      **判据**：`pnpm tauri build` 产物用 `signtool verify /pa` 通过，两个 exe 都签；
      SmartScreen 首次运行不再拦。
      *出处：docs/13 §7；docs/30 §9「发布」行。*

- [x] **P0-2 SOURCE-OFFER 源码地址核实**（2026-10-27 完成）：仓库定为
      `https://github.com/yeraph-plus/aiya-music-player`，三处同步改掉：
      `licenses/SOURCE-OFFER.txt`、`licenses/THIRD-PARTY-NOTICES.md`（§三 源码获取方式）、
      `README.md`（进度表下的剩余链接）。
      **顺带修的一处**：`src-tauri/src/beets.rs` 的 `RELEASE_BASE` 原来指向
      `github.com/aiya-music/aiya-beets/…`——那是个**不存在的独立 beets 仓库**。
      改成同仓 `…/aiya-music-player/releases/latest/download`，与 release.yml
      「beets 资产挂在同一个 release 上」的设计一致。
      **仍待验**：地址要等仓库公开后能打开才算真成立（现在的判据是"文件已指向真仓"，
      不是"链接已可访问"）。首次 push 后点一次即可销项。
      *出处：docs/30 §9；docs/33 §8.2（分包后此文件已改过一次文案）。*

- [ ] **P0-3 CSP 与 capabilities 收窄**：`src-tauri/tauri.conf.json` 里 `"csp": null`；
      `capabilities/default.json` 只有 `core:default` + dialog + 5 条窗口控制。
      **判据**：设上 `default-src 'self'` 一类的最小策略，`npm run build` + e2e 全绿
      （重点是 `aiya-artwork://` 与 `ipc://` 不被拦）。
      *出处：docs/30 §7 第 4 条；docs/13 §7。*

- [ ] **P0-4 干净机「安装 → 运行 → 卸载」闭环**：没在从未装过本程序的机器上走过一遍。
      **判据**：虚机或备用机：装 → 首次启动建库 → 播放一首 → 卸载 → 残留检查
      （`%APPDATA%\dev.aiya.music`、`%LOCALAPPDATA%`、注册表 uninstall 键）。
      *出处：docs/13 §7。*

- [ ] **P0-5 4h soak**：长播 / 万文件扫描 / 100 张专辑导入 / 设备切换四项都没跑。
      **判据**：连续 4 小时无 panic、无句柄数单调增长、内存不爬坡；
      日志里没有连续重复的 error。
      *出处：docs/13 §7。*

- [ ] **P0-6 杀软误报观察**：未签名 + 无声誉，Defender / 火绒对大二进制（尤其
      `aiya-audio-host.exe`）的处置未观察。
      **判据**：装 P0-1 之后在两台干净机各跑一次首次安装与首启，
      有拦截就记下触发规则。（**依赖 P0-1，不能并行**）
      *出处：docs/13 §7。*

---

## P1 beets 分发链（功能不可用，但核心播放不受影响）

- [ ] **P1-1 真 release 地址**：`RELEASE_BASE` 已改指向
      `https://github.com/yeraph-plus/aiya-music-player/releases/latest/download`（2026-10-27），
      但 beets 侧**从没打过 `beets-v*` tag**，`latest/download` 现在还下不到东西。
      **判据**：`beets-tool/` 打出 `.pyz` 与 `bundle.zip` → 传 release → 干净机点安装
      → `status.found == true`。地址可用 `AIYA_BEETS_RELEASE_BASE` 指过去做本地演练。
      *出处：docs/33 §M4「仍欠的」。*

- [x] **P1-2 `beets.releaseBase` 设置键没有 UI 入口**（2026-10-27 盘点发现，同日解决）：
      `release_base()` 读了这个键，但设置页、schema、mock 里都没有它——写了没人能设。
      **选了"删分支"而不是"补 UI"**：镜像源现在不存在，为一个不存在的后端加一个
      要用户手填 URL 的输入框，是把死代码换成半成品。现在只认
      `AIYA_BEETS_RELEASE_BASE`（CI 与本地演练都用它），注释与 README 都写清了
      "要支持镜像得先给入口再留分支"。
      顺带把 `fetch_asset` 的 `app` 参数删了（它只为读那个键而存在），
      一个签名里少一个永远是摆设的参数。
      *出处：本次盘点；`src-tauri/src/beets.rs::release_base`。*

- [ ] **P1-3 两条安装轨道在"从没装过 beets 的机器"走完整流程**：A 轨道（`.pyz` → 系统
      Python 建 venv）与 B 轨道（28 MB zip → 展开 embeddable）都只在单测与本机验过，
      本机恰好有 Python。
      **判据**：一台无 Python 的机器走 B 轨道成功；一台有 Python 的走 A 轨道成功；
      两条都要覆盖"下载中断"与"取消"。
      *出处：docs/33 §M4「仍欠的」；docs/31 §3 测试分层。*

- [ ] **P1-4 `status.schemaHash` 运行时比对**：宿主只是把工具自报的 hash 显示出来，
      没有拿它跟 `protocol/aiya-beets-v1.json` 的 sha256 比。
      **判据**：接上比对；**现在只有一个版本在世，接了也验不出来，等协议升 v2 一起做**。
      *出处：docs/33 §M4「仍欠的」。*

---

## P1 任务可见性（一块独立工程，不是补丁）

- [ ] **P1-5 任务登记表**：五条任务线（扫描 / 导入 / 维护 / 安装 / 刷新）现在各发各的事件，
      于是三件事做不到：
      | 做不到 | 现象 |
      |---|---|
      | 互斥原因可见 | 扫描进行中点导入，按钮灰着，但不说为什么、也不知道排没排队 |
      | 安装可取消 | `beets_install` 一跑到底，用户只能等 |
      | 维护失败不丢 | 维护失败的错误会被紧随其后的刷新事件盖掉 |
      **判据**：宿主维护 `jobId + kind + cancellable`，事件统一带 `jobId`；
      前端 `stores/tasks.svelte.ts` 投影成 `TaskView[]`，**只在一处渲染**。
      **注意**：`beets:import` 事件是从 beets-tool 的 NDJSON 转发来的，
      加字段时别改协议 v1 的字段名（会触发 `PROTOCOL_VERSION` 变更与 `proto:sync`）——
      宿主侧另起信封，不要污染线协议。
      *出处：docs/34 §6；docs/33 §7（让位规则）。*

---

## P2 功能欠账（都记过"本轮不做"，别当成遗漏）

- [ ] **P2-1 `modify_items`（手改标签）**：需要字段编辑器那套交互，可复用
      `update_library` 的 diff 结构反向（把"目标值"写成用户输入）。
      *出处：docs/29 §16.6。*

- [ ] **P2-2 查重只有报告，没有删除**：应用内不删文件是刻意的。若以后要加，
      必须走回收站且逐项确认（见 personal_files_safety 那套规矩）。
      *出处：docs/29 §16.6。*

- [ ] **P2-3 adopt 是单向的**：beets 有、曲库没有的条目，只有 `sync_items` 的
      `imported` 计数，没有"要不要纳入曲库"的 UI 流程。
      *出处：docs/29 §16.6。*

- [ ] **P2-4 首发档两个插件"看得见、用不了"**：replaygain 缺 `ffmpeg`、lyrics 缺 `bs4`。
      能力三态会如实报 `missing_deps`（这是 M2 的验收点，已做到），但能力本身没补齐。
      **判据**：二选一——补依赖，或从首发清单摘掉（摘掉比"报了缺依赖还留着"诚实）。
      *出处：docs/31 §10 第 1、2 问（当年挂起未定）。*

- [ ] **P2-5 Mp4 容器 ALAC vs AAC 判不出**（返回 `null`）：刻意的保守——
      容器下两种编码都有，猜错比显示"未知"更糟。要收窄得读 `alac` atom。
      *状态：**设计决定保留**，只在想收窄时做。*
      *出处：docs/18 §9 第 1 条。*

- [ ] **P2-6 猜你喜欢**（docs/28）：基于 top-15 做关联推荐。用户明说本批不做。
      *出处：docs/28 §5。*

- [ ] **P2-7 docs/29 批 E 观察档**：chroma / spotify 可选组件检测；
      xtractor / vibenet 待"猜你喜欢"定特征需求后再评估。
      *出处：docs/29 §10 第 5 条、§14 插件矩阵。*

- [ ] **P2-8 自动更新**：计划里本就后置到 v2。
      *出处：docs/13 §7。*

---

## P2 质量与覆盖

- [x] **P2-9 CI 已建并已实跑**（2026-10-27）：`.github/workflows/ci.yml` 两个 job——
      `verify`（cargo test + svelte-check + check:deps + check:boundary + proto:check +
      vite build + gen:types:check + e2e）与 `beets`（建 venv → 三个 smoke）。
      用 `windows-latest`：本体只在 Windows 发布，e2e 又用系统 Edge
      （`channel: 'msedge'`），Linux 上没有可比环境。

      **首次实跑（run 40451976473）两个 job 全红**，红的都是同一类问题——
      **"本地有、干净检出没有"的隐式假设**，不是逻辑错误：

      | job | 断在哪 | 根因 | 修法 |
      |---|---|---|---|
      | verify | 编译 `src-tauri` 时 build script 失败 | `bundle.resources` 要 `binaries/aiya-audio-host-*.exe`，那是构建产物（已 ignore），CI 从没构建过 | `npm run build:sidecar` 放进 `test:all` 第一步；`build.rs` 先自检并直接印出解法 |
      | beets | `smoke_apply.py` 第 21 行 | 三个 smoke 把音源写死成开发机的 `D:\MUSIC_Player\test-music` | `beets-tool/tests/_testmusic.py`：真音源优先，没有就现场合成 3 个 1s WAV |

      失败原文（本地复现，删掉 `src-tauri/binaries/` + `touch build.rs`）：
      `resource path 'binaries\aiya-audio-host-x86_64-pc-windows-msvc.exe' doesn't exist`

      顺带修的同源问题：`crates/aiya-audio/examples/gen_music.rs` 把输出目录也写死成
      同一个绝对路径（改成 `argv[1]`，默认 `./test-music`）；`.gitignore` 加 `/test-music/`。

      **仍待验**：修完要再推一次、看两个 job 变绿才算真销项。
      *出处：docs/30 §9「故意没做」。*

- [ ] **P2-10 `cargo clippy -D warnings` 从未跑过**：clippy 组件没装
      （`rustup component add clippy`）。首跑大概率有几十条，其中会不会有真 bug 未知。
      *出处：docs/30 §9「故意没做」。*

- [ ] **P2-11 mock 里 `beets.found` 恒为 `true`**：所以"未装 beets 时整理库段收成一行"
      只有肉眼验过，没有测试。
      **判据**：mock 加一个 `found: false` 的场景开关 + 一条 e2e。
      *出处：docs/34 §6。*

- [ ] **P2-12 联网契约场景平时不跑**：`protocol_contract.py` 里 album / albumError / done
      三类事件要 `AIYA_BEETS_ONLINE=1`，默认跳过（要联网匹配）。
      *出处：beets-tool/tests/protocol_contract.py §5；docs/33 §4.4。*

---

## 文档

- [x] **P2-13 README「进度与剩余」表过时**（2026-10-27 完成）：原来仍把"批 B / 批 C / 批 D"
      列为待办。实际批 B（rayon 两段式）与批 D（命令面）已完成，批 C（venv 预构建分发）
      **被 docs/33 整体取代**。现在表只留摘要，指向本台账，不再逐条维护两份。
      *出处：`README.md` 进度与剩余一节。*

- [ ] **P2-14 真机走查**（只有人能看，测不了）：
      docs/23 §7 三条——快捷键 16 动作逐条点、主题×密度目视（紧凑档 42px 行高在 9 列下）、
      960px 最小宽下的队列抽屉；
      docs/28 §5 四条——专辑/艺人详情页参数直达、自跳转失效、推荐页常听 15 首、
      图标工具条 hover。
      *出处：docs/23 §7；docs/28 §5。*

---

## 建议顺序（三条路线，按目标选一条）

**A. 想尽快有一个能发给别人装的版本**（最短路径，约 1 天 + 1 台干净机）
`P0-3`（CSP）→ `P0-1`（签名）→ `P0-4`（干净机）→ `P0-6`（杀软，紧随签名）

> `P0-2`（源码要约地址）、`P1-2`（死分支）与 `P2-13`（README 表）已在仓库托管
> 那一轮做掉；`P0-5` soak 可以并行跑，它不挡构建，只挡"敢不敢说不崩"。

**B. 想把 beets 能力真正交付出去**（约 2–3 天，需要一台无 Python 的机器）
`P1-1`（真发布）→ `P1-3`（两条轨道）→ `P2-4`（插件依赖，同时决定"摘还是补"）→
`P1-4`（等 v2 协议，可延）

**C. 想砍掉"看不见的任务"这类长期摩擦**
`P1-5`（任务登记表）→ `P2-11`（补 mock 场景，它正是这张表最容易漏的测试）

**D. 想让后来接手的人少踩坑**
`P2-9`（CI）→ `P2-10`（clippy 首跑）→ `P2-13` + `P2-14`

---

## 已完成：仓库托管准备（2026-10-27）

用户决定把源码放 GitHub 维护，顺带做了一轮仓库卫生与发布链路的检查。**检查结果比预想
干净**，实测数据如下：

| 项 | 实测 |
|---|---|
| `.git` 体积 | 4.6 MB |
| 跟踪文件数 | 421 |
| 历史最大对象 | `Cargo.lock` 147 KB（无大文件，不需要 LFS） |
| 密钥 / 本地绝对路径 | 0 处（`C:\Users\`、`D:\MUSIC_Player` 都查过） |
| 提交身份 | `AIYA Music <dev@aiya.local>`（不是真实邮箱） |
| 已忽略 | `target/`、`node_modules/`、`.venv/`、`beets-tool/.runtime/`、`dist/`、`binaries/`、`*.sqlite`、`*.log` |
| 大小写冲突路径 | 0 处（Windows 上开发、Linux 上拉取不会撞名） |

修掉的问题：

1. **`.gitignore` 里 `beets-tool/.runtime/` 重复两行**（连注释一起重复）——已去重。
2. **`src-tauri/Cargo.lock` 是死文件**：`src-tauri` 是 workspace member，cargo 只认根
   `Cargo.lock`。它停在 10-23，比根少 4 万字节，纯误导。已 `git rm --cached` + 删除 +
   加进 ignore（附原因注释）；删后 `npm run verify` 仍 exit 0，证明 workspace 确实只用根 lock。
3. **Python 常规产物缺失**：补 `.pytest_cache/`、`*.egg-info/`、`.mypy_cache/`、`.ruff_cache/`。
4. **许可证声明自相矛盾**：`beets-tool/pyproject.toml` 写 MIT，根 README 与
   `SOURCE-OFFER.txt` 说 GPL-3.0-or-later。查了上游——beets 2.14.1 本身是 MIT，
   所以 MIT 在法律上站得住，不是必须 GPL。**用户拍板统一为 GPL-3.0-or-later**。
5. **两个 beets 发布物里都没有许可副本**（选 GPL 后这是硬要求）。
   - `beets-tool/LICENSE` 新增（与根 `LICENSE` 同一份全文）
   - `build_installer.py`：`aiya_beets/data/LICENSE`；顺带把打包改成**临时 staging 目录**，
     不再往源码树写文件（以前每次打包都会在 git 里留一个未跟踪文件）
   - `build_bundle.py`：套件包 zip 根 `LICENSE`
   - 重建验证：`.pyz` 33 KB → **49 KB**；套件包 28.1 MB，zip 内 `LICENSE` 与
     `Lib/site-packages/aiya_beets/data/LICENSE` 都在
6. **CI 与发布**：`.github/workflows/ci.yml`（verify + beets 两个 job）、
   `.github/workflows/release.yml`。

### release 工作流的一个语义陷阱（写进 YAML 注释了）

`beets.rs` 的下载地址是 `…/releases/latest/download/<asset>`，而 `latest` 指"最近创建的
release"。若 beets 单独发过一版（`beets-v*`），latest 就指向它，播放器去那里下别的东西会
404。所以 **`v*` 的 release 里同时挂 beets 两个文件**，让 `latest/download` 永远有效。
资产名 `aiya-beets-installer.pyz` / `aiya-beets-bundle-win-x64.zip` 是 `beets.rs` 里的常量，
workflow 里有一步硬校验名字。

### 刻意没做的

- **签名步骤没有写进 release.yml**。没有证书，写一段"导入证书但不签"的脚本比没有更糟
  ——会让人以为签过了。见 P0-1。
- **不在 CI 里跑 `AIYA_BEETS_ONLINE=1`**：联网匹配受 MusicBrainz 限流影响，
  绿灯会变成随机红灯。

---

## 附：本文没列的东西，以及为什么

- **`src-tauri/src/lib.rs` 3127 行 / 68 条命令** —— 这是真实的结构债，但它是
  **重构**不是**收尾**，不进本表；结论见 docs/36 的结构评估。
- **`PowerInfo` / `StreamsInfo` 仍在 DTO 里**：UI 不再显示但后端还在采集，
  日志与将来的自检页要用，留着是对的。*（docs/34 §6）*
- **`beets_item_id` 这个名字**：只有一个 provider，改名成本 > 收益，拍板保留。
  *（docs/33 §10.3 第 6 条）*
