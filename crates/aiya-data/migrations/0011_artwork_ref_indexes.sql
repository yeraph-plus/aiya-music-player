-- 0011_artwork_ref_indexes.sql — 封面 GC 的引用探测加索引
--
-- gc_unreferenced（artwork.rs）用 NOT EXISTS 探测 tracks/albums/artists 三处
-- artwork_id 引用。tracks.artwork_id 在 0001 基线里就有索引（idx_tracks_artwork），
-- albums/artists 的没有——每条 artwork 行都全表扫两张表。5 万曲库约 1 万张
-- 去重封面 × (专辑 + 艺人) ≈ 数千万行探测，增量扫描收尾额外卡几百 ms 到秒级
-- （发布前审查 P2-2）。
--
-- 实体封面回填（backfill）的 UPDATE ... WHERE artwork_id = ? 同样吃这两个索引。

CREATE INDEX IF NOT EXISTS idx_albums_artwork  ON albums(artwork_id);
CREATE INDEX IF NOT EXISTS idx_artists_artwork ON artists(artwork_id);
