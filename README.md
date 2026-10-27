# AIYA Music — Tauri 2 本地音乐播放器

> 状态：**功能完成，进入收尾**（P0–P4 + 前端 docs/24 B0–B6 + docs/25–29 全部落地；
> 剩 docs/29 批 B/C/D 与 P5 发布项）。
> 上一代 Electron demo 在 `../AIYA_MUSIC`（tag `demo-final`），已归档，只作 UI/行为参考。
> **无历史兼容**：不迁移旧库、不兼容 Electron 时代的 `media://`、不保留 Electron IPC。

## 六条已定决策（不可回退）

1. **后端拥有音频输出**：共享/独占 WASAPI、解码、设备、时钟都在音频 sidecar
   （原生：Symphonia 解码 + wasapi-rs 输出，ADR-0006）；Rust host 编排；前端只显示状态。
2. **播放状态机在 Rust**：队列、随机、重复、恢复只有一份真相；系统媒体键/托盘/关窗都控制同一个 Actor。
3. **音频独立进程 + PCM 链内 tap**：`aiya-audio-host` sidecar 承载原生引擎；DSP 链内建订阅制
   "必要采样" tap（动画级频谱，关闭零开销、不破坏 bit-perfect）。
4. **契约以 Rust 为源**：`#[derive(TS)]` 生成到 `ui/src/lib/ipc/generated/`；UI 只走
   `lib/ipc/client.ts` 一层，参数侧的复合对象刻意收 `Json` 手动解析（否则参数不合法会绕开 `ApiError` 分类）。
5. **单一数据库写入者**：只有宿主开 SQLite；音频/beets sidecar 不碰宿主库。
6. **全新 schema v1**：没有历史迁移链，直接从零建库。

## 仓库结构（实际）

```
AIYA_MUSIC_TAURI/
├── docs/                  # 决策、审查、分批计划；最新在编号最大处
├── crates/
│   ├── aiya-player/       # 纯领域：队列、状态归约、命令/事件；不依赖 tauri/rusqlite/windows
│   ├── aiya-data/         # SQLite v1、Repository、扫描、封面、beets 读模型；不依赖 tauri
│   ├── aiya-audio/        # 原生引擎：Symphonia 解码 + wasapi-rs 输出 + DSP/频谱/EQ
│   ├── aiya-audio-host/   # 音频 sidecar 进程入口（命名管道行 JSON，不传 PCM）
│   └── aiya-beets-client/ # beets 协议客户端：发现/起进程/说协议；零 Tauri、零环境知识
├── beets-tool/            # beets 侧全部能力（独立发布单元，Python 包 aiya-beets）
├── src-tauri/             # Tauri 宿主：命令、事件桥、协议注册、窗口、日志
├── ui/                    # Svelte 5 + Vite：10 views / 16 stores / 组件分 content·shell·ui
└── tools/                 # 类型生成、依赖图、CDP 探针、notice 生成、打包辅助
```

依赖方向（实际，与上面 crate 一一对应）：

```
ui ── lib/ipc/client.ts ──▶ src-tauri(aiya) ──┬─▶ aiya-data
                                             ├─▶ aiya-player ──▶ aiya-audio
                                             └─▶ aiya-audio      ⇠ 同一 crate 也被 sidecar 用
src-tauri ⇠ 命名管道行 JSON ⇢ aiya-audio-host
src-tauri ──▶ aiya-beets-client ⇠ stdio NDJSON ⇢ aiya-beets（独立安装，可能根本没装）
```

`aiya-player` 不依赖 `tauri`、`tokio`、`rusqlite`、`windows`；`aiya-data` 不依赖 `tauri`、音频平台。
违反这条，三个月后会退化回上一代的结构。

**beets 是增强能力，不是播放依赖**（docs/33）：`aiya-audio` / `aiya-audio-host` /
`aiya-player` / `aiya-data` 连 `aiya-beets-client` 都不许依赖，所以"没装 beets
也能完整播放"是编译期事实。这条由 `npm run check:boundary` 守。

**两侧只通过协议耦合，协议 v1 已冻结**：真源在 `beets-tool/protocol/v1.schema.json`，
本仓镜像 `protocol/aiya-beets-v1.json`（字节副本 + sha256）。三条测试各守一面：
`smoke:proto`（工具自己吐的每一行都合 schema）、`cargo test -p aiya-beets-client`
（serde 形状对得上镜像）、`proto:check`（镜像没被单方面改过）。

## 构建 / 测试 / 打包

常用命令都收进根 `package.json`，在**仓库根**执行 `npm run <script>`。Rust 相关脚本统一经
`tools/run.mjs` 转发（把 `%USERPROFILE%\.cargo\bin` 前置进 PATH）。

| 命令 | 作用 |
|---|---|
| `npm run dev` | 开发：`tauri dev`（自动起 vite dev server） |
| `npm run dev:ui` | 只起前端（浏览器里跑 mock 数据，不开 Tauri） |
| `npm run build` / `bundle` | 出安装包：`vite build` → 编 sidecar → release 编译 → NSIS（**不含 beets**） |
| `npm run beets:installer` / `beets:bundle` | 出 beets 的两个发布物：`.pyz` 安装器（33KB）/ 免安装套件包（31MB） |
| `npm run beets:release` | 两个发布物一起出 |
| `npm run build:ui` | 只构建前端到 `ui/dist` |
| `npm run check` | `svelte-check`（当前 0 error / 0 warning 才算过） |
| `npm run test` | Rust 全量测试（当前 **320 条**，1 条 `--ignored` 是 perf_50k） |
| `npm run test:data` / `test:audio` / `test:player` / `test:app` | 单 crate 测试 |
| `npm run test:all` | Rust 全量 + UI 类型检查 + UI 构建 |
| `npm run gen:types` / `gen:types:check` | 从 Rust 生成 TS 类型 / 只校验不写盘 |
| `npm run verify` | `test:all` + `gen:types:check` + beets 导入/维护冒烟 |
| `npm run test:e2e` | Playwright（`ui/` 下跑；mock 后端 + 系统 Edge，24 条） |
| `npm run smoke:engine` / `smoke:offset` / `smoke:switch` / `smoke:rate` | 真机 WASAPI 出声冒烟（需要声卡） |
| `npm run smoke:supervisor` | sidecar 握手/心跳/看门狗冒烟（不需要声卡） |
| `npm run smoke:beets` | beets apply 五场景（隔离 `BEETSDIR`，不动真实库；跑在 `beets-tool/tests/`） |
| `npm run smoke:maint` | beets 曲库维护四件套（adopt/update/dup/move，隔离库，含冲突与库根校验） |
| `npm run bench:50k` | 5 万首查询性能基准（release，`--ignored`） |
| `npm run notices` | 刷新 `licenses/CARGO-DEPS.md`（依赖变更后跑） |
| `npm run check:deps` | store 依赖方向/环检测 |
| `npm run check:boundary` | 边界门禁：client 无环境知识、核心链路不依赖 beets 工具 |
| `npm run proto:check` | 协议门禁：镜像 schema 与工具侧真源字节一致（`proto:sync` 才改镜像） |
| `npm run probe:ui` | CDP 探针（对真实窗口做 UI 断言） |

打包前置两件事：`vite build` 出前端产物、`tools/build-sidecar.mjs` 编出
`aiya-audio-host` 到 `src-tauri/binaries/<name>-<triple>.exe`（Tauri `externalBin`
约定路径）。产物：`target/release/bundle/nsis/*.exe`（当前 **6.53 MiB**，release
编译约 5.5 分钟）。

**安装包不含 beets**（docs/33 两侧分离）：以前随包塞了一份 104MB 的 beets 运行时，
现在播放器安装包只有播放器。用户要导入整理时再装 beets 工具，两条轨道：

| 轨道 | 下载 | 前提 | 落地 |
|---|---|---|---|
| 快速安装（`.pyz`） | 33 KB | 机器上有 Python 3.10–3.14 | 用系统 Python 在那头建 venv |
| 免安装包（`.zip`） | 28 MB | 无 | 自带 embeddable 解释器，解压即用（展开 72 MB） |

播放器对两者做同一件事：下载 → 展开 → 记入口 → 跑一次 `status` 验证。**成功判据是
探测到的 `found`，不是退出码**——退出码会骗人。

**为什么套件包能随便搬**：embeddable 解释器把「解释器 + stdlib + DLL」都放在包内，
`sys.prefix` 由 exe 位置推出，整目录复制到任意路径都能跑（实测复制到 `%TEMP%` 后
`sys.prefix` 跟着走）。这和 venv 相反——venv 的 `pyvenv.cfg` 里 `home` 是硬指针，
换个路径就 `did not find executable`。

> `beforeBuildCommand` 的 cwd 是**调用 tauri CLI 时的目录**，所以别在 `ui/` 里跑 `tauri build`。

## 现在不做什么

- 不迁移旧数据库、不兼容 Electron `media://`、不保留 Electron preload/IPC。
- 不在前端建第二条音频路径（Web Audio 不回迁；频谱只能来自引擎链内 tap）。
- 不做 SSR/路由库/URL 状态、不做主题编辑器、不做移动端适配（只保证 960px 以上不破）。
- 不用 beets 查询语法替代内置索引（docs/29 拍板 5）：beets 是写权威，自有表是读模型。

## License

以 **GPL-3.0-or-later** 发布，全文见 [`LICENSE`](LICENSE)。

随包分发的许可材料（安装包内 `licenses/`）：

| 文件 | 内容 |
|---|---|
| `licenses/THIRD-PARTY-NOTICES.md` | 随包分发/链接的第三方组件与许可；**只列真在包里的**（v1 无 mpv/FFmpeg） |
| `licenses/CARGO-DEPS.md` | Rust 依赖名称/版本/许可（`npm run notices` 生成） |
| `licenses/SOURCE-OFFER.txt` | GPL-3.0 §6(d) 源码获取要约 |

## 进度与剩余

**已完成**：P0 宿主 + P1 数据只读 + P2 播放纵切（含 sidecar 进程化）+ P3 beets 预览/应用 +
P4 体验/系统 + 前端 B0–B6（UI 重做）+ T 系列视觉批 + docs/27 列表工具条 + docs/28 导航拆页 +
docs/29 批 A（扫描入库页 + 多目录）。

**剩余**（详细证据与方案见 [`30-pre-handover-audit-2026-10-27.md`](docs/30-pre-handover-audit-2026-10-27.md)）：

| 类别 | 待办 |
|---|---|
| 功能 | docs/29 批 B（扫描 worker 拆分 + rayon 两段式进度）、批 C（beets venv 预构建分发）、批 D（beets 命令面扩展） |
| 发布 | 代码签名（host 与 sidecar 两个 exe）、SOURCE-OFFER 源码地址核实、安装→运行→卸载在干净机复验、4h soak、杀软误报 |
| 加固 | CSP 与 capabilities 收窄（UI 已定稿，可单独一批做）、`cargo clippy -D warnings` 首次全量（toolchain 未装 clippy） |
| 走查 | 真机三项（docs/23 §7）：快捷键 16 动作逐条、主题×密度目视、960px 下队列抽屉 |

自动更新按计划后置到 v2。
