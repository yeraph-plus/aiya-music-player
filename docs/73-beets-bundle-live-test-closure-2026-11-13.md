# 73 · beets 包真机实测闭合（2026-11-13）

> 类型：收尾批（实测 → 修复 → 评估）　状态：closed（刮削专项与代理实现转出，见 §6）
> 关联：docs/71 的下一批（该批的「开工第一件事：核对线上 run」引出本批全部工作）；
> docs/58（单轨化与预置承诺）、docs/70（测试面审查——release-beets 自检闸出处）、
> docs/72（元数据依赖 / 支持表范围 / 代理评估）；逐条细节见 beets-tool /
> build-tooling / data-layer / beets-protocol 四份 ledger 的 2026-11-13 条目。

## 0 · 一句话

真 bundle 在真应用里走完 安装 → 换插件 → 分析 → 查询 → 存歌单 → 整理预览
全链（CDP 驱动真窗口 + 直连工具进程隔离复现），**修掉一个 P0 挂死与一个
发布闸脚本 bug**，登记表收口为单源，另发现三个结构性缺口转出（§6）。

## 1 · 线上打包（三次迭代）

- run 10 ❌：`release-beets` 自检闸首跑即抓——包内 beets 导入检查把 PowerShell
  的 `Write-Host` 混进 `python -c`。关键：`import beets` 本身成功，**bundle
  无恙**，坏的是检查脚本（68f4167 补的闸，发布闸干的就是这个活）。
- run 11 ✅：改 `print` 后发布成功；通道资产推新删旧（55.7MB zip + SHA256SUMS）。
- run 12 ✅：numpy 预导入修复（§2-B2）随批重发。

## 2 · 真机实测：修掉的与验证通的

### 2.1 P0 挂死（已修）

整理预览会话永久挂死：`preview_import` 在 scan 与 matching 事件之间卡死，
零输出、零日志、零 CPU。定位链：宿主会话零痕迹 → spawn 进程命令行核对
（裸 `-m aiya_beets` = serve 会话模式，命令经 stdin 下发——非 bug）→ 直连
工具进程隔离复现（75s 一条 scan）→ `faulthandler` 转储 + `-X importtime`
钉到 **`beets.autotag.distance` → numpy 的 C 扩展导入在 serve 会话工作线程
里永久挂起**（`numpy._distributor_init`，独立进程 0.3s 对照秒过）。

修复：serve 启动期**主线程预导入 numpy**（守卫式——无分析器的 bundle 变体
跳过）。修复后预览 2s 完整跑通（scan → matching → album → done）。

### 2.2 验证通的链路（真应用、真通道、真曲库 1381 首）

- **安装**：55.7MB 下载 → SHA256 逐行 → 解压 → 记入口 → probe；
  状态卡 beets 2.14.1 / Python 3.14.10 / **协议 v3** 握手成功。
- **换插件**：fetchart/mbsync/xtractor 勾选 → 包内 pip 真装
  （beets-xtractor-0.4.2 + 依赖链透传）→ 撤销同理；config 与 settings
  双落账一致。
- **xtractor 分析**：beets_enrich_start → bpm/mood 真实落 enrich.db
  （154bpm / relaxed 0.89 等；真曲 ~10s/首）。
- **标签查询 → 存为歌单**：原始语法 `bpm:120..170` → 命中 16 首 →
  全选 → 「Beets 查询」歌单落 library.sqlite（16 items）。

### 2.3 结构性缺口四项（两修两转出）

| 缺口 | 处置 |
|---|---|
| G1 启用面与 docs/58 预置承诺不符（5 件默认，xtractor/fetchart/mbsync 不在） | **已修**：profiles.workbench 扩至 8 件（§3）；settings.plugins 空基准（G4）与 G1 同源，预置面就位后随勾选落账 |
| G2 刮削 autotag 恒 0 候选 | **转出**（beets 2.14 metadata_plugins 集成专项，§6-1）——启用 musicbrainz 后预览会话中 metadata sources 仍为空，上游新架构与集成的接缝问题，非管线缺件 |
| G3 「开始分析」无 UI 入口（enrichment.start 零调用） | **已修**：IndexPanel 分析按钮（用户显式点击；无自动触发是拍板语义） |
| G4 enrich 运行中阻塞查询（E_BUSY 数小时）且无进度无取消反馈 | **部分缓解**：按钮带 10s 轮询进度 + beets_cancel 可取消；查询占用的结构性解法（读写分离/独立会话）随 G2 专项 |

## 3 · 登记表收口（插件清单单源化）

- **真源确认**：rust 侧 `include_str!` 直读工具侧 plugins.json——结构数据
  本就单源；真正的双份是 **plugins-zh_Hans.json**（746 行全结构复制 +
  奇偶校验手工对齐）。
- **收口**：binaries 的双语内容并入基表（`whyZh`/`vendorZh` 16 条归档）；
  插件段 title/note 核对后确认两份相同（双语分支只在 binaries）→ 变体
  文件删除、奇偶校验退役（catalog_contract [7] 改为 zh 归档覆盖检查 16 条）、
  rust 单文件 include_str + `LANGUAGE_KEY` 内联。
- **musicbrainz 条目入表**（beets 内置元数据源、零额外 pip 依赖——
  requests/requests-ratelimiter 随 beets 2.14 装齐）+ profiles.workbench
  扩至 8 件。依赖识别结论与支持表范围划界见 docs/72 §1–§2。

## 4 · 协议 v2 → v3（三处两轨时代遗留全删）

拍板全删：market 链（docs/40 §3 调研 → 429557c 落地 → cc7b323 回收 UI
留线 → docs/58 闭环后零调用）、uninstall venv-only 分支（c3c848d 两轨时代
产物）、pythonSupported（25c7e99 未净退役）。schema 治理规则「删字段必须
+1 版本」按字面执行；旧装 bundle 被版本/哈希门禁拒绝 = 无更新路径哲学的
预期行为。出身追溯与清单见 beets-protocol ledger 2026-11-13 条目。

## 5 · 验证矩阵

- CI：run 55/56/59 全绿（含 linux job 的 unix 分支与迁移 0013）；
  run 58 的 clippy/fmt 两红（机械）已修。
- 契约：protocol / catalog / uninstall（7 组重写）/ smoke:proto ALL PASS。
- 门禁：strict 命中 0（叙事词升阻断 + .css/.sql 扫描后全仓归零）、
  gen:types ✓、svelte-check 0/0、e2e 44/44、workspace 26 套件绿。
- 发布：v0.2.1 播放器 release + beets-portable 通道双资产在架。

## 6 · 剩余项（2026-11-13 追加拍板后更新）

1. ~~刮削专项（G2）~~ **已闭合（2026-11-13 追加）**：根因 = beets 2.14 把
   插件加载改为显式 `load_plugins()`（只挂在 beets 自己的 ui 入口），serve
   模式不经过 ui → 加载从未发生 → 元数据源恒空。修复 = serve 分支显式调
   `beets.plugins.load_plugins()`；直连工具进程验证：MusicBrainz 源加载 →
   5 候选 → apply_import 全通（写标签 + 拷贝落点 + 入库，3s，failed 0）。
   观察项：混合目录组（四首不同专辑挤一个 inbox）匹配度低（recommendation
   none，候选路径出现 Non-Album 分段）——真实场景整专辑目录导入不受影响。
2. **代理方案 A**：**延后一批**（拍板：只有 beets 需要代理，应用级全局代理
   可能没必要；但 GitHub 访问不稳可能连便携包都拉不下来——方案先想清楚）。
   评估全文见 docs/72 §3。
3. ~~分析续算~~ **全库跑不跑由用户自理**：xtractor 已实证正常出值
   （20/1381 幂等可续），不再作为遗留项跟踪。
4. **未覆盖面**：整理 move（真删）模式、http-server 的 beets 面、
   更新行为（本就无更新路径，按设计不测）。
5. **存量兼容**：REQUIRED_PLUGINS 并集在 ensure_config 的同步里生效——
   旧装 config 缺 musicbrainz 的，下一次会话启动/换插件自动补齐。
6. **刮削源追加实测（2026-11-14）**：beetsplug 里其余元数据源逐一过探针
   （load_plugins + candidates，Robert Miles / Dreamland，无 -I 对齐 serve
   真实条件）——
   - **deezer / spotify：可用，已入登记表**。零凭证零新依赖（requests/confuse
     已随包），实测出真候选（3/5 候选、15s 级）；bundle 登记表副本已同步，
     install 链十件目标集实跑通过（幂等 pip + 自检 + 零卸载差量）。
   - **tidal：插件加载干净，依赖只有 requests-oauthlib（wheel）**；候选阶段
     401——须 TIDAL 账号 OAuth 设备授权。失败模式是每请求报错，不挂会话。
     已入表（note 注明须账号）。beets 的 tidal 是纯 requests_oauthlib 实现，
     不依赖 tidalapi 包，无 sdist 构建问题。
   - **discogs：危险，先配 token 才准启用**。无 tokenfile/user_token 时插件
     `__init__` 的 OAuth 握手向 stdout 打印授权 URL（污染 NDJSON 协议流）+
     `beets.ui.input_()` 阻塞读 stdin（吞宿主下一条命令帧）——实测复现。
     note 已改为显式警告；条目保留（`bundle\python.exe -m beets` CLI 交互
     授权一次落 tokenfile 后可用，或配 user_token）。
   - **beatport：本机不可用**。`__init__` 即发 OAuth 请求，实测
     oauth-api.beatport.com DNS 解析失败（网络层），加载失败被 load_plugins
     吞（traceback 进 stderr，进程不死）；beets 官方宣布 3.0 移除。note 标记
     不建议启用。
   - **待拍板：凭证型源的启用闸**。discogs/beatport 的 note 只是提示，UI 仍
     可点启用 → 下一次 serve 启动即污染协议流。要不要在 serve 分支 load_plugins
     之前按"凭证证据"（tokenfile 存在 / user_token 非空）剔除无凭证的凭证型源
     （登记表加字段，load 失败在三态报告里显示），随刮削专项一起定。
   - 附带发现：bundle 解释器 user-site 开着——pip 会把用户级 Python314
     site-packages 里的包当"已满足"跳过装进 bundle（six 就这样没进包），
     免配置包的封闭性有缺口；且探针用 `-I` 会与 serve 条件不一致（误判缺依赖）。
     pip 走用户全局配置的清华镜像，镜像抖动时 install 链随镜像一起失败
     （本次实测镜像 000 / 官方 PyPI 200——用户级 pip.ini 是既有事实，工具不代改）。
