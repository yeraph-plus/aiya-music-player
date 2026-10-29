//! 智能歌单：rule_json → SQL 计划（docs/40 §1 方案 A 拍板版）。
//!
//! 规则**存播放器、查播放器**：`playlists.rule_json` 是唯一真源，打开歌单时
//! 对本地 SQLite 实时查询——与推荐页"最近添加/常听榜"同款动态查询先例。
//! beets 的角色只是供数据（genre 经 beets_sync 迁移），不参与规则计算。
//!
//! ## 规则 JSON 形状（camelCase，与全仓 DTO 惯例一致）
//!
//! ```json
//! { "all": [ { "field": "genre", "op": "contains", "value": "摇滚" } ],
//!   "sort": "added", "direction": "desc", "limit": 100 }
//! ```
//!
//! ## 演进边界
//!
//! `parse` 对未知 field/op 直接报错（不静默忽略）——规则是持久化数据，
//! 静默吞掉未知字段意味着"UI 改了规则、旧后端照单全收但查询语义变了"。
//! 特征字段（能量/效价/舞曲性）未来加进来时：加 Field 枚举值 + WHERE 分支
//! + 迁移加列，三处同 PR；rule_json 里的旧规则不受影响。

use rusqlite::Connection;
use serde_json::Value;

use crate::db::{DbError, DbResult};
use crate::norm::norm;

/// 规则条件字段（首期四类，docs/40 §1.2）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    Genre,
    Artist,
    Album,
    Year,
    PlayCount,
}

impl Field {
    fn parse(s: &str) -> DbResult<Self> {
        match s {
            "genre" => Ok(Field::Genre),
            "artist" => Ok(Field::Artist),
            "album" => Ok(Field::Album),
            "year" => Ok(Field::Year),
            "playCount" => Ok(Field::PlayCount),
            other => Err(DbError::Custom(format!("未知的规则字段: {other}"))),
        }
    }

    /// 该字段的合法操作符（UI 的 op 下拉按这个出）。
    fn allows(&self, op: &Op) -> bool {
        match self {
            Field::Genre | Field::Artist | Field::Album => {
                matches!(op, Op::Eq | Op::Contains)
            }
            Field::Year | Field::PlayCount => matches!(op, Op::Eq | Op::Gte | Op::Lte),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    Eq,
    Contains,
    Gte,
    Lte,
}

impl Op {
    fn parse(s: &str) -> DbResult<Self> {
        match s {
            "eq" => Ok(Op::Eq),
            "contains" => Ok(Op::Contains),
            "gte" => Ok(Op::Gte),
            "lte" => Ok(Op::Lte),
            other => Err(DbError::Custom(format!("未知的规则操作符: {other}"))),
        }
    }
}

/// 条件值：文本字段用 Text（存前做 norm），数字字段用 Int。
/// 文本字段给了数字（或反过来）在 parse 时就拒绝——UI 的 bug 不该变成
/// "查出一堆意外结果"。
#[derive(Debug, Clone, PartialEq)]
pub enum CondValue {
    Text(String),
    Int(i64),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Cond {
    pub field: Field,
    pub op: Op,
    pub value: CondValue,
}

/// 排序键（与曲库列表同一批语义；playlist 来源的手动顺序对智能歌单无意义）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortKey {
    Title,
    Added,
    Artist,
    Album,
}

impl SortKey {
    fn parse(s: &str) -> DbResult<Self> {
        match s {
            "title" => Ok(SortKey::Title),
            "added" => Ok(SortKey::Added),
            "artist" => Ok(SortKey::Artist),
            "album" => Ok(SortKey::Album),
            other => Err(DbError::Custom(format!("未知的排序键: {other}"))),
        }
    }

    fn expr(self, desc: bool) -> &'static str {
        // 方向拼进键里而不是让 repo 拼：智能歌单的 ORDER BY 是动态字符串，
        // repo 那套 &'static 固定顺序塞不进来
        match self {
            SortKey::Title => {
                if desc {
                    " ORDER BY t.title_norm DESC, t.id DESC"
                } else {
                    " ORDER BY t.title_norm ASC, t.id ASC"
                }
            }
            SortKey::Added => {
                if desc {
                    " ORDER BY t.added_at DESC, t.id DESC"
                } else {
                    " ORDER BY t.added_at ASC, t.id ASC"
                }
            }
            SortKey::Artist => {
                if desc {
                    " ORDER BY COALESCE(ar.name_norm,'') DESC, t.id DESC"
                } else {
                    " ORDER BY COALESCE(ar.name_norm,'') ASC, t.id ASC"
                }
            }
            SortKey::Album => {
                if desc {
                    " ORDER BY COALESCE(al.title_norm,'') DESC, t.id DESC"
                } else {
                    " ORDER BY COALESCE(al.title_norm,'') ASC, t.id ASC"
                }
            }
        }
    }
}

/// 解析完成的查询计划：WHERE 片段 + 绑定值 + ORDER BY + 上限。
/// 由 `repo::list_tracks` 消费（SmartPlaylist 来源）。
#[derive(Debug, Clone)]
pub struct SmartPlan {
    /// (WHERE 片段, serde_json::Value 绑定值)——repo 的 binder 机制吃 Value。
    pub clauses: Vec<(String, Value)>,
    /// ORDER BY 片段（键表达式是常量表，天然 'static）
    pub order_sql: &'static str,
    pub limit: Option<i64>,
}

impl SmartPlan {
    /// 解析 rule_json。`None`（列 NULL）= 恒真规则（无条件）+ 默认排序。
    pub fn parse(raw: Option<&str>) -> DbResult<SmartPlan> {
        let Some(raw) = raw.map(str::trim).filter(|s| !s.is_empty()) else {
            return Ok(SmartPlan::empty());
        };
        let v: Value = serde_json::from_str(raw)
            .map_err(|e| DbError::Custom(format!("规则 JSON 无法解析: {e}")))?;
        SmartPlan::from_value(&v)
    }

    pub fn empty() -> SmartPlan {
        SmartPlan { clauses: Vec::new(), order_sql: SortKey::Added.expr(true), limit: None }
    }

    pub fn from_value(v: &Value) -> DbResult<SmartPlan> {
        let obj = v
            .as_object()
            .ok_or_else(|| DbError::Custom("规则必须是 JSON 对象".into()))?;
        let mut clauses = Vec::new();
        // all: 条件数组，全部 AND（docs/40 拍板：首期不做 OR/嵌套，UI 也只出"满足全部"）
        if let Some(all) = obj.get("all") {
            let arr = all
                .as_array()
                .ok_or_else(|| DbError::Custom("规则 all 必须是数组".into()))?;
            clauses = parse_conds(arr)?;
        }
        // sort/direction：缺省 = 最近添加优先（与推荐页直觉一致）
        let order_sql = match (
            obj.get("sort").and_then(Value::as_str),
            obj.get("direction").and_then(Value::as_str),
        ) {
            (None, _) | (Some(""), _) => SortKey::Added.expr(true),
            (Some(s), dir) => SortKey::parse(s)?.expr(dir == Some("desc")),
        };
        let limit = match obj.get("limit") {
            None | Some(Value::Null) => None,
            Some(Value::Number(n)) => {
                let i = n
                    .as_i64()
                    .ok_or_else(|| DbError::Custom("limit 必须是整数".into()))?;
                if i < 0 {
                    return Err(DbError::Custom("limit 不能为负".into()));
                }
                Some(i)
            }
            Some(_) => return Err(DbError::Custom("limit 必须是整数".into())),
        };
        Ok(SmartPlan { clauses, order_sql, limit })
    }

    /// 供 src-tauri 摘要统计复用（曲目数 / 总时长 / 首个有封面曲目）。
    pub fn count_from(&self) -> String {
        let where_sql = self.where_sql();
        format!("FROM tracks t {JOIN_SMART}{where_sql}")
    }

    fn where_sql(&self) -> String {
        if self.clauses.is_empty() {
            String::new()
        } else {
            let parts: Vec<&str> = self.clauses.iter().map(|(c, _)| c.as_str()).collect();
            format!(" WHERE {}", parts.join(" AND "))
        }
    }
}

/// contains 条件的绑定值：`%needle%`（needle 先 norm 再转义 LIKE 通配符）。
fn like_pattern(s: &str) -> String {
    format!("%{}%", crate::norm::escape_like(&norm(s)))
}

/// artist/album 条件走 IN 子查询命中实体表（name_norm/title_norm 有索引，
/// 0005 迁移）。
const JOIN_SMART: &str = "LEFT JOIN artists ar ON ar.id = t.artist_id \
     LEFT JOIN albums al ON al.id = t.album_id";

/// 解析条件数组（`[{field,op,value}, ...]`）为 repo 可绑定的子句序列。
/// 智能歌单的 `all` 与**列表筛选器的 conds**（2026-10-28：同一批字段键，
/// 但筛选器不落库）共用这一套解析与类型校验。
fn parse_conds(arr: &[Value]) -> DbResult<Vec<(String, Value)>> {
    let mut clauses = Vec::new();
    for c in arr {
        let c = c
            .as_object()
            .ok_or_else(|| DbError::Custom("条件必须是对象".into()))?;
        let field = Field::parse(
            c.get("field").and_then(Value::as_str).unwrap_or_default(),
        )?;
        let op = Op::parse(c.get("op").and_then(Value::as_str).unwrap_or_default())?;
        if !field.allows(&op) {
            return Err(DbError::Custom(format!("字段 {field:?} 不支持该操作符")));
        }
        let raw_value = c.get("value").unwrap_or(&Value::Null);
        let value = match (field, raw_value) {
            // 文本字段：转字符串并非空（空串条件 = UI 没填，直接拒绝）
            (Field::Genre | Field::Artist | Field::Album, Value::String(s)) => {
                if s.trim().is_empty() {
                    return Err(DbError::Custom("文本条件值不能为空".into()));
                }
                CondValue::Text(s.trim().to_string())
            }
            (Field::Year | Field::PlayCount, Value::Number(n)) => {
                let i = n
                    .as_i64()
                    .ok_or_else(|| DbError::Custom("数字条件值必须是整数".into()))?;
                CondValue::Int(i)
            }
            (Field::Year | Field::PlayCount, _) => {
                return Err(DbError::Custom("数字条件值必须是整数".into()));
            }
            (_, _) => return Err(DbError::Custom("文本条件值必须是字符串".into())),
        };
        clauses.push(build_clause(field, op, &value));
    }
    Ok(clauses)
}

/// 列表筛选器的临时条件 → WHERE 子句（不入 rule_json、不落库）。
/// 入参是 `[{field,op,value}, ...]` 数组 JSON；未知 field/op 与类型错配
/// **明确报错**（与智能歌单同一原则：UI 的 bug 不该静默变成全量列表）。
pub fn conds_from_value(v: &Value) -> DbResult<Vec<(String, Value)>> {
    match v {
        Value::Null => Ok(Vec::new()),
        Value::Array(arr) => parse_conds(arr),
        _ => Err(DbError::Custom("conds 必须是数组".into())),
    }
}

fn build_clause(field: Field, op: Op, value: &CondValue) -> (String, Value) {
    match (field, value) {
        (Field::Genre, CondValue::Text(s)) => match op {
            // genre_norm 与值同走 norm（大小写/空白语义一致）；eq 用 = 精确匹配；
            // contains 的 LIKE 值必须过 escape_like（repo/facets 同一套规则），
            // 否则规则里写 "50%" 会匹配出全部曲目
            Op::Eq => ("t.genre_norm = ?".into(), Value::from(norm(s))),
            Op::Contains => (
                "t.genre_norm LIKE ? ESCAPE '\\'".into(),
                Value::from(like_pattern(s)),
            ),
            _ => unreachable!("parse 已拒绝文本字段的 gte/lte"),
        },
        (Field::Artist, CondValue::Text(s)) => {
            let n = norm(s);
            match op {
                Op::Eq => (
                    "t.artist_id IN (SELECT id FROM artists WHERE name_norm = ?)".into(),
                    Value::from(n),
                ),
                Op::Contains => (
                    "t.artist_id IN (SELECT id FROM artists WHERE name_norm LIKE ? ESCAPE '\\')"
                        .into(),
                    Value::from(like_pattern(s)),
                ),
                _ => unreachable!(),
            }
        }
        (Field::Album, CondValue::Text(s)) => {
            let n = norm(s);
            match op {
                Op::Eq => (
                    "t.album_id IN (SELECT id FROM albums WHERE title_norm = ?)".into(),
                    Value::from(n),
                ),
                Op::Contains => (
                    "t.album_id IN (SELECT id FROM albums WHERE title_norm LIKE ? ESCAPE '\\')"
                        .into(),
                    Value::from(like_pattern(s)),
                ),
                _ => unreachable!(),
            }
        }
        (Field::Year, CondValue::Int(i)) => match op {
            Op::Eq => ("t.year = ?".into(), Value::from(*i)),
            Op::Gte => ("t.year >= ?".into(), Value::from(*i)),
            Op::Lte => ("t.year <= ?".into(), Value::from(*i)),
            _ => unreachable!(),
        },
        (Field::PlayCount, CondValue::Int(i)) => match op {
            Op::Eq => ("t.play_count = ?".into(), Value::from(*i)),
            Op::Gte => ("t.play_count >= ?".into(), Value::from(*i)),
            Op::Lte => ("t.play_count <= ?".into(), Value::from(*i)),
            _ => unreachable!(),
        },
        // parse 阶段已保证类型配对
        _ => unreachable!("CondValue 类型与 Field 不配对"),
    }
}

/// 从 playlists 表读规则并编译。歌单不存在/是手动歌单 → 恒真计划
/// （调用方保证来源正确；这里防御性兜底，别让一行坏数据炸掉整个列表页）。
pub fn load_plan(conn: &Connection, playlist_id: &str) -> DbResult<SmartPlan> {
    let rule: Option<String> = conn
        .query_row(
            "SELECT rule_json FROM playlists WHERE id = ?1 AND kind = 'smart'",
            [playlist_id],
            |r| r.get(0),
        )
        .map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => {
                DbError::Custom(format!("智能歌单不存在: {playlist_id}"))
            }
            other => DbError::Sql(other),
        })?;
    SmartPlan::parse(rule.as_deref())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn plan(v: Value) -> SmartPlan {
        SmartPlan::from_value(&v).unwrap()
    }

    #[test]
    fn empty_rule_matches_all_with_default_order() {
        let p = plan(json!({}));
        assert!(p.clauses.is_empty());
        assert_eq!(p.order_sql, SortKey::Added.expr(true));
        assert_eq!(p.limit, None);
    }

    #[test]
    fn genre_contains_binds_normed_value() {
        let p = plan(json!({ "all": [ { "field": "genre", "op": "contains", "value": " Rock " } ] }));
        assert_eq!(p.clauses.len(), 1);
        assert_eq!(p.clauses[0].0, "t.genre_norm LIKE ? ESCAPE '\\'");
        assert_eq!(p.clauses[0].1, json!("%rock%"), "绑定值必须 norm 过且含通配包裹");
    }

    /// contains 的 LIKE 值按字面处理：`%`/`_` 必须转义（repo/facets 同规则）。
    #[test]
    fn contains_escapes_like_wildcards() {
        let p = plan(json!({ "all": [ { "field": "genre", "op": "contains", "value": "ro%k_" } ] }));
        assert_eq!(p.clauses[0].1, json!("%ro\\%k\\_%"));
        let p = plan(json!({ "all": [ { "field": "artist", "op": "contains", "value": "a\\b" } ] }));
        assert_eq!(p.clauses[0].1, json!("%a\\\\b%"));
        assert!(p.clauses[0].0.contains("ESCAPE '\\'"));
    }

    #[test]
    fn unknown_field_is_rejected_not_ignored() {
        let err = SmartPlan::from_value(&json!({ "all": [ { "field": "energy", "op": "gte", "value": 7 } ] }))
            .unwrap_err();
        assert!(err.to_string().contains("energy"));
    }

    #[test]
    fn op_field_mismatch_is_rejected() {
        // 文本字段不收 gte，数字字段不收 contains——UI 的 bug 不该变成错误结果
        assert!(SmartPlan::from_value(
            &json!({ "all": [ { "field": "genre", "op": "gte", "value": "rock" } ] })
        )
        .is_err());
        assert!(SmartPlan::from_value(
            &json!({ "all": [ { "field": "playCount", "op": "contains", "value": 3 } ] })
        )
        .is_err());
    }

    #[test]
    fn type_mismatch_is_rejected() {
        assert!(SmartPlan::from_value(
            &json!({ "all": [ { "field": "year", "op": "gte", "value": "2020" } ] })
        )
        .is_err());
        assert!(SmartPlan::from_value(
            &json!({ "all": [ { "field": "genre", "op": "eq", "value": 5 } ] })
        )
        .is_err());
    }

    #[test]
    fn empty_text_value_is_rejected() {
        assert!(SmartPlan::from_value(
            &json!({ "all": [ { "field": "genre", "op": "eq", "value": "  " } ] })
        )
        .is_err());
    }

    #[test]
    fn filter_conds_compile_and_reject() {
        // 筛选器临时条件（与智能歌单同键、不落库）走同一套解析
        let clauses = conds_from_value(&json!([
            { "field": "genre", "op": "contains", "value": "rock" },
            { "field": "year", "op": "gte", "value": 2000 }
        ]))
        .unwrap();
        assert_eq!(clauses.len(), 2);
        assert!(clauses[0].0.contains("genre_norm"));
        assert!(clauses[1].0.contains("year >="));
        // 非数组 / 未知字段 / 类型错配照旧拒绝
        assert!(conds_from_value(&json!({"field":"genre"})).is_err());
        assert!(conds_from_value(&json!([{ "field": "bpm", "op": "eq", "value": 1 }])).is_err());
        assert!(conds_from_value(&json!([{ "field": "genre", "op": "eq", "value": 5 }])).is_err());
        // Null = 未传，恒过
        assert!(conds_from_value(&Value::Null).unwrap().is_empty());
    }

    #[test]
    fn sort_and_limit_parse() {
        let p = plan(json!({ "sort": "title", "direction": "asc", "limit": 50 }));
        assert_eq!(p.order_sql, SortKey::Title.expr(false));
        assert_eq!(p.limit, Some(50));
        assert!(SmartPlan::from_value(&json!({ "limit": -1 })).is_err());
        assert!(SmartPlan::from_value(&json!({ "sort": "energy" })).is_err());
    }
}
