//! 镜像契约测试（docs/33 §9 M3）：本 crate 的 serde 形状 vs 播放器仓根镜像的 schema。
//!
//! 三条断言，各守一类漂移：
//!
//! 1. **版本**：镜像 meta 的 `protocolVersion` 必须等于 `PROTOCOL_VERSION`。
//!    改了协议但没改客户端常量 → 红（客户端会继续用旧版本号握手）。
//! 2. **字段**：`StatusData.required` 必须**逐字**等于序列化后的 `StatusReport`
//!    键集合。少一个 = 客户端读不到；多一个 = 客户端在建模对面不存在的东西。
//! 3. **终态事件名**：`TERMINAL_EVENTS` 必须都是 schema 里存在的 `type`。
//!    改名而忘了改 reader → 会话永远不结束，这是最难现场定位的一类 bug。
//!
//! 这些断言都是**对着文件**跑的，不联网、不需要 Python、不需要工具安装。

use std::collections::BTreeSet;

use aiya_beets_client::{StatusReport, PROTOCOL_VERSION, TERMINAL_EVENTS};
use serde_json::Value;

/// 仓库根（`crates/aiya-beets-client` → `../..`）。
fn repo_root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn mirror() -> (Value, Value) {
    let root = repo_root();
    let schema_path = root.join("protocol/aiya-beets-v2.json");
    let meta_path = root.join("protocol/aiya-beets-v2.meta.json");
    let schema: Value = serde_json::from_str(
        &std::fs::read_to_string(&schema_path).unwrap_or_else(|e| {
            panic!("读不到镜像 schema（{}）：{e}\n——跑 npm run proto:sync", schema_path.display())
        }),
    )
    .expect("镜像不是合法 JSON");
    let meta: Value = serde_json::from_str(
        &std::fs::read_to_string(&meta_path)
            .unwrap_or_else(|e| panic!("读不到镜像元数据（{}）：{e}", meta_path.display())),
    )
    .expect("镜像元数据不是合法 JSON");
    (schema, meta)
}

fn defs<'a>(schema: &'a Value, name: &str) -> &'a Value {
    schema
        .get("$defs")
        .and_then(|d| d.get(name))
        .unwrap_or_else(|| panic!("schema 缺 $defs.{name}"))
}

fn strings(v: &Value) -> BTreeSet<String> {
    v.as_array()
        .unwrap_or_else(|| panic!("期望数组，实际 {v}"))
        .iter()
        .map(|x| x.as_str().expect("数组项必须是字符串").to_string())
        .collect()
}

/// 全字段填充，用来逼出 `StatusReport` 的**完整**键集合。
fn full_report() -> StatusReport {
    StatusReport {
        protocol_version: Some(1),
        schema_hash: Some("0".repeat(64)),
        tool_version: Some("0.1.0".into()),
        runtime_version: Some("3.14.0".into()),
        library_path: Some("C:/x/library.db".into()),
        library_exists: Some(true),
        library_root: Some("C:/Music".into()),
        plugins: Some(vec!["a".into()]),
        config_dir: Some("C:/x".into()),
        runtime_supported: Some(true),
        capability_health: Some(serde_json::json!([])),
        capability_summary: Some(serde_json::json!({})),
        data_home: Some("C:/x".into()),
        library_lock: Some("free".into()),
        library_lock_holder: None,
    }
}

#[test]
fn mirror_meta_version_matches_client() {
    let (_schema, meta) = mirror();
    let v = meta.get("protocolVersion").and_then(Value::as_u64).expect("meta.protocolVersion");
    assert_eq!(
        v as u32, PROTOCOL_VERSION,
        "镜像协议版本 {v} 与客户端 PROTOCOL_VERSION {PROTOCOL_VERSION} 不一致：\
         改了协议就要同时改 proto.rs 的常量与镜像（npm run proto:sync）"
    );
}

#[test]
fn status_report_fields_match_schema_required() {
    let (schema, _meta) = mirror();
    let required = strings(&defs(&schema, "StatusData")["required"]);

    let wire = serde_json::to_value(full_report()).expect("StatusReport 必须可序列化");
    let ours: BTreeSet<String> = wire
        .as_object()
        .expect("StatusReport 序列化后必须是对象")
        .keys()
        .cloned()
        .collect();

    let missing: Vec<_> = required.difference(&ours).cloned().collect();
    let extra: Vec<_> = ours.difference(&required).cloned().collect();
    assert!(
        missing.is_empty() && extra.is_empty(),
        "StatusReport 与 schema 的 StatusData 不一致：\n  协议有而客户端没建模：{missing:?}\n  客户端建模了协议里没有的：{extra:?}"
    );
}

/// 每个 `required` 里的字段都必须真的在 `properties` 里（schema 自身自洽）。
/// 不写这条的话，"required 里多了一个没人定义的字段"会让上面那条测试
/// 以"客户端缺字段"的形式报错，指向错的地方。
#[test]
fn mirror_schema_is_self_consistent() {
    let (schema, _meta) = mirror();
    for name in ["StatusData", "PluginHealth", "PluginSummary", "CapabilitiesData", "PingData"] {
        let block = defs(&schema, name);
        let req = strings(&block["required"]);
        let props: BTreeSet<String> = block["properties"]
            .as_object()
            .unwrap_or_else(|| panic!("$defs.{name} 缺 properties"))
            .keys()
            .cloned()
            .collect();
        let undefined: Vec<_> = req.difference(&props).cloned().collect();
        assert!(undefined.is_empty(), "$defs.{name} 的 required 里有未定义的字段：{undefined:?}");
    }
}

#[test]
fn terminal_events_exist_in_schema() {
    let (schema, _meta) = mirror();
    let event = defs(&schema, "Event");
    let variants = event["oneOf"].as_array().expect("Event 必须是 oneOf");

    let mut types = BTreeSet::new();
    for v in variants {
        let name = v["properties"]["type"]["const"]
            .as_str()
            .unwrap_or_else(|| panic!("每个 Event 变体都要有 type const：{v}"));
        types.insert(name.to_string());
    }
    // 18 种纯事件（2026-10-28 +pluginProgress/pluginDone；终态错误信封在 Response）
    assert_eq!(variants.len(), 18, "Event 变体数变了：{}", variants.len());

    for t in TERMINAL_EVENTS {
        // sessionFailed 的形状在 Response 里（它有 ok 字段），不在 Event 里
        if *t == "sessionFailed" {
            let err = defs(&schema, "Response")["oneOf"][1]["properties"]["type"]["const"]
                .as_str();
            assert_eq!(err, Some("sessionFailed"), "错误应答必须带 type=sessionFailed 才被当成终态");
            continue;
        }
        assert!(types.contains(*t), "TERMINAL_EVENTS 里的 {t} 在 schema 里不存在：{types:?}");
    }
}

#[test]
fn every_request_variant_has_cmd_and_is_closed() {
    let (schema, _meta) = mirror();
    let variants = defs(&schema, "Request")["oneOf"].as_array().expect("Request 必须是 oneOf");
    // 2026-10-28 加法：plugin_catalog / plugin_install（docs/40 §3 白名单版）
    assert_eq!(variants.len(), 13, "命令数变了（加命令要改 schema 与这里的期望）");
    for v in variants {
        let cmd = v["properties"]["cmd"]["const"].as_str().expect("每个请求都要有 cmd const");
        assert_eq!(
            v["additionalProperties"], Value::Bool(false),
            "请求 {cmd} 必须关闭 additionalProperties（否则拼错字段名不会报错）"
        );
    }
}
