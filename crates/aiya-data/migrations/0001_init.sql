-- 0001_init.sql — AIYA Music schema v1 基线（单一迁移，第二次压平：
-- 原 0001+0002+0003 曾压成旧基线，0002..0011 十个增量再次吸收进来；
-- 项目未发布、无存量库需要保留，按"新库第一版即最终形态"直接建全量结构。
-- 吸收清单（列按 ALTER 追加序折叠，索引按编号序）：
--   * 0002 → playlists.sort_position（回填 UPDATE 是存量数据操作，不吸收）
--   * 0003 → tracks.genre/genre_norm、playlists.kind/rule_json、idx_tracks_genre
--   * 0004 → idx_history_played_at
--   * 0005 → idx_artists_name_norm / idx_albums_title_norm
--   * 0006/0007 → 一次性数据回填/清理，新库无数据，不吸收
--   * 0008/0009 → track_enrichment 建后即弃的净零对，不吸收
--   * 0010 → queue_items 重建形（kind/track_id 可空 + payload + CHECK）
--   * 0011 → idx_albums_artwork / idx_artists_artwork
-- 设计依据 docs/02 §5.2：
--   * tracks 不暴露路径给 UI（locator/文件事实只存在于本库）
--   * 标签写操作归 beets，tracks 只存播放/展示需要的字段
--   * 队列规范化（queue_state 单行 + queue_items 位置表），不写 JSON 整行
--   * 搜索用 title_norm + LIKE（FTS5 的两字中文坑已验证，不上）
--   * 路径对账键 path_lower（Windows 大小写不敏感语义）直接内联，
--     不走 ALTER+回填+去重的历史包袱
--   * artworks 按内容哈希去重内联（同一专辑几十首共享一张嵌入图）

PRAGMA foreign_keys = ON;

-- ---------------------------------------------------------------- 实体

CREATE TABLE artists (
  id        TEXT PRIMARY KEY,
  name      TEXT NOT NULL,
  name_norm TEXT NOT NULL,
  artwork_id TEXT
);

CREATE TABLE albums (
  id         TEXT PRIMARY KEY,
  title      TEXT NOT NULL,
  title_norm TEXT NOT NULL,
  artist_id  TEXT REFERENCES artists(id) ON DELETE SET NULL,
  year       INTEGER,
  artwork_id TEXT
);

-- 一首曲目 = 一个逻辑实体；文件事实（path/size/mtime）只在这里，不出 DTO。
CREATE TABLE tracks (
  id          TEXT PRIMARY KEY,
  title       TEXT NOT NULL,
  title_norm  TEXT NOT NULL,
  artist_id   TEXT REFERENCES artists(id) ON DELETE SET NULL,
  album_id    TEXT REFERENCES albums(id) ON DELETE SET NULL,
  duration_ms INTEGER,
  track_no    INTEGER,
  disc_no     INTEGER,
  year        INTEGER,
  added_at    INTEGER NOT NULL,          -- UTC unix 秒
  artwork_id  TEXT,
  gain_db     REAL,                      -- ReplayGain track gain（标签）
  -- 音频规格（media_assets 不分离，直接进 tracks；列表查询不带出，单曲查询用）
  codec          TEXT,
  container      TEXT,
  sample_rate    INTEGER,
  bits_per_sample INTEGER,
  bitrate        INTEGER,
  channels       INTEGER,
  -- 统计反范式列：记录历史时同步更新，列表查询不做 history 聚合
  play_count     INTEGER NOT NULL DEFAULT 0,
  last_played_at INTEGER,
  -- 文件事实（扫描对账用；UI 不可见）
  path        TEXT NOT NULL UNIQUE,
  -- 对账/去重键（lower(path)）。注意：ids::track_id 的派生逻辑不动
  --（不参与 ID 生成），本列只作大小写不敏感匹配。
  path_lower  TEXT NOT NULL UNIQUE,
  size_bytes  INTEGER NOT NULL DEFAULT 0,
  mtime       INTEGER NOT NULL DEFAULT 0,
  availability TEXT NOT NULL DEFAULT 'online' CHECK (availability IN ('online','offline','missing')),
  beets_item_id INTEGER,                 -- 关联回 beets；可空不唯一
  -- 标签 genre：智能/动态歌单的规则数据源（beets DumpItem 同步迁移）
  genre        TEXT,
  genre_norm   TEXT
);

CREATE INDEX idx_tracks_order       ON tracks(title_norm, id);
CREATE INDEX idx_tracks_added       ON tracks(added_at, id);
CREATE INDEX idx_tracks_artist      ON tracks(artist_id);
CREATE INDEX idx_tracks_album       ON tracks(album_id);
CREATE INDEX idx_tracks_artwork     ON tracks(artwork_id);
CREATE INDEX idx_tracks_genre       ON tracks(genre_norm);

-- ---------------------------------------------------------------- 封面

CREATE TABLE artworks (
  id           TEXT PRIMARY KEY,            -- aw_<hash 前 24>
  content_hash TEXT NOT NULL UNIQUE,
  mime         TEXT NOT NULL,
  width        INTEGER,
  height       INTEGER,
  cache_key    TEXT NOT NULL,               -- originals/ 下的文件名
  created_at   INTEGER NOT NULL
);

-- ---------------------------------------------------------------- 歌单

CREATE TABLE playlists (
  id         TEXT PRIMARY KEY,
  name       TEXT NOT NULL,
  name_norm  TEXT NOT NULL,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  -- 侧栏顺序：稠密 0..n-1（reorder 整体重写；create 取 MAX+1 追加到末尾）
  sort_position INTEGER NOT NULL DEFAULT 0,
  -- 歌单种类：'manual' | 'smart'（规则存 rule_json；动态歌单按规则物化
  -- 进 playlist_items，与普通歌单能力相同——见 0007 模型切换）
  kind         TEXT NOT NULL DEFAULT 'manual',
  rule_json    TEXT
);

-- 独立行 ID + 稠密 position（0..n-1，每次结构变化同事务重排）。
-- 行 ID 是一次性身份（不按 position 派生：位置会重排，派生 id 会与残留行撞主键）；
-- 同一首曲目在同一歌单里只出现一次（加歌去重，契约 PlaylistAddResult.skipped 即此语义）。
CREATE TABLE playlist_items (
  id          TEXT PRIMARY KEY,
  playlist_id TEXT NOT NULL REFERENCES playlists(id) ON DELETE CASCADE,
  track_id    TEXT NOT NULL REFERENCES tracks(id) ON DELETE CASCADE,
  position    INTEGER NOT NULL,
  added_at    INTEGER NOT NULL
);

CREATE INDEX idx_playlist_items ON playlist_items(playlist_id, position);

-- ---------------------------------------------------------------- 队列（规范化）

CREATE TABLE queue_state (
  id            INTEGER PRIMARY KEY CHECK (id = 1),
  current_index INTEGER NOT NULL DEFAULT -1,
  position_ms   INTEGER NOT NULL DEFAULT 0,
  repeat_mode   TEXT    NOT NULL DEFAULT 'off' CHECK (repeat_mode IN ('off','all','one')),
  shuffle       INTEGER NOT NULL DEFAULT 0,
  volume        REAL    NOT NULL DEFAULT 0.85,
  muted         INTEGER NOT NULL DEFAULT 0,
  rate          REAL    NOT NULL DEFAULT 1,
  updated_at    INTEGER NOT NULL
);

INSERT INTO queue_state (id, updated_at) VALUES (1, 0);

-- 队列多态：能放"不在曲库里的东西"（拖入的本地文件 / 直播流）。
-- 保留外键、只允许 NULL：SQLite 对 NULL 不校验外键，ON DELETE CASCADE 仍对
-- 命中的行生效——库内曲目被扫描淘汰时自动清掉，transient 行不受影响。
-- CHECK 把"kind 与 track_id 是否为空"锁成同一件事。
CREATE TABLE queue_items (
  position INTEGER PRIMARY KEY,
  kind     TEXT NOT NULL DEFAULT 'track',              -- 'track' | 'file' | 'stream'
  track_id TEXT REFERENCES tracks(id) ON DELETE CASCADE,
  payload  TEXT,
  CHECK ((kind = 'track') = (track_id IS NOT NULL))
);

-- ---------------------------------------------------------------- 播放历史

CREATE TABLE history (
  id         INTEGER PRIMARY KEY AUTOINCREMENT,
  track_id   TEXT NOT NULL REFERENCES tracks(id) ON DELETE CASCADE,
  played_at  INTEGER NOT NULL,
  seconds_played INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX idx_history_track ON history(track_id, played_at);
CREATE INDEX idx_history_time  ON history(played_at);
-- 保留周期裁剪（DELETE ... played_at < ?）走索引而不是全表扫
CREATE INDEX idx_history_played_at ON history (played_at);

-- ---------------------------------------------------------------- 设置

CREATE TABLE settings (
  key        TEXT PRIMARY KEY,
  value_json TEXT NOT NULL
);

INSERT INTO settings (key, value_json) VALUES
  ('library.sources', '[{"id":"default","kind":"local","enabled":true,"displayPath":null}]');

-- ---------------------------------------------------------------- 排序/引用索引
-- facets 浏览与各处 ORDER BY xxx_norm, id 走复合索引（同键并列不再额外排序）；
-- 封面 GC 的引用探测与实体封面回填走 artwork_id 索引（万级封面 × 两表全扫
-- 曾卡增量收尾几百 ms 到秒级）。

CREATE INDEX idx_artists_name_norm ON artists (name_norm, id);
CREATE INDEX idx_albums_title_norm ON albums (title_norm, id);
CREATE INDEX idx_albums_artwork    ON albums(artwork_id);
CREATE INDEX idx_artists_artwork   ON artists(artwork_id);
