-- v3：路径大小写不敏感对账键（Windows 文件系统语义）。
-- 为什么：tracks.path 的 UNIQUE 只约束精确相等，"A.mp3" 与 "a.mp3"
-- 会被当成两个文件重复入库；扫描/对账需要统一的小写键。
-- 注意：ids::track_id 的派生逻辑不动（保持已有 id 稳定），
-- 本列只作对账/去重键，不参与 ID 生成。

ALTER TABLE tracks ADD COLUMN path_lower TEXT;

UPDATE tracks SET path_lower = lower(path);

-- 历史库里可能已有大小写变体并存（旧 UNIQUE 管不住）：
-- 每组 path_lower 保留 rowid 最小（最早入库）的一行，其余删除。
-- playlist_items / queue_items / history 的 FK 均为 ON DELETE CASCADE，自动跟随。
DELETE FROM tracks
 WHERE rowid NOT IN (SELECT MIN(rowid) FROM tracks GROUP BY path_lower);

CREATE UNIQUE INDEX idx_tracks_path_lower ON tracks(path_lower);
