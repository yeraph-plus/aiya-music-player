# 72 · 元数据插件依赖 / 插件支持表范围 / 代理方案评估（2026-11-13）

> 类型：评估　状态：结论待拍板（代理方案 A 建议采纳，实现另批）
> 关联：docs/58（单轨化）、docs/71（语域治理交接）、docs/60（治理 v2）；
> 触发来源：真 bundle 实测批（安装→换插件→分析→整理预览）发现刮削 0 候选
> 与「开始分析」无入口（beets-tool ledger 2026-11-13 条目）。

## 1 · 元数据插件依赖（beets 2.14 新架构）

beets 2.14 把元数据源重构为**插件化接口**（`beets/metadata_plugins.py`）：
`SearchApiMetadataSourcePlugin` 基类 + 带 `data_source` 属性的实现插件，
候选查询经 `find_metadata_source_plugins()` 分发。MusicBrainz 源 =
`beetsplug/musicbrainz.py`（**beets 内置随包发**，零额外 pip 依赖）。

- 依赖盘点（bundle 实证）：`requests` + `requests-ratelimiter` 随 beets
  2.14 自身装齐（site-packages 实测在位）；**无需 musicbrainzngs**（beets
  2.14 已弃用，改走 REST + ratelimiter 自实现）。
- 开关：`config.yaml` 的 `plugins:` 行——**musicbrainz 必须在启用列表里**
  才参与匹配。此前 fresh install 的启用面（profiles.workbench 五件）不含
  它 → 整理预览恒 0 候选（静默：无候选无报错，正是"缺件不炸"的隐蔽形态）。
- 本批修复：plugins.json 登记表新增 `musicbrainz` 条目（builtin/bundle，
  零 packages）+ `profiles.workbench` 扩充为 8 件（含 musicbrainz/fetchart/
  mbsync）——fresh install 启用面与 docs/58 预置承诺对齐。

## 2 · 插件支持表的范围结论

- **登记表 = plugins.json 单源**（25 条）：aiya 插件 + 影响应用功能的
  beets 内置插件（musicbrainz）。依赖字段已齐：`packages`（pip 目标）、
  `imports`（import 自检）、`exe`（外置二进制）。
- **不进表**：与应用功能无关的 beets 生态插件（用户可经 config 手工启用，
  进表反而扩大维护面）。支持表范围按"应用功能面"划界，不追求 beets
  插件全集。
- **zh 文本归档**：plugins-zh_Hans.json 结构变体并入基表（binaries 的
  `whyZh`/`vendorZh` 16 条全覆盖；插件的 title/note 本就是中文，无需变体），
  双语奇偶校验随之退役。

## 3 · 代理方案评估（只评估，未实现）

实测：MusicBrainz REST 可达但 **503 限流常态**（共享出口 IP）；coverart
可达。刮削可用性依赖 MB 的限流窗口——代理（国内可用的出口）能显著改善。

- **方案 A（建议采纳）：工具侧 env 代理**。宿主 `tool_env` 按设置下发
  `HTTPS_PROXY`/`HTTP_PROXY`/`NO_PROXY`（来源：settings.json 新键，如
  `beetsProxy`）；Python requests 栈**原生遵循环境变量**，元数据源/fetchart
  零代码改动。实现面 = host-core tool_env 三行 + settings DTO + 设置 UI
  一个输入框；影响面仅 beets 工具会话。
- **方案 B（暂缓）：应用级全局代理**。播放器自身网络（bundle 下载、
  release API、流媒体）全走代理——ureq 需代理 feature + 各网络出口逐个
  接线，改动面与回归面都大，等真实需求再议。
- 附带：MB 503 限流本身就是"慢"的来源之一（beets 有重试退避）——代理
  出口若换到限流更宽松的 IP，刮削速度同步受益。
