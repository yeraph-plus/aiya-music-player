//! DTO wire 快照：锁住"Rust 结构 → JSON 字段名/类型"这一层（docs/17 §6 第 2 道防线）。
//!
//! 为什么需要它：`ts-rs` 保证"Rust → TS 类型"一致，但**不保证 serde 实际发出去的
//! JSON 与那个类型一致**（`rename_all`、`skip_serializing`、`Option` 的处理都在
//! serde 侧）。这两边对不上时，类型检查全绿、运行期字段却是 `undefined` ——
//! 历史上 `TrackWindowQuery` 嵌套形状、`exclusiveMode → output` 都属于这类。
//!
//! 这里直接比对 JSON 字面量/键集合，改字段名会立刻红。

use serde_json::Value;

/// 取对象的键（排序后，便于整体比对）
fn keys(v: &Value) -> Vec<String> {
    let mut ks: Vec<String> = v
        .as_object()
        .expect("必须是对象")
        .keys()
        .cloned()
        .collect();
    ks.sort();
    ks
}

fn sorted(list: &[&str]) -> Vec<String> {
    let mut v: Vec<String> = list.iter().map(|s| s.to_string()).collect();
    v.sort();
    v
}

#[test]
fn api_error_wire_shape() {
    let err = aiya_lib::dto::ApiError::invalid_arg("查询参数不合法");
    let v = serde_json::to_value(&err).unwrap();
    assert_eq!(keys(&v), sorted(&["code", "detail", "message", "retryable"]));
    assert_eq!(v["code"], "invalidArg", "错误码必须是 camelCase 字符串");
    assert_eq!(v["retryable"], false);
    // detail 即使为空也必须是 null（不是缺字段）：TS 生成的是 `string | null`
    assert!(v["detail"].is_null(), "detail 必须有键且为 null");
}

#[test]
fn library_stats_sends_numbers_not_strings() {
    let stats = aiya_lib::dto::LibraryStats {
        tracks: 50_000,
        albums: 4_000,
        artists: 1_200,
        locations: 50_000,
        missing_locations: 0,
        total_duration_ms: 9_000_000,
        db_bytes: 42,
        schema_version: 1,
    };
    let v = serde_json::to_value(&stats).unwrap();
    assert_eq!(
        keys(&v),
        sorted(&[
            "tracks",
            "albums",
            "artists",
            "locations",
            "missingLocations",
            "totalDurationMs",
            "dbBytes",
            "schemaVersion",
        ])
    );
    // JSON number（不是字符串、不是 bigint 的字符串化）
    assert!(v["totalDurationMs"].is_number(), "时间必须是 number");
    assert!(v["missingLocations"].is_number());
}

#[test]
fn track_row_has_no_path_and_uses_ms() {
    let row = aiya_data::repo::TrackRow {
        id: "t1".into(),
        title: "标题".into(),
        artist_id: None,
        artist: Some("艺人".into()),
        album_id: None,
        album: None,
        album_artist: None,
        duration_ms: Some(180_000),
        track_no: Some(3),
        disc_no: None,
        year: Some(2020),
        added_at_ms: 1_700_000_000_000,
        artwork_id: None,
        play_count: 2,
        last_played_at_ms: Some(1_700_000_100_000),
        gain_db: None,
        availability: "online".into(),
    };
    let v = serde_json::to_value(&row).unwrap();
    let ks = keys(&v);
    // 时间字段必须带单位后缀
    assert!(ks.contains(&"addedAtMs".to_string()), "键：{ks:?}");
    assert!(ks.contains(&"lastPlayedAtMs".to_string()), "键：{ks:?}");
    assert!(!ks.contains(&"addedAt".to_string()), "秒级旧字段必须消失：{ks:?}");
    // 毫秒值原样透出
    assert_eq!(v["addedAtMs"], 1_700_000_000_000i64);
    // 列表行不得出现 path（TrackRow 本就没有，TrackRef 才是敏感点）
    assert!(!ks.contains(&"path".to_string()));
}

#[test]
fn player_state_error_is_structured_and_path_is_hidden() {
    let state = aiya_player::PlayerState {
        queue_length: 2,
        queue_version: 7,
        index: 0,
        cursor: 0,
        repeat: aiya_player::RepeatMode::Off,
        shuffle: false,
        status: aiya_player::PlaybackStatus::Error,
        position_ms: 1000,
        duration_ms: Some(2000),
        volume: 1.0,
        muted: false,
        rate: 1.0,
        error: Some(aiya_player::AudioError::new(
            aiya_player::AudioErrorCode::FileMissing,
            "文件不见了",
        )),
        current: Some(aiya_player::TrackRef {
            id: "t1".into(),
            title: "标题".into(),
            artist: None,
            album: None,
            duration_ms: Some(1000),
            artwork_id: None,
            gain_db: None,
            audio: None,
            path: "D:/music/a.flac".into(),
        }),
        token: 3,
        updated_at: 1_700_000_000_000,
    };
    let v = serde_json::to_value(&state).unwrap();
    // 错误：对象 + 分类字符串（v1 这里是一句话，UI 没法判断该不该跳过）
    assert_eq!(v["error"]["code"], "fileMissing");
    assert_eq!(v["error"]["message"], "文件不见了");
    // 绝对路径不下发（注释多年写着"不出现"，代码一直照发）
    let current = &v["current"];
    assert!(
        current.get("path").is_none(),
        "player:state.current 不得包含 path：{current}"
    );
    // 数字一律 number
    assert!(v["queueVersion"].is_number());
    assert!(v["updatedAt"].is_number());
}
