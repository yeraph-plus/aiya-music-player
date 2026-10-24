-- 0002_playlist_order.sql — 歌单侧栏顺序
--
-- 为什么需要这一列：`playlist_reorder(from, to)` 是"拖动侧栏改变歌单顺序"，
-- 但 v1 基线里 playlists 只有 created_at —— 靠 created_at 假装顺序的话，
-- 拖动结果完全无法持久化（重启就回到创建顺序），且新建歌单会按创建时间
-- 插到中间而不是末尾（旧仓库规格：新建永远排最后）。
--
-- 回填策略：按 (created_at, id) 排出稠密的 0..n-1 —— 与 v1 之前
-- "ORDER BY created_at ASC" 的可见顺序逐行一致（升级后列表不跳动），
-- 且与 reorder 写入的 0..n-1、create 取的 MAX+1 三者语义统一。
ALTER TABLE playlists ADD COLUMN sort_position INTEGER NOT NULL DEFAULT 0;

UPDATE playlists SET sort_position = (
  SELECT COUNT(*) FROM playlists p2
   WHERE p2.created_at < playlists.created_at
      OR (p2.created_at = playlists.created_at AND p2.id < playlists.id)
);
