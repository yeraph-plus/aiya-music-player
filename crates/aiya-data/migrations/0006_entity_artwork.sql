-- 实体封面一次性回填（2026-10-30）。
--
-- albums/artists 的 artwork_id 列在 0001 就存在、facets 查询也在读，
-- 但此前没有任何写入方（实测存量库 albums 0/1581、artists 0/1226 非
-- NULL）——专辑宫格因此只能显示占位图，内页走"首曲封面"回落（首曲
-- 被删则封面跳变）。
--
-- 聚合口径：曲目中出现次数最多的封面，并列取 artwork_id 最小（稳定）；
-- 曲目全部无封面时清 NULL。此后由扫描收尾与 beets 同步收尾持续维护
-- （artwork::refresh_entity_artwork，必须在封面 GC 之前跑）。

UPDATE albums SET artwork_id = (
    SELECT t.artwork_id FROM tracks t
    WHERE t.album_id = albums.id AND t.artwork_id IS NOT NULL
    GROUP BY t.artwork_id ORDER BY COUNT(*) DESC, t.artwork_id ASC LIMIT 1
);

UPDATE artists SET artwork_id = (
    SELECT t.artwork_id FROM tracks t
    WHERE t.artist_id = artists.id AND t.artwork_id IS NOT NULL
    GROUP BY t.artwork_id ORDER BY COUNT(*) DESC, t.artwork_id ASC LIMIT 1
);
