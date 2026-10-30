-- 0010_queue_transient.sql — 队列多态：能放"不在曲库里的东西"（docs/52 §6）
--
-- 为什么要重建表：`queue_items.track_id` 是 `NOT NULL REFERENCES tracks(id)`
-- 且 `PRAGMA foreign_keys = ON`。拖进来的本地文件 / 直播流一进队列就外键失败，
-- `save_queue_full` 的整笔 IMMEDIATE 事务回滚，上层只有一句 warn —— UI 完全
-- 无感，重启后队列退回上一次成功保存的样子。
--
-- 为什么**保留**外键、只允许 NULL：SQLite 对 NULL 不校验外键，而
-- `ON DELETE CASCADE` 只对命中的行生效。于是库内曲目被扫描淘汰时仍自动清掉，
-- transient 行不受影响 —— 比"整条外键拿掉、改应用层校验"少一处可能忘记校验
-- 的地方（少一处就多一个"队列里留着一行指向不存在的曲目"）。
--
-- payload 只存最小显示字段（path / title / artist / durationMs），不给 transient
-- 行进 artworks（没有 artwork 生命周期，docs/52 §6.2）。
--
-- CHECK 把"kind 与 track_id 是否为空"锁成同一件事：写反了（kind=track 却
-- track_id NULL）在插入时就炸，而不是等到读出来才显示成空标题。

CREATE TABLE queue_items_new (
  position INTEGER PRIMARY KEY,
  kind     TEXT NOT NULL DEFAULT 'track',              -- 'track' | 'file' | 'stream'
  track_id TEXT REFERENCES tracks(id) ON DELETE CASCADE,
  payload  TEXT,
  CHECK ((kind = 'track') = (track_id IS NOT NULL))
);

INSERT INTO queue_items_new (position, kind, track_id)
  SELECT position, 'track', track_id FROM queue_items;

DROP TABLE queue_items;
ALTER TABLE queue_items_new RENAME TO queue_items;
