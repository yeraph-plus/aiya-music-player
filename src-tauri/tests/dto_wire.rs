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
        index: Some(0),
        cursor: Some(0),
        repeat: aiya_player::RepeatMode::Off,
        shuffle: false,
        status: aiya_player::PlaybackStatus::Error,
        source: aiya_player::AudioSource::File,
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
        updated_at_ms: 1_700_000_000_000,
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
    assert!(v["updatedAtMs"].is_number(), "快照时间必须带 Ms 后缀且为 number");
    // index/cursor 是 Option：非空队列走 number，不许再出现 -1 哨兵
    assert_eq!(v["index"], 0);
    assert_eq!(v["cursor"], 0);
}

/// 空队列的 index/cursor 必须是 `null`（而不是 -1 哨兵）——
/// docs/17 §9.2 偏差 1 的偿还，wire 上锁死，防止有人改回 `-1`。
#[test]
fn player_state_empty_queue_sends_null_not_minus_one() {
    let state = aiya_player::PlayerState {
        queue_length: 0,
        queue_version: 0,
        index: None,
        cursor: None,
        repeat: aiya_player::RepeatMode::Off,
        shuffle: false,
        status: aiya_player::PlaybackStatus::Idle,
        source: aiya_player::AudioSource::File,
        position_ms: 0,
        duration_ms: None,
        volume: 0.85,
        muted: false,
        rate: 1.0,
        error: None,
        current: None,
        token: 0,
        updated_at_ms: 0,
    };
    let v = serde_json::to_value(&state).unwrap();
    assert!(v["index"].is_null(), "空队列 index=null，wire 值：{}", v["index"]);
    assert!(v["cursor"].is_null(), "空队列 cursor=null，wire 值：{}", v["cursor"]);
}

/// 播放条的音频规格：`lossless` 必须在 payload 里，且三个值都要能表达。
///
/// 这条测试是补历史的窟窿——`TrackAudio` 曾经**根本没有** `lossless` 字段，
/// 而 UI 的规格文字/Hi-Res 高亮/tooltip 全靠它，于是徽章恒显示「未知」，
/// 跨了一个多月没人发现：类型检查（TS 侧写成可选）和运行期（字段缺失渲染
/// 成空）都不会报错。所以要在这里锁死 wire 形状。
#[test]
fn track_audio_wire_carries_three_state_lossless() {
    use aiya_player::TrackAudio;

    let flac = TrackAudio {
        codec: Some("flac".into()),
        container: Some("Flac".into()),
        sample_rate: Some(44_100),
        bits_per_sample: Some(16),
        // 单位 kbps：FLAC 44.1k/16 实测约 900 kbps（写 900_000 就是踩了 bps 的坑）
        bitrate: Some(900),
        channels: Some(2),
        lossless: TrackAudio::infer_lossless(Some("flac"), Some("Flac")),
    };
    let v = serde_json::to_value(&flac).unwrap();
    assert_eq!(
        keys(&v),
        sorted(&[
            "codec",
            "container",
            "sampleRate",
            "bitsPerSample",
            "bitrate",
            "channels",
            "lossless",
        ]),
        "键集合变了就要同步前端 AudioFormat"
    );
    assert_eq!(v["lossless"], true, "FLAC 必须判无损");

    let aac = TrackAudio {
        codec: None,
        container: Some("Mp4".into()),
        sample_rate: Some(44_100),
        bits_per_sample: Some(16),
        bitrate: Some(256), // kbps
        channels: Some(2),
        lossless: TrackAudio::infer_lossless(None, Some("Mp4")),
    };
    let v = serde_json::to_value(&aac).unwrap();
    // 三态里的第三态：Mp4 可能是 ALAC 也可能是 AAC，必须发 null 而不是 false。
    // 发 false 会把无损的 ALAC 标成「有损」——错得比不说更糟。
    assert!(v["lossless"].is_null(), "认不出来时要发 null：{v}");
}

/* ---------------- B2/B3：歌单 / 历史 / 歌词 / 扫描 / 选择 ---------------- */

#[test]
fn playlist_summary_converts_seconds_to_ms() {
    // 造一个"库里是秒、出口是毫秒"的摘要，锁住换算方向：
    // 写反了 UI 会把 1970 年当创建时间，而且是**静默**的。
    let p = aiya_lib::dto::playlist::PlaylistSummary {
        id: "pl_1".into(),
        name: "通勤".into(),
        description: None,
        track_count: 12,
        duration_ms: 3_600_000,
        artwork_id: Some("a1".into()),
        created_at_ms: 1_700_000_000_000,
        updated_at_ms: 1_700_000_600_000,
        sort_position: 3,
    };
    let v = serde_json::to_value(&p).unwrap();
    assert_eq!(
        keys(&v),
        sorted(&[
            "id",
            "name",
            "description",
            "trackCount",
            "durationMs",
            "artworkId",
            "createdAtMs",
            "updatedAtMs",
            "sortPosition",
        ])
    );
    // 旧名必须消失：UI 那边只要还写着 createdAt 就是没跟上
    assert!(v.get("createdAt").is_none(), "秒级旧字段必须消失");
    assert!(v.get("updatedAt").is_none());
    // 同结构里两个时间字段单位一致（都是毫秒），差别只在语义
    assert_eq!(v["createdAtMs"], 1_700_000_000_000i64);
    assert_eq!(v["durationMs"], 3_600_000i64);
    assert!(v["description"].is_null(), "description 必须存在且为 null");
}

#[test]
fn playlist_add_result_nests_summary() {
    let r = aiya_lib::dto::playlist::PlaylistAddResult {
        added: 2,
        skipped: 1,
        summary: aiya_lib::dto::playlist::PlaylistSummary {
            id: "pl_1".into(),
            name: "通勤".into(),
            description: None,
            track_count: 3,
            duration_ms: 0,
            artwork_id: None,
            created_at_ms: 0,
            updated_at_ms: 0,
            sort_position: 0,
        },
    };
    let v = serde_json::to_value(&r).unwrap();
    assert_eq!(keys(&v), sorted(&["added", "skipped", "summary"]));
    // 嵌套对象的字段名同样遵守 camelCase（serde 是逐层生效的）
    assert!(v["summary"]["trackCount"].is_number());
}

#[test]
fn history_stats_uses_ms_and_number_play_counts() {
    let s = aiya_lib::dto::history::HistoryStats {
        total_plays: 42,
        total_ms: 2_520_000,
        completed: 40,
        skipped: 2,
        first_play_at_ms: Some(1_600_000_000_000),
        last_play_at_ms: Some(1_700_000_000_000),
        top_tracks: vec![aiya_lib::dto::history::HistoryTopTrack {
            id: "t1".into(),
            title: "标题".into(),
            artist: Some("艺人".into()),
            play_count: 9,
        }],
    };
    let v = serde_json::to_value(&s).unwrap();
    assert_eq!(
        keys(&v),
        sorted(&[
            "totalPlays",
            "totalMs",
            "completed",
            "skipped",
            "firstPlayAtMs",
            "lastPlayAtMs",
            "topTracks",
        ])
    );
    assert!(v.get("firstPlayAt").is_none(), "秒级旧字段必须消失");
    assert!(v["firstPlayAtMs"].is_number());
    assert!(v["topTracks"][0]["playCount"].is_number());
}

#[test]
fn lyrics_encoding_is_null_when_missing() {
    let empty = aiya_lib::dto::lyrics::Lyrics::empty();
    let v = serde_json::to_value(&empty).unwrap();
    assert_eq!(keys(&v), sorted(&["found", "lines", "offsetMs", "synced", "encoding"]));
    assert_eq!(v["found"], false);
    assert!(v["lines"].as_array().unwrap().is_empty());
    // 无歌词时 encoding 是 null（不是缺字段、不是空字符串）
    assert!(v["encoding"].is_null(), "encoding 必须是 null 而不是空字符串");
}

#[test]
fn lyrics_from_parsed_keeps_time_ms() {
    let parsed = aiya_data::lyrics::ParsedLyrics {
        found: true,
        lines: vec![aiya_data::lyrics::LyricLine { time_ms: 12_345, text: "第一句".into() }],
        offset_ms: -200,
        synced: true,
        encoding: "utf-8".into(),
    };
    let v = serde_json::to_value(aiya_lib::dto::lyrics::Lyrics::from(parsed)).unwrap();
    // 契约字段是 timeMs：写成 time_ms 会让 UI 的 `line.timeMs <= position` 恒 false
    assert_eq!(v["lines"][0]["timeMs"], 12_345);
    assert!(v["lines"][0].get("time_ms").is_none());
    assert_eq!(v["encoding"], "utf-8");
    assert_eq!(v["offsetMs"], -200);
}

#[test]
fn scan_result_sends_numbers_and_camel_case() {
    let r = aiya_data::scanner::ScanResult {
        scanned: 100,
        imported: 90,
        updated: 5,
        unchanged: 4,
        failed: 1,
        walk_errors: 0,
        cancelled: false,
        errors: vec![aiya_data::scanner::ScanError {
            path: "D:/m/bad.mp3".into(),
            message: "解析失败".into(),
        }],
        elapsed_ms: 1234,
    };
    let v = serde_json::to_value(&r).unwrap();
    assert_eq!(
        keys(&v),
        sorted(&[
            "scanned",
            "imported",
            "updated",
            "unchanged",
            "failed",
            "walkErrors",
            "cancelled",
            "errors",
            "elapsedMs",
        ])
    );
    assert!(v["elapsedMs"].is_number());
    assert!(v["walkErrors"].is_number());
    assert_eq!(v["errors"][0]["message"], "解析失败");
}

#[test]
fn small_result_dtos_keep_their_exact_keys() {
    // 这几条命令的返回值都只有一个布尔/数字，最容易"随手改名"而没人发现
    let reveal = serde_json::to_value(aiya_lib::dto::library::RevealResult {
        revealed: false,
        reason: Some("曲目不在库中".into()),
    })
    .unwrap();
    assert_eq!(keys(&reveal), sorted(&["revealed", "reason"]));
    assert_eq!(reveal["reason"], "曲目不在库中");

    let cancel = serde_json::to_value(aiya_lib::dto::library::ScanCancel { cancelled: true }).unwrap();
    assert_eq!(keys(&cancel), sorted(&["cancelled"]));

    let removed = serde_json::to_value(aiya_lib::dto::library::RemoveTracks { removed: 3 }).unwrap();
    assert!(removed["removed"].is_number());

    let job = serde_json::to_value(aiya_lib::dto::beets::JobStarted { started: true }).unwrap();
    assert_eq!(keys(&job), sorted(&["started"]));

    let req = serde_json::to_value(aiya_lib::dto::beets::CancelRequested { requested: false }).unwrap();
    assert_eq!(keys(&req), sorted(&["requested"]));
}

#[test]
fn beets_status_protocol_version_is_number() {
    let st = aiya_lib::dto::beets::BeetsStatus {
        found: true,
        interpreter: Some("aiya-beets.exe".into()),
        script_path: None,
        entry_source: Some("settings".into()),
        protocol_version: Some(3),
        beets_version: Some("2.14.1".into()),
        python_version: None,
        python_supported: Some(true),
        library_path: None,
        library_exists: Some(true),
        library_directory: Some("D:\\Music".into()),
        plugins: Some(vec!["alpha".into()]),
        plugin_health: Some(vec![aiya_lib::dto::beets::BeetsPluginHealth {
            name: "alpha".into(),
            state: "ready".into(),
            enabled: true,
            missing: vec![],
            note: String::new(),
        }]),
        plugin_summary: Some(aiya_lib::dto::beets::BeetsPluginSummary {
            ready: 1,
            missing_deps: 0,
            unavailable: 0,
        }),
        config_dir: None,
        data_home: None,
        error_code: None,
        error_message: None,
    };
    let v = serde_json::to_value(&st).unwrap();
    // Option<u32> 在 ts-rs 里是 bigint，必须 number（挡回归）
    assert!(v["protocolVersion"].is_number(), "protocolVersion 必须 number");
    assert!(v["libraryExists"].is_boolean());
    // 库根目录：UI 用它显示"整理到 <dir>"，并在 null 时禁用整理文件
    assert_eq!(v["libraryDirectory"], "D:\\Music");
    assert!(v["scriptPath"].is_null());
    // 入口来源与能力三态：设置页靠它们回答"为什么用的这个 / 到底能干什么"
    assert_eq!(v["entrySource"], "settings");
    assert_eq!(v["pluginHealth"][0]["state"], "ready");
    assert_eq!(v["pluginSummary"]["missingDeps"], 0);
    assert!(v["pythonSupported"].is_boolean());
    // 入口来源与能力三态：设置页要靠它们回答"为什么用的这个 / 到底能干什么"
    assert_eq!(v["entrySource"], "settings");
    assert_eq!(v["pluginHealth"][0]["state"], "ready");
    assert_eq!(v["pluginSummary"]["missingDeps"], 0);
    assert!(v["pythonSupported"].is_boolean());
}

/// 诊断页两张表的 wire 形状。
///
/// 这两个数组以前是 `Vec<serde_json::Value>`（恒空），UI 只能靠本地视图类型 +
/// 一次 `as` 转换硬顶。现在有真 DTO 了，字段名必须锁住：`tookMs`/`dbTookMs`
/// 少一个下划线都变 `took_ms`，TS 侧就是 `undefined`，而类型检查看不出来。
#[test]
fn diagnostics_timing_and_error_wire_shape() {
    let timing = aiya_lib::dto::ListTiming {
        at_ms: 1_700_000_000_000,
        source: "library".into(),
        rows: 50,
        total: 50_000,
        took_ms: 2.6,
        db_took_ms: 2.3,
        cursor: None,
    };
    let v = serde_json::to_value(&timing).unwrap();
    assert_eq!(
        keys(&v),
        sorted(&["atMs", "source", "rows", "total", "tookMs", "dbTookMs", "cursor"])
    );
    assert!(v["atMs"].is_number(), "i64 必须发 number（不是 bigint/string）");
    assert!(v["tookMs"].is_f64(), "耗时是小数毫秒，别退化成整数");
    assert!(v["cursor"].is_null(), "首屏游标是 null 而不是缺字段");

    let err = aiya_lib::dto::RecentError {
        at_ms: 1_700_000_000_001,
        code: aiya_lib::dto::ApiErrorCode::Io,
        message: "读不到文件".into(),
        detail: Some("os error 5".into()),
    };
    let v = serde_json::to_value(&err).unwrap();
    assert_eq!(keys(&v), sorted(&["atMs", "code", "message", "detail"]));
    assert_eq!(v["code"], serde_json::json!("io"), "枚举走 camelCase");
    assert!(v["detail"].is_string());
}

/// `Diagnostics` 不再有 `shortcutConflicts`（改由前端 `findConflicts` 算）。
/// 这条防的是"顺手把它加回来"——那会让同一件事有两个可能不一致的来源。
#[test]
fn diagnostics_has_no_shortcut_conflicts_field() {
    // 两侧都查：DTO 源码（它是真相）与已入库的 TS 生成物（漂移会被看见）
    // 查字段声明而不是任意出现：注释里解释"为什么删掉"时会写出这个词
    let src = include_str!("../../src-tauri/src/dto/app.rs");
    assert!(
        !src.contains("pub shortcut_conflicts"),
        "shortcut_conflicts 已从 Diagnostics 删除（改由前端 findConflicts 算），别加回来"
    );
    let ts = include_str!("../../ui/src/lib/ipc/generated/Diagnostics.ts");
    assert!(
        !ts.contains("shortcutConflicts"),
        "生成的 Diagnostics 类型里还有 shortcutConflicts —— 跑 npm run gen:types"
    );
}

/* ---------------- m3u/m3u8 导入导出（docs/22 G2） ---------------- */

/// 导入结果的 wire 形状：`playlist` 可空（用户取消是正常分支）、
/// matched/duplicates 分开、未匹配条目带行号。
#[test]
fn playlist_import_result_wire_shape() {
    use aiya_lib::dto::playlist::{PlaylistImportResult, PlaylistImportUnmatched};
    let r = PlaylistImportResult {
        playlist: None, // 用户取消
        matched: 0,
        duplicates: 0,
        unmatched: vec![PlaylistImportUnmatched {
            line: 7,
            raw: "D:/lost/song.flac".into(),
            reason: "notFound".into(),
        }],
    };
    let v = serde_json::to_value(&r).unwrap();
    assert!(v["playlist"].is_null(), "取消时 playlist=null，wire：{v}");
    assert_eq!(v["unmatched"][0]["line"], 7);
    assert_eq!(v["unmatched"][0]["raw"], "D:/lost/song.flac");
    assert_eq!(v["unmatched"][0]["reason"], "notFound");
    assert!(v["matched"].is_number());
}

/// 导出结果的 wire 形状。
#[test]
fn playlist_export_result_wire_shape() {
    use aiya_lib::dto::playlist::PlaylistExportResult;
    let v = serde_json::to_value(PlaylistExportResult {
        path: "D:/out/x.m3u8".into(),
        count: 12,
    })
    .unwrap();
    assert_eq!(v["path"], "D:/out/x.m3u8");
    assert_eq!(v["count"], 12);
}

/// AudioSource 的 wire 值锁定（docs/22 G3）：`"file"` / `"stream"` 小写。
/// UI 进度条按它禁拖/显示"直播中"，拼错就是静默失效。
#[test]
fn audio_source_wire_values() {
    assert_eq!(
        serde_json::to_value(aiya_player::AudioSource::File).unwrap(),
        "file"
    );
    assert_eq!(
        serde_json::to_value(aiya_player::AudioSource::Stream).unwrap(),
        "stream"
    );
}
