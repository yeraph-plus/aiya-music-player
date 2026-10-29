-- 0003_smart_playlist.sql — 动态歌单 + 流派列（docs/40 §1 方案 A 拍板版）。
--
-- 为什么是两条线一起加：
--   * tracks.genre / genre_norm：智能歌单的标签数据链路（beets DumpItem 里
--     本来就有 genre，client 侧此前没接——补上后 sync 直接迁移）。
--   * playlists.kind / rule_json：智能歌单的规则存储。规则**不物化曲目**，
--     打开歌单时对本地 SQLite 实时查询（smart.rs 把 rule_json 编译成 SQL）。
--
-- 兼容性：ALTER TABLE ADD COLUMN 对存量行填默认值——kind 全部落 'manual'，
-- 老歌单行为零变化。特征字段（能量/效价/舞曲性）**有意不加**：没有可靠的
-- 数据来源（Spotify audio features API 已不对新应用开放、beets 无内置、
-- 本地 librosa 计算重），rule_json 是自由 JSON，未来加字段不需要迁移。
ALTER TABLE tracks ADD COLUMN genre TEXT;
ALTER TABLE tracks ADD COLUMN genre_norm TEXT;
ALTER TABLE playlists ADD COLUMN kind TEXT NOT NULL DEFAULT 'manual';
ALTER TABLE playlists ADD COLUMN rule_json TEXT;

CREATE INDEX idx_tracks_genre ON tracks(genre_norm);
