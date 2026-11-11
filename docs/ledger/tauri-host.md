# ledger · Tauri 宿主（tauri-host）

> 代码位置：src-tauri/、crates/aiya-host-core/
> 相关档案：docs/03、12、13、56

## 条目（新→旧）

<!-- 每条一行：- YYYY-MM-DD · 代码位置 · 事实/取舍 · 来龙去脉（可选） -->
- 2026-11-12 · crates/aiya-host-core/src/beets.rs（busy_status / busy_status_stays_found）· 会话槽被占 ≠ 未安装：probe 对 Busy 回 found:true + E_BEETS_BUSY 的忙态壳，不复用 absent 壳 · probe 曾把 Busy 统一翻成 E_BEETS_UNREACHABLE（found=false），整理会话期间整理库页自行跳"未安装"安装页，组件看起来凭空消失——真相只是用户刚点的预览/整理还在跑
- 2026-11-12 · src-tauri/tests/ipc_contract.rs SERVER_ONLY · station_* 豁免必须两侧对称（web SERVER_ONLY 与桌面壳同表同步维护）· 2026-11-11 收尾发现 2451486 起 main 此测试一直红——"豁免清空"只清了 web 侧、桌面侧对称豁免漏配，补齐后绿；规则已回写注释，事件链归档
- 2026-11-12 · src-tauri/src/lib.rs app_diagnostics · 诊断快照只装有消费方的字段（压平原则）· v1 曾塞 20 个字段大半无页面读（统计与 LibraryStats 重复、流计数恒 0、audioMode 真实出口是 audio:mode 事件），每次拉取白跑 COUNT；media_keys 随 SMTC 常开退役，本批清理其注释残留
- 2026-11-12 · crates/aiya-host-core/src/{lib.rs,beets.rs} · 裸根祖先判定修复 + unix 夹具拆分（批 0.7，linux-test 分层暴露的最后一层）：path_overlaps_roots / path_within_roots 的子路径边界 `p+sep` 对裸根失效——unix 根 `/` 补成分隔符得 `//`，没有路径以它开头，把 `/` 当整理源/inbox 时「预览递归扫全库」的重叠闸整个旁路（Windows 未炸纯因盘符前缀 `d:\` 恰好等于 rc+sep 的巧合）→ canon_boundary（canon 形态自带尾分隔符则原样），重叠闸与落点闸（根设为 `/` 时一切绝对路径都在根内）双修；pick_dest 两测试夹具按平台拆（unix 落点判据走 `/` 边界，盘符路径在 unix 非绝对路径，within 恒 false 静默回退根）· host-core 的 ledger 归属：十主题划分早于 server 拆分（docs/66），host-core 编排归本域｜验证：cargo fmt/clippy/test -p aiya-host-core 绿（Windows 侧 40/0）；unix 行为经 CI linux job 实证
- 2026-11-04 · app_id.rs `heal_shortcut_aumid`、dto Diagnostics、前端 settings/media-session · **SMTC 常开无开关**（拍板）："系统媒体键"开关退役（旧键走 settings 迁移剔除）、mediaSession 恒全量注册、`smtc_active` 诊断镜像删除；同批修 SMTC 浮层「未知应用」——壳层解析应用名认**快捷方式的 System.AppUserModel.ID 属性**（进程级 AUMID + 注册表 DisplayName 都不够，P2-15 判据分支实测命中），方案为启动自愈：对已存在的开始菜单 .lnk 补写属性（windows 0.62 的 IPropertyStore，SetValue 拷贝值故无需 forget），便携版无快捷方式保持「未知应用」 · media-session.ts"失焦走主进程 globalShortcut"为失真注释（主进程从未注册），全局媒体键唯一实现是后端 SMTC 转发——这决定 P2-16 不能走"按设置 gate spawn"（会连失焦媒体键一起杀掉）
- 2026-11-04 · lib.rs setup 托盘块、`CloseRequested`、dto `ApiError::record_background` · 托盘健康口径：正常**静默**、失败进"最近错误"台账+日志、应用照常跑（此前 `tray.build(app)?` 失败会中止整个 setup）；`tray_ready` 标志不再做诊断指示器，唯一用途是兜 `CloseRequested`——托盘缺席时"关闭到托盘"回落为直接退出，否则窗口藏进一个没有唤回入口的隐藏态 · 设置页"当前：已生效"尾巴与 Diagnostics.tray 字段一并删除（托盘无条件创建，指示器恒真、零信息量）；Ctrl+Q 退出快捷键从未存在，设置页假文案同步清除
- 2026-11-04 · lib.rs `prune_queue_for_deleted` · 曲库删除（remove_tracks/roots_prune/clear）后必须同步摘除 Actor 队列中的已删 id 并**无条件**全量落库 · queue_items→tracks 外键 CASCADE 只管库内行，内存队列残留已删 id 会让此后每次 save_queue_full 外键违规整体回滚、状态落库永久静默失败。
- 2026-11-04 · lib.rs `library_scan` · `scanning` 标志统一挂 refresh.rs 的 FlagGuard（RAII Drop 兜底）· 手工 store(false) 在 spawn_blocking panic 路径不执行，标志永真后手动扫描与自动刷新全瘫；同款防线 refresh 链已付过学费。
- 2026-11-04 · lib.rs · 重播类命令（play_query/play_tracks/open_paths/lyrics_get/ids_for_selection/library_tracks）一律 async + spawn_blocking；`track_refs_by_ids` 分批 IN（900/批）且仅当 ids 含临时前缀才读队列表 · 同步命令跑主线程，5 万曲库点"播放全部"曾冻结整个窗口消息循环。
- 2026-11-04 · beets.rs · `start_uninstall` 与 install/plugins_set 共用 `beets_installing` CAS；`uninstall_plan` 子进程 30s 超时（超时报错，不回落自算删除——没过四道闸就删目录树更危险）· 卸载与安装解压互踩、工具挂死永不收尾是同一互斥假设的漏网点。
- 2026-11-04 · sidecar.rs · Welcome 握手读取挪独立线程 + `recv_timeout(10s)`，超时 kill 子进程收尾 · miow 管道读无超时 API，deadline 只在 read 返回后检查管不住"已 accept、未写 Welcome"的僵死。
- 2026-11-04 · lib.rs `player_set_volume` · 音量变更保留当前 muted · 静音态拖音量条被静默解除静音不符主流播放器语义。
- 2026-11-04 · refresh.rs · `Trigger::Manual` 变体删除（无调用方的死代码），client.ts 两处孤儿注释同批清理 · 需要手动刷新入口时随命令一起加回。


- 2026-11-03 · src-tauri/src/dto/beets.rs、tests/dto_wire.rs · BeetsStatus 不再携带 pythonSupported：单轨化后环境信息全部由工具自报，能力面 = plugin_health 三态；dto_wire 的陈旧断言随字段移除同步清理
