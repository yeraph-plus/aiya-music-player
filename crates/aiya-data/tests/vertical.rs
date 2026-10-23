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
        scan_directory(conn, dir.path(), |_| {}).map_err(|e| aiya_data::DbError::Custom(e.to_string()))
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
