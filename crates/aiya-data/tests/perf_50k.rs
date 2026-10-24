//! 5 万首性能验收（对照旧 Electron 基线：冷启动 366–403ms、深分页 2.3ms）。
//!
//! 运行：cargo test -p aiya-data --release --test perf_50k -- --ignored --nocapture
//!
//! 口径说明：旧基线"冷启动"含 Electron 主进程建库 + 首屏查询 + renderer 首帧，
//! 本测只覆盖 Rust 侧可对比的部分（建库灌数 + 冷连接首屏 + 深分页 + 搜索），
//! UI 首帧待 P1 端到端时用真实 app 计时。

use std::time::Instant;

use aiya_data::db::open_in_memory;
use rusqlite::Connection;

/// 直接 SQL 灌 5 万行（不走 scanner：验收对象是查询层，不是解析器）。
fn seed(conn: &Connection, n: usize) -> std::time::Duration {
    let started = Instant::now();
    conn.execute_batch("BEGIN;").unwrap();
    // 500 专辑 × 100 首；艺人 2000 个
    for i in 0..n {
        let artist_no = i / 25; // 每艺人 25 首
        let album_no = i / 100; // 每专辑 100 首
        let artist = format!("艺术家{:05}", artist_no % 2000);
        let album = format!("专辑{:05}", album_no);
        let title = if i % 7 == 0 {
            format!("Track 晴天{:06}", i) // 1/7 命中"晴天"搜索词
        } else {
            format!("曲目{:06}", i)
        };
        let title_norm = aiya_data::norm::norm(&title);
        // 父表先行（FK ON）
        conn.execute(
            "INSERT OR IGNORE INTO artists (id,name,name_norm) VALUES (?1,?2,?3)",
            rusqlite::params![aiya_data::ids::artist_id(&artist), artist, aiya_data::norm::norm(&artist)],
        )
        .unwrap();
        conn.execute(
            "INSERT OR IGNORE INTO albums (id,title,title_norm,artist_id) VALUES (?1,?2,?3,?4)",
            rusqlite::params![
                aiya_data::ids::album_id(&album, &artist),
                album,
                aiya_data::norm::norm(&album),
                aiya_data::ids::artist_id(&artist)
            ],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO tracks (id,title,title_norm,artist_id,album_id,duration_ms,added_at,
               path,path_lower,size_bytes,mtime,availability)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?8,1,1,'online')",
            rusqlite::params![
                format!("t{:06}", i),
                title,
                title_norm,
                aiya_data::ids::artist_id(&artist),
                aiya_data::ids::album_id(&album, &artist),
                180_000 + (i % 120) as i64 * 1000,
                1000 + i as i64,
                format!("D:/music/{:06}.flac", i),
            ],
        )
        .unwrap();
    }
    conn.execute_batch("COMMIT;").unwrap();
    started.elapsed()
}

fn q_page(conn: &Connection, offset: usize) -> std::time::Duration {
    let started = Instant::now();
    let q = aiya_data::repo::TrackWindowQuery {
        sort: aiya_data::repo::Sort::Title,
        direction: aiya_data::repo::Direction::Asc,
        search: None,
        artist_id: None,
        album_id: None,
        source: aiya_data::repo::TrackSource::Library,
        limit: 200,
        // 深分页必须用 Offset 模式：Cursor 模式**根本不读 offset**，
        // 之前这里写的是 Cursor(offset)，三次"深分页"测的其实都是首页，
        // assert 全部自证通过（假绿）。
        mode: aiya_data::repo::Mode::Offset,
        cursor: None,
        offset: Some(offset),
    };
    let page = aiya_data::repo::list_tracks(conn, &q).unwrap();
    assert_eq!(page.items.len(), 200);
    assert!(page.total >= 50_000);
    started.elapsed()
}

#[test]
#[ignore = "性能验收：cargo test -p aiya-data --release --test perf_50k -- --ignored --nocapture"]
fn perf_50k_baseline() {
    let conn = open_in_memory().unwrap();

    let seed_t = seed(&conn, 50_000);
    println!("灌 50000 行:      {:>8.1?}", seed_t);

    // 冷连接（内存库无法重开；用"新查询句柄"近似冷查询——含语句准备）
    let t1 = q_page(&conn, 0);
    println!("首屏 200 首:      {:>8.1?}  （基线口径：全链路 366-403ms，本测仅查询层）", t1);

    // 深分页：offset 49000（对照旧基线深分页 2.3ms）
    let mut worst = std::time::Duration::ZERO;
    for off in [25_000, 40_000, 49_800] {
        let t = q_page(&conn, off);
        println!("深分页 @{:>5}:     {:>8.1?}", off, t);
        worst = worst.max(t);
    }
    // 游标模式（推荐路径，无 OFFSET 扫描）；cursor = base64url(JSON[value,id])。
    // value/id 从库里**真实取**中位的 title_norm：之前手写成 `曲目049999`，
    // 夹具并不保证那一行存在，测的是空页（items.len()==0 也照样通过）。
    let mid_title_norm: String = conn
        .query_row(
            "SELECT title_norm FROM tracks ORDER BY title_norm ASC, id ASC LIMIT 1 OFFSET 25000",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let cursor_json = serde_json::to_string(&[mid_title_norm.clone(), String::new()]).unwrap();
    use base64::Engine as _;
    let cursor = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(cursor_json);
    let qc = aiya_data::repo::TrackWindowQuery {
        sort: aiya_data::repo::Sort::Title,
        direction: aiya_data::repo::Direction::Asc,
        search: None,
        artist_id: None,
        album_id: None,
        source: aiya_data::repo::TrackSource::Library,
        limit: 200,
        mode: aiya_data::repo::Mode::Cursor,
        cursor: Some(cursor),
        offset: None,
    };
    let t_cursor = Instant::now();
    let page = aiya_data::repo::list_tracks(&conn, &qc).unwrap();
    let cursor_t = t_cursor.elapsed();
    // 空页不能算通过：命中真实中位游标时必须是满页
    assert_eq!(page.items.len(), 200, "游标续页必须返回满页（空页=假绿）");
    println!("游标续页(中位):   {:>8.1?}  （{} 行）", cursor_t, page.items.len());

    // 搜索（LIKE '%晴天%'，title_norm 索引前缀 + 全表 LIKE 回退）
    let t_search = Instant::now();
    let qs = aiya_data::repo::TrackWindowQuery {
        sort: aiya_data::repo::Sort::Title,
        direction: aiya_data::repo::Direction::Asc,
        search: Some("晴天".into()),
        artist_id: None,
        album_id: None,
        source: aiya_data::repo::TrackSource::Library,
        limit: 50,
        mode: aiya_data::repo::Mode::Cursor,
        cursor: None,
        offset: None,
    };
    let spage = aiya_data::repo::list_tracks(&conn, &qs).unwrap();
    let search_t = t_search.elapsed();
    println!("搜索\"晴天\":       {:>8.1?}  （命中 {} 行）", search_t, spage.total);
    assert!(spage.total >= 7_000, "expect ~7143 hits, got {}", spage.total);

    // 验收门槛（对照旧基线 cargo 侧口径）：
    // * 首屏 / 游标续页走索引 seek，必须与旧基线同量级（旧基线深分页 2.3ms）；
    // * offset 深分页天然 O(offset)（SQLite 要跳 49800 条索引项），只作为
    //   "兜底路径"设宽门槛；UI 的曲库列表必须用 cursor 模式。
    assert!(t1.as_millis() < 60, "首屏 {}ms 过慢", t1.as_millis());
    assert!(worst.as_millis() < 100, "深分页(offset) worst {}ms 过慢", worst.as_millis());
    assert!(cursor_t.as_millis() < 10, "游标续页 {}ms 过慢", cursor_t.as_millis());
    assert!(search_t.as_millis() < 100, "搜索 {}ms 过慢", search_t.as_millis());
}
