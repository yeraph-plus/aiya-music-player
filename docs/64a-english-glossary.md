# 64a · 英语迁移术语表与风格规则（2026-11-05）

> 英语支持执行批的**强制引用**：所有参与文案迁移的代理/人，遇到本表词条
> 必须用表内译法——术语不一致是双语质量的头号杀手。表未覆盖的词，按
> §2 风格规则自译并保持全仓一致。

## 1 · 核心名词（必须一致）

| 中文 | English | 备注 |
|---|---|---|
| 曲库 | library | |
| 歌单 | playlist | |
| 动态歌单 | smart playlist | 规则驱动物化歌单 |
| 专辑 | album | |
| 艺人 | artist | |
| 曲目 / 歌 | track | |
| 播放队列 / 队列 | queue / play queue | |
| 正在播放 | Now Playing | 页面名专有大小写 |
| 直播 | Live | 页面名；"直播流"= live stream |
| 流 | stream | HLS 直播流 |
| 整理库 | Organize | beets 整理页/页签名 |
| 预配置环境 | preconfigured bundle | beets 免配置套件包 |
| 提取器 | extractor | Essentia 提取器 |
| 播放条 | player bar | |
| 侧栏 | sidebar | |
| 播放历史 | play history | |
| 最近播放 | Recently Played | 区块标题专有大小写 |
| 最近添加 | Recently Added | 同上 |
| 常听 | Most Played | 同上 |
| 收听时长 | listening time | |
| 累计播放 | total plays | |
| 位置缺失 | missing locations | availability 三态 |
| 封面 | cover art / artwork | 名词 artwork，UI 文案 cover art |
| 歌词 | lyrics | |
| 频谱 | spectrum | |
| 均衡器 | EQ | |
| 音频输出 | audio output | |
| 独占模式 / 共享模式 | exclusive mode / shared mode | WASAPI |
| 采样率 | sample rate | |
| 位深 | bit depth | |
| 外置工具 | external tools | binaries |
| 插件 | plugin | |
| 协议 | protocol | |
| 校验和 | checksum | |
| 索引 | index | beets-index |
| 标签 | tag | 音频元数据标签 |
| 会话 | session | beets 会话 |
| 通道 / 管道 | channel / named pipe | sidecar 通信 |
| 心跳 | heartbeat | |
| 看门狗 | watchdog | |

## 2 · 高频动词与短语

| 中文 | English | 备注 |
|---|---|---|
| 播放 / 暂停 | Play / Pause | |
| 下一首 / 上一首 | Next / Previous | |
| 加入队列 | Add to queue | |
| 从队列移除 | Remove from queue | |
| 创建歌单 | Create playlist | |
| 重命名 | Rename | |
| 删除 / 移除 | Delete / Remove | 删文件=Delete；从列表拿掉=Remove |
| 导入 / 导出 | Import / Export | |
| 下载 | Download | |
| 安装 / 卸载 | Install / Uninstall | |
| 重试 | Retry | |
| 取消 / 保存 / 确认 | Cancel / Save / Confirm | |
| 刷新 | Refresh | |
| 搜索 | Search | |
| 全选 | Select all | |
| 随机播放 | Shuffle | |
| 列表循环 / 单曲循环 | Repeat all / Repeat one | |
| 扫描 | Scan | 文件系统扫描 |
| 打开目录 | Open folder | |
| 跳转 / 定位 | Seek / Seek to | |
| 快进 / 快退 | Seek forward / Seek backward | |
| 失败 | failed | 错误句小写开头 |
| 正在… | …ing / Now … | "正在扫描" = "Scanning…" |
| 已就绪 | ready | |
| 未安装 | not installed | |

## 3 · 不译清单（原样保留）

beets、WASAPI、HLS、m3u8、SMTC、AUMID、ReplayGain、Essentia、Gaia、
Symphonia、MP3 / FLAC / AAC / Opus / Ogg / WAV / AIFF / DSD、pip、venv、
SQLite、WAL、NDJSON、sha256、SDK、DLL、GUI、API、URL、URI、id。

## 4 · 风格规则

1. **错误 / 状态消息**：小写开头，陈述句，无感叹号。
   `"无法打开文件"` → `"failed to open file: {path}"`。
2. **UI 标签 / 按钮 / 标题**：sentence case（只首词大写）。
   `"加入队列"` → `"Add to queue"`。
3. **标点一律 ASCII**：中文全角标点（，。：""？！）随句子消灭。
4. **占位符原样保留**：`{}`、`{name}`、`%s`、`?` 的个数与顺序不变——
   format!/write!/positional 参数的对应关系不能错。
5. **语义不变**：错误分类、退出码、协议字段名、键名、JSON 结构不改，
   只改展示文本。
6. **简洁优先**：能一句说完不用两句；不解释原因时省略原因
   （`"failed to write tags"`，不写 `"failed to write tags because something went wrong"`）。
7. **大小写一致**：专有名词按 §3 清单（beets 小写、WASAPI 全大写）。
8. **数字与单位**：数字与单位间空格（`"3 days 2 hours"`、`"320 kbps"`）。

## 5 · 保留中文（不在迁移范围）

- `///` 与 `//` 注释（项目文风）；
- tracing 日志宏（info!/warn!/error!/debug!/trace!）的中文消息——内部诊断；
- mock/backend.ts 假数据（浏览器预览专用）；
- aiya-audio/examples 开发示例。
