-- 0005：artists.name_norm / albums.title_norm 排序索引。
--
-- 为什么需要：facets 浏览（library_artists / library_albums）与 repo 的
-- Artist/Album 排序都在这两个 norm 列上 ORDER BY，没有索引时每一页都要
-- temp B-tree 全表排序；smart.rs / repo.rs 筛选器里按 name_norm/title_norm
-- 的 IN 子查询与 LIKE 同样只能全表扫。
--
-- 第二列带 id：查询统一是 `ORDER BY xxx_norm ASC, id ASC`（稳定次序），
-- 复合索引把整个排序吃掉，同键并列不再额外排序。
--
-- 不改 0001_init.sql：现有开发库靠本迁移获得索引（0001 是已应用的基线）。
CREATE INDEX IF NOT EXISTS idx_artists_name_norm ON artists (name_norm, id);
CREATE INDEX IF NOT EXISTS idx_albums_title_norm ON albums (title_norm, id);
