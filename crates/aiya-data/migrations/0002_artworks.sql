-- v2：封面表（tracks.artwork_id 列 0001 已建好）。
-- 为什么按 content_hash 去重：同一专辑几十首共享一张嵌入图，
-- 存储与缩放都只做一次；track→artwork 是多对一。
CREATE TABLE artworks (
  id           TEXT PRIMARY KEY,            -- aw_<hash 前 24>
  content_hash TEXT NOT NULL UNIQUE,
  mime         TEXT NOT NULL,
  width        INTEGER,
  height       INTEGER,
  cache_key    TEXT NOT NULL,               -- originals/ 下的文件名
  created_at   INTEGER NOT NULL
);

CREATE INDEX idx_tracks_artwork ON tracks(artwork_id);
