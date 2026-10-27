//! 歌单写操作（repo 层）。
//!
//! 契约来自旧仓库 `packages/database/src/playlists.repo.ts` + 其 255 条测试里的
//! 播放列表规格（`tests/playlists.test.ts`）。重写版此前只实现了 list/detail/create，
//! 其余 7 条写命令在 `generate_handler!` 里根本不存在 → UI 一调就是 Tauri 层的
//! promise reject（不是 `ok:false` 信封，UI 的 `result.ok` 分支连错误提示都走不到）。
//!
//! 四条不变量，列表查询与 UI 的下标寻址都依赖它们：
//!
//! 1. `playlist_items.position` 恒为 `0..n-1` 稠密序列。UI 的 `removeAt` / `move`
//!    传的是"第几首"（下标），出现空洞就会删错行、移错位。
//! 2. item 的 `id` **不能由 position 派生**。位置会重排，位置派生的 id 在
//!    "删掉某一项、又加回同一首"时会与残留行撞主键，`INSERT OR IGNORE`
//!    会把它静默吞掉（曲目加了但列表里没有）。旧仓库用随机 id，同此。
//! 3. 同一首曲目在同一歌单里只出现一次（加歌去重），这是产品约定：
//!    契约的 `PlaylistAddResult { added, skipped }` 里 `skipped` 就是这个语义。
//! 4. `playlists.updated_at` 随任何内容变化更新；侧栏顺序由 `sort_position`
//!    承载（`reorder` 整体重写 0..n-1），列表按 `(sort_position, created_at)` 排。

use std::collections::HashSet;

use rusqlite::{params, Connection};

use crate::db::{DbError, DbResult};
use crate::ids;
use crate::norm::norm;

/// 与旧仓库一致：名称按字符截断到 80，避免把 UTF-8 多字节字符切成半个。
pub const MAX_NAME_CHARS: usize = 80;

/// 加歌位置语义。`Append` = 队尾追加（UI 默认）；`Next` = 插到队首。
/// 歌单本身没有"当前播放游标"，所以 `next` 只能解释成"接下来就听这些"。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddMode {
    Append,
    Next,
}

impl AddMode {
    pub fn parse(raw: Option<&str>) -> DbResult<Self> {
        match raw.map(str::trim) {
            None | Some("") | Some("append") => Ok(AddMode::Append),
            Some("next") => Ok(AddMode::Next),
            Some(other) => Err(DbError::Custom(format!("未知的加歌模式: {other}"))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AddOutcome {
    pub added: i64,
    pub skipped: i64,
}

/// 名称规范化 + 截断。空名直接报错（UI 侧也有校验，但命令层不能依赖它）。
pub fn sanitize_name(raw: &str) -> DbResult<String> {
    let name = raw.trim();
    if name.is_empty() {
        return Err(DbError::Custom("歌单名不能为空".into()));
    }
    Ok(name.chars().take(MAX_NAME_CHARS).collect())
}

pub fn exists(conn: &Connection, playlist_id: &str) -> DbResult<bool> {
    match conn.query_row("SELECT 1 FROM playlists WHERE id = ?1", [playlist_id], |_| Ok(())) {
        Ok(()) => Ok(true),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(false),
        Err(e) => Err(DbError::Sql(e)),
    }
}

/// 建歌单 + 批量追加曲目。同名（同 id）已存在时不重复建行，
/// 但曲目仍按去重规则追加 —— 与 `playlist_create` 的 ON CONFLICT DO NOTHING 行为一致。
pub fn create(
    conn: &mut Connection,
    playlist_id: &str,
    name: &str,
    track_ids: &[String],
    now: i64,
) -> DbResult<AddOutcome> {
    let name_norm = norm(name);
    let tx = conn.transaction().map_err(DbError::Sql)?;
    // 侧栏顺序：追加到最后（sort_position 是显式列，不能靠 created_at 猜）
    let next_sort: i64 = tx.query_row(
        "SELECT COALESCE(MAX(sort_position), -1) + 1 FROM playlists",
        [],
        |r| r.get(0),
    )?;
    tx.execute(
        "INSERT INTO playlists (id, name, name_norm, created_at, updated_at, sort_position)
         VALUES (?1, ?2, ?3, ?4, ?4, ?5)
         ON CONFLICT(id) DO NOTHING",
        params![playlist_id, name, name_norm, now, next_sort],
    )?;
    let outcome = append_tracks(&tx, playlist_id, track_ids, now, AddMode::Append)?;
    touch(&tx, playlist_id, now)?;
    tx.commit().map_err(DbError::Sql)?;
    Ok(outcome)
}

pub fn rename(conn: &Connection, playlist_id: &str, name: &str, now: i64) -> DbResult<bool> {
    let name = sanitize_name(name)?;
    let changed = conn.execute(
        "UPDATE playlists SET name = ?2, name_norm = ?3, updated_at = ?4 WHERE id = ?1",
        params![playlist_id, name, norm(&name), now],
    )?;
    Ok(changed > 0)
}

/// 删歌单。playlist_items 靠 FK ON DELETE CASCADE 一起走。
pub fn delete(conn: &Connection, playlist_id: &str) -> DbResult<bool> {
    let changed = conn.execute("DELETE FROM playlists WHERE id = ?1", [playlist_id])?;
    Ok(changed > 0)
}

pub fn clear(conn: &mut Connection, playlist_id: &str, now: i64) -> DbResult<bool> {
    if !exists(conn, playlist_id)? {
        return Ok(false);
    }
    let tx = conn.transaction().map_err(DbError::Sql)?;
    tx.execute("DELETE FROM playlist_items WHERE playlist_id = ?1", [playlist_id])?;
    touch(&tx, playlist_id, now)?;
    tx.commit().map_err(DbError::Sql)?;
    Ok(true)
}

pub fn add_tracks(
    conn: &mut Connection,
    playlist_id: &str,
    track_ids: &[String],
    mode: AddMode,
    now: i64,
) -> DbResult<Option<AddOutcome>> {
    if !exists(conn, playlist_id)? {
        return Ok(None);
    }
    let tx = conn.transaction().map_err(DbError::Sql)?;
    let outcome = append_tracks(&tx, playlist_id, track_ids, now, mode)?;
    touch(&tx, playlist_id, now)?;
    tx.commit().map_err(DbError::Sql)?;
    Ok(Some(outcome))
}

/// 删第 `position` 项。歌单不存在 → `None`；越界 → `Some(())` 的空操作
/// （与旧仓库一致：越界不报错，返回未变化摘要）。
pub fn remove_at(
    conn: &mut Connection,
    playlist_id: &str,
    position: i64,
    now: i64,
) -> DbResult<Option<()>> {
    if !exists(conn, playlist_id)? {
        return Ok(None);
    }
    let tx = conn.transaction().map_err(DbError::Sql)?;
    let mut items = ordered_item_ids(&tx, playlist_id)?;
    let idx = position as usize;
    if position >= 0 && idx < items.len() {
        let target = items.remove(idx);
        tx.execute("DELETE FROM playlist_items WHERE id = ?1", [target])?;
        rewrite_positions(&tx, &items)?;
        touch(&tx, playlist_id, now)?;
    }
    tx.commit().map_err(DbError::Sql)?;
    Ok(Some(()))
}

/// 移动第 `from` 项到下标 `to`（移动后仍占据 `to` 这个位置，与 UI 拖拽语义一致）。
pub fn move_item(
    conn: &mut Connection,
    playlist_id: &str,
    from: i64,
    to: i64,
    now: i64,
) -> DbResult<Option<()>> {
    if !exists(conn, playlist_id)? {
        return Ok(None);
    }
    let tx = conn.transaction().map_err(DbError::Sql)?;
    let mut items = ordered_item_ids(&tx, playlist_id)?;
    let (f, t) = (from as usize, to as usize);
    if from >= 0 && to >= 0 && f < items.len() && t < items.len() && f != t {
        let moved = items.remove(f);
        items.insert(t, moved);
        rewrite_positions(&tx, &items)?;
        touch(&tx, playlist_id, now)?;
    }
    tx.commit().map_err(DbError::Sql)?;
    Ok(Some(()))
}

/// 侧栏拖动排序：按当前顺序取全部 id，移动后整体重写 `sort_position = 0..n-1`。
/// 越界/原地不动 → 空操作（不写库）。
pub fn reorder_playlists(conn: &mut Connection, from: i64, to: i64) -> DbResult<()> {
    let ids: Vec<String> = {
        let mut stmt = conn.prepare(
            "SELECT id FROM playlists ORDER BY sort_position ASC, created_at ASC",
        )?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        rows.collect::<rusqlite::Result<Vec<_>>>()?
    };
    let (f, t) = (from as usize, to as usize);
    if from < 0 || to < 0 || from == to || f >= ids.len() || t >= ids.len() {
        return Ok(());
    }
    let mut ids = ids;
    let moved = ids.remove(f);
    ids.insert(t, moved);
    let tx = conn.transaction().map_err(DbError::Sql)?;
    for (i, id) in ids.iter().enumerate() {
        tx.execute(
            "UPDATE playlists SET sort_position = ?2 WHERE id = ?1",
            params![id, i as i64],
        )?;
    }
    tx.commit().map_err(DbError::Sql)
}

/* ---------------- 内部：位置维护 ---------------- */

fn touch(conn: &Connection, playlist_id: &str, now: i64) -> DbResult<()> {
    conn.execute(
        "UPDATE playlists SET updated_at = ?2 WHERE id = ?1",
        params![playlist_id, now],
    )?;
    Ok(())
}

/// 按播放顺序取 item 行 id。加 `id ASC` 兜底：历史数据若有 position 撞车，
/// 至少顺序稳定（重排后会恢复正常）。
fn ordered_item_ids(conn: &Connection, playlist_id: &str) -> DbResult<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT id FROM playlist_items WHERE playlist_id = ?1 ORDER BY position ASC, id ASC",
    )?;
    let rows = stmt.query_map([playlist_id], |r| r.get::<_, String>(0))?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

fn rewrite_positions(conn: &Connection, item_ids: &[String]) -> DbResult<()> {
    for (i, id) in item_ids.iter().enumerate() {
        conn.execute(
            "UPDATE playlist_items SET position = ?2 WHERE id = ?1",
            params![id, i as i64],
        )?;
    }
    Ok(())
}

fn next_position(conn: &Connection, playlist_id: &str) -> DbResult<i64> {
    Ok(conn.query_row(
        "SELECT COALESCE(MAX(position), -1) + 1 FROM playlist_items WHERE playlist_id = ?1",
        [playlist_id],
        |r| r.get(0),
    )?)
}

/// 追加曲目（去重 + 过滤已从库中删除的 ID）。
///
/// 为什么过滤不存在的曲目而不是让外键报错：UI 的多选可能横跨一次重新扫描，
/// 传进来的 ID 里混着刚被删掉的曲目；整批报错会让"加入 20 首成功 19 首"变成零。
fn append_tracks(
    conn: &Connection,
    playlist_id: &str,
    track_ids: &[String],
    now: i64,
    mode: AddMode,
) -> DbResult<AddOutcome> {
    let mut outcome = AddOutcome::default();
    if track_ids.is_empty() {
        return Ok(outcome);
    }

    let mut existing: HashSet<String> = {
        let mut stmt =
            conn.prepare("SELECT track_id FROM playlist_items WHERE playlist_id = ?1")?;
        let rows = stmt.query_map([playlist_id], |r| r.get::<_, String>(0))?;
        rows.collect::<rusqlite::Result<HashSet<_>>>()?
    };

    // 先筛出真正要插的（去重/空/不存在都算 skipped），再决定位置 ——
    // 队首插入要按"实际新增条数"整体后移旧行，先插后移会出现 position 撞车。
    let mut fresh: Vec<&String> = Vec::with_capacity(track_ids.len());
    for tid in track_ids {
        if tid.is_empty() || existing.contains(tid) || !track_exists(conn, tid)? {
            outcome.skipped += 1;
            continue;
        }
        existing.insert(tid.clone());
        fresh.push(tid);
    }
    if fresh.is_empty() {
        return Ok(outcome);
    }

    let start = match mode {
        AddMode::Append => next_position(conn, playlist_id)?,
        AddMode::Next => {
            conn.execute(
                "UPDATE playlist_items SET position = position + ?2 WHERE playlist_id = ?1",
                params![playlist_id, fresh.len() as i64],
            )?;
            0
        }
    };

    for (i, tid) in fresh.iter().enumerate() {
        conn.execute(
            "INSERT INTO playlist_items (id, playlist_id, track_id, position, added_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                ids::playlist_item_id(),
                playlist_id,
                tid.as_str(),
                start + i as i64,
                now
            ],
        )?;
        outcome.added += 1;
    }
    Ok(outcome)
}

fn track_exists(conn: &Connection, track_id: &str) -> DbResult<bool> {
    match conn.query_row("SELECT 1 FROM tracks WHERE id = ?1", [track_id], |_| Ok(())) {
        Ok(()) => Ok(true),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(false),
        Err(e) => Err(DbError::Sql(e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_in_memory;

    fn fixture() -> Connection {
        let conn = open_in_memory().unwrap();
        conn.execute_batch(
            r#"
            INSERT INTO tracks (id,title,title_norm,duration_ms,added_at,path,path_lower)
            VALUES ('t1','一','一',1000,1,'/m/1.flac','/m/1.flac'),
                   ('t2','二','二',2000,2,'/m/2.flac','/m/2.flac'),
                   ('t3','三','三',3000,3,'/m/3.flac','/m/3.flac'),
                   ('t4','四','四',4000,4,'/m/4.flac','/m/4.flac');
            "#,
        )
        .unwrap();
        conn
    }

    fn order(conn: &Connection, pid: &str) -> Vec<String> {
        let mut stmt = conn
            .prepare("SELECT track_id FROM playlist_items WHERE playlist_id = ?1 ORDER BY position ASC")
            .unwrap();
        let rows = stmt.query_map([pid], |r| r.get::<_, String>(0)).unwrap();
        rows.map(|r| r.unwrap()).collect()
    }

    fn positions(conn: &Connection, pid: &str) -> Vec<i64> {
        let mut stmt = conn
            .prepare("SELECT position FROM playlist_items WHERE playlist_id = ?1 ORDER BY position ASC")
            .unwrap();
        let rows = stmt.query_map([pid], |r| r.get::<_, i64>(0)).unwrap();
        rows.map(|r| r.unwrap()).collect()
    }

    fn v(ids: &[&str]) -> Vec<String> {
        ids.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn create_keeps_input_order_and_dedups() {
        let mut conn = fixture();
        let out = create(&mut conn, "p1", "通勤", &v(&["t2", "t1", "t2"]), 100).unwrap();
        assert_eq!(out.added, 2);
        assert_eq!(out.skipped, 1);
        assert_eq!(order(&conn, "p1"), v(&["t2", "t1"]));
        assert_eq!(positions(&conn, "p1"), vec![0, 1]);
    }

    #[test]
    fn add_tracks_skips_duplicates_and_missing_tracks() {
        let mut conn = fixture();
        create(&mut conn, "p1", "去重", &v(&["t1", "t2"]), 100).unwrap();
        let out = add_tracks(&mut conn, "p1", &v(&["t2", "t3", "不存在", ""]), AddMode::Append, 200)
            .unwrap()
            .expect("playlist exists");
        assert_eq!(out, AddOutcome { added: 1, skipped: 3 });
        assert_eq!(order(&conn, "p1"), v(&["t1", "t2", "t3"]));
    }

    /// position 派生 id 的回归点：删掉一项再把同一首加回来，
    /// 新行 id 不能与残留行撞主键（撞了会被静默吞掉，列表里看不到）。
    #[test]
    fn re_add_after_remove_inserts_a_new_row() {
        let mut conn = fixture();
        create(&mut conn, "p1", "回来", &v(&["t1", "t2"]), 100).unwrap();
        remove_at(&mut conn, "p1", 0, 200).unwrap().unwrap();
        assert_eq!(order(&conn, "p1"), v(&["t2"]));
        let out = add_tracks(&mut conn, "p1", &v(&["t1"]), AddMode::Append, 300)
            .unwrap()
            .unwrap();
        assert_eq!(out.added, 1, "行 id 撞车会表现为 added=0 而列表没变化");
        assert_eq!(order(&conn, "p1"), v(&["t2", "t1"]));
        assert_eq!(positions(&conn, "p1"), vec![0, 1]);
    }

    #[test]
    fn add_tracks_next_mode_inserts_at_front() {
        let mut conn = fixture();
        create(&mut conn, "p1", "下一首", &v(&["t1", "t2"]), 100).unwrap();
        add_tracks(&mut conn, "p1", &v(&["t3", "t4"]), AddMode::Next, 200)
            .unwrap()
            .unwrap();
        assert_eq!(order(&conn, "p1"), v(&["t3", "t4", "t1", "t2"]));
        assert_eq!(positions(&conn, "p1"), vec![0, 1, 2, 3]);
    }

    #[test]
    fn remove_at_renumbers_and_touches_updated_at() {
        let mut conn = fixture();
        create(&mut conn, "p1", "删", &v(&["t1", "t2", "t3"]), 100).unwrap();
        remove_at(&mut conn, "p1", 1, 500).unwrap().unwrap();
        assert_eq!(order(&conn, "p1"), v(&["t1", "t3"]));
        assert_eq!(positions(&conn, "p1"), vec![0, 1]);
        let updated: i64 = conn
            .query_row("SELECT updated_at FROM playlists WHERE id='p1'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(updated, 500);
    }

    #[test]
    fn remove_at_out_of_range_is_noop_not_error() {
        let mut conn = fixture();
        create(&mut conn, "p1", "越界", &v(&["t1"]), 100).unwrap();
        assert_eq!(remove_at(&mut conn, "p1", 9, 200).unwrap(), Some(()));
        assert_eq!(order(&conn, "p1"), v(&["t1"]));
        assert_eq!(remove_at(&mut conn, "nope", 0, 200).unwrap(), None);
    }

    #[test]
    fn move_item_keeps_dense_positions() {
        let mut conn = fixture();
        create(&mut conn, "p1", "移", &v(&["t1", "t2", "t3", "t4"]), 100).unwrap();
        move_item(&mut conn, "p1", 0, 2, 200).unwrap().unwrap();
        assert_eq!(order(&conn, "p1"), v(&["t2", "t3", "t1", "t4"]));
        assert_eq!(positions(&conn, "p1"), vec![0, 1, 2, 3]);
    }

    #[test]
    fn clear_keeps_playlist_row() {
        let mut conn = fixture();
        create(&mut conn, "p1", "清空", &v(&["t1", "t2"]), 100).unwrap();
        assert!(clear(&mut conn, "p1", 200).unwrap());
        assert_eq!(order(&conn, "p1").len(), 0);
        assert!(exists(&conn, "p1").unwrap());
        assert!(!clear(&mut conn, "nope", 200).unwrap());
    }

    #[test]
    fn rename_then_delete() {
        let mut conn = fixture();
        create(&mut conn, "p1", "旧名", &v(&["t1"]), 100).unwrap();
        assert!(rename(&conn, "p1", "  新名  ", 200).unwrap());
        let (name, name_norm): (String, String) = conn
            .query_row("SELECT name, name_norm FROM playlists WHERE id='p1'", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .unwrap();
        assert_eq!(name, "新名");
        assert_eq!(name_norm, "新名");
        assert!(!rename(&conn, "nope", "x", 200).unwrap());
        assert!(delete(&conn, "p1").unwrap());
        assert!(!exists(&conn, "p1").unwrap());
        // 级联：items 一起清掉
        let left: i64 = conn
            .query_row("SELECT COUNT(*) FROM playlist_items WHERE playlist_id='p1'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(left, 0);
    }

    #[test]
    fn rename_rejects_empty_name() {
        let mut conn = fixture();
        create(&mut conn, "p1", "有名字", &[], 100).unwrap();
        assert!(rename(&conn, "p1", "   ", 200).is_err());
    }

    #[test]
    fn reorder_persists_and_new_playlist_goes_last() {
        let mut conn = fixture();
        for (pid, name) in [("a", "A"), ("b", "B"), ("c", "C")] {
            create(&mut conn, pid, name, &[], 100).unwrap();
        }
        reorder_playlists(&mut conn, 2, 0).unwrap();
        let names: Vec<String> = {
            let mut stmt = conn
                .prepare("SELECT name FROM playlists ORDER BY sort_position ASC, created_at ASC")
                .unwrap();
            let rows = stmt.query_map([], |r| r.get::<_, String>(0)).unwrap();
            rows.map(|r| r.unwrap()).collect()
        };
        assert_eq!(names, v(&["C", "A", "B"]));
        // 再建一个应排到最后，而不是插进中间
        create(&mut conn, "d", "D", &[], 100).unwrap();
        let names: Vec<String> = {
            let mut stmt = conn
                .prepare("SELECT name FROM playlists ORDER BY sort_position ASC, created_at ASC")
                .unwrap();
            let rows = stmt.query_map([], |r| r.get::<_, String>(0)).unwrap();
            rows.map(|r| r.unwrap()).collect()
        };
        assert_eq!(names, v(&["C", "A", "B", "D"]));
        let c_sort: i64 = conn
            .query_row("SELECT sort_position FROM playlists WHERE id='c'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(c_sort, 0);
    }

    #[test]
    fn reorder_out_of_range_is_noop() {
        let mut conn = fixture();
        create(&mut conn, "a", "A", &[], 100).unwrap();
        reorder_playlists(&mut conn, 0, 5).unwrap();
        reorder_playlists(&mut conn, -1, 0).unwrap();
        assert!(exists(&conn, "a").unwrap());
    }

    #[test]
    fn add_mode_parse_rejects_unknown() {
        assert_eq!(AddMode::parse(None).unwrap(), AddMode::Append);
        assert_eq!(AddMode::parse(Some("append")).unwrap(), AddMode::Append);
        assert_eq!(AddMode::parse(Some("next")).unwrap(), AddMode::Next);
        assert!(AddMode::parse(Some("front")).is_err());
    }

    #[test]
    fn sanitize_name_truncates_by_chars_not_bytes() {
        let long: String = "汉".repeat(200);
        let out = sanitize_name(&long).unwrap();
        assert_eq!(out.chars().count(), MAX_NAME_CHARS);
        assert!(out.is_char_boundary(out.len()));
    }
}
