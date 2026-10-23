-- 0001_init.sql — AIYA Music schema v1 基线（全新建库，无历史迁移链）
-- 设计依据 docs/02 §5.2：
--   * tracks 不暴露路径给 UI（locator/文件事实只存在于本库）
--   * 标签写操作归 beets，tracks 只存播放/展示需要的字段
--   * 队列规范化（queue_state 单行 + queue_items 位置表），不写 JSON 整行
--   * 搜索用 title_norm + LIKE（FTS5 的两字中文坑已验证，不上）

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
  size_bytes  INTEGER NOT NULL DEFAULT 0,
  mtime       INTEGER NOT NULL DEFAULT 0,
  availability TEXT NOT NULL DEFAULT 'online' CHECK (availability IN ('online','offline','missing')),
  beets_item_id INTEGER                 -- 关联回 beets；可空不唯一
);

CREATE INDEX idx_tracks_order       ON tracks(title_norm, id);
CREATE INDEX idx_tracks_added       ON tracks(added_at, id);
CREATE INDEX idx_tracks_artist      ON tracks(artist_id);
CREATE INDEX idx_tracks_album       ON tracks(album_id);

-- ---------------------------------------------------------------- 歌单

CREATE TABLE playlists (
  id         TEXT PRIMARY KEY,
  name       TEXT NOT NULL,
  name_norm  TEXT NOT NULL,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);

-- 独立行 ID + 稳定 position：同一曲目可重复加入歌单（契约要求）
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

CREATE TABLE queue_items (
  position INTEGER PRIMARY KEY,
  track_id TEXT NOT NULL REFERENCES tracks(id) ON DELETE CASCADE
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

-- ---------------------------------------------------------------- 设置

CREATE TABLE settings (
  key        TEXT PRIMARY KEY,
  value_json TEXT NOT NULL
);

INSERT INTO settings (key, value_json) VALUES
  ('library.sources', '[{"id":"default","kind":"local","enabled":true,"displayPath":null}]');
