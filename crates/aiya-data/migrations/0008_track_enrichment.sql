-- 0008：索引增强关联表（docs/45 §5.1）。
--
-- 一句话：beets 侧的增强流程（`enrich`）逐 (文件, 源) 回吐字段，**宿主**把它
-- 落到这张表上。表只存"分析结论 + 判定失效所需的两条文件事实"，不存路径原文
-- 之外的任何 tracks 副本——路径以 `path_lower` 为键（与 tracks 同口径），
-- 行存活与曲目存活彼此独立：曲库根被移除后这里仍留着历史结论，等 GC 收。
--
-- 为什么不挂 `tracks` 外键：`tracks` 行会被扫描器整行删掉再重建（改名/缺席），
-- 外键级联会把一份还算新鲜的分析结论一起带走；查询期 LEFT JOIN 反而更诚实
-- （没有曲目 → 查不到这条结论，但结论本身没丢）。
--
-- `source` 进主键：同一文件可以被多个源分析（xtractor 出数值特征、lastgenre
-- 出流派），单列主键会让后一个源把前一个源的整行覆盖掉。
CREATE TABLE track_enrichment (
  path_lower   TEXT NOT NULL,
  source       TEXT NOT NULL,
  -- 提升为可查列（全部来自 xtractor 真字段，见 docs/45 §3.2）
  bpm            REAL,
  danceability   REAL,
  -- 注意列名是 loudness 而不是 energy：xtractor 没有 energy，
  -- 语义最接近的是 average_loudness。
  loudness       REAL,
  mood_mirex     TEXT,               -- essentia 原始簇标签（mood_mirex_cluster_N）
  mood_label     TEXT,               -- 宿主对 7 个 mood_* 概率取 argmax 的结论
  mood_score     REAL,               -- 该类的概率
  -- 其余字段（含 lastgenre 的 genre）整体进 JSON：列是给人查的，JSON 是给溯源用的
  data         TEXT NOT NULL DEFAULT '{}',
  -- 失效判定用的文件事实，写入时从 tracks 现查（不快照 beets：beets 的库
  -- 不知道文件什么时候被外部工具改过）
  size_bytes   INTEGER NOT NULL,
  mtime        INTEGER NOT NULL,
  analyzed_at  INTEGER NOT NULL,     -- UTC unix 秒
  -- ok = 至少一个期望字段非空；empty = 跑通了但一个字段都没拿到
  --（不看退出码：插件静默不产出才是常态里的真正故障）
  status       TEXT NOT NULL DEFAULT 'ok' CHECK (status IN ('ok','empty')),
  tool_version TEXT,                 -- 分析它的插件/sidecar 版本，升级后可选择性重算
  PRIMARY KEY (path_lower, source)
);

CREATE INDEX idx_enrich_mood  ON track_enrichment(mood_label);
CREATE INDEX idx_enrich_dance ON track_enrichment(danceability);
