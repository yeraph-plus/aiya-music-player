# 后端修复与 E2E 验证（2026-10-24，第二轮审查之后）

> 输入：`docs/11-review-round2-2026-10-24.md`（5 个 P0 / 16 个 P1）
> 本轮范围（用户指令）：**只修后端** → 后端修完跑 E2E → UI 将整体重做（本轮不动）
> 计划类文档（docs/00–10）不动。
>
> 结论：**5 个 P0 全修；16 个 P1 修 17 项（含 1 项用等价机制替代）、2 项属 UI 移交重做**。
> 后端测试 76 → **114 条全绿**；4 个真机冒烟（WASAPI 出声 / 起点 / 切歌 / 变速 / sidecar 看门狗）
> + beets 5 场景冒烟全通过；5 万首性能经修正口径后 **首屏 0.80ms、游标续页 2.2ms**。

---

## 一、P0（5/5 已修）

| # | 问题 | 修法（落点） | 证据 |
|---|---|---|---|
| P0-1 | 进度节流恒假 → 进度条冻结、落库进度停在装载点 | 节流判定抽成纯函数 `position_emit_due(now, last_emit)`（`lib.rs`），基准是泵私有 `last_emit_ms`，与任何"刚被归约"的时间戳无关 | `lib_tests::position_throttle_is_based_on_last_emit_only`（首帧放行 / 500ms 内压掉 / 满 500ms 放行 / 时钟回拨不 panic）；`engine_smoke` 的 Position 每 ~10ms 连续推进 |
| P0-2 | 选区换算死循环冻死 App | 新增 `repo::collect_track_ids(conn, q, max)`：有游标键走游标翻页，**否则必须走 offset 翻页**（旧代码只写 `q.cursor`，offset 模式下游标被忽略 → 每轮同一页）；`library_ids_for_selection` / `player_play_query` 改为调用它 | `collect_ids_terminates_with_offset_mode_input`、`collect_ids_covers_playlist_source`、`collect_ids_reports_truncation` |
| P0-3 | 7 条 playlist 命令未注册（invoke 直接 reject） | 新增 `crates/aiya-data/src/playlist.rs`（rename/delete/reorder/add_tracks/remove_at/move/clear + create 复用）与 7 条 `#[tauri::command]`，全部注册；另有 `ipc_contract` 测试把"UI 调用 ↔ 注册表"对起来，此后再漏一条直接红色 | `playlist::tests::*`（15 条）；`ipc_contract::every_ui_invoked_command_is_registered` |
| P0-4 | beets stdio 未钉 UTF-8（中文路径 → 乱码 / 会话无声死掉） | 三层钉死：①Python `_force_utf8_stdio()` 启动即 `reconfigure(encoding="utf-8")`；②Rust `sidecar_env()` 注入 `PYTHONIOENCODING=utf-8` + `PYTHONUTF8=1`（三处 spawn 共用，含 probe）；③读取改 `read_until` + 显式 `from_utf8`，**非法字节记 `valid_up_to` 日志后跳过**（旧实现 `lines()` 遇 Err 直接 break） | 手动复现：不设任何编码环境变量跑 `dump_items`，输出 `iconv -f UTF-8` 校验通过，含 `中音测试 440Hz.wav` / `测试曲目 01`；`dump_parse_and_sync_end_to_end` 走同一环境 |
| P0-5 | 行聚焦按空格双派发 | **UI 项**，随 UI 重做处理（本轮不动 UI） | — |

---

## 二、P1（17 项已修 / 2 项移交 UI）

### 数据可靠性

| # | 修法 | 证据 |
|---|---|---|
| P1-1 迁移超前版本直接拒绝打开（升级即砖） | `open_checked()`：`user_version > 迁移数` → **关闭连接 → 主库与 `-wal`/`-shm` 一起改名留底** `xxx.sqlite.v{N}-{时间戳}.bak` → 原路径建新库并跑迁移。只挪主库会让新库套用旧 WAL 的脏页，所以三个文件一起挪 | `db::tests::future_version_db_is_quarantined_and_recreated`（新库结构无残留 + 恰好一个备份文件） |
| P1-2 `play_count` / `last_played_at` 从未维护 | 抽 `aiya_data::history::{record, clear}`：history 行与 tracks 统计列**同事务**增/清；`history_clear` 一并把统计列归零 | `history::tests::record_updates_play_count_and_last_played_at`、`clear_zeroes_stats_in_same_transaction` |
| P1-3 `limit` 硬钳 500 静默截断"播放全部" | 见 P0-2：`collect_track_ids` 独立取全量 + `truncated` 信号（上限 5 万，命中即 `tracing::warn`） | `collect_ids_reports_truncation` |
| P1-4 扫描器"跳过隐藏目录"是空操作 | `walk()` 改 `into_iter()` + `skip_current_dir()` **整棵子树剪掉**；`depth==0` 的库根例外（根目录本身叫 `.music` 也必须进；tempfile 的目录名以 `.tmp` 开头，剪根 = 一个文件都扫不到 —— 这条是回归测试抓出来的） | `walk_prunes_hidden_and_noise_subtrees` |
| P1-5 扫批 DEFERRED 事务升级写锁即 BUSY | 扫批事务与 `mark_missing` 事务改 `TransactionBehavior::Immediate`（`busy_timeout` 对锁升级不生效，只对"从开始就等写锁"生效） | 全部扫描测试（含取消/恢复/大小写变体）通过 |
| P1-6 beets 同步单事务 5 万条 → 播放历史静默丢失 | `sync_items` 改**分块提交**（`SYNC_CHUNK = 500`），块间让出写锁；对齐本身幂等，某块失败前面已提交的块保留 | `import_then_unchanged_then_update` 等 5 条 beets_sync 测试 |

### beets 子系统

| # | 修法 | 证据 |
|---|---|---|
| P1-7 取消的"已入队未派发"窗口被 `CANCEL.clear()` 抹掉 | Python 改**自增取消代数**：请求行入队时记下当前代数，派发时比较"当前代数是否变过"（变过=排队期间被取消），工作循环同样比较代数；`CANCEL.clear()` 删除 | `tools/smoke_apply.py` 场景 5 断言改排他：终止事件必须**恰好一个**且**必须是 `cancelled`**（旧断言 `cancelled or applyDone` 正是 P1-7 的庇护伞）。实跑输出：`['cancelAck','progress','progress','cancelled']` |
| P1-8 apply 只按专辑发进度 → 大专辑被 600s 看门狗误杀 | apply 循环里**逐文件**发 `progress`（带 `fileCurrent/fileTotal`），既是真实进度也是看门狗心跳 | 场景 5 中 36 个文件期间持续有 `progress` 事件 |
| P1-9 `apply_move` 后同一首裂成两行 | `sync_items` 建 `beets_item_id → 行` 索引：path 没命中但 id 命中 = 文件被移走 → `relocate_track` 跟随新路径（**行 id 不变**，歌单/队列引用与 play_count 都还在）；目标路径已被别的行占用时只告警不中断整次同步 | `moved_item_follows_new_path_without_duplicating`（1 行、id 不变、play_count=3 保留、再同步为 unchanged） |
| P1-10 dump 失败被当"库里 0 条"→ 伪成功 + orphaned 全量 | `collect_items` 必须收到 `dumpDone` 才算成功，否则 `Err("dump 未收到 dumpDone…")`；整体超时 `DUMP_TIMEOUT=180s`，超时/`sessionFailed` 一律 kill + 收尸 | 代码路径 + `dump_parse_and_sync_end_to_end`（真 sidecar dump 3 条 → 解析 → 对齐 → 幂等） |
| P1-11 probe 不注入 `AIYA_BEETS_DIR`（设置页显示真实库、导入写隔离库） | `sidecar_env()` 三处 spawn 共用，`query_sidecar`（probe 路径）也带上了 | `probe_finds_dev_venv_beets` 通过 |

### 播放语义

| # | 修法 | 证据 |
|---|---|---|
| P1-12 装载起点被丢弃（"记住进度"从不生效） | `EngineCommand::Load { path, start_ms, token }`；`play_file` 开流前 `decoder.seek_ms(start_ms)`；四处下发点统一走 `apply_engine_intents`（SetRate → Load → SetVolume → 必要时 Pause） | **新冒烟** `start_offset_smoke`：8s 素材、请求 5000ms 起播 → `首个 Position: 5000ms`、`wall=3.12s`、`max_pos=8000ms`（若从头播会是 ~8s）。另覆盖中文文件名 `八秒.wav` |
| P1-13 `repeat='cycle'` 落空 | `player_set_repeat` 支持 `off → all → one → off` 三态轮转，未知值 `E_INVALID_ARG` | actor 测试通过；契约测试覆盖参数名 |
| P1-14 队尾 Next / 删到队尾不下发 Stop（引擎继续放） | Actor 新增 `pending_stop`，`Next/Prev/Ended/ReplaceAll` 的边界分支置位，`apply_engine_intents` 收尾下发 `Stop` | `actor` 15 条测试全绿 |
| P1-15 引擎事件不带身份 → 曲尾 500ms 内按 Next 连跳两首 | **用 token 而非 seq**：`Load` 携带 `token`，`Loaded/Position/Eof/StateChanged/Error` 全部原样回显，宿主泵按 `token == actor.state().token` 丢弃过期回报（管道本身有序，seq 提供不了额外信息；ADR-0003 的 generation 就是 token）。`PROTOCOL_VERSION` 升 2 并更新形状锚点 | `proto::wire_roundtrips` / `wire_shapes_are_stable` / `incompatible_protocol_is_detectable`；`supervisor_smoke` 握手 `protocol=2` |
| P1-16 tap 推源帧 → 变速时频谱丢帧 | tap 改推**重采样后的输出帧**（`resample_take` 同步写单声道 `tap_mono`，`tap.push_mono`） | `tap_disabled_pushes_nothing`、`tap_pushes_mono_average`、`push_mono_is_rate_independent`（原两条只断言 `dropped_count()==0`，环未满时恒真 = 假绿） |
| P1-17 `uiState` 参数名不匹配（永不落盘） | Rust 参数改 `state`；并新增 `ipc_contract` 测试：UI 每个 `call('cmd', {key})` 的 key 必须能映射到该命令的 Rust 参数（camelCase→snake_case），名字不同直接红 | `ipc_contract::every_ui_argument_maps_to_a_rust_parameter` |
| P1-18 歌词 DTO 缺 `found`、`time_ms` | `ParsedLyrics.found`、`LyricLine` 加 `rename_all="camelCase"`；契约锚点测试锁形状 | `lyrics::shape_tests::lyrics_serialize_matches_contract` |
| P1-19 歌词 loader 早退不回填 | **UI 项**（`lyrics-loader.ts`），随 UI 重做 | — |

---

## 三、报告之外新发现并修掉的问题

| 问题 | 为什么危险 | 修法 |
|---|---|---|
| `ids::playlist_item_id` 由 `(歌单, 曲目, position)` 派生 | position 会重排，而 id 写入后不变 → "删掉第 0 项再把同一首加回来"新行 id 与残留行撞主键，`INSERT OR IGNORE` **静默吞掉**（`added=1` 但列表没变） | 改一次性 id（纳秒+进程内自增做盐走 v5）；`re_add_after_remove_inserts_a_new_row`、`playlist_item_ids_are_unique`（2000 次不重复） |
| `playlists` 没有排序列，`playlist_reorder` 无处落盘 | 侧栏拖动重启即失效；新建歌单按 created_at 插到中间 | 新增迁移 `0002_playlist_order.sql`（`sort_position` + 按 `(created_at,id)` 稠密回填，升级后可见顺序不跳动）；列表按 `(sort_position, created_at)` 排 |
| `TrackRow` 缺 `availability`；`library_stats` 的 `locations`/`missingLocations` 是写死的 `1`/`0` | 侧栏永远不提示"位置缺失"，用户看着满列表的歌其实一半文件不在盘上 | DTO 补 `availability`；两个统计改真值（对应旧仓库 `media_locations` 两列） |
| `list_tracks` 每页都算 COUNT，而 COUNT 带着 3 个 LEFT JOIN | 5 万行 15 万次索引探测 → **22.7ms/次**，首屏 31.6ms 全是它 | 拆 `build_where`/`build_count`，COUNT 用最小 FROM（只有搜索谓词引用 `ar.name_norm`/`al.title_norm` 时才带 JOIN）：**22.7ms → 38µs** |
| 性能验收测试 `perf_50k` 根本跑不起来 | seed 的 INSERT 少 `path_lower`（NOT NULL）→ `#[ignore]` 让它一直没人跑；同时"深分页"用 `Mode::Cursor` 传 offset（cursor 模式不读 offset，三次测的都是首页），游标用例手写不存在的标题（测空页） | 补列、改 `Mode::Offset`、游标取库里真实中位 `title_norm` 并断言满页 |
| 线协议字段拼法混用（`start_ms` vs `positionMs`） | 同一份协议两种命名，人肉读日志容易误判 | `EngineCommand` 加 `rename_all_fields="camelCase"`，与 `EngineEvent` 对齐 |

---

## 四、E2E 验证矩阵

### 4.1 单元 / 契约测试（`cargo test --workspace`）

```
aiya（宿主）        5 passed   ← 进度节流 + camelCase 映射
aiya（契约）        4 passed   ← ipc_contract：命令注册 / 参数映射 / 反向漂移
aiya-audio         18 passed   ← proto 形状锚点、spectrum tap、resample …
aiya-data          70 passed   ← repo/scan/queue/lyrics/beets_sync/playlist/history/db
  └ vertical        1 passed   ← 扫描→查询→歌单 全链路
aiya-player        15 passed   ← Actor 状态机
合计              114 passed / 0 failed
```

`cargo build --workspace --all-targets`：**0 warning 0 error**。
（`cargo clippy` 在本机未安装组件，`rustup component add clippy` 需联网，未执行 —— 见遗留项。）

### 4.2 真机冒烟（WASAPI 出声，非 mock）

| 冒烟 | 内容 | 输出 |
|---|---|---|
| `engine_smoke` | 2s 素材装载 → 出声 → EOF | `max_pos=2000ms eof=true` |
| `start_offset_smoke`（新） | 8s 素材、起点 5000ms | `首个 Position: 5000ms`、`wall=3.12s` |
| `switch_smoke` | 播放中途 `Load` 切歌 | `Loaded(second): 1000ms` → 播到 EOF |
| `rate_smoke` | 2.0× 变速 | 2000ms 素材 1.09s 播完，`max_pos=2000ms` |
| `supervisor_smoke` | sidecar 握手/心跳/看门狗 | `握手完成 protocol=2`、`PING/PONG OK`、`wall=2.12s` |

### 4.3 beets 五场景（`tools/smoke_apply.py`，隔离 BEETSDIR）

```
[1] 只写标签 OK（文件未移动）
[2] 复制进库 OK: …\lib\Non-Album\测试艺人\A曲目 01.wav
[3] 目标冲突不覆盖 OK
[4] 移动进库 OK
[5] 取消 OK: ['cancelAck', 'progress', 'progress', 'cancelled']
ALL PASS
```

中文路径/标题全程贯穿（`中音测试 440Hz.wav`、`测试艺人`、`测试专辑`），
且脚本显式设了 `PYTHONIOENCODING` —— 另单独验证过**不设**该变量时 sidecar 自身
`reconfigure` 也能产出合法 UTF-8。

### 4.4 5 万首性能（修正口径后，`--release`）

| 项目 | 本轮 | 旧基线 / 修复前 |
|---|---|---|
| 灌 5 万行 | 1.6s | — |
| 首屏 200 首（cursor） | **0.80ms** | 修复前 31.6ms（COUNT 带 JOIN） |
| 游标续页（真实中位游标） | **2.2ms** | 旧基线 2.3ms ✓（此前 2.6ms 是"测空页"得来的） |
| offset 深分页 @49800 | 39.1ms | offset 天然 O(offset)，作兜底；UI 曲库列表必须用 cursor |
| 搜索"晴天"（7143 命中） | 28.7ms | 搜索谓词必须 JOIN，属预期 |

---

## 五、与审查建议的偏差（公正记录）

1. **P1-15 用 token 而非新增 seq**：管道是单一有序流，seq 只能发现"乱序"（不可能发生），
   而 token 直接回答"这条回报属于哪一次装载"——后者才是"连跳两首"的成因。
   ADR-0003 里 generation 与 token 是同一个概念，不另造字段。
2. **P1-19（歌词 loader 早退）与 P0-5（空格双派发）移交 UI 重做**：都在 `ui/` 内，
   本轮约定不动 UI；后端侧已把 P1-18 的 DTO 形状修好，重做时按新契约实现即可。
3. **泵级事件序列测试只做了一半**：`position_emit_due` 已抽成纯函数并测；
   完整的"构造 EngineEvent 序列 → 断言 emit 次数与下发命令序列"需要 `AppHandle`/`AppState`，
   当前泵逻辑与 Tauri 强耦合。建议 UI 重做后把泵主体抽成 `Pump`（输入 EngineEvent 流 +
   可注入 emitter trait）再补，否则测试要起整个 Tauri 运行时。
4. **`cargo clippy` 未执行**：本机没装组件且需要联网；已用 `cargo build --all-targets`
   保证 0 warning。

---

## 六、遗留（按建议优先级）

| 项 | 位置 | 说明 |
|---|---|---|
| 泵级测试 | `lib.rs` 事件泵 | 见偏差 3，需要先抽 `Pump` |
| 歌词负缓存（P1-19 的后端一半） | `lyrics_get` | 每曲都文件探查；影响 ~1ms/次，等 UI 重做后按需加 |
| 音量 ramp（docs/04 §3.4） | `engine.rs` | 音量跳变无逐样本过渡（会有咔哒声风险） |
| `capabilities/default.json` 过宽、`csp: null` | `src-tauri/` | 打包前必须收 |
| 封面缓存 `immutable, max-age=1y` + URL 只含 id | 宿主 http 响应头 | 换封面后永久显示旧图 |
| 设置四项未接线（preventSleep/autoplayOnStart/rememberPosition/normalizeVolume） | `lib.rs` | 只有默认值字符串 |
| 诊断页标签 "Electron"、三段硬编码 | `lib.rs` 诊断命令 | 值与标签对不上 |
| 密度设置无 CSS 消费者、`--row-height` 死变量 | `ui/` | 随 UI 重做 |

---

## 七、一句话结论

后端从"能跑但接线错位"变成"契约有锚点、数据有事务、播放身份有 generation"：
**114 条测试 + 5 个真机冒烟 + 5 个 beets 场景全绿**，5 万首首屏 0.80ms、游标续页 2.2ms（与旧基线持平）。
UI 可以开始按新设计方案重做了 —— 后端契约（命令名、参数名、DTO 形状）现在有 `ipc_contract` 测试兜着，
重做时改契约会立刻红。
