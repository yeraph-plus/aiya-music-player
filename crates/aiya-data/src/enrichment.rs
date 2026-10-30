//! 索引增强关联表（docs/45 §5.1–§5.2）：谁写、怎么判失效、怎么查。
//!
//! ## 边界：本模块只被宿主进程调用
//!
//! beets 侧的 `enrich` 流程产出 `(path, source, status, fields)` 事件，**宿主**
//! 收下来落这张表。sidecar 不连这个库，理由三条（docs/44 §9.1）：依赖边界门禁
//! 不许工具侧出现应用库的形状；`scanning` 互斥是宿主进程内的 `AtomicBool`，
//! 外部进程物理上拿不到；工具侧的写行逻辑对老版本应用会变成"隐形迁移器"。
//!
//! ## 失效判定：文件事实取自 `tracks`，不快照 beets
//!
//! beets 的库不知道文件什么时候被外部工具改过（mp3tag 改标签、foobar 重写
//! 增益），所以"结论还算不算数"只能拿**扫描器的观察**（`size_bytes` / `mtime`）
//! 来判：任一不等 ⇒ 过期 ⇒ 进工作清单重算。

use rusqlite::{params, Connection, OptionalExtension};
use serde_json::Value;

use crate::db::DbResult;

/// 写库的 source 规范名。历史上只有一个数值源，名字写在这里避免各处硬编码。
pub const SOURCE_XTRACTOR: &str = "xtractor";

/// 参与 argmax 的 7 个情绪概率。
///
/// 刻意**不含** `mood_mirex_cluster_N`：那是簇号文本，不是概率，混进来会让
/// "取最大概率"退化成字符串比较。字段名与原串都来自 xtractor 的真实输出
/// （docs/45 §3.2 的实测清单）。
pub const MOOD_CLASSES: [&str; 7] = [
    "mood_happy",
    "mood_sad",
    "mood_relaxed",
    "mood_aggressive",
    "mood_party",
    "mood_acoustic",
    "mood_electronic",
];

/// 关联键：与 `tracks.path_lower` 同口径。
///
/// 扫描器那一侧就是 `path.to_lowercase()`（scanner.rs），而 sidecar 报回来的
/// 路径可能带正斜杠（beets 内部按 POSIX 风格切片）。所以 Windows 上先把分隔符
/// 统一成 `\` 再小写——两处任缺一条，症状都是"分析完了但一条也对不上"。
pub fn path_key(path: &str) -> String {
    let unify = if cfg!(windows) {
        path.replace('/', "\\")
    } else {
        path.to_string()
    };
    unify.to_lowercase()
}

/// 从字段里取情绪 argmax：(标签, 概率)。
///
/// 只认 [`MOOD_CLASSES`] 里的键，且**至少要有一个**才给结论——一个都没有时
/// 返回 `None`，让 UI 显示"该文件没有情绪结论"而不是假装 `happy 0.0`。
pub fn argmax_mood(fields: &Value) -> Option<(String, f64)> {
    let obj = fields.as_object()?;
    let mut best: Option<(String, f64)> = None;
    for name in MOOD_CLASSES {
        let Some(score) = obj.get(name).and_then(Value::as_f64) else {
            continue;
        };
        // 严格大于：并列时保留先出现的类，保证同一批数据永远同一个结论
        let better = match &best {
            None => true,
            Some((_, b)) => score > *b,
        };
        if better {
            best = Some((name.to_string(), score));
        }
    }
    best
}

/// 工作清单里的一行（只有算清单需要的东西，不把 tracks 整行搬来搬去）。
#[derive(Debug, Clone, PartialEq)]
pub struct WorkItem {
    pub path: String,
    pub size_bytes: i64,
    pub mtime: i64,
}

/// 一张卡要显示的全部计数。
///
/// `work(include_empty)` 才是"点一下会算多少首"——`empty` 默认不重试
/// （docs/45 §5.2：否则每次分析都把上次静默失败的文件再跑一遍）。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Stats {
    /// 曲库里在线（`availability='online'`）的曲目数
    pub total: i64,
    /// 有结论且文件事实没变
    pub analyzed: i64,
    /// 跑过但一个字段都没拿到（且文件事实没变）
    pub empty: i64,
    /// 从来没分析过
    pub missing: i64,
    /// 有结论但 size/mtime 变了
    pub stale: i64,
    /// 关联行找不到对应曲目（曲库根被移走之后的残留）
    pub orphan: i64,
}

impl Stats {
    pub fn work(&self, include_empty: bool) -> i64 {
        self.missing + self.stale + if include_empty { self.empty } else { 0 }
    }
}

/// sidecar 回吐的一条结果。`status` 用协议原词（ok / empty / failed / skipped），
/// 落库前由 [`save`] 过滤——`failed`/`skipped` 不写行，见那里的说明。
#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub path: String,
    pub source: String,
    pub status: String,
    pub fields: Value,
}

/// 一批写入的结果。三个计数分开是刻意的：跳过原因不同，用户能做的事也不同
/// （`not_in_library` = 文件不在 beets 库里=补登没成功；`skipped` = 源没产出）。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Saved {
    pub written: usize,
    /// 状态不是 ok/empty（failed / skipped），不写行
    pub skipped: usize,
    /// 曲库里没有这个 path_lower（扫描器还没收录 / 路径已漂移），不写行
    pub not_in_library: usize,
    /// 状态是 ok 但一个可提升字段都没有（只进 JSON，不影响计数，供诊断）
    pub empty_fields: usize,
}

/* ------------------------------------------------------------------ 读 */

/// 计数（一次聚合查询 + 一次孤儿计数）。
pub fn stats(conn: &Connection, source: &str) -> DbResult<Stats> {
    let (total, analyzed, empty, missing, stale) = conn.query_row(
        "SELECT COUNT(*),
                COALESCE(SUM(CASE WHEN e.path_lower IS NOT NULL
                                   AND e.size_bytes = t.size_bytes AND e.mtime = t.mtime
                                   AND e.status = 'ok' THEN 1 ELSE 0 END), 0),
                COALESCE(SUM(CASE WHEN e.path_lower IS NOT NULL
                                   AND e.size_bytes = t.size_bytes AND e.mtime = t.mtime
                                   AND e.status = 'empty' THEN 1 ELSE 0 END), 0),
                COALESCE(SUM(CASE WHEN e.path_lower IS NULL THEN 1 ELSE 0 END), 0),
                COALESCE(SUM(CASE WHEN e.path_lower IS NOT NULL
                                   AND (e.size_bytes <> t.size_bytes OR e.mtime <> t.mtime)
                                  THEN 1 ELSE 0 END), 0)
         FROM tracks t
         LEFT JOIN track_enrichment e
                ON e.path_lower = t.path_lower AND e.source = ?1
         WHERE t.availability = 'online'",
        [source],
        |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, i64>(2)?,
                r.get::<_, i64>(3)?,
                r.get::<_, i64>(4)?,
            ))
        },
    )?;
    let orphan: i64 = conn.query_row(
        "SELECT COUNT(*) FROM track_enrichment e
         WHERE e.source = ?1
           AND NOT EXISTS (SELECT 1 FROM tracks t WHERE t.path_lower = e.path_lower)",
        [source],
        |r| r.get(0),
    )?;
    Ok(Stats { total, analyzed, empty, missing, stale, orphan })
}

/// 工作清单（docs/45 §5.2 那条 SQL）。
///
/// `include_empty=true` 是"强制重算"：把 `status='empty'` 的也捞回来。
/// 顺序按 `path_lower`：同一目录的文件相邻，sidecar 侧补登时命令行聚类更好，
/// 也保证分批/续跑是确定性的。
pub fn worklist(
    conn: &Connection,
    source: &str,
    include_empty: bool,
    limit: usize,
) -> DbResult<Vec<WorkItem>> {
    let sql = format!(
        "SELECT t.path, t.size_bytes, t.mtime
         FROM tracks t
         LEFT JOIN track_enrichment e
                ON e.path_lower = t.path_lower AND e.source = ?1
         WHERE t.availability = 'online'
           AND (e.path_lower IS NULL
                OR e.size_bytes <> t.size_bytes
                OR e.mtime <> t.mtime{}
                )
         ORDER BY t.path_lower
         LIMIT ?2",
        if include_empty { " OR e.status = 'empty'" } else { "" }
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![source, limit as i64], |r| {
        Ok(WorkItem {
            path: r.get(0)?,
            size_bytes: r.get(1)?,
            mtime: r.get(2)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// 查询命中行（曲库页/智能歌单用）。
#[derive(Debug, Clone, PartialEq)]
pub struct Match {
    pub track_id: String,
    pub mood_label: Option<String>,
    pub mood_score: Option<f64>,
    pub bpm: Option<f64>,
    pub danceability: Option<f64>,
    pub loudness: Option<f64>,
    pub analyzed_at: i64,
}

/// 查询条件。全部可空 = 不限制；`limit` 由调用方裁剪（repo 侧统一 1..=500）。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Filter {
    pub source: Option<String>,
    pub mood: Option<String>,
    pub dance_min: Option<f64>,
    pub dance_max: Option<f64>,
    pub bpm_min: Option<f64>,
    pub bpm_max: Option<f64>,
    pub limit: Option<usize>,
}

/// 按维度过滤。**只返回曲库里还在的曲目**（`EXISTS tracks`），
/// 残留行留给 GC——查询结果里出现"点开已经没有的曲目"只是噪音。
pub fn find(conn: &Connection, f: &Filter) -> DbResult<Vec<Match>> {
    let mut sql = String::from(
        "SELECT t.id, e.mood_label, e.mood_score, e.bpm, e.danceability, e.loudness,
                e.analyzed_at
         FROM track_enrichment e
         JOIN tracks t ON t.path_lower = e.path_lower
         WHERE t.availability = 'online' AND e.status = 'ok'",
    );
    let mut binds: Vec<rusqlite::types::Value> = Vec::new();
    if let Some(src) = &f.source {
        sql.push_str(" AND e.source = ?");
        binds.push(src.clone().into());
    }
    if let Some(mood) = &f.mood {
        sql.push_str(" AND e.mood_label = ?");
        binds.push(mood.clone().into());
    }
    if let Some(v) = f.dance_min {
        sql.push_str(" AND e.danceability >= ?");
        binds.push(v.into());
    }
    if let Some(v) = f.dance_max {
        sql.push_str(" AND e.danceability <= ?");
        binds.push(v.into());
    }
    if let Some(v) = f.bpm_min {
        sql.push_str(" AND e.bpm >= ?");
        binds.push(v.into());
    }
    if let Some(v) = f.bpm_max {
        sql.push_str(" AND e.bpm <= ?");
        binds.push(v.into());
    }
    sql.push_str(" ORDER BY e.danceability DESC, t.id ASC LIMIT ?");
    binds.push((f.limit.unwrap_or(200).clamp(1, 500) as i64).into());

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(rusqlite::params_from_iter(binds.iter()), |r| {
        Ok(Match {
            track_id: r.get(0)?,
            mood_label: r.get(1)?,
            mood_score: r.get(2)?,
            bpm: r.get(3)?,
            danceability: r.get(4)?,
            loudness: r.get(5)?,
            analyzed_at: r.get(6)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/* ------------------------------------------------------------------ 写 */

/// 一批落库（**唯一**写入入口，只在宿主进程里调用）。
///
/// ## 为什么失败/跳过的结果不写行
///
/// `status='empty'` 的语义是"这个源对这类文件没结论"，宿主据此**不再重试**。
/// 而 `failed`（异常）/`skipped`（补登后仍不在 beets 库）是**过程**故障：
/// 写进去就等于永久放弃 —— 用户下次点"分析"时这些文件会被当成"已经算过"，
/// 症状是"明明有几百首没数据，进度条却一次比一次短"。所以它们一律不落行，
/// 留在工作清单里，下次自然重试；计数通过 [`Saved`] 与事件暴露给用户。
///
/// ## 文件事实为什么现查 `tracks`
///
/// 分析期间文件可能已经被别处改过。用 sidecar 算清单时看到的 size/mtime 会
/// 把"已经过期的结论"标成新鲜，下一轮就不会再算它。以写入时刻的 `tracks`
/// 为准，最坏情况是多算一次，不会少算。
pub fn save(conn: &mut Connection, now: i64, items: &[Item]) -> DbResult<Saved> {
    let mut out = Saved::default();
    let tx = conn.transaction()?;
    {
        let mut facts = tx.prepare_cached(
            "SELECT size_bytes, mtime FROM tracks WHERE path_lower = ?1",
        )?;
        let mut upsert = tx.prepare_cached(
            "INSERT INTO track_enrichment
                 (path_lower, source, bpm, danceability, loudness, mood_mirex,
                  mood_label, mood_score, data, size_bytes, mtime, analyzed_at, status)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
             ON CONFLICT(path_lower, source) DO UPDATE SET
                 bpm = excluded.bpm,
                 danceability = excluded.danceability,
                 loudness = excluded.loudness,
                 mood_mirex = excluded.mood_mirex,
                 mood_label = excluded.mood_label,
                 mood_score = excluded.mood_score,
                 data = excluded.data,
                 size_bytes = excluded.size_bytes,
                 mtime = excluded.mtime,
                 analyzed_at = excluded.analyzed_at,
                 status = excluded.status",
        )?;

        for item in items {
            let status = item.status.as_str();
            if status != "ok" && status != "empty" {
                out.skipped += 1;
                continue;
            }
            let key = path_key(&item.path);
            let fact: Option<(i64, i64)> = facts
                .query_row([key.as_str()], |r| Ok((r.get(0)?, r.get(1)?)))
                .optional()?;
            let Some((size_bytes, mtime)) = fact else {
                out.not_in_library += 1;
                continue;
            };
            let p = promoted(&item.fields);
            if p.is_empty() {
                out.empty_fields += 1;
            }
            let data = serde_json::to_string(&item.fields).unwrap_or_else(|_| "{}".into());
            upsert.execute(params![
                key.as_str(),
                item.source,
                p.bpm,
                p.danceability,
                p.loudness,
                p.mood_mirex,
                p.mood_label,
                p.mood_score,
                data,
                size_bytes,
                mtime,
                now,
                status,
            ])?;
            out.written += 1;
        }
    }
    tx.commit()?;
    Ok(out)
}

/// 清理残留行：曲库已经没有对应曲目，且 `keep_days` 天内没更新过。
///
/// 为什么要留宽限期：换曲库根的那一分钟里所有旧路径都会"找不到曲目"，
/// 立刻删等于把用户几小时的分析成果一键清零。宽限期之后才收，代价只是
/// 一段时间内多几千行（每行几百字节）。
pub fn gc(conn: &Connection, source: &str, now: i64, keep_days: u32) -> DbResult<usize> {
    let cutoff = now - i64::from(keep_days.max(1)) * 86_400;
    let n = conn.execute(
        "DELETE FROM track_enrichment
         WHERE source = ?1
           AND analyzed_at < ?2
           AND NOT EXISTS (SELECT 1 FROM tracks t WHERE t.path_lower = track_enrichment.path_lower)",
        params![source, cutoff],
    )?;
    Ok(n)
}

/// 提升列的取值。
struct Promoted {
    bpm: Option<f64>,
    danceability: Option<f64>,
    loudness: Option<f64>,
    mood_mirex: Option<String>,
    mood_label: Option<String>,
    mood_score: Option<f64>,
}

impl Promoted {
    fn is_empty(&self) -> bool {
        self.bpm.is_none()
            && self.danceability.is_none()
            && self.loudness.is_none()
            && self.mood_mirex.is_none()
            && self.mood_label.is_none()
    }
}

fn promoted(fields: &Value) -> Promoted {
    let num = |k: &str| fields.get(k).and_then(Value::as_f64);
    let (mood_label, mood_score) = match argmax_mood(fields) {
        Some((label, score)) => (Some(label), Some(score)),
        None => (None, None),
    };
    Promoted {
        bpm: num("bpm"),
        danceability: num("danceability"),
        // 列名叫 loudness，源字段叫 average_loudness（xtractor 没有 energy）
        loudness: num("average_loudness"),
        mood_mirex: fields
            .get("mood_mirex")
            .and_then(Value::as_str)
            .map(str::to_string),
        mood_label,
        mood_score,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn db() -> Connection {
        crate::db::open_in_memory().unwrap()
    }

    /// 插一首曲目，返回它的 path_lower。
    fn track(conn: &Connection, id: &str, path: &str, size: i64, mtime: i64) -> String {
        let key = path_key(path);
        conn.execute(
            "INSERT INTO tracks (id, title, title_norm, added_at, path, path_lower, size_bytes, mtime)
             VALUES (?1, ?2, ?3, 0, ?4, ?5, ?6, ?7)",
            params![id, id, id, path, key, size, mtime],
        )
        .unwrap();
        key
    }

    fn item(path: &str, status: &str, fields: Value) -> Item {
        Item {
            path: path.to_string(),
            source: SOURCE_XTRACTOR.to_string(),
            status: status.to_string(),
            fields,
        }
    }

    fn sample() -> Value {
        json!({
            "bpm": 128.0,
            "danceability": 0.73,
            "average_loudness": 0.91,
            "mood_mirex": "mood_mirex_cluster_3",
            "mood_happy": 0.62, "mood_sad": 0.05, "mood_relaxed": 0.11,
            "mood_aggressive": 0.4, "mood_party": 0.5,
            "mood_acoustic": 0.02, "mood_electronic": 0.66,
        })
    }

    #[test]
    fn migration_creates_table_and_indexes() {
        let conn = db();
        for name in ["track_enrichment", "idx_enrich_mood", "idx_enrich_dance"] {
            let n: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE name = ?1",
                    [name],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(n, 1, "{name} 缺失");
        }
    }

    #[test]
    fn path_key_matches_scanner_case_folding() {
        // 大小写折叠与扫描器同口径（scanner.rs 就是 `to_lowercase()`）
        assert_eq!(path_key(r"D:\Music\A.FLAC"), r"d:\music\a.flac");
        if cfg!(windows) {
            // sidecar 报的路径可能带正斜杠：Windows 上必须是同一个键
            assert_eq!(path_key("D:/music/a.flac"), r"d:\music\a.flac");
        } else {
            // 非 Windows 不动分隔符——把 `/` 换成 `\` 会毁掉合法路径
            assert_eq!(path_key("/music/a.flac"), "/music/a.flac");
        }
    }

    #[test]
    fn worklist_covers_missing_stale_and_skips_empty_by_default() {
        let conn = db();
        track(&conn, "t1", "/m/a.mp3", 100, 1);
        track(&conn, "t2", "/m/b.mp3", 200, 2);
        track(&conn, "t3", "/m/c.mp3", 300, 3);

        let all = worklist(&conn, SOURCE_XTRACTOR, false, 100).unwrap();
        assert_eq!(all.len(), 3, "全都没有结论时应全在清单里");
        assert!(all.iter().all(|w| w.size_bytes > 0), "size/mtime 必须带出来");

        let mut conn = conn;
        save(
            &mut conn,
            1_000,
            &[
                item("/m/a.mp3", "ok", sample()),
                item("/m/b.mp3", "empty", json!({})),
            ],
        )
        .unwrap();

        let left = worklist(&conn, SOURCE_XTRACTOR, false, 100).unwrap();
        let paths: Vec<&str> = left.iter().map(|w| w.path.as_str()).collect();
        assert_eq!(paths, vec!["/m/c.mp3"], "已分析的不该再出现；empty 默认不重试");

        let forced = worklist(&conn, SOURCE_XTRACTOR, true, 100).unwrap();
        let paths: Vec<&str> = forced.iter().map(|w| w.path.as_str()).collect();
        assert!(paths.contains(&"/m/b.mp3"), "强制重算要把 empty 捞回来: {paths:?}");

        // 文件被外部改过 → size 变 → 重新进清单
        conn.execute(
            "UPDATE tracks SET size_bytes = 101, mtime = 9 WHERE path_lower = ?1",
            [path_key("/m/a.mp3")],
        )
        .unwrap();
        let stale = worklist(&conn, SOURCE_XTRACTOR, false, 100).unwrap();
        assert!(
            stale.iter().any(|w| w.path == "/m/a.mp3"),
            "size 变了必须重算（否则结论永远停在旧文件上）"
        );

        let s = stats(&conn, SOURCE_XTRACTOR).unwrap();
        assert_eq!((s.total, s.missing, s.stale, s.empty), (3, 1, 1, 1), "{s:?}");
        assert_eq!(s.work(false), 2);
        assert_eq!(s.work(true), 3);
    }

    #[test]
    fn offline_tracks_are_not_in_worklist() {
        let conn = db();
        track(&conn, "t1", "/m/a.mp3", 100, 1);
        conn.execute("UPDATE tracks SET availability = 'missing'", []).unwrap();
        assert!(worklist(&conn, SOURCE_XTRACTOR, false, 100).unwrap().is_empty());
        assert_eq!(stats(&conn, SOURCE_XTRACTOR).unwrap().total, 0);
    }

    #[test]
    fn save_promotes_columns_and_argmax() {
        let mut conn = db();
        track(&conn, "t1", r"D:\m\a.mp3", 100, 1);
        let saved = save(&mut conn, 42, &[item(r"D:\m\a.mp3", "ok", sample())]).unwrap();
        assert_eq!(saved.written, 1, "{saved:?}");

        let (bpm, dance, loud, mirex, label, score, data, size, mtime, at): (
            f64, f64, f64, String, String, f64, String, i64, i64, i64,
        ) = conn
            .query_row(
                "SELECT bpm, danceability, loudness, mood_mirex, mood_label, mood_score,
                        data, size_bytes, mtime, analyzed_at
                 FROM track_enrichment",
                [],
                |r| {
                    Ok((
                        r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?,
                        r.get(6)?, r.get(7)?, r.get(8)?, r.get(9)?,
                    ))
                },
            )
            .unwrap();
        assert_eq!(bpm, 128.0);
        assert_eq!(dance, 0.73);
        assert_eq!(loud, 0.91, "loudness 列来自 average_loudness");
        assert_eq!(mirex, "mood_mirex_cluster_3");
        assert_eq!(label, "mood_electronic", "argmax 应取 0.66 那一类");
        assert_eq!(score, 0.66);
        // 原始 JSON 一并留底：列只服务查询，溯源还得看原字段
        assert!(data.contains("mood_party"), "{data}");
        // 文件事实取自 tracks（不是 sidecar 报的）
        assert_eq!((size, mtime, at), (100, 1, 42));
    }

    #[test]
    fn save_skips_failures_and_unknown_paths_but_upserts_idempotently() {
        let mut conn = db();
        track(&conn, "t1", "/m/a.mp3", 100, 1);

        let saved = save(
            &mut conn,
            10,
            &[
                item("/m/a.mp3", "failed", json!({})),
                item("/m/a.mp3", "skipped", json!({})),
                item("/m/ghost.mp3", "ok", sample()),
            ],
        )
        .unwrap();
        assert_eq!((saved.written, saved.skipped, saved.not_in_library), (0, 2, 1));
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM track_enrichment", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 0, "过程故障与库外路径都不许落行（否则会被当成已算过）");

        // 同一 (path, source) 写两次 → 还是一条，值被更新
        save(&mut conn, 20, &[item("/m/a.mp3", "ok", sample())]).unwrap();
        let mut second = sample();
        second["bpm"] = json!(90.0);
        save(&mut conn, 30, &[item("/m/a.mp3", "ok", second)]).unwrap();
        let (n, bpm, at): (i64, f64, i64) = conn
            .query_row(
                "SELECT COUNT(*), MAX(bpm), MAX(analyzed_at) FROM track_enrichment",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!((n, bpm, at), (1, 90.0, 30), "必须原地更新，不是插第二行");
    }

    #[test]
    fn save_counts_ok_without_fields() {
        let mut conn = db();
        track(&conn, "t1", "/m/a.mp3", 100, 1);
        let saved = save(&mut conn, 1, &[item("/m/a.mp3", "ok", json!({"unrelated": 1}))]).unwrap();
        assert_eq!((saved.written, saved.empty_fields), (1, 1));
        let label: Option<String> = conn
            .query_row("SELECT mood_label FROM track_enrichment", [], |r| r.get(0))
            .unwrap();
        assert!(label.is_none(), "没有 mood_* 就不该编一个标签出来");
    }

    #[test]
    fn gc_only_removes_old_orphans() {
        let mut conn = db();
        track(&conn, "t1", "/m/a.mp3", 100, 1);
        track(&conn, "t2", "/m/b.mp3", 200, 1);
        save(
            &mut conn,
            1_000,
            &[item("/m/a.mp3", "ok", sample()), item("/m/b.mp3", "ok", sample())],
        )
        .unwrap();
        // a 变成孤儿（曲目行没了），b 还在
        conn.execute("DELETE FROM tracks WHERE id = 't1'", []).unwrap();

        let now = 1_000 + 86_400 * 2;
        assert_eq!(gc(&conn, SOURCE_XTRACTOR, now, 30).unwrap(), 0, "宽限期内不许删");
        let now = 1_000 + 86_400 * 31;
        assert_eq!(gc(&conn, SOURCE_XTRACTOR, now, 30).unwrap(), 1);
        let left: i64 = conn
            .query_row("SELECT COUNT(*) FROM track_enrichment", [], |r| r.get(0))
            .unwrap();
        assert_eq!(left, 1, "有曲目的行永不被 GC 带走");
        assert_eq!(stats(&conn, SOURCE_XTRACTOR).unwrap().orphan, 0);
    }

    #[test]
    fn find_filters_by_mood_and_range() {
        let mut conn = db();
        track(&conn, "t1", "/m/a.mp3", 100, 1);
        track(&conn, "t2", "/m/b.mp3", 200, 1);
        let mut slow = sample();
        slow["bpm"] = json!(70.0);
        slow["danceability"] = json!(0.2);
        slow["mood_electronic"] = json!(0.1);
        slow["mood_sad"] = json!(0.9);
        save(
            &mut conn,
            5,
            &[item("/m/a.mp3", "ok", sample()), item("/m/b.mp3", "ok", slow)],
        )
        .unwrap();

        let all = find(&conn, &Filter::default()).unwrap();
        assert_eq!(all.len(), 2);
        // 默认按 danceability 降序：舞曲性高的在前
        assert_eq!(all[0].track_id, "t1");

        let happy = find(&conn, &Filter { mood: Some("mood_sad".into()), ..Default::default() })
            .unwrap();
        assert_eq!(happy.len(), 1);
        assert_eq!(happy[0].track_id, "t2");

        let dance = find(
            &conn,
            &Filter { dance_min: Some(0.5), ..Default::default() },
        )
        .unwrap();
        assert_eq!(dance.len(), 1);
        assert_eq!(dance[0].track_id, "t1");

        let bpm = find(
            &conn,
            &Filter { bpm_min: Some(100.0), bpm_max: Some(200.0), ..Default::default() },
        )
        .unwrap();
        assert_eq!(bpm.len(), 1);
        assert_eq!(bpm[0].bpm, Some(128.0));

        // 曲目没了 → 查询里看不到（残留行留给 GC）
        conn.execute("DELETE FROM tracks WHERE id = 't1'", []).unwrap();
        assert_eq!(find(&conn, &Filter::default()).unwrap().len(), 1);
    }

    #[test]
    fn source_is_part_of_the_key() {
        let mut conn = db();
        track(&conn, "t1", "/m/a.mp3", 100, 1);
        save(
            &mut conn,
            1,
            &[
                item("/m/a.mp3", "ok", sample()),
                Item {
                    path: "/m/a.mp3".into(),
                    source: "lastgenre".into(),
                    status: "ok".into(),
                    fields: json!({"genre": "Electronic"}),
                },
            ],
        )
        .unwrap();
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM track_enrichment", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 2, "两个源各一行，不能互相覆盖");
        assert_eq!(stats(&conn, SOURCE_XTRACTOR).unwrap().analyzed, 1, "统计按源隔离");
    }
}
