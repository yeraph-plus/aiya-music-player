# ledger · 数据层（data-layer）

> 代码位置：crates/aiya-data/、crates/aiya-beets-store/
> 相关档案：docs/02、03、29

## 条目（新→旧）

<!-- 每条一行：- YYYY-MM-DD · 代码位置 · 事实/取舍 · 来龙去脉（可选） -->

- 2026-11-04 · migrations/、db.rs `MIGRATIONS` · 迁移链第二次压平：0002..0011 十个增量全部吸收进 0001 单文件基线（列按 ALTER 追加序折叠；0006/0007 数据回填与 0008/0009 净零对不吸收，见 0001 头注吸收清单）；本机开发库以 `PRAGMA user_version = 1` 回拨消化（结构等价已脚本实证：10 表 × 列/FK/UNIQUE/CHECK + 14 索引 + 种子数据全等）· 项目未发布无存量用户库，压平是既定纪律（此前 0001+0002+0003 同例）。教训一条：tauri dev 的 crate 监视会在改 db.rs 时**自动用新代码重启应用**——旧库 v11 > 新链长 1 触发版本守卫留底重建，用户库一度被隔离成空库；恢复路径 = 备份改名回来 + user_version 回拨（留底机制本身工作正常，`library.sqlite.v11-*.bak` 完整）。发布后此纪律冻结：改结构只允许增量迁移
- 2026-11-04 · crates/aiya-data/src/scanner.rs upsert · tracks 音频规格六列各归各位；INSERT 里 codec 恒占 NULL（编码名归 beets 同步写，见 `rescan_does_not_wipe_codec`）· 此前 VALUES 少占一位，container..channels 五参数整体左移一格——bitrate 列存成声道数（全库恒 2，UI 码率徽章永远 "2 kbps"）、采样率串进 container、码率串进 bits_per_sample；数值列错位无声，此前没有任何测试读过这六列。回归测试 `scan_writes_audio_columns_in_their_own_slots` 守住；存量 1381 行脏数据用 `UPDATE tracks SET mtime = 0` 迫使重扫全量重解析修复（mtime=0 本就是"当初没读到"哨位，unchanged 预筛必跳过）
