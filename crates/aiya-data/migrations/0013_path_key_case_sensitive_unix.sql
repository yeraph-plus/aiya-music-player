-- path_key 语义切换的存量重推导：键从小写基线改为原路径（unix 不做任何
-- 变换，大小写敏感文件系统上 A.flac 与 a.flac 是两个文件）。等价性：旧键
-- （小写）恒等于新键经折叠层（unix 对账原样），本迁移只改存储形状、不改
-- 对账结果；对账折叠层见 repo.rs 的 path_key_folded。
UPDATE tracks SET path_lower = path;
