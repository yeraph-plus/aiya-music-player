# 第三方组件与许可声明

AIYA Music 本体以 **GPL-3.0-or-later** 发布（全文见仓库根 `LICENSE`）。
本文件列出安装包内实际分发/链接的第三方组件及其许可，以及不属于安装包、
但运行时会调用的外部程序。

> 原则：只声明**真的在包里**的东西。v1 用原生引擎（Symphonia + WASAPI），
> **不捆绑 mpv / FFmpeg**，因此这里没有它们的条目 —— 早期计划里写过，
> 那是 mpv 方案时期的遗留，别照抄。

## 一、随安装包分发的组件

### 1. 应用程序本体与 sidecar

| 组件 | 版本 | 许可 | 说明 |
|---|---|---|---|
| AIYA Music（`aiya.exe`） | 0.1.0 | GPL-3.0-or-later | 本程序 |
| `aiya-audio-host.exe` | 0.1.0 | GPL-3.0-or-later | 音频引擎 sidecar，与本程序同一源码树 |
| Tauri（运行时 + WebView2 引导） | 2.12.x | Apache-2.0 OR MIT | 应用壳与 IPC |

### 2. Rust 依赖（静态链接进上面两个 exe）

完整清单（registry crate 的名称/版本/许可，条目数以 `CARGO-DEPS.md` 头部为准）见同目录 `CARGO-DEPS.md`。
关键项：

| 组件 | 版本 | 许可 | 用途 |
|---|---|---|---|
| symphonia（含 core / codec-*） | 0.5.5 | **MPL-2.0** | FLAC/MP3/AAC/OGG/WAV 解码 |
| rusqlite + libsqlite3-sys（bundled SQLite） | 0.40.2 / 0.38.2 | MIT | 本地库 |
| lofty | 0.25.4 | MIT OR Apache-2.0 | 标签读写 |
| wasapi | 0.26.0 | MIT | WASAPI 独占/共享输出 |
| souvlaki | 0.8.3 | MIT | Windows SMTC / 媒体键 |
| image | 0.25.10 | MIT OR Apache-2.0 | 封面解码 |
| walkdir / uuid / sha2 / base64 / miow / serde / tokio / tracing | 见 CARGO-DEPS | MIT 或 Apache-2.0 | 基础设施 |

**MPL-2.0 说明**：Symphonia 是文件级 copyleft，与 GPL-3.0 兼容（MPL-2.0 §3.3
允许按 GPL 二次授权分发）。我们**未修改** Symphonia 源码；若将来修改，
修改后的文件需按 MPL-2.0 公开。

### 3. 前端依赖（打进 `resources`/内嵌资源）

| 组件 | 版本 | 许可 |
|---|---|---|
| Svelte | 5.x | MIT |
| Vite | 8.x | MIT |
| Tailwind CSS | 4.x | MIT |
| zod | 4.x | MIT |
| @tauri-apps/api | 2.9.x | Apache-2.0 OR MIT |

## 二、不随包分发、运行时按需调用的外部程序

| 组件 | 许可 | 说明 |
|---|---|---|
| Python 3.10–3.14 | PSF-2.0 | 「快速安装」轨道需要用户自备；「免安装包」轨道自带 embeddable 解释器 |
| beets 2.14.x | MIT | 必装依赖。缺失时应用明示"未检测到 beets 工具"，播放与曲库不受影响 |

两者都由用户侧安装，本程序只通过 `stdio` 启动并与其通信，
**不复制、不修改、不静态链接**它们的代码。

「免安装包」（`aiya-beets-bundle-win-x64.zip`，约 31 MB）是**另一个独立发布物**，
随包分发 embeddable Python（PSF-2.0）与 beets 及其依赖（各自许可见包内
`Lib/site-packages` 下的元数据）。它不出现在播放器安装包里。

### 独立发布的组件（`aiya-beets` 工具）

| 组件 | 版本 | 许可 | 说明 |
|---|---|---|---|
| `aiya-beets`（`beets-tool/`） | 0.1.0 | GPL-3.0-or-later | beets 工具本体，本程序自有代码。**不随播放器安装包分发** |
| embeddable Python（免安装包内） | 3.14.x | PSF-2.0 | 仅出现在「免安装包」这个独立发布物里 |
| beets 及其依赖（免安装包内） | 2.14.1 | 各自许可 | 清单见 `beets-tool/requirements.lock` 与包内元数据 |

这个工具是**可选**的：不装它时播放、曲库、播放历史、歌单、EQ、频谱全部照常，
只是少了导入整理与写标签。它的源码与播放器同仓库（`beets-tool/`），
源码要约同下一节。

## 三、源码获取方式（GPL-3.0 第 6 条）

`aiya.exe` 与 `aiya-audio-host.exe` 的完整对应源码（含构建脚本）：

- 仓库：<https://github.com/yeraph-plus/aiya-music-player>（对应 tag 见应用「关于」页版本号）
- 或随安装包同目录的 `licenses/SOURCE-OFFER.txt` 中给出的书面要约

若你在别处分发本程序的二进制，须一并提供同样的源码获取方式。

## 四、更新本文件的时机

- 新增/升级直接依赖后，重跑 `node tools/gen-notices.mjs` 刷新 `CARGO-DEPS.md`；
- 一旦真的捆绑 mpv/FFmpeg，必须补上其 GPLv2+/LGPL-2.1+ 条目与源码要约
  （FFmpeg 若动态链接，还需保留其 LGPL 声明与可替换性）。
