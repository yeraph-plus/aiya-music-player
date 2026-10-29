-- 0004：history.played_at 索引 + 最近播放保留周期（2026-10-28）。
--
-- 为什么只有索引、没有周期列：保留周期是**设置**（app_settings 的
-- `historyDays`，0=永久），不是 schema 事实——裁剪由宿主在启动 /
-- 每次写历史 / 设置变更时执行（`history::prune`），DB 只需要让
-- `DELETE FROM history WHERE played_at < ?` 走索引而不是全表扫。
--
-- 背景：history 此前只增不删（每曲播完插一行），库大的重度用户
-- 一年能堆几十万行；"最近播放" tab 与常听榜的语义本来就是"还在
-- 听什么"，全量历史没有消费方。
CREATE INDEX IF NOT EXISTS idx_history_played_at
  ON history (played_at);
