//! 协议消息形状（NDJSON 信封 + status 应答）。
//!
//! **这是协议层的镜像，不是工具的领域模型。** 字段名通过 `serde(rename)` 与
//! 线上 JSON 严格一一对应，Rust 标识符则刻意用中性词（`tool_version` /
//! `runtime_version` / `library_root`）——宿主只知道"有个工具、它报了个版本、
//! 它的数据根在这"，不需要知道工具是怎么实现自己的。
//!
//! M3 起用随仓镜像的 JSON Schema 逐字段校验这里（`tests/proto_mirror.rs`），
//! 所以**改字段名必须同时改协议**，否则契约测试会红。

use serde::{Deserialize, Serialize};

/// 本 crate 支持的协议版本。
///
/// v2（2026-10-29，docs/41 §6）：加法为主（requestId/resume/evidence/
/// libraryLock/E_LIBRARY_BUSY/resumedSkipped），拍板升版 + 新字段在 v2 内
/// 定为 required。v1 宿主连 v2 工具会被 `E_PROTOCOL_MISMATCH` 拒绝并在
/// message 里说明升级方向——不做降级协商。
///
/// 用途：① 请求里带 `clientProtocol`（工具据此拒掉版本不符的一方）；
/// ② `tests/proto_mirror.rs` 拿它与镜像 meta 里的版本比对。
pub const PROTOCOL_VERSION: u32 = 2;

/// 一次性应答的外层信封。所有命令共用这个形状。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Envelope {
    pub ok: bool,
    pub data: Option<serde_json::Value>,
    pub error: Option<EnvelopeError>,
}

/// 错误细节。**线上字段是 snake 的 `beets_message`**（schema EnvelopeError
/// required 一字不差）——此前误挂 `rename_all = "camelCase"`，导致 ok:false
/// 信封永远解析失败（被上层当成"协议解析失败"而不是工具自己的诊断）。
/// 正常 status 走 ok:true 所以从未暴露，v2 拒旧组件的路径一跑就现形。
/// extras（`trace`/`holder`/`clientProtocol`…）按 schema 允许未知字段，忽略。
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct EnvelopeError {
    pub code: String,
    pub stage: String,
    pub beets_message: String,
}

/// `status` 命令的 `data` 段。
///
/// 全部可空的理由：status 是"体检报告"，不是"要么全成功要么全失败"的接口。
/// 工具可能报得出自己版本、却报不出数据根（还没初始化），宿主照原样展示。
///
/// 为什么也 derive `Serialize`：镜像测试（`tests/proto_mirror.rs`）要把这个
/// 结构体序列化回 JSON、拿**键集合**与 schema 的 `required` 比。没有它，
/// 校验"字段名对不对"就只能靠人肉对照——那正是契约测试要取代的东西。
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatusReport {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub protocol_version: Option<u32>,
    /// 契约指纹：工具自报的 schema sha256。
    ///
    /// v2 起宿主拿它与编译期嵌入的镜像哈希比对（`schemaMatched`，docs/35
    /// P1-4 闭合）——"组件与播放器分别升级"不再只靠版本号兜底。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema_hash: Option<String>,
    /// 工具自身版本（线上字段原名 `beetsVersion`，见模块注释）。
    #[serde(default, rename = "beetsVersion", skip_serializing_if = "Option::is_none")]
    pub tool_version: Option<String>,
    /// 工具宿主运行时的版本（线上字段原名 `pythonVersion`）。
    #[serde(default, rename = "pythonVersion", skip_serializing_if = "Option::is_none")]
    pub runtime_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub library_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub library_exists: Option<bool>,
    /// 工具**显式配置**的数据根；None = 用户没配过。
    ///
    /// 与 `library_path` 的区别：后者恒有值（工具有内置默认），拿它做展示会
    /// 误导用户；整理类操作的目标根只认显式配置的这个。
    #[serde(default, rename = "libraryDirectory", skip_serializing_if = "Option::is_none")]
    pub library_root: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plugins: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config_dir: Option<String>,
    /// 工具的运行时是否在支持窗口内（工具自报）。
    #[serde(default, rename = "pythonSupported", skip_serializing_if = "Option::is_none")]
    pub runtime_supported: Option<bool>,
    /// 工具自报的**能力健康**（形状由工具定义，client 只透传不解释）。
    ///
    /// 为什么用 `Value` 而不是类型化：能力清单是工具的内部结构，client
    /// 一旦给它建模就等于"知道对面有哪些能力"——那正是边界要挡的东西。
    /// 宿主拿到后自己映射成 UI 形状。
    #[serde(default, rename = "pluginHealth", skip_serializing_if = "Option::is_none")]
    pub capability_health: Option<serde_json::Value>,
    #[serde(default, rename = "pluginSummary", skip_serializing_if = "Option::is_none")]
    pub capability_summary: Option<serde_json::Value>,
    /// 工具的数据根（宿主给的那个目录，工具回报确认）。
    #[serde(default, rename = "dataHome", skip_serializing_if = "Option::is_none")]
    pub data_home: Option<String>,
    /// v2：库文件锁状态（`free|held|unknown`）。
    ///
    /// 字符串而非枚举：client crate 不解释取值语义（宿主 UI 才翻译），
    /// 工具侧加新状态值时这里不需要跟着发版。
    /// 这两个字段在协议里是 required（holder 可 null），所以 None 也序列化
    /// 成 null，不用 `skip_serializing_if`——否则镜像测试的键集合对不上。
    #[serde(default, rename = "libraryLock")]
    pub library_lock: Option<String>,
    /// v2：held 时锁文件里记录的持有者（`pid=…,cmd=…`）；free/unknown 为 null。
    #[serde(default, rename = "libraryLockHolder")]
    pub library_lock_holder: Option<String>,
}

impl StatusReport {
    /// 协议版本是否在宿主能理解的范围内。
    ///
    /// M3 起由调用方（`src-tauri/src/beets.rs`）在探测后立即拦截：
    /// 版本不符时 `found: false` + `E_PROTOCOL_MISMATCH`，并在文案里说清
    /// 该升哪一边。这里只回答"是不是同一版"。
    pub fn speaks(&self, expected: u32) -> bool {
        self.protocol_version == Some(expected)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 线上形状锚点：字段名必须与工具输出的 camelCase 一字不差。
    /// 这条测试红了说明协议镜像漂了，不是"测试写错了"。
    #[test]
    fn status_report_matches_wire_shape() {
        // 值一律用中性占位：本测试验的是**字段名**能不能对上，
        // 不涉及任何具体工具的内容（crate 也不该认识那些名字）。
        let wire = serde_json::json!({
            "protocolVersion": 2,
            "schemaHash": "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
            "beetsVersion": "1.2.3",
            "pythonVersion": "3.14.7",
            "libraryPath": "C:/data/catalog.dat",
            "libraryExists": true,
            "libraryDirectory": "C:/Music",
            "plugins": ["alpha", "beta"],
            "configDir": "C:/data",
            "pythonSupported": true,
            "pluginHealth": [{"name": "alpha", "state": "ready", "enabled": true, "missing": [], "note": ""}],
            "pluginSummary": {"ready": 1, "missingDeps": 0, "unavailable": 0},
            "dataHome": "C:/data",
            "libraryLock": "free",
            "libraryLockHolder": null
        });
        let r: StatusReport = serde_json::from_value(wire).expect("线上形状必须可解析");
        assert_eq!(r.protocol_version, Some(PROTOCOL_VERSION));
        assert_eq!(r.tool_version.as_deref(), Some("1.2.3"));
        assert_eq!(r.runtime_version.as_deref(), Some("3.14.7"));
        assert_eq!(r.library_exists, Some(true));
        assert_eq!(r.library_root.as_deref(), Some("C:/Music"));
        assert_eq!(r.plugins.as_deref(), Some(&["alpha".to_string(), "beta".to_string()][..]));
        assert!(r.speaks(PROTOCOL_VERSION));
        assert!(!r.speaks(PROTOCOL_VERSION + 1));
        // 能力段是透传的：client 不解释形状，只保证能原样取到
        assert_eq!(r.runtime_supported, Some(true));
        assert_eq!(r.data_home.as_deref(), Some("C:/data"));
        assert_eq!(
            r.capability_health.as_ref().and_then(|v| v.get(0)).and_then(|v| v.get("state")).and_then(|v| v.as_str()),
            Some("ready")
        );
        assert!(r.capability_summary.is_some());
    }

    /// 缺字段不是错误：工具可能还没初始化完就应答了。
    #[test]
    fn status_report_tolerates_missing_fields() {
        let r: StatusReport = serde_json::from_value(serde_json::json!({})).unwrap();
        assert_eq!(r, StatusReport::default());
        assert!(!r.speaks(PROTOCOL_VERSION));
    }

    #[test]
    fn envelope_parses_error_branch() {
        let wire = serde_json::json!({
            "ok": false,
            "error": { "code": "E_NO_TOOL", "stage": "probe", "beets_message": "not importable" }
        });
        let e: Envelope = serde_json::from_value(wire).unwrap();
        assert!(!e.ok);
        let err = e.error.expect("错误分支必须能取到细节");
        assert_eq!(err.code, "E_NO_TOOL");
        assert_eq!(err.stage, "probe");
        assert_eq!(err.beets_message, "not importable");
    }
}
