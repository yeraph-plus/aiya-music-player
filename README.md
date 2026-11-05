# AIYA Music

Windows 桌面本地音乐播放器：Rust 原生音频引擎、自有曲库索引、可选 [beets](https://beets.io) 集成。
以 GPL-3.0-or-later 开源。

| 正在播放 | 主界面 | 设置界面 |
|---|---|---|
| ![正在播放](docs/img/now-playing.png) | ![主界面](docs/img/library.png) | ![设置](docs/img/settings.png) |

## 功能

**曲库：** QLite 索引，目录扫描 5 万首曲库规模有查询性能基准覆盖（`npm run bench:50k`）自动增量刷新。

**播放：** 播放状态机等后端全部采用Rust实现，支持队列、随机、重复、播放位置恢复、均衡器、播放速度调节。

**音频引擎：** Symphonia 解码 + WASAPI 共享/独占输出支持，按源采样率开流，独占模式 bit-perfect。解码、DSP、设备枚举、播放时钟均独立进程单独实现，规避 Tauri2 前端阻塞。

**可视化：** 引擎链内频谱 tap：48 对数分带实时频谱使用拷贝流，界面关闭频谱时零开销。

**直播：** 支持HLS（m3u8）直播流电台播放。

**[beets](https://beets.io) 集成（可选）：** GUI 集成 beets 工具维护曲库，独立免安装套件包。

## 下载

从 [GitHub Releases](https://github.com/yeraph-plus/aiya-music-player/releases/latest) 获取：

| 文件 | 说明 |
|---|---|
| `AIYA-Music_<ver>_x64-setup.exe` | 安装包（NSIS，当前用户级，开始菜单 + 卸载登记） |
| `AIYA-Music_<ver>_x64-portable.zip` | 免安装便携版，解压即用，不写注册表 |
| `beets-portable-win-x64.zip` | beets 套件的免配置包，解压后在播放器目录内 |

此项目为自制软件，安装包与便携版未做代码签名，Windows 10/11 首次运行时 SmartScreen 会提示"未知发布者" ，从非本仓库 release 获得的可执行文件仓库作者无法控制。

## 从源码运行

开发环境：Windows 10/11、Node ≥ 24、Rust stable（MSVC 工具链）。

```sh
git clone https://github.com/yeraph-plus/aiya-music-player.git
cd aiya-music-player
npm --prefix ui install
npm run dev          # 开发运行（自动起 vite dev server）
```

全量验证（Rust 测试 + UI 类型检查与构建 + e2e + lint + 类型生成校验 + 文风门禁 + beets 契约冒烟）：

```sh
npm run verify
```

完整命令表、环境细节与注意事项见 [AGENTS.md](AGENTS.md) §2。

## 架构简述

项目本体为 Rust workspace（5 个库 crate + Tauri 宿主与音频 sidecar 两个二进制）
+ TypeScript 前端；beets 侧为独立 Python 包（aiya-beets）。

```
ui (Svelte 5) ── ipc/client.ts ──▶ src-tauri（Tauri 宿主）──┬─▶ aiya-data（SQLite）
                                                           ├─▶ aiya-player（状态机）──▶ aiya-audio
                                                           ├─▶ aiya-beets-store（beets 派生数据）
                                                           └─▶ aiya-audio ⇢ 音频 sidecar 进程
src-tauri ⇠ 命名管道行 JSON ⇢ aiya-audio-host
src-tauri ──▶ aiya-beets-client ⇠ stdio NDJSON ⇢ aiya-beets（可选）
```

核心边界：播放状态机唯一真源在 Rust；音频独立进程，命名管道只传命令与事件、
不传 PCM；IPC 契约以 Rust 为源生成 TypeScript；核心链路不依赖 beets 客户端。
全部约束见 [AGENTS.md](AGENTS.md) §3，crate 划分见 [crates/README.md](crates/README.md)。

此项目的主要依赖组件有：

| 层 | 组件 | 用途 |
|---|---|---|
| 桌面宿主 | [Tauri 2](https://v2.tauri.app/) | WebView 壳、窗口与进程、NSIS 打包 |
| 音频解码 | [Symphonia](https://github.com/pdeljanov/Symphonia) | mp3 / flac / ogg / wav / aiff / aac / alac / vorbis / pcm 解码 |
| 音频输出 | [wasapi](https://github.com/HEnquist/wasapi-rs) | WASAPI 共享/独占输出 |
| 数据 | [SQLite](https://sqlite.org/)（[rusqlite](https://github.com/rusqlite/rusqlite)） | 曲库索引、设置、封面缓存、beets 派生数据 |
| 标签解析 | [lofty](https://github.com/Serial-ATA/lofty-rs) | 扫描入库时读取音频标签 |
| IPC 契约 | [ts-rs](https://github.com/Aleph-Alpha/ts-rs) | 从 Rust 类型生成 TypeScript |
| 直播拉流 | [ureq](https://github.com/algesten/ureq) | HLS（m3u8）HTTP 拉流 |
| 进程管道 | [miow](https://github.com/Yamakaky/miow) | 命名管道（宿主 ⇄ 音频 sidecar） |
| UI 框架 | [Svelte 5](https://svelte.dev/) + [Vite](https://vite.dev/) + TypeScript | 界面与构建 |
| UI 样式 | [Tailwind CSS 4](https://tailwindcss.com/) | 样式 |
| UI 组件 | [bits-ui](https://bits-ui.com/)、[@lucide/svelte](https://lucide.dev/) | 基础交互组件与图标 |
| 可视化 | [three.js](https://threejs.org/) | 频谱等 WebGL 渲染 |
| 数据校验 | [zod](https://zod.dev/) | 前端侧数据校验 |
| e2e 测试 | [Playwright](https://playwright.dev/) | mock 后端 + 系统 Edge |
| beets | [beets](https://beets.io/) | 导入、标签、整理、查重、维护（beets独立套件包） |
| beets音频特征支持 | [Essentia](https://essentia.upf.edu/) + SVM 模型 | beets 索引增强的特征提取 |
| Python 运行时 | [CPython embeddable](https://www.python.org/) | beets 套件包内置解释器 |

## Web 版（Docker）

同一套 Svelte UI + Rust 服务端单二进制（`aiya-server`）的容器部署形态：
播放经浏览器（HTTP Range / `<audio>`），解码不在服务端；扫描、歌单、播放
队列、记账与事件流（SSE）全量可用，beets 集成暂缺（Linux 套件包规划中）。

```sh
docker compose up -d --build
# 浏览器打开 http://<host>:8787
```

- 曲库：宿主目录只读挂载到 `/music`（compose 里改 `/path/to/music`）；
- 数据：`aiya-data` 命名卷（SQLite / 封面缓存），升级容器不丢数据；
- 登录门（可选，单管理员）：设置 `AIYA_ADMIN_PASSWORD` 后浏览器进闸登录
  （静态页面公开、命令与媒体面凭据放行，会话 7 天滑动过期）；`AIYA_TOKEN`
  作为脚本/curl 的第二凭据并行可用；TLS/反代交给部署方；
- 无 beets 时曲库/播放/歌单完整可用（编译期边界保证），设置页的 Beets 入口
  显示未安装三态。

## License

[GPL-3.0-or-later](LICENSE)。随包分发的第三方许可材料见 `licenses/`
（`THIRD-PARTY-NOTICES.md`、`CARGO-DEPS.md`，后者由 `npm run notices` 生成）。
