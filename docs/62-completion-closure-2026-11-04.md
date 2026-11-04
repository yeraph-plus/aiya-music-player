# 62 · 完工收尾档（2026-11-04）

> 项目进入"已完工、等首批用户"状态的结档记录。本档汇总最后一批后端收尾
> （真机数据暴露的问题为主），逐项记录病灶、修法与验证方式；文末是发布门
> 体检结论与**已知缺口清单**——那些"有了用户才值得排优先级"的事都在那里。
> 域内细节见各 ledger（data-layer / tauri-host / ui-ipc / ui-shell），本档
> 只做索引与决策记录。

## 1 · 本批收尾项（按发现顺序）

### 1.1 码率徽章恒 "2 kbps"（扫描器写库错位）

- **病灶**：`scanner.rs` upsert 的 INSERT VALUES 少占一个 codec 位，
  `container..channels` 五参数整体左移一格——`bitrate` 列存的是声道数
  （立体声恒 2），采样率串进 container、码率串进 bits_per_sample。读路径
  与前端展示规则都没错，纯扫库写错；数值列错位无声，此前没有任何测试读过
  这六列。
- **修法**：VALUES 补 NULL 占位（codec 归 beets 同步领地，扫描器只有
  lofty 的 FileType）；回归测试
  `scan_writes_audio_columns_in_their_own_slots` 守六列各归各位。
- **存量修复**：1381 行脏数据用 `UPDATE tracks SET mtime = 0` 迫使重扫
  全量重解析（mtime=0 本就是"没读到"哨位）。真窗口实测徽章恢复
  `320 kbps`（tooltip：MP3 · 44.1 kHz · 2 声道 · 有损 · 320 kbps）。

### 1.2 托盘健康口径（静默成功 / 后台报错）

- **病灶**：`tray.build(app)?` 失败会中止整个 setup——托盘建不成等于应用
  起不来；设置页却挂着一个恒真的"当前：已生效"指示器（托盘无条件创建，
  它永远是真）。
- **修法**：托盘失败改记日志 + `ApiError::record_background` 进"最近错误"
  台账，应用照常跑；`tray_ready` 标志专职兜 `CloseRequested`——托盘缺席时
  "关闭到托盘"回落为直接退出（否则窗口藏进没有唤回入口的隐藏态）。
  SMTC 初始化失败同口径接入。诊断快照的 `tray` 字段与设置页尾巴删除。
- **顺带**：Ctrl+Q 退出快捷键从未存在（无全局快捷键插件、托盘退出项无
  加速器、快捷键表无退出动作），设置页"或 Ctrl+Q"假文案删除。

### 1.3 诊断快照压平（20 字段 → 4）

- **病灶**：`Diagnostics` 按 v1"诊断页"设想塞了 20 个字段，逐字段清点
  消费面后 15 个没有任何 UI 读过（统计与 LibraryStats 重复、流计数恒 0、
  audioMode 的真实出口是 `audio:mode` 事件），每次拉取白跑一轮 COUNT。
- **修法**：压到 `version / dataDir / recentErrors / mediaKeys` 四个有
  消费方的字段；`ListTiming` / `AudioModeInfo` / `PowerInfo` /
  `StreamsInfo` 结构体连同后端耗时环、`sleep_prevented` 诊断镜像一并退役；
  `AppInfo` 瘦到只剩 `dataDir`。`MediaKeysInfo.failed` 删除（旧实现设置
  关闭时误报"初始化失败"）。dto_wire 加压平形状守卫。
- **曲目模型三套结构（TrackRow/TrackDetail/TrackRef）审过不动**：分层
  各有使命，TrackDetail 的显式手拷贝是有意的编译期同步闸。

### 1.4 数据库迁移归一化（二次压平）

- **动作**：`0002..0011` 十个增量全部吸收进 `0001_init.sql` 单文件基线
  （列按 ALTER 追加序折叠；0006/0007 数据回填与 0008/0009 净零对不吸收，
  吸收清单在 0001 头注）。结构等价有验证脚本实证：10 表 × 列/FK/UNIQUE/
  CHECK + 14 索引 + 种子数据全等。
- **本机库消化**：`PRAGMA user_version` 11 → 1 回拨。**教训**：tauri dev
  的 crate 监视在改 `db.rs` 时自动用新代码重启了应用，v11 > 新链长 1 触发
  版本守卫留底重建——备份机制工作正常（`library.sqlite.v11-*.bak` 完整），
  恢复 = 备份改名回来 + 版本回拨，数据零丢失。
- **纪律**：发布后压平冻结，改结构只允许增量迁移。

### 1.5 首页统计块占位方案（前端，随本批一并归档）

- 卡组几何互钉（height 148 + margin-bottom 60 + flex-end）在歌单行空态
  时锚点失效；改为普通块顶部对齐、`::before` 预留一个区块标题盒
  （`--z-stage` 同批进 token 阶梯，清掉 LiveView 三处裸 z-index）。
- 快捷键表删"打开设置"（Ctrl+,）——全表唯一无价值项；退役动作键在
  settings 迁移层读时剔除，不随快照永久写回。

### 1.6 SMTC 常开与「未知应用」修复（收尾追加批，2026-11-05 归档）

- **SMTC 常开无开关**（P2-16 销项）：压平审计发现"系统媒体键"开关没有任何
  后端消费方——SMTC 无条件 spawn，开关的"关闭"承诺是空话；且全局媒体键的
  唯一实现就是后端 SMTC 转发（media-session.ts 里"失焦走主进程
  globalShortcut"是失真注释），按设置 gate spawn 会连失焦媒体键一起杀掉。
  拍板：**SMTC 常开、不设用户开关**——开关、设置字段、诊断 `mediaKeys`
  组整体退役（旧键走 settings 迁移剔除），mediaSession 恒全量注册。
- **SMTC 浮层「未知应用」**（P2-15①）：壳层解析浮层应用名认的是**开始菜单
  快捷方式上的 `System.AppUserModel.ID` 属性**——进程级 AUMID + 注册表
  DisplayName 都补不了（实测踩中 P2-15 预言的判据分支）。修法为**应用启动
  自愈**（app_id.rs `heal_shortcut_aumid`）：对已存在的开始菜单 .lnk 补写
  属性（windows 0.62 IPropertyStore），装了安装版的机器首启即修好、升级
  重装可再自愈；NSIS hook 手拼 COM vtable 或引插件都不值得。便携版无快捷
  方式保持「未知应用」（已接受取舍）。**待真实安装验证**：浮层显示
  AIYA Music + 任务栏固定项不分裂。
- 发布产物已按本批代码重打（setup 6.78 MiB / portable zip 9.8 MB）。

## 2 · 发布门体检结论（2026-11-04）

| 检查项 | 状态 |
|---|---|
| `npm run verify`（Rust 全量 + 类型 + 构建 + e2e + lint + 契约冒烟） | ✅ exit 0 |
| 版本一致性（tauri.conf / Cargo / package.json / ui） | ✅ 全 0.1.0 |
| 打包配置（NSIS currentUser、双语、图标、sidecar、许可资源、CSP） | ✅ 齐 |
| `npm run bundle` + `npm run portable`（发布形态真实产物） | ✅ `AIYA Music_0.1.0_x64-setup.exe` 6.78 MiB（NSIS）+ `AIYA-Music_0.1.0_x64-portable.zip` 9.8 MB（8 条目：主程序/引擎/便携标记/README/许可） |
| README（下载表 / SmartScreen 说明 / 源码运行） | ✅ 齐 |
| 调试钩子（`?failCmd` 等）只活在 mock chunk，release 不加载 | ✅ |
| devtools feature 未进 release；无代码签名（决策已文档化） | ✅ |
| CI 双 job（windows-latest）跑 verify 链 + beets smokes | ✅ |
| 发布工作流（release.yml：v* tag 触发，发布门禁 + 脏树检查 + bundle + portable） | ✅ 在位，尚未被真实 tag 跑过（首次 tag 前先手动 dispatch 试一轮） |

## 3 · 已知缺口（等用户反馈排优先级）

> 本清单已镜像进 docs/35（唯一真源）：自动更新见 P2-8（计划本就后置 v2）。
> 下面是索引与语境。

1. ~~`mediaKeys` 开关不 gate SMTC~~ **已销项（P2-16）**：拍板 SMTC 常开、不设
   用户开关——"系统媒体键"开关、设置字段、诊断 `mediaKeys` 组整体退役，
   media-session 的失真注释改真。同批顺带修掉 SMTC 浮层「未知应用」：进程级
   AUMID + 注册表登记都补不了壳层解析所需的**快捷方式属性**（P2-15 判据分支
   命中），改为应用启动自愈（app_id.rs 对已存在的开始菜单快捷方式补写
   `System.AppUserModel.ID`，便携版无快捷方式保持「未知应用」——已接受取舍）。
   **待真实安装验证**：浮层显示 AIYA Music + 任务栏固定项不分裂（P2-15①）。
2. **获取更新按钮是占位**（恒禁用）：没有更新器基建（docs/35 P2-8，自动
   更新计划后置 v2）；v0.1 靠手动分发。
3. **无代码签名**：SmartScreen 提示"未知发布者"，README 与便携版 README
   已如实写明处理方式；证书成本决策未变（docs/35 P0-1 已拍板不做）。
4. **release.yml 未被真实 tag 验证过**：工作流在位且本机 bundle/portable
   双双成功，但"打 tag → 公开 release"的完整链路要等第一次发版才走通；
   首发前用手动 dispatch 演练一轮。
5. **推流 Docker 服务未实现**：`LIVE_STATION_FIELDS.md` 只落了字段契约
   （`source: 'service'` 入库途径、封面 http(s) URL 均已预留），服务端
   站单同步是独立工程（作者自留批次，不算应用欠账）。
6. **docs/35 的 P0-4/5/6 与 P1-1/1-3 仍是未结项**：干净机安装闭环、4h
   soak、杀软观察、真 release 地址、beets 双轨道干净机流程——全部只有
   人能跑，正是"有了其他用户才能发现问题"的那部分。
