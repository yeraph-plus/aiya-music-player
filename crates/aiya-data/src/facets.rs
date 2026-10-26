//! 专辑 / 艺人聚合浏览（docs/24 §3.2 决策 2）。
//!
//! 为什么独立成文件而不是塞进 repo.rs：repo 是"曲目窗口"（游标分页 +
//! 四种来源），这里的形状完全不同——**GROUP BY 聚合、一次性返回前 N、
//! 无游标**。硬放一起只会让 844 行的 repo 再长出一截互不相干的职责。
//!
//! ## 与 UI 的契约
//!
//! 宿主侧拆成两条命令（`library_albums` / `library_artists`），而不是文档
//! 草案里的一条 `library_facets(kind)`：仓库的派发表是命令一一对应的
//! （mock/契约测试都按名字对账），一条命令收 kind 字符串再返回带 tag 的
//! enum，等于让"参数校验 + 判别联合"这两层复杂度落在两端调用点上。
//! docs/24 的 B4-3a 章节记录了这个偏差。
//!
//! ## 只显示"实际有曲目"的条目
//!
//! `JOIN tracks`（不是 LEFT JOIN）：空专辑 / 无主艺人在浏览页没有意义——
//! 点进去是空列表。统计口径（`library_stats` 的 COUNT(*) FROM albums）
//! 与这里**允许不一致**：stats 数的是表行数，这里数的是可浏览实体。
//!
//! ## 聚合性能
//!
//! 一次 GROUP BY 全表扫描，5 万行量级实测个位数毫秒（tracks 表有
//! album_id / artist_id 索引，聚合走索引扫描）。所以不需要游标分页：
//! 前端一次拿 500 条做宫格足够，超过按名字搜索收窄。

use rusqlite::Connection;
use serde::Serialize;
use ts_rs::TS;

use crate::db::DbResult;
use crate::norm::norm;

/// 一次返回的条目上限。浏览页是宫格（一次全画），不是无限列表；
/// 500 张专辑已远超本地曲库的正常规模，真到这个量级用户该用搜索。
/// 截断不静默：响应带 `total`（过滤后总数），UI 靠 `total > items.len()` 提示。
pub const FACET_LIMIT: usize = 500;

/// 专辑浏览响应：截断后的条目 + 过滤后总数。
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct AlbumPage {
    pub items: Vec<AlbumFacet>,
    /// LIKE 过滤后的总条数（不受 [`FACET_LIMIT`] 截断影响）
    #[ts(type = "number")]
    pub total: i64,
}

/// 艺人浏览响应：形状与 [`AlbumPage`] 同理。
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct ArtistPage {
    pub items: Vec<ArtistFacet>,
    #[ts(type = "number")]
    pub total: i64,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct AlbumFacet {
    pub id: String,
    pub title: String,
    pub artist_id: Option<String>,
    pub artist: Option<String>,
    #[ts(type = "number | null")]
    pub year: Option<i64>,
    pub artwork_id: Option<String>,
    #[ts(type = "number")]
    pub track_count: i64,
    /// 专辑内曲目总时长。空专辑不会出现（JOIN tracks），恒 ≥ 0
    #[ts(type = "number")]
    pub total_duration_ms: i64,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct ArtistFacet {
    pub id: String,
    pub name: String,
    pub artwork_id: Option<String>,
    #[ts(type = "number")]
    pub track_count: i64,
    /// 名下**有曲目的**专辑数（DISTINCT album_id）；与 albums 表行数可能不同
    #[ts(type = "number")]
    pub album_count: i64,
    #[ts(type = "number")]
    pub total_duration_ms: i64,
}

/// LIKE 通配符按字面转义（与 repo::build_where 同一套规则；独立成小函数
/// 是因为两边的 WHERE 构造完全不同，为此抽公共模块得不偿失）。
fn escape_like(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in norm(s).chars() {
        match c {
            '%' => out.push_str("\\%"),
            '_' => out.push_str("\\_"),
            '\\' => out.push_str("\\\\"),
            _ => out.push(c),
        }
    }
    out
}

fn like_pattern(search: Option<&str>) -> Option<String> {
    search
        .map(escape_like)
        .filter(|s| !s.is_empty())
        .map(|s| format!("%{s}%"))
}

/// 专辑浏览（按名称排序）。`search` 为空/全空白 = 不过滤。
///
/// total 语义：过滤后**全部**条数；items 是按名称序的前 [`FACET_LIMIT`] 条。
/// 实现上先取全量再 Rust 侧截断（而不是 SQL LIMIT + 第二条 COUNT）——聚合行
/// 很小，一万张专辑也就 MB 级，一条查询换两个数字，省掉 WHERE 复制。
pub fn list_albums(conn: &Connection, search: Option<&str>) -> DbResult<AlbumPage> {
    let like = like_pattern(search);
    let sql = format!(
        "SELECT al.id, al.title, al.artist_id, ar.name, al.year, al.artwork_id, \
                COUNT(t.id), COALESCE(SUM(t.duration_ms), 0) \
         FROM albums al \
         JOIN tracks t ON t.album_id = al.id \
         LEFT JOIN artists ar ON ar.id = al.artist_id \
         {} \
         GROUP BY al.id \
         ORDER BY al.title_norm ASC, al.id ASC",
        if like.is_some() {
            "WHERE al.title_norm LIKE ?1 ESCAPE '\\'"
        } else {
            ""
        },
    );
    let mut stmt = conn.prepare(&sql)?;
    if let Some(pattern) = &like {
        stmt.raw_bind_parameter(1, pattern)?;
    }
    let mut rows = stmt.raw_query();
    let mut out = Vec::new();
    while let Some(r) = rows.next()? {
        out.push(AlbumFacet {
            id: r.get(0)?,
            title: r.get(1)?,
            artist_id: r.get(2)?,
            artist: r.get(3)?,
            year: r.get(4)?,
            artwork_id: r.get(5)?,
            track_count: r.get(6)?,
            total_duration_ms: r.get(7)?,
        });
    }
    let total = out.len() as i64;
    out.truncate(FACET_LIMIT);
    Ok(AlbumPage { items: out, total })
}

/// 艺人浏览（按名称排序）。`artist_id IS NULL` 的曲目（扫描时没有艺人标签）
/// 不归属任何条目，不出现在这里——浏览页是"有名有姓"的实体。
pub fn list_artists(conn: &Connection, search: Option<&str>) -> DbResult<ArtistPage> {
    let like = like_pattern(search);
    let sql = format!(
        "SELECT ar.id, ar.name, ar.artwork_id, \
                COUNT(t.id), COUNT(DISTINCT t.album_id), COALESCE(SUM(t.duration_ms), 0) \
         FROM artists ar \
         JOIN tracks t ON t.artist_id = ar.id \
         {} \
         GROUP BY ar.id \
         ORDER BY ar.name_norm ASC, ar.id ASC",
        if like.is_some() {
            "WHERE ar.name_norm LIKE ?1 ESCAPE '\\'"
        } else {
            ""
        },
    );
    let mut stmt = conn.prepare(&sql)?;
    if let Some(pattern) = &like {
        stmt.raw_bind_parameter(1, pattern)?;
    }
    let mut rows = stmt.raw_query();
    let mut out = Vec::new();
    while let Some(r) = rows.next()? {
        out.push(ArtistFacet {
            id: r.get(0)?,
            name: r.get(1)?,
            artwork_id: r.get(2)?,
            track_count: r.get(3)?,
            album_count: r.get(4)?,
            total_duration_ms: r.get(5)?,
        });
    }
    let total = out.len() as i64;
    out.truncate(FACET_LIMIT);
    Ok(ArtistPage { items: out, total })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_in_memory;

    fn fixtures(conn: &Connection) {
        conn.execute_batch(
            r#"
            INSERT INTO artists (id, name, name_norm) VALUES
              ('a1','周杰伦','周杰伦'),
              ('a2','Beethoven','beethoven');
            INSERT INTO albums (id, title, title_norm, artist_id, year) VALUES
              ('al1','叶惠美','叶惠美','a1',2003),
              ('al2','第九交响曲','第九交响曲','a2',1824),
              ('al3','空专辑','空专辑','a1',2020);
            INSERT INTO tracks (id,title,title_norm,artist_id,album_id,duration_ms,added_at,path,path_lower,size_bytes,mtime) VALUES
              ('t1','晴天','晴天','a1','al1',269000,100,'/m/1.flac','/m/1.flac',1,1),
              ('t2','以父之名','以父之名','a1','al1',342000,110,'/m/2.flac','/m/2.flac',1,1),
              ('t3','Adagio','adagio','a2','al2',1000,120,'/m/3.mp3','/m/3.mp3',1,1);
            "#,
        )
        .unwrap();
    }

    #[test]
    fn albums_aggregate_and_skip_empty() {
        let conn = open_in_memory().unwrap();
        fixtures(&conn);
        let got = list_albums(&conn, None).unwrap();
        // 空专辑（无曲目）不出现
        assert_eq!(got.items.len(), 2);
        assert_eq!(got.total, 2);
        let al1 = got.items.iter().find(|a| a.id == "al1").unwrap();
        assert_eq!(al1.track_count, 2);
        assert_eq!(al1.total_duration_ms, 611_000);
        assert_eq!(al1.artist.as_deref(), Some("周杰伦"));
        assert_eq!(al1.year, Some(2003));
    }

    #[test]
    fn artists_aggregate_with_album_count() {
        let conn = open_in_memory().unwrap();
        fixtures(&conn);
        let got = list_artists(&conn, None).unwrap();
        assert_eq!(got.items.len(), 2);
        assert_eq!(got.total, 2);
        let a1 = got.items.iter().find(|a| a.id == "a1").unwrap();
        assert_eq!(a1.track_count, 2);
        // al3 是空专辑，不计入"有曲目的专辑数"
        assert_eq!(a1.album_count, 1);
        let a2 = got.items.iter().find(|a| a.id == "a2").unwrap();
        assert_eq!(a2.track_count, 1);
        assert_eq!(a2.album_count, 1);
    }

    #[test]
    fn total_counts_all_matches_even_when_truncated() {
        // 截断语义：total 是过滤后总数，items 只留前 FACET_LIMIT 条
        let conn = open_in_memory().unwrap();
        conn.execute_batch("BEGIN").unwrap();
        for i in 0..520 {
            conn.execute(
                "INSERT INTO albums (id, title, title_norm) VALUES (?1, ?2, ?2)",
                rusqlite::params![format!("al{i}"), format!("专辑{i:03}")],
            )
            .unwrap();
            // 每张专辑挂一首曲目（JOIN tracks 才可见）；path/path_lower 有唯一约束，逐曲目编号
            conn.execute(
                "INSERT INTO tracks (id,title,title_norm,album_id,duration_ms,added_at,path,path_lower,size_bytes,mtime) \
                 VALUES (?1,'x','x',?2,1,1,?3,?3,1,1)",
                rusqlite::params![format!("t{i}"), format!("al{i}"), format!("/m/{i}.flac")],
            )
            .unwrap();
        }
        conn.execute_batch("COMMIT").unwrap();
        let got = list_albums(&conn, Some("专辑")).unwrap();
        assert_eq!(got.items.len(), FACET_LIMIT);
        assert_eq!(got.total, 520);
        // 截断按名称序：留下的是前 500 条
        assert_eq!(got.items.first().unwrap().id, "al0");
        assert_eq!(got.items.last().unwrap().id, "al499");
    }

    #[test]
    fn search_filters_by_norm_name() {
        let conn = open_in_memory().unwrap();
        fixtures(&conn);
        let got = list_albums(&conn, Some("叶惠")).unwrap();
        assert_eq!(got.items.len(), 1);
        assert_eq!(got.items[0].id, "al1");

        let got = list_artists(&conn, Some("BEETHOVEN")).unwrap();
        assert_eq!(got.items.len(), 1);
        assert_eq!(got.items[0].id, "a2");
    }

    #[test]
    fn search_wildcards_are_literal() {
        let conn = open_in_memory().unwrap();
        fixtures(&conn);
        assert!(list_artists(&conn, Some("周_伦")).unwrap().items.is_empty());
        assert!(list_albums(&conn, Some("%叶%")).unwrap().items.is_empty());
    }

    #[test]
    fn blank_search_means_no_filter() {
        let conn = open_in_memory().unwrap();
        fixtures(&conn);
        assert_eq!(list_albums(&conn, Some("   ")).unwrap().items.len(), 2);
        assert_eq!(list_artists(&conn, Some("")).unwrap().items.len(), 2);
        assert_eq!(list_albums(&conn, None).unwrap().items.len(), 2);
    }
}
