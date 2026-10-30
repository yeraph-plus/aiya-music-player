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

use rusqlite::{params, Connection, TransactionBehavior};

use crate::db::{DbError, DbResult};
use crate::ids;
use crate::norm::norm;

/// 本文件的写事务统一 IMMEDIATE（不是默认 DEFERRED）：扫描连接与主 DB 线程是
/// 两个连接，DEFERRED 事务"先读后写"时要在事务中途升级写锁——30s 的自动刷新
/// 恰好持有写锁时，锁升级立刻 SQLITE_BUSY（busy_timeout 对升级不生效），
/// 用户加歌/建单就失败。IMMEDIATE 在事务开始就排队等写锁，等得到。
fn write_tx(conn: &mut Connection) -> DbResult<rusqlite::Transaction<'_>> {
    conn.transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(DbError::Sql)
}

/// 与旧仓库一致：名称按字符截断到 80，避免把 UTF-8 多字节字符切成半个。
pub const MAX_NAME_CHARS: usize = 80;

/// 加歌位置语义（2026-10-30 硬迭代）：所有"添加到歌单"一律**首行插入**——
/// 最后入库的内容在最前，歌单固定按入库倒序工作（新块整体顶到头部，
/// 块内保持入参顺序）。旧的双模式（Append/Next）随本次迭代删除。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AddOutcome {
    pub added: i64,
    pub skipped: i64,
}

/// 动态歌单刷新结果：`added` = 本次新追加的匹配数；`total` = 刷新后表内
/// 总曲目数（追加只进不出，total 只增不减）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RefreshOutcome {
    pub added: i64,
    pub total: i64,
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
    match conn.query_row(
        "SELECT 1 FROM playlists WHERE id = ?1",
        [playlist_id],
        |_| Ok(()),
    ) {
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
    let tx = write_tx(conn)?;
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
    let outcome = insert_head(&tx, playlist_id, track_ids, now)?;
    touch(&tx, playlist_id, now)?;
    tx.commit().map_err(DbError::Sql)?;
    Ok(outcome)
}

/// 建动态歌单（2026-10-30 硬迭代：物化模型）——曲目**入库**（playlist_items），
/// 规则存 `rule_json` 供刷新/重算；与普通歌单能力完全相同（拖拽/删曲/混入）。
/// 建单即按规则物化一次：命中的曲目按确定性顺序（title_norm、id）写入首部。
/// 规则在入库前先编译一遍（`SmartPlan::from_value`）——写库时校验比读库时炸友好得多。
pub fn create_smart(
    conn: &mut Connection,
    playlist_id: &str,
    name: &str,
    rule: &serde_json::Value,
    now: i64,
) -> DbResult<AddOutcome> {
    let name_norm = norm(name);
    let plan = crate::smart::SmartPlan::from_value(rule)?;
    let rule_json = serde_json::to_string(rule).map_err(|e| DbError::Custom(e.to_string()))?;
    let matched = plan.matched_ids(conn)?;
    let tx = write_tx(conn)?;
    let next_sort: i64 = tx.query_row(
        "SELECT COALESCE(MAX(sort_position), -1) + 1 FROM playlists",
        [],
        |r| r.get(0),
    )?;
    tx.execute(
        "INSERT INTO playlists (id, name, name_norm, created_at, updated_at, sort_position, kind, rule_json)
         VALUES (?1, ?2, ?3, ?4, ?4, ?5, 'smart', ?6)
         ON CONFLICT(id) DO NOTHING",
        params![playlist_id, name, name_norm, now, next_sort, rule_json],
    )?;
    let outcome = insert_head(&tx, playlist_id, &matched, now)?;
    touch(&tx, playlist_id, now)?;
    tx.commit().map_err(DbError::Sql)?;
    Ok(outcome)
}

/// 更新动态歌单的规则：规则落库 + **替换重算**（清空现有曲目、按新规则重新
/// 物化——所见即所得；日常增量走 refresh_smart）。返回 false = 歌单不存在；
/// 手动歌单 = `Custom` 错误（手动/动态互不转换，docs/40 §1.4 拍板）。
pub fn set_rule(
    conn: &mut Connection,
    playlist_id: &str,
    rule: &serde_json::Value,
    now: i64,
) -> DbResult<bool> {
    // 校验在写库前（与 create_smart 同理）
    let plan = crate::smart::SmartPlan::from_value(rule)?;
    let rule_json = serde_json::to_string(rule).map_err(|e| DbError::Custom(e.to_string()))?;
    let kind: Option<String> = match conn.query_row(
        "SELECT kind FROM playlists WHERE id = ?1",
        [playlist_id],
        |r| r.get(0),
    ) {
        Ok(k) => Some(k),
        Err(rusqlite::Error::QueryReturnedNoRows) => None,
        Err(e) => return Err(DbError::Sql(e)),
    };
    match kind.as_deref() {
        None => Ok(false),
        Some("manual") => Err(DbError::Custom("手动歌单不支持设置规则".into())),
        Some(_) => {
            let matched = plan.matched_ids(conn)?;
            let tx = write_tx(conn)?;
            tx.execute(
                "UPDATE playlists SET rule_json = ?2, updated_at = ?3 WHERE id = ?1",
                params![playlist_id, rule_json, now],
            )?;
            tx.execute(
                "DELETE FROM playlist_items WHERE playlist_id = ?1",
                [playlist_id],
            )?;
            insert_head(&tx, playlist_id, &matched, now)?;
            touch(&tx, playlist_id, now)?;
            tx.commit().map_err(DbError::Sql)?;
            Ok(true)
        }
    }
}

/// 刷新动态歌单：重跑规则，**只追加**——命中的曲目里不在表内的，作为新块
/// 插入首部（块内规则顺序）；已存在的曲目原位不动（策展语义：用户的排序、
/// 手动混入的歌都不被打乱），也绝不删除已不匹配的行。刷新后计数只增不减。
/// 歌单不存在 → `None`；手动歌单 → `Custom` 错误。
pub fn refresh_smart(
    conn: &mut Connection,
    playlist_id: &str,
    now: i64,
) -> DbResult<Option<RefreshOutcome>> {
    let row: Option<(String, Option<String>)> = conn
        .query_row(
            "SELECT kind, rule_json FROM playlists WHERE id = ?1",
            [playlist_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map(Some)
        .or_else(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => Ok(None),
            other => Err(other),
        })
        .map_err(DbError::Sql)?;
    let Some((kind, rule_json)) = row else {
        return Ok(None);
    };
    if kind != "smart" {
        return Err(DbError::Custom("只有动态歌单支持刷新".into()));
    }
    let plan = crate::smart::SmartPlan::parse(rule_json.as_deref())?;
    let matched = plan.matched_ids(conn)?;

    let tx = write_tx(conn)?;
    let existing: HashSet<String> = {
        let mut stmt = tx.prepare("SELECT track_id FROM playlist_items WHERE playlist_id = ?1")?;
        let rows = stmt.query_map([playlist_id], |r| r.get::<_, String>(0))?;
        rows.collect::<rusqlite::Result<HashSet<_>>>()?
    };
    // 命中里剔除已在表内的（只加不删）；matched 已按确定性顺序排好
    let fresh: Vec<String> = matched
        .into_iter()
        .filter(|id| !existing.contains(id))
        .collect();
    let outcome = insert_head(&tx, playlist_id, &fresh, now)?;
    let total: i64 = tx.query_row(
        "SELECT COUNT(*) FROM playlist_items WHERE playlist_id = ?1",
        [playlist_id],
        |r| r.get(0),
    )?;
    touch(&tx, playlist_id, now)?;
    tx.commit().map_err(DbError::Sql)?;
    Ok(Some(RefreshOutcome {
        added: outcome.added,
        total,
    }))
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
    let tx = write_tx(conn)?;
    tx.execute(
        "DELETE FROM playlist_items WHERE playlist_id = ?1",
        [playlist_id],
    )?;
    touch(&tx, playlist_id, now)?;
    tx.commit().map_err(DbError::Sql)?;
    Ok(true)
}

/// 动态歌单的曲目列表此前是规则的投影（2026-10-30 硬迭代前）——加歌/删歌/
/// 排序/清空都被 `ensure_manual` 拦下。物化模型后与普通歌单能力完全相同，
/// 守卫删除；规则只影响"刷新/重算"两个动作，不影响手动修改。
pub fn add_tracks(
    conn: &mut Connection,
    playlist_id: &str,
    track_ids: &[String],
    now: i64,
) -> DbResult<Option<AddOutcome>> {
    if !exists(conn, playlist_id)? {
        return Ok(None);
    }
    let tx = write_tx(conn)?;
    let outcome = insert_head(&tx, playlist_id, track_ids, now)?;
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
    let tx = write_tx(conn)?;
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
    let tx = write_tx(conn)?;
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
        let mut stmt =
            conn.prepare("SELECT id FROM playlists ORDER BY sort_position ASC, created_at ASC")?;
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
    let tx = write_tx(conn)?;
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

/// 追加曲目（去重 + 过滤已从库中删除的 ID）。
///
/// 为什么过滤不存在的曲目而不是让外键报错：UI 的多选可能横跨一次重新扫描，
/// 传进来的 ID 里混着刚被删掉的曲目；整批报错会让"加入 20 首成功 19 首"变成零。
/// 首行插入（2026-10-30 语义）：去重 + 过滤已从库中删除的 ID 后，把新增块
/// 整体顶到歌单最前（块内保持入参顺序）——最后入库的内容在最前，歌单固定
/// 按入库倒序工作。先按"实际新增条数"整体后移旧行再插 0..n-1，顺序反了会
/// position 撞车。
///
/// 为什么过滤不存在的曲目而不是让外键报错：UI 的多选可能横跨一次重新扫描，
/// 传进来的 ID 里混着刚被删掉的曲目；整批报错会让"加入 20 首成功 19 首"变成零。
fn insert_head(
    conn: &Connection,
    playlist_id: &str,
    track_ids: &[String],
    now: i64,
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

    // 存在性校验一次性查回（`WHERE id IN (...)`）：逐条点查在几百首的批量
    // 加歌场景是 N 次往返；ID 超过单批变量上限时分块查。
    let in_library = existing_track_ids(conn, track_ids)?;

    let mut fresh: Vec<&String> = Vec::with_capacity(track_ids.len());
    for tid in track_ids {
        if tid.is_empty() || existing.contains(tid) || !in_library.contains(tid.as_str()) {
            outcome.skipped += 1;
            continue;
        }
        existing.insert(tid.clone());
        fresh.push(tid);
    }
    if fresh.is_empty() {
        return Ok(outcome);
    }

    conn.execute(
        "UPDATE playlist_items SET position = position + ?2 WHERE playlist_id = ?1",
        params![playlist_id, fresh.len() as i64],
    )?;

    // 批量写 prepare 一次循环复用，不为每条重新编译 SQL
    let mut stmt = conn.prepare(
        "INSERT INTO playlist_items (id, playlist_id, track_id, position, added_at)
         VALUES (?1, ?2, ?3, ?4, ?5)",
    )?;
    for (i, tid) in fresh.iter().enumerate() {
        stmt.execute(params![
            ids::playlist_item_id(),
            playlist_id,
            tid.as_str(),
            i as i64,
            now
        ])?;
        outcome.added += 1;
    }
    Ok(outcome)
}

/// 入参 ID 里真实存在于 tracks 的那部分。分块 IN 查询（单批 500 个占位符，
/// 远低于 SQLite 变量上限），入参里的重复 ID 天然被 HashSet 去重。
fn existing_track_ids(conn: &Connection, track_ids: &[String]) -> DbResult<HashSet<String>> {
    let mut out = HashSet::new();
    for chunk in track_ids.chunks(500) {
        let placeholders = (1..=chunk.len())
            .map(|i| format!("?{i}"))
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!("SELECT id FROM tracks WHERE id IN ({placeholders})");
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(rusqlite::params_from_iter(chunk.iter()), |r| {
            r.get::<_, String>(0)
        })?;
        for row in rows {
            out.insert(row?);
        }
    }
    Ok(out)
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
            .prepare(
                "SELECT track_id FROM playlist_items WHERE playlist_id = ?1 ORDER BY position ASC",
            )
            .unwrap();
        let rows = stmt.query_map([pid], |r| r.get::<_, String>(0)).unwrap();
        rows.map(|r| r.unwrap()).collect()
    }

    fn positions(conn: &Connection, pid: &str) -> Vec<i64> {
        let mut stmt = conn
            .prepare(
                "SELECT position FROM playlist_items WHERE playlist_id = ?1 ORDER BY position ASC",
            )
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
        let out = add_tracks(&mut conn, "p1", &v(&["t2", "t3", "不存在", ""]), 200)
            .unwrap()
            .expect("playlist exists");
        assert_eq!(
            out,
            AddOutcome {
                added: 1,
                skipped: 3
            }
        );
        // 首行插入：新块顶到最前（2026-10-30 语义）
        assert_eq!(order(&conn, "p1"), v(&["t3", "t1", "t2"]));
    }

    /// position 派生 id 的回归点：删掉一项再把同一首加回来，
    /// 新行 id 不能与残留行撞主键（撞了会被静默吞掉，列表里看不到）。
    #[test]
    fn re_add_after_remove_inserts_a_new_row() {
        let mut conn = fixture();
        create(&mut conn, "p1", "回来", &v(&["t1", "t2"]), 100).unwrap();
        remove_at(&mut conn, "p1", 0, 200).unwrap().unwrap();
        assert_eq!(order(&conn, "p1"), v(&["t2"]));
        let out = add_tracks(&mut conn, "p1", &v(&["t1"]), 300)
            .unwrap()
            .unwrap();
        assert_eq!(out.added, 1, "行 id 撞车会表现为 added=0 而列表没变化");
        assert_eq!(
            order(&conn, "p1"),
            v(&["t1", "t2"]),
            "首行插入：t1 回到最前"
        );
        assert_eq!(positions(&conn, "p1"), vec![0, 1]);
    }

    /// 双曲块首行插入：块内保持入参顺序、整块顶到最前（2026-10-30 语义）。
    #[test]
    fn add_tracks_inserts_block_at_head_in_input_order() {
        let mut conn = fixture();
        create(&mut conn, "p1", "首插", &v(&["t1", "t2"]), 100).unwrap();
        add_tracks(&mut conn, "p1", &v(&["t3", "t4"]), 200)
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
            .query_row("SELECT updated_at FROM playlists WHERE id='p1'", [], |r| {
                r.get(0)
            })
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
            .query_row(
                "SELECT name, name_norm FROM playlists WHERE id='p1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(name, "新名");
        assert_eq!(name_norm, "新名");
        assert!(!rename(&conn, "nope", "x", 200).unwrap());
        assert!(delete(&conn, "p1").unwrap());
        assert!(!exists(&conn, "p1").unwrap());
        // 级联：items 一起清掉
        let left: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM playlist_items WHERE playlist_id='p1'",
                [],
                |r| r.get(0),
            )
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
            .query_row(
                "SELECT sort_position FROM playlists WHERE id='c'",
                [],
                |r| r.get(0),
            )
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

    /* ---------------- 动态歌单物化（2026-10-30 硬迭代） ---------------- */

    fn smart_rule(album: &str) -> serde_json::Value {
        serde_json::json!({
            "all": [{"field": "album", "op": "contains", "value": album}],
            "sort": "title",
            "direction": "asc"
        })
    }

    fn smart_fixture() -> Connection {
        let conn = open_in_memory().unwrap();
        conn.execute_batch(
            r#"
            INSERT INTO albums (id, title, title_norm) VALUES
              ('al1','Hits Vol.1','hits vol.1'),
              ('al2','Hits Vol.2','hits vol.2'),
              ('al3','别的专辑','别的专辑');
            INSERT INTO tracks (id,title,title_norm,added_at,path,path_lower,album_id) VALUES
              ('t1','Alpha','alpha',1,'/m/1.flac','/m/1.flac','al1'),
              ('t2','Beta','beta',2,'/m/2.flac','/m/2.flac','al2'),
              ('t3','Gamma','gamma',3,'/m/3.flac','/m/3.flac','al3');
            "#,
        )
        .unwrap();
        conn
    }

    /// 建单即物化：命中曲目按确定性顺序进 playlist_items（空规则命中 0 首 = 空表）。
    #[test]
    fn create_smart_materializes_matches() {
        let mut conn = smart_fixture();
        let out = create_smart(&mut conn, "sp", "Hits", &smart_rule("Hits"), 100).unwrap();
        assert_eq!(out.added, 2);
        assert_eq!(
            order(&conn, "sp"),
            v(&["t1", "t2"]),
            "确定性顺序 = title_norm"
        );
        assert!(exists(&conn, "sp").unwrap());

        let out = create_smart(&mut conn, "sp0", "空", &smart_rule("不存在的专辑"), 100).unwrap();
        assert_eq!(out.added, 0);
        assert_eq!(order(&conn, "sp0"), v(&[]));
    }

    /// 刷新只加不删：新命中的作为新块插入首部，已在表内的原位不动；
    /// 从此不再匹配的行也保留（策展语义）；手动混入的歌刷新绝不碰。
    #[test]
    fn refresh_smart_appends_only_new_matches() {
        let mut conn = smart_fixture();
        create_smart(&mut conn, "sp", "Hits", &smart_rule("Hits"), 100).unwrap();
        // 用户手动把 t3 加进来（非匹配曲目）+ 拖到头部语义下 t3 在最前
        add_tracks(&mut conn, "sp", &v(&["t3"]), 200)
            .unwrap()
            .unwrap();
        assert_eq!(order(&conn, "sp"), v(&["t3", "t1", "t2"]));

        // 库里出现了新的命中曲目 t4（t2 改名为不再命中也不影响——只加不删）
        conn.execute_batch(
            "INSERT INTO albums (id, title, title_norm) VALUES ('al4','Hits Vol.3','hits vol.3');
             INSERT INTO tracks (id,title,title_norm,added_at,path,path_lower,album_id) VALUES
               ('t4','Delta','delta',4,'/m/4.flac','/m/4.flac','al4');",
        )
        .unwrap();
        let out = refresh_smart(&mut conn, "sp", 300).unwrap().unwrap();
        assert_eq!(out.added, 1);
        assert_eq!(out.total, 4);
        // 新块顶到最前，块内规则顺序；既有内容（含手动混入的 t3）原位后移
        assert_eq!(order(&conn, "sp"), v(&["t4", "t3", "t1", "t2"]));

        // 无新命中 → added 0，顺序不动
        let out = refresh_smart(&mut conn, "sp", 400).unwrap().unwrap();
        assert_eq!(out.added, 0);
        assert_eq!(order(&conn, "sp"), v(&["t4", "t3", "t1", "t2"]));
    }

    /// 规则替换重算：清空现有曲目、按新规则重新物化（所见即所得）。
    #[test]
    fn set_rule_replaces_materialization() {
        let mut conn = smart_fixture();
        create_smart(&mut conn, "sp", "Hits", &smart_rule("Hits"), 100).unwrap();
        add_tracks(&mut conn, "sp", &v(&["t3"]), 200)
            .unwrap()
            .unwrap();
        assert_eq!(order(&conn, "sp"), v(&["t3", "t1", "t2"]));

        assert!(set_rule(&mut conn, "sp", &smart_rule("别的"), 300).unwrap());
        assert_eq!(order(&conn, "sp"), v(&["t3"]), "替换重算：旧物化被清掉");
        assert!(!set_rule(&mut conn, "p-nope", &smart_rule("x"), 300).unwrap());
        assert!(set_rule(&mut conn, "sp", &serde_json::json!({"all": []}), 400).is_ok());
    }

    /// 物化后动态歌单与普通歌单能力相同：删曲/重排/清空全部放开。
    #[test]
    fn smart_playlist_allows_manual_item_ops() {
        let mut conn = smart_fixture();
        create_smart(&mut conn, "sp", "Hits", &smart_rule("Hits"), 100).unwrap();
        remove_at(&mut conn, "sp", 0, 200).unwrap().unwrap();
        assert_eq!(order(&conn, "sp"), v(&["t2"]));
        move_item(&mut conn, "sp", 0, 0, 250).unwrap().unwrap();
        assert!(clear(&mut conn, "sp", 300).unwrap());
        assert_eq!(order(&conn, "sp"), v(&[]));
        // 清空后刷新照常重建
        let out = refresh_smart(&mut conn, "sp", 400).unwrap().unwrap();
        assert_eq!(out.added, 2);
        assert_eq!(order(&conn, "sp"), v(&["t1", "t2"]));
    }

    /// 手动歌单不支持刷新/规则（与 set_rule 的守卫对称）。
    #[test]
    fn refresh_smart_rejects_manual_playlist() {
        let mut conn = smart_fixture();
        create(&mut conn, "p1", "手动", &v(&["t1"]), 100).unwrap();
        assert!(matches!(
            refresh_smart(&mut conn, "p1", 200),
            Err(DbError::Custom(_))
        ));
        assert!(set_rule(&mut conn, "p1", &smart_rule("x"), 200).is_err());
    }

    #[test]
    fn sanitize_name_truncates_by_chars_not_bytes() {
        let long: String = "汉".repeat(200);
        let out = sanitize_name(&long).unwrap();
        assert_eq!(out.chars().count(), MAX_NAME_CHARS);
        assert!(out.is_char_boundary(out.len()));
    }

    /// 存在性校验的 IN 查询按 500 分块：跨块的大批量加歌必须全部命中，
    /// 不能只识别第一块里的 ID。
    #[test]
    fn batch_add_spans_multiple_in_chunks() {
        let mut conn = fixture();
        let ids: Vec<String> = (0..601).map(|i| format!("t{i:03}")).collect();
        let mut sql = String::from("BEGIN;");
        for id in &ids {
            sql.push_str(&format!(
                "INSERT INTO tracks (id,title,title_norm,added_at,path,path_lower)
                 VALUES ('{id}','{id}','{id}',1,'/m/{id}','/m/{id}');"
            ));
        }
        sql.push_str("COMMIT;");
        conn.execute_batch(&sql).unwrap();

        // 601 首（>500 跨两块）+ 一个不存在的 + 一个重复的
        let mut input: Vec<String> = ids.clone();
        input.push("不存在".into());
        input.push(ids[0].clone());
        let out = create(&mut conn, "p1", "大单", &input, 100).unwrap();
        assert_eq!(out.added, 601);
        assert_eq!(out.skipped, 2);
        let n: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM playlist_items WHERE playlist_id='p1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n, 601);
    }
}
