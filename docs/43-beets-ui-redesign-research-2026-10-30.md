# beets 集成 UI 重做：目录卡 / 插件开关与配置 / YAML 编辑器

> 状态：**调研完成，方案待拍板（本批不改代码）**。日期：2026-10-30。
> 用户三个原始想法：① 移除曲库目录里的单选"整理输出"，为 beets 单独一张卡片（列出
> 曲库扫描根 + 允许添加非曲库目录），输出位置保持唯一；② 待整理目录设置类似；
> ③ 插件按**开关**写入 beets 配置（而不是装了全开），考虑给每个插件写配置段落
> 描述（用户原话 "maincust"，按 manifest/config-fields 理解），以及直接引入
> YAML 编辑器。
> 前置：docs/33（P1/P2/P3）、docs/40（十三轮的 inbox/输出根模型）、docs/41（v2）、
> docs/42（进程形态维持不变——本方案全部在现有形态内做）。

---

## 1. 现状盘点：三个"实底"发现

### 1.1 整理输出 radio 目前是**纯装饰**（功能没有体现的实底）

代码走查的完整链路：

- UI radio（`LibraryTools.svelte` 曲库卡内）写 `beets.targetRoot` 设置；
- `beets_config.effectiveTarget` = targetRoot（仍是扫描根之一时）或回退第一根——
  **只用于渲染 radio 的选中态和一条 toast**；
- 实际决定文件落点的是 sidecar config.yaml 的 `directory:`——它在**首次**
  `ensure_config()` 时从 `AIYA_BEETS_SCAN_ROOT`（当时的第一个扫描根）写入，
  **此后任何机制都不再更新它**（ensure_config 永不覆盖，docs/33 P1 的正确实现）；
- `moveBeets` 传给 `move_items` 的是 `status.libraryDirectory`（工具自报的 config
  值），校验值等于配置值，**恒通过**。

结论：用户切换 radio 后，beets 实际输出不变；toast 说"已指向 X"是谎报。radio
换到与首次运行不同的根也不会触发 `E_LIBRARY_DIR_MISMATCH`（因为传的是工具自己
的值）——整条"输出根选择"链路没有一处真正接到 beets。**方案的核心就是把这条
链路接通，并以工具侧为唯一真相。**

### 1.2 插件没有任何启停机制

- `DEFAULT_PLUGINS`（6 个首发档）在首次 `ensure_config` 时写进 `plugins:` 行，
  此后**唯一**的启停方式是手改 config.yaml；
- `market_report()` 的 `enabled` 字段硬编码 `False`（注释："启停归 config，
  目录报告不带"）——UI 市场只有"安装"按钮（pypi 档），没有开关；
- 部分插件没有配置就没法用：`smartplaylist` 无 `playlists:` 段等于空转，
  `convert` 的格式、`lyrics` 的来源顺序都是配置驱动。

### 1.3 UI 分布混乱（用户体感的出处）

beets 相关控件散在两处：曲库卡里有"整理输出"radio（**跨域**——它是 beets 的
配置，却长在曲库卡）；扫描入库页又有工具块 + ImportPanel（含 moveMode 选择）+
维护折叠 + 市场折叠，共五块。加上 1.1 的假 radio，"加入了的功能没有体现"属实。

---

## 2. 三个想法 × 架构约束对照

| 想法 | 约束 | 结论 |
|---|---|---|
| ① beets 卡片 + 扫描根单选 + 非曲库目录 + 输出唯一 | beets 本身只支持**一个** `directory:`——"唯一"与 beets 事实一致；P1：播放器不得知道 config.yaml 语法 → **切换必须走工具命令**，工具做外科手术式编辑 | 采纳；数据流改为 `settings → config_set_directory 命令 → 工具改 config → status 回读`，工具自报值为唯一真相 |
| ② 待整理目录类似 | inbox 是**播放器侧**概念（扫描排除 + preview 排除表的锚点），不是 beets 配置 → 保持 settings 存储，但 UI 归入同一张目录卡 | 采纳；重叠校验规则沿用（inbox 不得等于/位于曲库根或输出根内部） |
| ③ 插件开关 + 每插件配置段 + YAML 编辑器 | P1：YAML 语法是工具的知识 → 开关/表单/编辑器全部经工具命令落盘；`ensure_config` "启动永不覆盖"规则**保留**（不冲突：那是防启动覆盖，用户显式指令的编辑是新增能力）；YAML 编辑器必须带工具侧校验，否则用户改坏配置后 serve 起不来，宿主只看到"不可达"而非诊断（见 §4.4 兜底） | 采纳；描述符驱动表单（工具声明字段，UI 只渲染），YAML 编辑器为高级折叠入口 |

---

## 3. 新信息架构（扫描入库页 · 整理库组）

```text
整理库组（BeetsSection）
├─ 工具状态块（现状保留：版本/入口/插件健康/库锁/schema 警告/安装）
├─ 目录卡（新，BeetsDirs）
│   ├─ 整理输出根（唯一）：radio = [曲库扫描根…] + [其他目录…]（自定义添加）
│   │   · 选中即发 config_set_directory；显示"实际生效"值（工具回读，status 为准）
│   │   · 自定义目录允许不在扫描根内，但 UI 提示"加入曲库扫描后才能在曲库看到"
│   ├─ 待整理目录（唯一）：现值 + 更换/清除 + 打开按钮 + 已导入排除表计数
│   └─ 校验沿用：inbox 与曲库根/输出根重叠 → invalid_arg（人话提示）
├─ 导入面板（ImportPanel，工作流不动）
│   · 顶部显示"输出到 <实际生效根>"（只读展示，点击滚动到目录卡）
│   · moveMode 三选保持（copy/move 是逐次动作，不是配置——docs/40 拍板不变）
├─ 维护折叠（现状保留）
└─ 插件卡（新，升级 BeetsMarket）
    ├─ 每插件一行：名称 / 说明 / 三态徽标 / 来源（builtin|pypi:包）/ 安装按钮
    ├─ **开关**：写 `plugins:` 行（工具命令），三态缺失时开关禁用并注明缺什么
    ├─ 可展开配置表单：由插件描述符渲染（见 §4.2），无描述符的插件只有开关
    └─ 高级折叠：YAML 原文编辑器（§4.4）
```

曲库卡（LibraryTools）摘除"整理输出"radio；扫描根的增删保持现状
（`clear_target_if_invalid` 的联动改为"输出根被删时提示用户重选"而不是静默回退）。

---

## 4. 设计要点

### 4.1 目录模型：接通数据流，真相唯一

- **真相源 = 工具自报**：`status.libraryDirectory`。radio 只是"请求值"；选择后
  发命令、由工具确认、回读 status/ConfigData 后才更新选中态——失败（如目录不
  存在）时 radio 归位并显示工具的人话错误。
- **`config_set_directory`**（工具侧）：校验目录存在（不存在拒绝，不代建——
  与 move_items "不猜"同一条准则）→ 外科手术式改 `directory:` 行 → 原子写
  （tmp + replace）→ 先备份 `config.yaml.bak` → 返回改后快照。
- **settings 变化**：`beets.targetRoot` 不再限"必须是扫描根"（允许自定义目录）；
  校验改为"不得与 inbox 重叠"；`clear_target_if_invalid` 相应放宽。
- **自定义输出目录不在扫描根内**：允许 + 提示（建议拍板 A，见 §7）——输出到
  扫描根之外是合法用法（比如专门给移动设备整理的目录），只是曲库看不到。

### 4.2 插件模型：描述符驱动，播放器不认识 beets 语义

- **描述符扩展**（`plugins.py`）：`MarketSpec` 增 `config_fields: tuple[FieldSpec, …]`；
  `FieldSpec = {key, label, type: bool|int|str|str_list, default, note}`。首批给
  四个有真实配置需求的插件写描述符：`replaygain`（backend/overwrite）、
  `convert`（format/command）、`lyrics`（sources 顺序）、`smartplaylist`
  （playlists 列表）。其余插件只有开关。
- **协议增量（v2 纯加法，不升版）**，全部是**信封应答**（快命令，无会话无新事件）：
  - `config_get` → `ConfigData { directory, plugins:[{name,enabled,state,missing}], stanzas:{name:{…}}, raw, rawHash }`
  - `config_set_directory {path}` → ConfigData
  - `plugin_toggle {name, enabled}` → ConfigData（改 `plugins:` 行；未知名字拒绝；
    缺依赖允许开但 warnings 带回）
  - `plugin_config_set {name, fields:{…}}` → ConfigData（工具按描述符渲染 stanza；
    未知字段/类型不符 → E_INVALID_ARG）
  - `config_raw_set {raw}` → ConfigData + warnings（**保存前工具侧验证**：
    yaml 解析 + 用 beets 自己的 config 机器加载一遍 + 关键字段 sanity；失败回
    行级错误，原文件不动）
- **YAML 渲染与编辑全部在工具侧**（新 `configops.py`）：行级定位 `plugins:` 行、
  顶层 stanza 的增删改（从 `key:` 到下一个无缩进行）、`yaml_path` 引号规则复用、
  原子写 + .bak。纯文本函数，可单测。
- **`market_report` 的 `enabled` 改为真实读取**（config 的 plugins 列表），
  `capability_report` 本来就吃 enabled 列表，联动免费。

### 4.3 启停规则与首发档

- 开关语义：`plugins:` 行 = 打开的插件集合；首发档六插件默认开（保持现状），
  用户可关。**关闭有关联功能的插件时 UI 提示后果**（replaygain→gain_db 失效、
  lyrics→歌词无来源、smartplaylist→推荐页导出失效、duplicates→维护查重不可用
  ——这些关联写进描述符的 `note`，UI 直接展示）。
- 装了 pypi 依赖 ≠ 开：安装与开关解耦（现状语义保持：安装按钮管依赖，开关管
  config）。

### 4.4 YAML 编辑器与损坏配置兜底（编辑器的前置修复）

- **serve 兜底放宽**：现在 beets 导入只捕 `ImportError/SystemExit`——config.yaml
  被 YAML 编辑器改坏时 beets 导入抛的是 yaml 错误，serve 直接崩、无信封，宿主
  只能显示"不可达"。改为捕 `Exception` → `E_CONFIG` 信封（"配置损坏：<原文>，
  可在编辑器修复或还原 config.yaml.bak"）。**这条必须先于编辑器落地。**
- 编辑器形态：折叠的"高级"区；原文等宽编辑 + 保存前工具校验 + 错误行回显 +
  `config.yaml.bak` 一键还原。播放器始终把 YAML 当不透明文本（P1 不破）。

---

## 5. 协议与测试影响

- **schema**：5 个新命令 + `ConfigData`/`PluginConfigField` 两个 $def；请求 still
  `additionalProperties:false`；无新事件、无新 Stage、无新错误码（复用
  E_INVALID_ARG/E_CONFIG/E_BEETS_ERROR）→ **v2 不升版**（README 增量规则 +
  docs/40 先例）。
- **契约测试**：新命令各一组离线消息（config_get 的 StatusData 式按命令校验、
  config_set_directory 的拒绝路径、plugin_toggle 未知名、config_raw_set 坏 YAML
  的行级错误）；configops 纯函数单测（行级编辑的边界：注释行、缩进块、文件末尾
  无换行）。
- **既有测试回归点**：`beets_config` DTO 字段变化（effectiveTarget 语义更新）、
  LibraryTools 摘除 radio 后的 e2e（35 条里涉及输出根 radio 的用例要跟着改）、
  smoke_maint 不受影响（move 校验路径不变）。

## 6. 批次拆分

| 批 | 内容 | 依赖 |
|---|---|---|
| 1 | sidecar：`configops.py` + 5 个命令 + serve 兜底放宽 + market enabled 真实化 + 契约/单测 | 协议 schema 先行 |
| 2 | 协议 schema + 镜像 + 宿主命令透传 + DTO + settings 校验放宽 | 批1 |
| 3 | UI：目录卡（含数据流接通）+ LibraryTools 摘 radio + ImportPanel 输出展示 | 批2 |
| 4 | UI：插件卡（开关 + 描述符表单）+ 四个插件描述符 | 批2 |
| 5 | UI：YAML 编辑器 + e2e 更新 + 全量验证 + docs 拍板记录 | 批4（§4.4 兜底在批1） |

## 7. 待拍板

（已拍板，见 §8。）

## 8. 拍板记录（2026-10-30）

| # | 决策 | 补充 |
|---|---|---|
| A | **允许**输出目录在曲库扫描根之外——beets 可以只当整理器用，整理结果不进曲库 | 目录卡对该状态给明确提示 |
| B | 插件配置表单首批四个（replaygain/convert/lyrics/smartplaylist），其余只给开关 | 描述符机制留扩展 |
| C | YAML 编辑器放批 5，§4.4 的 serve 配置损坏兜底先行 | 接口批 1 就留好 |
| D | 首发六插件允许关，关闭时按描述符 note 提示功能后果 | — |
| E | **beets 与自动刷新管线彻底解耦（比 §6 的"批 0"更彻底）**：dump→sync 对账链**整体取消**——定时/聚焦/动作收尾/手动对齐全都不再碰 beets；`beets_sync` Tauri 命令与"从整理库对齐"按钮移除（应用曲库自带刷新能力）；曲库的成员资格与行数据 100% 归扫描器 | 用户原话："keep 命令与事件（preview/apply/status）才是有用的，后期需要曲库内调用 beets 时直接构造命令指定曲库内文件操作，而不是异步隐身的工作方法" |

**拍板 E 的代价与归属**（评审时向用户明示）：对账链的三个产出各有新归属——
① 行连续性（beets 移动文件后曲库行跟随）→ 扫描器的改名/移动启发式扩展
（现 mtime+size 启发式对"先写标签再移动"接不住，后续扫描器批）；
② gain_db（replaygain）→ 扫描器学读 REPLAYGAIN 标签（后续扫描器批）；
③ beets_item_id 关联 → 不再需要（显式命令按路径定位曲库文件）。

## 9. 批 0 落地记录（2026-10-30，终态手术）

| 项 | 内容 |
|---|---|
| refresh.rs | 重写为**纯扫描**管线：删 beets 段（should_run_beets/forces_beets/collect_and_sync/emit_synced/Outcome.sync）、删失败退避机器（BEETS_FAIL_STREAK）、删 beetsBusy 让位（刷新不再起 sidecar）；`librarySynced`/`librarySyncFailed` 事件消失 |
| dto/refresh.rs | RefreshSnapshot 删 `sync` 字段（UI 无消费者），补 `skipped()` 构造器 |
| beets.rs | 删 `collect_items` + `DUMP_TIMEOUT` + `available()`（唯一调用方是 beets 段）；e2e 改纯 dump 解析锚点（`dump_parse_end_to_end`） |
| lib.rs | 删 `beets_sync` Tauri 命令与注册 |
| beets_sync.rs | 收缩为只剩 `online_paths`（adopt 仍需）；sync_items/BeetsItem/SyncStats/注入语义全部移除；`tracks.beets_item_id` 列保留、值不再有写入方 |
| **scanner.rs（新增）** | **学读 genre 标签**（ItemKey::Genre → tracks.genre/genre_norm，INSERT + ON CONFLICT）——genre 此前只有对账一个来源，移除后必须由扫描器接管，否则动态歌单（docs/40 §1）断粮 |
| vertical.rs | 智能歌单纵切改走扫描器 genre（TCON 帧），beets 同步步骤删除 |
| UI | "从整理库对齐"按钮/syncNow/syncing 阶段/synced 状态/librarySynced·SyncFailed 事件全部移除；applyDone 直接收尾，主视图重拉由 `libraryRefreshed(changed)` 驱动 |
| 协议 | **零改动**：librarySynced 是宿主合成事件不在 sidecar schema 里；`dump_items` 命令保留（移除命令 = 破坏性变更），宿主无常规调用方 |

**门禁**：cargo test --workspace 18 套件全绿（459 tests）；clippy -D warnings 通过；
gen:types / svelte-check 0/0 / build:ui 通过。已知的扫描器侧欠账（§8-E 代价①②）登记在案，
作为独立小批执行。

### 9.1 追加：多值标签支持（2026-10-30，用户拍板）

"艺人之类的字段应该允许写回多个值，毕竟 ID3 也支持"。落地为**规范连接串语义**：
扫描器读全部值（lofty `get_strings`，含单帧内 "; " 分隔的拆分）→ 去重保序 →
以 `"; "` 连接成单一登记名入库（`MULTI_SEP`）。选型理由：与 beets 的 Item 多值
字段语义**完全一致**（beets 存 "; " 连接、写文件按分隔符拆回多帧），两侧可无损
互换；不做 N:M 关系表迁移，未来要升级时按同一分隔符拆开即可。覆盖字段：
artist / albumartist / genre（title/album 是单值概念，保持取首值）。
beets apply 写回路径天然支持多值（候选串带 "; " 时 beets/mutagen 拆帧写出）。

**实测发现的上游限制**：ID3v2.4 的另一种合法多值形态——同键**多帧**（Picard 等
工具的写法）——在 lofty 0.25.4 读侧会塌缩成最后一帧（文件字节正确、解析后
只剩一帧，已用字节级探针证实）。beets/mutagen 的写法（单帧内 NUL 分隔）与
"; " 单帧写法都能完整读出。多帧塌缩作为上游问题登记：要么等 lofty 修复，
要么扫描器自解析 ID3（不值得）；测试 `multi_value_artist_and_genre_are_joined`
与 `semicolon_separated_single_frame_is_normalized` 锁住可支持的两条路径。
