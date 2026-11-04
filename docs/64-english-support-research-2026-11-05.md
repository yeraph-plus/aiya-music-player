# 64 · 英语支持调研 v2（2026-11-05）

> 应用需要加英语支持（初版规划的漏项）。本档为调研 + 拍板记录：文本面统计、
> 机制选型、分片计划。边界四条已拍板（见 §5）：
> ① 语言默认按系统提取（首启写库，之后恒按设置值）；② plugins.json 按语言
> 分支文件（zh_Hans 变体 + 英文基名，找不到回退基名，仅 Rust 侧需要改）；
> ③ 支持面只有发布产物；④ 后端先行，前端殿后，子代理并发 3 个一批。

## 1 · 文本面统计（字符串字面量口径，已剔除注释与日志宏）

| 层 | 文件数 | 字符串约数 | 头部文件（处） |
|---|---|---|---|
| UI（svelte/ts） | 70 | **~2,750** | SettingsPanel 382、LibraryView 152、PlayerBar 150、OverlayHost 145、LibraryTools 137、OrganizePanel 130、IndexPanel 126、LiveView 110、InstallPanel 102 |
| Rust 宿主（src-tauri） | 13 | **~750** | beets.rs 354、lib.rs 199、pump.rs 61、sidecar.rs 32、enrichment 25、smtc 19 |
| Rust crates | 44 | **~1,590** | scanner 131、actor 129、hls 115、engine 84、beets-client session 77、repo 69、ts.rs 56 |
| beets-tool（py） | 23 | **~890** | cfg 124、bins 100、enrich 78、cli 65、plugins 54、uninstall 52 |
| **合计** | 150 | **~5,980** | |

口径说明：已排除注释（项目文风注释保持中文，**不在迁移范围**）与 Rust
tracing 日志宏（内部诊断，保持中文）。mock/backend.ts（177）是浏览器预览
假数据、aiya-audio/examples（118）是开发示例——**支持面只有发布产物**，
两者均不在范围。净执行量 ≈ **5,300 处**。

## 2 · 范围界定

| 类别 | 处理 | 说明 |
|---|---|---|
| UI 字符串（svelte 标记/ts 字面量） | **双语表 + t() 调用** | 唯一需要 i18n 机制的一层 |
| format.ts 时长/次数/日期（"3 分 5 秒""x 次"） | 双语表 + 条件单位 | 格式化函数，单位与语序随语言 |
| HomeView 时段问候 | 双语表 | "夜深了/早上好…"整句换 |
| Rust 报错（ApiError/BeetsError/ScanError/AudioError/BinError、dialog、record_background） | **硬迁移英文** | 中文界面下原样显示英文（已拍板） |
| Rust tracing 日志 | **不动** | 内部诊断，非用户面 |
| 插件表 plugins.json 的展示字段（why/vendor） | **双语分支文件** | 见 §3.3；下载/校验字段两边必须逐字节一致（契约守） |
| beets-tool 协议消息（install/维护/错误） | **硬迁移英文** | 经 beets 桥直达 UI 错误条，同报错口径；py 侧纯 CLI，不做语言分支 |
| mock/backend.ts 假数据、examples | **不进范围** | 支持面只有发布产物（已拍板） |
| e2e 断言（中文文案） | 不动 | 首启后语言恒有值，e2e 默认值由首启写库决定——CI runner 是英文系统 → e2e 会跑 **en**！见 §3.4 |

## 3 · 机制选型

### 3.1 语言值与首启提取（已拍板①）

- `AppSettings.language: string`，**初始为空串**（DEFAULT_SETTINGS 里
  `"language": ""`，schemaless 透传零迁移）。
- **首次启动**：宿主启动流程里读 `language` 键——空 → 用
  `GetUserDefaultUILanguage`（windows-sys `Win32_Globalization` feature，
  已有依赖）取系统 UI 语言，主语言 `zh` 开头 → `"zh"`，其余 → `"en"`；
  `setting_set` 写库。**此后恒按设置值**（用户在设置页改，宿主不再介入）。
- 前端不自行探测：boot 读到 settings 时语言必有值，`<html data-lang>`
  与 i18n locale 都从设置走。

### 3.2 i18n 词典（前端）

- `ui/src/lib/i18n/`：`zh.ts` / `en.ts` 平面键值表（key 按文件域命名，
  如 `settings.theme.title`），`i18n.ts` 出 `t(key, params?)` 与响应式
  locale；en 表 `satisfies Record<keyof zh, string>`——缺键编译期报错。
- **装饰字取消**：纯 CSS——`[data-lang='en'] .page-kicker { display: none }`，
  零组件改动。
- 字体：现栈已含 Segoe UI 回落，英文无需改字体。
- **分表策略**：每个分片写自己的表段（`i18n/domains/<分片>.ts`），集成代理
  合并——子代理之间零文件冲突。

### 3.3 插件表双语分支（已拍板②）

- 现状：`beets.rs:2176` `include_str!(".../data/plugins.json")` **编译期
  内嵌**，py 侧 `plugins.py` 读同一份字节（五个消费方）。
- 分支：`plugins.json` 改为**英文基名**（why/vendor 重写英文）；
  新增 `plugins-zh_Hans.json` = 中文变体。Rust 侧 `include_str!` 两份，
  `field_catalog` 按设置语言选择，**变体缺失回退基名**（match 默认分支）。
- py 侧不改（纯 CLI：读的是 name/url/sha256 等结构字段，展示文案与它无关）。
- **漂移防线**：catalog_contract 增结构奇偶校验——两份表的
  name/target/url/sha256/size/bundle 必须逐字段一致，只有 why/vendor 可异。
  两份表人工维护，没有这道闸必然漂。
- 后缀命名按拍板用 `zh_Hans`；设置值 `zh` → 文件后缀映射 `zh → zh_Hans`
  （未来 zh-Hant 加映射即可）。

### 3.4 e2e 与首启语言（v2 新增的坑）

CI runner 是**英文系统** → 首启写库得 `en` → e2e 全部断言中文文案会集体红。
两条解法：e2e 的 `addInitScript` 预置 `language: "zh"`（既有 live spec 已有
同款预置模式），或 AIYA_BEETS 式环境变量。**执行批选前者**（不动宿主）。

## 4 · 分片计划（子代理，并发 3 个一批；后端先行已拍板④）

### A 阶段 · 后端（全部硬迁移英文，无 i18n 机制）

| # | 分片 | 主要文件 | 字符串约数 |
|---|---|---|---|
| B1 | 宿主报错：lib.rs(199)、pump(61)、sidecar(32)、enrichment(25)、smtc(19) 等 | src-tauri/src | ~400 |
| B2 | beets.rs 报错 + **插件表双语分支**（en 基名重写 / zh_Hans 变体 / include_str 选择 / catalog_contract 奇偶校验） | beets.rs(354)、data/plugins*.json | ~420 |
| B3 | crates A：aiya-audio(hls 115/engine 84/decoder 45/ts 56)、aiya-player(actor 129) | crates | ~500 |
| B4 | crates B：aiya-data(scanner 131/repo 69/playlist 44/playlist_io 45/lyrics 45)、aiya-beets-client(77) | crates | ~450 |
| B5 | beets-tool：cfg(124)/bins(100)/enrich(78)/cli(65)/plugins(54)/uninstall(52)/protocol(45)/maintain(45)/其余 | beets-tool/src | ~890 |

**波次**：[B1+B2+B3] → 验证（cargo clippy/test + 契约 + e2e 仍绿）→ [B4+B5] → 验证。
B2 的插件表双语分支依赖 B5 之前的术语定稿——**B0 术语表先行**（见 C0），
后端波次开始前由集成者人工出表（不占子代理）。

### B 阶段 · 前端（A 阶段验证通过后）

| # | 分片 | 主要文件 | 字符串约数 |
|---|---|---|---|
| C0 | **基建**：i18n 模块、settings.language（空值/首启语义）、data-lang、kicker CSS、format.ts 双语化、HomeView 问候 | lib/i18n/、settings.ts、pages.css | ~60 |
| C1 | 壳层：AppShell、Sidebar、TitleBar(73)、PlayerBar(150)、notify/errors、通用组件（EmptyState/InlineError/OverlayHost 145） | components/shell、ui | ~550 |
| C2 | 设置页：SettingsPanel(382)（含新增语言切换行） | 单文件 | ~380 |
| C3 | 曲库簇：LibraryView(152)、LibraryTools(137)、TrackListPanel、TrackTable、BrowseView、AlbumView、ArtistView | views+components | ~500 |
| C4 | 直播+beets 簇：LiveView(110)、OrganizePanel(130)、IndexPanel(126)、InstallPanel(102) | views+components/beets | ~500 |
| C5 | 播放簇：HomeView、NowPlayingView、QueuePanel、LyricsPanel、频谱 UI | views+components | ~350 |

**波次**：C0 单独 → [C1+C2+C3] → [C4+C5] → 验证（svelte-check + 键位对齐 +
e2e 预置 zh）。

### C 阶段 · 集成

- 键位对齐校验脚本（zh/en 键集一致）进 verify；
- 全量 verify + en 模式截图走查（kicker 取消确认、设置页切换确认）；
- 真实安装冒烟：系统语言 zh 的机器首启 → 库里是 zh；改设置 en → 重启生效。

## 5 · 拍板记录

1. **默认语言**：空值初始化，首启按系统 UI 语言提取写库，之后恒按设置值。✅
2. **插件表**：`plugins.json` = 英文基名，`plugins-zh_Hans.json` = 中文变体，
   加载按语言追加后缀、找不到回退基名；仅 Rust 侧改（py 纯 CLI）。✅
3. **支持面**：只有发布产物（mock/examples 不进）。✅
4. **执行**：后端先行、验证通过再前端；子代理并发 3 个一批。✅
5. 另：后端/内部报错硬迁移英文（中文界面下原样显示英文）——上一轮已拍板。✅
