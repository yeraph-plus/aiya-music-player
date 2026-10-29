//! 纵切集成测试：临时目录造真实 MP3 → open()（带线程的 DB）→ 扫描 → 查询 →
//! 歌单创建 → 列表断言。覆盖 open/scanner/repo/ids 四模块协作。

use std::io::Write as _;

use aiya_data::repo::{list_tracks, Mode, Sort, TrackSource, TrackWindowQuery};
use aiya_data::scanner::scan_directory;

/// 生成最小合法 MP3（同 scanner 单测：ID3v2.4 + 真实 MPEG 帧）。
fn write_mp3(dir: &std::path::Path, name: &str, title: &str, artist: &str, album: &str) {
    let path = dir.join(name);
    let mut f = std::fs::File::create(&path).unwrap();
    let mut body: Vec<u8> = Vec::new();
    for (id, content) in [
        ("TIT2", title.as_bytes()),
        ("TPE1", artist.as_bytes()),
        ("TALB", album.as_bytes()),
    ] {
        body.extend_from_slice(id.as_bytes());
        body.extend_from_slice(&((content.len() + 1) as u32).to_be_bytes());
        body.extend_from_slice(&[0, 0]);
        body.push(0x03);
        body.extend_from_slice(content);
    }
    let size = body.len() as u32;
    f.write_all(b"ID3\x04\x00\x00").unwrap();
    f.write_all(&[
        ((size >> 21) & 0x7f) as u8,
        ((size >> 14) & 0x7f) as u8,
        ((size >> 7) & 0x7f) as u8,
        (size & 0x7f) as u8,
    ])
    .unwrap();
    f.write_all(&body).unwrap();
    let frame = {
        let mut fr = vec![0u8; 417];
        fr[0] = 0xff;
        fr[1] = 0xfb;
        fr[2] = 0x90;
        fr[3] = 0xc0;
        fr
    };
    for _ in 0..20 {
        f.write_all(&frame).unwrap();
    }
}

#[test]
fn vertical_scan_query_playlist() {
    let dir = tempfile::tempdir().unwrap();
    write_mp3(dir.path(), "01.mp3", "晴天", "周杰伦", "叶惠美");
    write_mp3(dir.path(), "02.mp3", "七里香", "周杰伦", "七里香");
    write_mp3(dir.path(), "03.mp3", "Nocturne", "Secret Garden", "Songs");

    let db_path = dir.path().join("library.sqlite");
    let mut db = aiya_data::open(&db_path).unwrap();

    // 1) 扫描走 DB 线程
    let scan_summary: aiya_data::DbResult<aiya_data::scanner::ScanResult> = db.send(move |conn| {
        scan_directory(conn, dir.path(), None, |_| {}).map_err(|e| aiya_data::DbError::Custom(e.to_string()))
    });
    let scan = scan_summary.unwrap();
    assert_eq!(scan.imported, 3, "errors={:?}", scan.errors);
    assert_eq!(scan.failed, 0);

    // 2) 列表 + 搜索
    let page = db
        .send(|conn| {
            list_tracks(
                conn,
                &TrackWindowQuery {
                    sort: Sort::Title,
                    direction: aiya_data::repo::Direction::Asc,
                    search: Some("周杰伦".into()),
                    artist_id: None,
                    album_id: None,
            added_from_ms: None,
            added_to_ms: None,
            last_played_from_ms: None,
            last_played_to_ms: None,
            last_played_never: None,
            conds: None,
                    source: TrackSource::Library,
                    limit: 10,
                    mode: Mode::Offset,
                    cursor: None,
                    offset: Some(0),
                },
            )
        })
        .unwrap();
    assert_eq!(page.total, 2, "search by artist name");

    // 3) 全量 ID（点行播整库的输入）
    let ids_page = db
        .send(|conn| {
            list_tracks(
                conn,
                &TrackWindowQuery {
                    sort: Sort::Added,
                    direction: aiya_data::repo::Direction::Asc,
                    search: None,
                    artist_id: None,
                    album_id: None,
            added_from_ms: None,
            added_to_ms: None,
            last_played_from_ms: None,
            last_played_to_ms: None,
            last_played_never: None,
            conds: None,
                    source: TrackSource::Library,
                    limit: 500,
                    mode: Mode::Offset,
                    cursor: None,
                    offset: Some(0),
                },
            )
        })
        .unwrap();
    let all_ids: Vec<String> = ids_page.items.iter().map(|t| t.id.clone()).collect();
    assert_eq!(all_ids.len(), 3);

    // 4) 建歌单（复用 playlist_create 的表语义：直接插）
    let qingtian_id = ids_page
        .items
        .iter()
        .find(|t| t.title == "晴天")
        .map(|t| t.id.clone())
        .expect("晴天 in library");
    db.send(move |conn| {
        conn.execute_batch(&format!(
            "INSERT INTO playlists (id,name,name_norm,created_at,updated_at) VALUES ('p1','测试单','测试单',1,1);
             INSERT INTO playlist_items (id,playlist_id,track_id,position,added_at) VALUES
               ('i1','p1','{qingtian_id}',0,1);"
        ))
        .map_err(aiya_data::DbError::Sql)
    })
    .unwrap();
    let pl_page = db
        .send(|conn| {
            list_tracks(
                conn,
                &TrackWindowQuery {
                    sort: Sort::Title,
                    direction: aiya_data::repo::Direction::Asc,
                    search: None,
                    artist_id: None,
                    album_id: None,
            added_from_ms: None,
            added_to_ms: None,
            last_played_from_ms: None,
            last_played_to_ms: None,
            last_played_never: None,
            conds: None,
                    source: TrackSource::Playlist { playlist_id: "p1".into() },
                    limit: 10,
                    mode: Mode::Offset,
                    cursor: None,
                    offset: Some(0),
                },
            )
        })
        .unwrap();
    assert_eq!(pl_page.total, 1);
    assert_eq!(pl_page.items[0].title, "晴天");

    // 5) 关库（验证 close 不挂）
    db.close();
}

/// 智能歌单纵切（docs/40 §1 方案 A）：beets 同步 genre → 规则建单 →
/// Playlist 来源按 kind 自动路由到规则查询 → 手动歌单守卫仍然生效。
#[test]
fn vertical_smart_playlist_rule_query() {
    let dir = tempfile::tempdir().unwrap();
    write_mp3(dir.path(), "01.mp3", "Track A", "Artist One", "Album X");
    write_mp3(dir.path(), "02.mp3", "Track B", "Artist Two", "Album X");
    write_mp3(dir.path(), "03.mp3", "Track C", "Artist One", "Album Y");

    let db_path = dir.path().join("library.sqlite");
    let mut db = aiya_data::open(&db_path).unwrap();

    // 1) 扫描 + beets 同步（给 01/03 打 Rock 标签，02 不打——模拟"beets 是增强不是依赖"）
    db.send(move |conn| {
        scan_directory(conn, dir.path(), None, |_| {})
            .map_err(|e| aiya_data::DbError::Custom(e.to_string()))?;
        let items = vec![
            aiya_data::beets_sync::BeetsItem {
                id: 1,
                path: dir.path().join("01.mp3").to_string_lossy().into_owned(),
                title: Some("Track A".into()),
                artist: Some("Artist One".into()),
                albumartist: None,
                album: Some("Album X".into()),
                genre: Some("Rock".into()),
                year: Some(2020),
                track: Some(1),
                disc: None,
                length: Some(12.0),
                bitrate: None,
                format: Some("MP3".into()),
                samplerate: None,
                bitdepth: None,
                channels: None,
                mtime: None,
                rg_track_gain: None,
            },
            aiya_data::beets_sync::BeetsItem {
                id: 2,
                path: dir.path().join("02.mp3").to_string_lossy().into_owned(),
                title: Some("Track B".into()),
                artist: Some("Artist Two".into()),
                albumartist: None,
                album: Some("Album X".into()),
                genre: None,
                year: None,
                track: Some(2),
                disc: None,
                length: Some(12.0),
                bitrate: None,
                format: Some("MP3".into()),
                samplerate: None,
                bitdepth: None,
                channels: None,
                mtime: None,
                rg_track_gain: None,
            },
            aiya_data::beets_sync::BeetsItem {
                id: 3,
                path: dir.path().join("03.mp3").to_string_lossy().into_owned(),
                title: Some("Track C".into()),
                artist: Some("Artist One".into()),
                albumartist: None,
                album: Some("Album Y".into()),
                genre: Some("rock".into()),
                year: Some(2021),
                track: Some(1),
                disc: None,
                length: Some(12.0),
                bitrate: None,
                format: Some("MP3".into()),
                samplerate: None,
                bitdepth: None,
                channels: None,
                mtime: None,
                rg_track_gain: None,
            },
        ];
        let stats = aiya_data::beets_sync::sync_items(conn, &items, 10)
            .map_err(|e| aiya_data::DbError::Custom(e.to_string()))?;
        // 扫描器已按路径建行 → beets 同步走"命中更新"（补 genre），不是新导入
        assert_eq!(stats.matched, 3, "三首全部按路径对账命中：{:?}", stats);
        assert_eq!(stats.updated, 3, "genre/beets_item_id 等字段有差异必须更新：{:?}", stats);
        Ok::<(), aiya_data::DbError>(())
    })
    .unwrap();

    // 2) 建智能歌单：genre eq "Rock"（norm 后大小写不敏感）+ 按标题升序
    db.send(|conn| {
        aiya_data::playlist::create_smart(
            conn,
            "smart1",
            "我的摇滚",
            &serde_json::json!({
                "all": [{ "field": "genre", "op": "eq", "value": "Rock" }],
                "sort": "title",
                "direction": "asc"
            }),
            10,
        )
        .map_err(|e| aiya_data::DbError::Custom(e.to_string()))
    })
    .unwrap();

    // 3) UI 发的还是 kind:'playlist' —— 数据层按 kind 自动路由到规则查询
    let page = db
        .send(|conn| {
            list_tracks(
                conn,
                &TrackWindowQuery {
                    sort: Sort::Title,
                    direction: aiya_data::repo::Direction::Asc,
                    search: None,
                    artist_id: None,
                    album_id: None,
                    added_from_ms: None,
                    added_to_ms: None,
                    last_played_from_ms: None,
                    last_played_to_ms: None,
                    last_played_never: None,
            conds: None,
                    source: TrackSource::Playlist { playlist_id: "smart1".into() },
                    limit: 10,
                    mode: Mode::Offset,
                    cursor: None,
                    offset: Some(0),
                },
            )
        })
        .unwrap();
    assert_eq!(page.total, 2, "只有打了 Rock 标签的 01/03 命中：{:?}", page.items);
    assert_eq!(page.items[0].title, "Track A");
    assert_eq!(page.items[1].title, "Track C", "genre 'rock' 与 'Rock' 同 norm，按标题排序");

    // 4) 规则改条件（artist contains）后查询跟着变
    db.send(|conn| {
        aiya_data::playlist::set_rule(
            conn,
            "smart1",
            &serde_json::json!({
                "all": [{ "field": "artist", "op": "contains", "value": "two" }],
                "limit": 5
            }),
            11,
        )
        .map_err(|e| aiya_data::DbError::Custom(e.to_string()))
    })
    .unwrap();
    let page = db
        .send(|conn| {
            list_tracks(
                conn,
                &TrackWindowQuery {
                    sort: Sort::Added,
                    direction: aiya_data::repo::Direction::Desc,
                    search: None,
                    artist_id: None,
                    album_id: None,
                    added_from_ms: None,
                    added_to_ms: None,
                    last_played_from_ms: None,
                    last_played_to_ms: None,
                    last_played_never: None,
            conds: None,
                    source: TrackSource::Playlist { playlist_id: "smart1".into() },
                    limit: 10,
                    mode: Mode::Offset,
                    cursor: None,
                    offset: Some(0),
                },
            )
        })
        .unwrap();
    assert_eq!(page.total, 1);
    assert_eq!(page.items[0].title, "Track B");

    // 5) 手动歌单守卫：set_rule / add_tracks 对手动歌单明确报错
    db.send(|conn| {
        aiya_data::playlist::create(conn, "p_manual", "手动单", &[], 10)
            .map_err(|e| aiya_data::DbError::Custom(e.to_string()))
    })
    .unwrap();
    let err = db
        .send(|conn| {
            aiya_data::playlist::set_rule(
                conn,
                "p_manual",
                &serde_json::json!({}),
                11,
            )
            .map_err(|e| aiya_data::DbError::Custom(e.to_string()))
        })
        .unwrap_err();
    assert!(err.to_string().contains("手动歌单"), "实际错误：{err}");
    let err = db
        .send(|conn| {
            aiya_data::playlist::add_tracks(conn, "smart1", &[], aiya_data::playlist::AddMode::Append, 12)
                .map_err(|e| aiya_data::DbError::Custom(e.to_string()))
        })
        .unwrap_err();
    assert!(err.to_string().contains("智能歌单"), "实际错误：{err}");

    db.close();
}

/// P1 纵切回归：文件改名/移动后重扫，必须 relocate 到同一行——
/// 播放统计与歌单引用不丢，不产生重复行（scanner 单测有同款，
/// 这里走 open()+DB 线程的真实入口再锁一遍）。
#[test]
fn vertical_rename_rescan_keeps_same_row() {
    let dir = tempfile::tempdir().unwrap();
    write_mp3(dir.path(), "01.mp3", "晴天", "周杰伦", "叶惠美");
    let root = dir.path().to_path_buf();

    let db_path = dir.path().join("library.sqlite");
    let mut db = aiya_data::open(&db_path).unwrap();

    let scan_root = root.clone();
    db.send(move |conn| {
        scan_directory(conn, &scan_root, None, |_| {})
            .map_err(|e| aiya_data::DbError::Custom(e.to_string()))
    })
    .unwrap();

    let track_id: String = db
        .send(|conn| Ok(conn.query_row("SELECT id FROM tracks", [], |r| r.get(0))?))
        .unwrap();
    let tid_for_ref = track_id.clone();
    db.send(move |conn| {
        conn.execute("UPDATE tracks SET play_count = 9 WHERE id = ?1", [&tid_for_ref])
            .map_err(aiya_data::DbError::Sql)?;
        conn.execute_batch(
            "INSERT INTO playlists (id,name,name_norm,created_at,updated_at) VALUES ('p1','单','单',1,1);
             INSERT INTO playlist_items (id,playlist_id,track_id,position,added_at)
             SELECT 'i1','p1',id,0,1 FROM tracks;",
        )
        .map_err(aiya_data::DbError::Sql)
    })
    .unwrap();

    // 改名（同目录，mtime/size 不变）→ 重扫
    std::fs::rename(dir.path().join("01.mp3"), dir.path().join("02.mp3")).unwrap();
    let scan_root = root.clone();
    let scan: aiya_data::DbResult<aiya_data::scanner::ScanResult> = db.send(move |conn| {
        scan_directory(conn, &scan_root, None, |_| {})
            .map_err(|e| aiya_data::DbError::Custom(e.to_string()))
    });
    let s = scan.unwrap();
    assert_eq!(s.imported, 0, "改名不得生成新行：{:?}", s.errors);
    assert_eq!(s.missing, 0, "relocate 后没有行需要标 missing");

    let (id, avail, plays, refs): (String, String, i64, i64) = db
        .send(|conn| {
            Ok(conn.query_row(
                "SELECT t.id, t.availability, t.play_count, COUNT(pi.id)
                 FROM tracks t
                 LEFT JOIN playlist_items pi ON pi.track_id = t.id",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )?)
        })
        .unwrap();
    assert_eq!(id, track_id, "行身份不变（播放统计/歌单引用不丢）");
    assert_eq!(avail, "online");
    assert_eq!(plays, 9);
    assert_eq!(refs, 1, "歌单引用仍指向同一行");
    let count: i64 = db.send(|conn| Ok(conn.query_row("SELECT COUNT(*) FROM tracks", [], |r| r.get(0))?)).unwrap();
    assert_eq!(count, 1, "不产生重复行");

    db.close();
}
