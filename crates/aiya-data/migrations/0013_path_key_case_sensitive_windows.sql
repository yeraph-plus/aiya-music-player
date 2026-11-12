-- path_key 语义切换的存量重推导：键从小写基线改为分隔符归一、大小写保留。
-- 等价性：旧键 = path_key 旧实现（小写）恒等于新键经折叠层（Windows 大小写
-- 不敏感对账），所以本迁移只改存储形状、不改对账结果；对账折叠层见
-- repo.rs 的 path_key_folded（全仓唯一折叠出口）。tracks.path 的取值来自
-- 扫描器 PathBuf（Windows 为反斜杠形态），REPLACE 兜住混入的正斜杠。
UPDATE tracks SET path_lower = REPLACE(path, '/', '\');
