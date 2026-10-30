//! beets 模块自有的派生数据：索引增强（enrich）的结论。
//!
//! ## 为什么是独立文件，而不是应用库的一张表
//!
//! 这张表曾经在应用库里（migration 0008），批 3 之后被搬到这里，原因是它
//! 把 beets 模块和应用本体焊在了一起：
//!
//! 1. **数据模型耦合**：应用库的 schema 里出现 `track_enrichment`，等于
//!    应用的数据模型里有一条"因为装了 beets 才有"的表。应用降级、回滚、
//!    或被第三方工具读库时，都得理解一条与它无关的记录。
//! 2. **运行时耦合**：写它要拿 `scanning`（应用刷新管线的互斥），于是
//!    "beets 在分析"会挡住"用户在扫描"，症状是点扫描没反应。同一个库文件
//!    里的写者天然抢锁，绕不过去；不同文件根本没有这把锁。
//! 3. **生命周期耦合**：应用库被备份/迁移/清理时，一堆 beets 中间产物跟着
//!    走；用户卸载 beets 组件后它们还在。
//!
//! 现在它落在 `{app_data}/beets-index/enrich.db`——**beets 模块自己的目录**，
//! 与用户的 beets 库、应用的曲库都不共享文件。模块被删掉时整个目录一起走。
//!
//! ## 它不知道"曲库"
//!
//! 这里的 API 只认 [`Arena`]——一份由调用方给的 `path_lower → (path, size,
//! mtime)` 快照。谁提供快照、快照从哪来，本 crate 不关心。这样"曲库有哪些
//! 文件"这条知识留在应用侧，本 crate 只回答"这份快照里哪些还没结论/已过期"。
//!
//! 唯一的方向依赖是：**beets 模块向应用要一份文件清单**（只读），反过来
//! 应用不 import 本 crate。

use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;

use rusqlite::{params, Connection};
use serde_json::Value;

/// 默认识别源。宿主只知道"数值特征来自这个源"，不知道它怎么装、开没开
/// ——那是工具侧 `fields` 命令的回答。
pub const SOURCE_XTRACTOR: &str = "xtractor";

/// 7 个情绪类别（xtractor 的 `mood_*` 概率）。
///
/// 刻意不含 `mood_mirex_cluster_N`：那是簇编号，不是情绪语义，参与 argmax
/// 会让"最可能的情绪"变成"第 3 号簇"。
pub const MOOD_CLASSES: [&str; 7] = [
    "mood_happy",
    "mood_electronic",
    "mood_relaxed",
    "mood_party",
    "mood_sad",
    "mood_acoustic",
    "mood_aggressive",
];

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("增强库出错：{0}")]
    Sql(#[from] rusqlite::Error),
    #[error("增强库目录建不出来（{path}）：{detail}")]
    Dir { path: String, detail: String },
}

/// 表结构。与迁移 0008 同形——**列名刻意保持一致**，因为历史数据要能直接
/// 搬过来（`ATTACH` 拷贝），迁移脚本里会按列名对齐。
///
/// 不挂任何外键：曲目行会被扫描器整行删掉再重建，级联会把一份还算新鲜的
/// 结论一起带走；查询期缺行的表现是"查不到"，而不是"结论丢了"。
const SCHEMA: &str = "\
CREATE TABLE IF NOT EXISTS track_enrichment (
  path_lower   TEXT NOT NULL,
  source       TEXT NOT NULL,
  bpm            REAL,
  danceability   REAL,
  loudness       REAL,
  mood_mirex     TEXT,
  mood_label     TEXT,
  mood_score     REAL,
  data         TEXT NOT NULL DEFAULT '{}',
  size_bytes   INTEGER NOT NULL,
  mtime        INTEGER NOT NULL,
  analyzed_at  INTEGER NOT NULL,
  status       TEXT NOT NULL DEFAULT 'ok' CHECK (status IN ('ok','empty')),
  tool_version TEXT,
  PRIMARY KEY (path_lower, source)
);
CREATE INDEX IF NOT EXISTS idx_enrich_mood  ON track_enrichment(mood_label);
CREATE INDEX IF NOT EXISTS idx_enrich_dance ON track_enrichment(danceability);
";

/// 路径归一：与扫描器同口径（Windows 大小写不敏感，正/反斜杠等价）。
///
/// 两侧算法必须一致，否则同一个文件在曲库里叫 `D:\Music\a.flac`、在结论表里
/// 叫 `d:/music/a.flac`，工作清单会永远认为它"没算过"。
pub fn path_key(path: &str) -> String {
    let normalized = if cfg!(windows) {
        path.replace('/', "\\")
    } else {
        path.to_string()
    };
    normalized.to_lowercase()
}

/// 7 类概率里取最大。严格大于才替换——并列时保留先出现的那个，
/// 保证同一份输入永远同一个结论（否则两次运行的结果会随机漂移）。
pub fn argmax_mood(fields: &Value) -> Option<(String, f64)> {
    let obj = fields.as_object()?;
    let mut best: Option<(&str, f64)> = None;
    for name in MOOD_CLASSES {
        let Some(score) = obj.get(name).and_then(Value::as_f64) else {
            continue;
        };
        if let Some((_, b)) = best {
            if score <= b {
                continue;
            }
        }
        best = Some((name, score));
    }
    best.map(|(name, score)| (name.to_string(), score))
}

/* ---------------- 调用方给的快照 ---------------- */

/// 一个文件在应用曲库里的身份。`size/mtime` 是判"结论是否过期"的唯一依据。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fact {
    pub path: String,
    pub size: i64,
    pub mtime: i64,
}

/// `path_lower → Fact` 快照。
///
/// 为什么用快照而不是回调：一个会话要对着同一份曲库问几十次"这首算过没"，
/// 每次回应用库查一遍等于把一次会话变成几百次跨库往返；而且中途曲库变了
/// 会让同一批里的行忽快忽慢地失效。快照的代价是"会话期间新增的曲目这轮
/// 不参与"，下一轮自然补上——比不确定性好。
#[derive(Debug, Clone, Default)]
pub struct Arena(HashMap<String, Fact>);

impl Arena {
    /// 从 `(path, path_lower, size_bytes, mtime)` 行构造。
    ///
    /// `path_lower` 由调用方给（应用库里有这一列），本 crate 不再自己算：
    /// 曲库的归一化口径只有一个源，重复推导迟早漂移。
    pub fn from_rows<I>(rows: I) -> Self
    where
        I: IntoIterator<Item = (String, String, i64, i64)>,
    {
        Self(
            rows.into_iter()
                .map(|(path, key, size, mtime)| (key, Fact { path, size, mtime }))
                .collect(),
        )
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn get(&self, key: &str) -> Option<&Fact> {
        self.0.get(key)
    }
}

/* ---------------- 写 ---------------- */

/// 一条待落库的结论。
#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub path: String,
    pub source: String,
    /// `ok` / `empty` 落库；其余（failed / skipped）不落
    pub status: String,
    pub fields: Value,
}

/// 一批写入的结果。三个计数分开是刻意的：跳过原因不同，用户能做的事也不同
/// （`not_in_library` = 文件不在曲库快照里=补登没成功；`skipped` = 源没产出）。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Saved {
    pub written: usize,
    /// 状态不是 ok/empty（failed / skipped），不写行
    pub skipped: usize,
    /// 快照里没有这个 path_lower（扫描器还没收录 / 路径已漂移），不写行
    pub not_in_library: usize,
    /// 状态是 ok 但一个可提升字段都没有（只进 JSON，不影响计数，供诊断）
    pub empty_fields: usize,
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

/* ---------------- 读 ---------------- */

/// 一张卡需要的全部数字。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Stats {
    /// 快照里的曲目总数
    pub total: usize,
    /// 有新鲜 ok 结论
    pub analyzed: usize,
    /// 有新鲜 empty 结论（跑通了但没字段，默认不重试）
    pub empty: usize,
    /// 从没算过
    pub missing: usize,
    /// 算过但文件变了（size/mtime 不符）
    pub stale: usize,
    /// 结论表里有、快照里没有的行（等 GC 收）
    pub orphan: usize,
}

impl Stats {
    pub fn work(&self, include_empty: bool) -> usize {
        self.missing + self.stale + if include_empty { self.empty } else { 0 }
    }
}

/// 查询命中行。**没有 track_id**——那是应用侧的概念，本 crate 不认识；
/// 调用方拿 `path_lower` 回曲库换 id。
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub path_lower: String,
    pub mood_label: Option<String>,
    pub mood_score: Option<f64>,
    pub bpm: Option<f64>,
    pub danceability: Option<f64>,
    pub loudness: Option<f64>,
    pub analyzed_at: i64,
}

/// 查询条件。全部可空 = 不限制。
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

/* ---------------- 存储 ---------------- */

/// 独立文件上的增强结论表。内部一把 `Mutex`：落库线程与命令线程会同时进来，
/// 而 SQLite 单连接不是线程安全的。争用极低（命令是毫秒级读），不值得为它
/// 上连接池。
pub struct Store {
    conn: Mutex<Connection>,
}

impl Store {
    /// 打开（必要时创建）增强库。
    ///
    /// WAL + busy_timeout 都设了：这个文件的写者只有本进程的落库线程，但
    /// 万一有第二个实例（同一用户开了两遍应用），也不会立刻 BUSY。
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| Error::Dir {
                path: dir.to_string_lossy().into_owned(),
                detail: e.to_string(),
            })?;
        }
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "busy_timeout", 5000)?;
        conn.execute_batch(SCHEMA)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    fn conn(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// 一批落库（**唯一**写入入口）。
    ///
    /// ## 为什么失败/跳过的结果不写行
    ///
    /// `status='empty'` 的语义是"这个源对这类文件没结论"，宿主据此**不再重试**。
    /// 而 `failed`（异常）/`skipped`（补登后仍不在 beets 库）是**过程**故障：
    /// 写进去就等于永久放弃 —— 用户下次点"分析"时这些文件会被当成"已经算过"，
    /// 症状是"明明有几百首没数据，进度条却一次比一次短"。所以它们一律不落行，
    /// 留在工作清单里，下次自然重试；计数通过 [`Saved`] 与事件暴露给用户。
    ///
    /// ## 文件事实为什么取快照而不是现查
    ///
    /// 分析期间文件可能已经被别处改过。用 sidecar 算清单时看到的 size/mtime 会
    /// 把"已经过期的结论"标成新鲜，下一轮就不会再算它。以**落库时刻**的快照
    /// 为准，最坏情况是多算一次，不会少算。
    pub fn save(&self, now: i64, items: &[Item], alive: &Arena) -> Result<Saved> {
        let mut out = Saved::default();
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        {
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
                let Some(fact) = alive.get(&key) else {
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
                    fact.size,
                    fact.mtime,
                    now,
                    status,
                ])?;
                out.written += 1;
            }
        }
        tx.commit()?;
        Ok(out)
    }

    /// 结论表里属于该源的全部行 → `(size, mtime, status)`。
    ///
    /// 5 万行约 4 MB。一次拉完比逐条查快两个数量级，而统计/清单本来就是
    /// "对全库做一次集合运算"，没有增量查询的必要。
    fn index_of(&self, source: &str) -> Result<HashMap<String, (i64, i64, String)>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT path_lower, size_bytes, mtime, status FROM track_enrichment WHERE source = ?1",
        )?;
        let rows = stmt.query_map([source], |r| {
            Ok((r.get::<_, String>(0)?, (r.get(1)?, r.get(2)?, r.get(3)?)))
        })?;
        let mut out = HashMap::new();
        for row in rows {
            let (key, val) = row?;
            out.insert(key, val);
        }
        Ok(out)
    }

    /// 计数：把快照与结论表做一次集合运算。
    pub fn stats(&self, source: &str, alive: &Arena) -> Result<Stats> {
        let have = self.index_of(source)?;
        let mut s = Stats {
            total: alive.len(),
            ..Stats::default()
        };
        for (key, fact) in &alive.0 {
            match have.get(key) {
                None => s.missing += 1,
                Some((size, mtime, _)) if *size != fact.size || *mtime != fact.mtime => {
                    s.stale += 1;
                }
                Some((_, _, status)) if status == "empty" => s.empty += 1,
                Some(_) => s.analyzed += 1,
            }
        }
        s.orphan = have.keys().filter(|k| !alive.0.contains_key(*k)).count();
        Ok(s)
    }

    /// 工作清单：快照里"没结论 / 结论过期 / 空结论（强制重算时）"的路径。
    ///
    /// 顺序按 `path_lower`：同一目录的文件相邻，sidecar 侧补登时命令行聚类
    /// 更好，也保证分批/续跑是确定性的。
    ///
    /// `limit` 传 `i64::MAX` 表示不限——**不要传 `usize::MAX`**：`as i64` 会
    /// 变成 `-1`，而 SQLite 恰好把 `LIMIT -1` 当"不限"，于是"看起来对、语义
    /// 靠巧合"。这种巧合一旦换库或换绑定就静默变成"只算一条"。
    pub fn worklist(
        &self,
        source: &str,
        alive: &Arena,
        include_empty: bool,
        limit: usize,
    ) -> Result<Vec<String>> {
        let have = self.index_of(source)?;
        let mut out: Vec<(String, String)> = Vec::new();
        for (key, fact) in &alive.0 {
            let need = match have.get(key) {
                None => true,
                Some((size, mtime, status)) => {
                    *size != fact.size
                        || *mtime != fact.mtime
                        || (include_empty && status == "empty")
                }
            };
            if need {
                out.push((key.clone(), fact.path.clone()));
            }
        }
        out.sort_by(|a, b| a.0.cmp(&b.0));
        out.truncate(limit.min(i64::MAX as usize));
        Ok(out.into_iter().map(|(_, p)| p).collect())
    }

    /// 按维度过滤。
    ///
    /// 只按结论表自己的列过滤，**不保证**结果还在曲库里——残留行由 GC 收。
    /// 调用方若只想要"点开能放"的曲目，自己拿 `path_lower` 回曲库对一遍。
    pub fn find(&self, f: &Filter) -> Result<Vec<Row>> {
        let mut sql = String::from(
            "SELECT path_lower, mood_label, mood_score, bpm, danceability, loudness,
                    analyzed_at
             FROM track_enrichment
             WHERE status = 'ok'",
        );
        let mut binds: Vec<rusqlite::types::Value> = Vec::new();
        if let Some(src) = &f.source {
            sql.push_str(" AND source = ?");
            binds.push(src.clone().into());
        }
        if let Some(mood) = &f.mood {
            sql.push_str(" AND mood_label = ?");
            binds.push(mood.clone().into());
        }
        if let Some(v) = f.dance_min {
            sql.push_str(" AND danceability >= ?");
            binds.push(v.into());
        }
        if let Some(v) = f.dance_max {
            sql.push_str(" AND danceability <= ?");
            binds.push(v.into());
        }
        if let Some(v) = f.bpm_min {
            sql.push_str(" AND bpm >= ?");
            binds.push(v.into());
        }
        if let Some(v) = f.bpm_max {
            sql.push_str(" AND bpm <= ?");
            binds.push(v.into());
        }
        sql.push_str(" ORDER BY danceability DESC, path_lower ASC LIMIT ?");
        binds.push((f.limit.unwrap_or(200).clamp(1, 500) as i64).into());

        let conn = self.conn();
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(rusqlite::params_from_iter(binds.iter()), |r| {
            Ok(Row {
                path_lower: r.get(0)?,
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

    /// 清理残留行：快照里没有对应文件，且 `keep_days` 天内没更新过。
    ///
    /// 为什么要留宽限期：换曲库根的那一分钟里所有旧路径都会"找不到曲目"，
    /// 立刻删等于把用户几小时的分析成果一键清零。宽限期之后才收，代价只是
    /// 一段时间内多几千行（每行几百字节）。
    pub fn gc(&self, source: &str, now: i64, keep_days: u32, alive: &Arena) -> Result<usize> {
        let cutoff = now - i64::from(keep_days.max(1)) * 86_400;
        let victims: Vec<String> = {
            let conn = self.conn();
            let mut stmt = conn.prepare(
                "SELECT path_lower FROM track_enrichment WHERE source = ?1 AND analyzed_at < ?2",
            )?;
            let rows = stmt.query_map(params![source, cutoff], |r| r.get::<_, String>(0))?;
            rows.filter_map(std::result::Result::ok)
                .filter(|k| alive.get(k).is_none())
                .collect()
        };
        if victims.is_empty() {
            return Ok(0);
        }
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        let mut n = 0usize;
        {
            let mut del = tx.prepare_cached(
                "DELETE FROM track_enrichment WHERE path_lower = ?1 AND source = ?2",
            )?;
            for key in &victims {
                n += del.execute(params![key, source])?;
            }
        }
        tx.commit()?;
        Ok(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn store() -> (Store, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let s = Store::open(&dir.path().join("enrich.db")).unwrap();
        (s, dir)
    }

    fn arena(rows: &[(&str, i64, i64)]) -> Arena {
        Arena::from_rows(
            rows.iter()
                .map(|(p, size, mtime)| (p.to_string(), path_key(p), *size, *mtime)),
        )
    }

    fn ok_item(path: &str, fields: Value) -> Item {
        Item {
            path: path.into(),
            source: SOURCE_XTRACTOR.into(),
            status: "ok".into(),
            fields,
        }
    }

    fn sample() -> Value {
        json!({
            "bpm": 128.0,
            "danceability": 0.81,
            "average_loudness": 0.72,
            "mood_mirex": "mood_mirex_cluster_3",
            "mood_happy": 0.71,
            "mood_electronic": 0.12,
        })
    }

    #[test]
    fn path_key_matches_scanner_case_folding() {
        let a = path_key(r"D:\Music\A.FLAC");
        let b = path_key("D:/music/a.flac");
        assert_eq!(a.to_lowercase(), b.to_lowercase(), "大小写不敏感");
        if cfg!(windows) {
            assert_eq!(a, b, "Windows 上正/反斜杠必须同一个键");
        }
    }

    #[test]
    fn argmax_ignores_cluster_labels_and_ties_keep_first() {
        // 只有 cluster 号 → 没有结论
        assert_eq!(
            argmax_mood(&json!({"mood_mirex": "mood_mirex_cluster_1"})),
            None
        );
        // 并列时取 MOOD_CLASSES 里先出现的那个，保证确定性
        let tie = json!({"mood_happy": 0.5, "mood_sad": 0.5});
        assert_eq!(argmax_mood(&tie).unwrap().0, "mood_happy");
        // 严格大于才替换
        let win = json!({"mood_happy": 0.4, "mood_sad": 0.6});
        assert_eq!(argmax_mood(&win).unwrap(), ("mood_sad".to_string(), 0.6));
    }

    #[test]
    fn save_skips_failures_and_unknown_paths() {
        let (s, _d) = store();
        let a = arena(&[("D:\\m\\a.flac", 10, 20)]);
        let saved = s
            .save(
                100,
                &[
                    ok_item("D:\\m\\a.flac", sample()),
                    Item {
                        path: "D:\\m\\a.flac".into(),
                        source: SOURCE_XTRACTOR.into(),
                        status: "failed".into(),
                        fields: json!({}),
                    },
                    ok_item("D:\\m\\ghost.flac", sample()),
                ],
                &a,
            )
            .unwrap();
        assert_eq!(saved.written, 1, "只有 ok 且在快照里的那条落库");
        assert_eq!(saved.skipped, 1);
        assert_eq!(saved.not_in_library, 1);
        assert_eq!(s.stats(SOURCE_XTRACTOR, &a).unwrap().analyzed, 1);
    }

    #[test]
    fn save_is_idempotent_and_promotes_columns() {
        let (s, _d) = store();
        let a = arena(&[("D:\\m\\a.flac", 10, 20)]);
        for _ in 0..2 {
            s.save(100, &[ok_item("D:\\m\\a.flac", sample())], &a)
                .unwrap();
        }
        let row = &s.find(&Filter::default()).unwrap()[0];
        assert_eq!(row.bpm, Some(128.0));
        assert_eq!(row.danceability, Some(0.81));
        assert_eq!(row.loudness, Some(0.72));
        assert_eq!(row.mood_label.as_deref(), Some("mood_happy"));
        assert!(!row.path_lower.is_empty());
    }

    #[test]
    fn stats_splits_missing_stale_empty_analyzed() {
        let (s, _d) = store();
        // 先按"文件没变"的快照算完 a、b
        let before = arena(&[("D:\\m\\a.flac", 10, 20), ("D:\\m\\b.flac", 11, 21)]);
        s.save(
            100,
            &[
                ok_item("D:\\m\\a.flac", sample()),
                ok_item("D:\\m\\b.flac", sample()),
            ],
            &before,
        )
        .unwrap();

        // 再换一份快照：a 没变、b 长了（stale）、c 只有空结论、d 从没算过
        let after = arena(&[
            ("D:\\m\\a.flac", 10, 20),
            ("D:\\m\\b.flac", 99, 21),
            ("D:\\m\\c.flac", 12, 22),
            ("D:\\m\\d.flac", 13, 23),
        ]);
        s.save(
            100,
            &[Item {
                path: "D:\\m\\c.flac".into(),
                source: SOURCE_XTRACTOR.into(),
                status: "empty".into(),
                fields: json!({}),
            }],
            &after,
        )
        .unwrap();

        let st = s.stats(SOURCE_XTRACTOR, &after).unwrap();
        assert_eq!(
            (st.total, st.analyzed, st.empty, st.missing, st.stale),
            (4, 1, 1, 1, 1)
        );
        assert_eq!(st.work(false), 2);
        assert_eq!(st.work(true), 3, "强制重算把 empty 也算进来");
    }

    #[test]
    fn worklist_honours_force_and_returns_original_paths() {
        let (s, _d) = store();
        let a = arena(&[("D:\\m\\a.flac", 10, 20), ("D:\\m\\b.flac", 11, 21)]);
        s.save(100, &[ok_item("D:\\m\\a.flac", sample())], &a)
            .unwrap();
        s.save(
            100,
            &[Item {
                path: "D:\\m\\b.flac".into(),
                source: SOURCE_XTRACTOR.into(),
                status: "empty".into(),
                fields: json!({}),
            }],
            &a,
        )
        .unwrap();
        assert!(s
            .worklist(SOURCE_XTRACTOR, &a, false, 100)
            .unwrap()
            .is_empty());
        let forced = s.worklist(SOURCE_XTRACTOR, &a, true, 100).unwrap();
        assert_eq!(forced.len(), 1);
        assert_eq!(forced[0], "D:\\m\\b.flac", "给回的是原始大小写路径");
    }

    #[test]
    fn gc_respects_grace_period_and_arena() {
        let (s, _d) = store();
        let a = arena(&[("D:\\m\\a.flac", 10, 20)]);
        s.save(1_000, &[ok_item("D:\\m\\a.flac", sample())], &a)
            .unwrap();
        // 30 天宽限期：刚写的不收
        assert_eq!(
            s.gc(SOURCE_XTRACTOR, 1_000, 30, &Arena::default()).unwrap(),
            0
        );
        // 过期的收掉
        let later = 1_000 + 40 * 86_400;
        assert_eq!(
            s.gc(SOURCE_XTRACTOR, later, 30, &Arena::default()).unwrap(),
            1
        );
        // 还在快照里的不收
        s.save(1_000, &[ok_item("D:\\m\\a.flac", sample())], &a)
            .unwrap();
        assert_eq!(s.gc(SOURCE_XTRACTOR, later, 30, &a).unwrap(), 0);
    }

    #[test]
    fn filter_narrows_by_mood_and_bpm() {
        let (s, _d) = store();
        let a = arena(&[("D:\\m\\slow.flac", 10, 20), ("D:\\m\\fast.flac", 11, 21)]);
        let mut slow = sample();
        slow["bpm"] = json!(70.0);
        slow["mood_happy"] = json!(0.2);
        slow["mood_sad"] = json!(0.9);
        s.save(
            100,
            &[
                ok_item("D:\\m\\slow.flac", slow),
                ok_item("D:\\m\\fast.flac", sample()),
            ],
            &a,
        )
        .unwrap();
        let fast = s
            .find(&Filter {
                bpm_min: Some(120.0),
                ..Filter::default()
            })
            .unwrap();
        assert_eq!(fast.len(), 1);
        assert_eq!(fast[0].bpm, Some(128.0));
        let sad = s
            .find(&Filter {
                mood: Some("mood_sad".into()),
                ..Filter::default()
            })
            .unwrap();
        assert_eq!(sad.len(), 1);
    }

    #[test]
    fn sources_do_not_overwrite_each_other() {
        let (s, _d) = store();
        let a = arena(&[("D:\\m\\a.flac", 10, 20)]);
        s.save(100, &[ok_item("D:\\m\\a.flac", sample())], &a)
            .unwrap();
        s.save(
            100,
            &[Item {
                path: "D:\\m\\a.flac".into(),
                source: "lastgenre".into(),
                status: "ok".into(),
                fields: json!({"genre": "Jazz"}),
            }],
            &a,
        )
        .unwrap();
        assert_eq!(s.stats(SOURCE_XTRACTOR, &a).unwrap().analyzed, 1);
        assert_eq!(s.stats("lastgenre", &a).unwrap().analyzed, 1);
        assert_eq!(
            s.find(&Filter {
                source: Some("lastgenre".into()),
                ..Filter::default()
            })
            .unwrap()
            .len(),
            1
        );
    }
}
