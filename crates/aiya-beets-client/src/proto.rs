//! 协议消息形状（NDJSON 信封 + status 应答）。
//!
//! **这是协议层的镜像，不是工具的领域模型。** 字段名通过 `serde(rename)` 与
//! 线上 JSON 严格一一对应，Rust 标识符则刻意用中性词（`tool_version` /
//! `runtime_version` / `library_root`）——宿主只知道"有个工具、它报了个版本、
//! 它的数据根在这"，不需要知道工具是怎么实现自己的。
//!
//! M3 会用随仓镜像的 JSON Schema 逐字段校验这里（`tests/proto_mirror.rs`），
//! 所以**改字段名必须同时改协议**，否则契约测试会红。

use serde::Deserialize;

/// 一次性应答的外层信封。所有命令共用这个形状。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Envelope {
    pub ok: bool,
    pub data: Option<serde_json::Value>,
    pub error: Option<EnvelopeError>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct EnvelopeError {
    pub code: String,
    pub stage: String,
    pub beets_message: String,
}

/// `status` 命令的 `data` 段。
///
/// 全部可空的理由：status 是"体检报告"，不是"要么全成功要么全失败"的接口。
/// 工具可能报得出自己版本、却报不出数据根（还没初始化），宿主照原样展示。
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StatusReport {
    #[serde(default)]
    pub protocol_version: Option<u32>,
    /// 工具自身版本（线上字段原名 `beetsVersion`，见模块注释）。
    #[serde(default, rename = "beetsVersion")]
    pub tool_version: Option<String>,
    /// 工具宿主运行时的版本（线上字段原名 `pythonVersion`）。
    #[serde(default, rename = "pythonVersion")]
    pub runtime_version: Option<String>,
    #[serde(default)]
    pub library_path: Option<String>,
    #[serde(default)]
    pub library_exists: Option<bool>,
    /// 工具**显式配置**的数据根；None = 用户没配过。
    ///
    /// 与 `library_path` 的区别：后者恒有值（工具有内置默认），拿它做展示会
    /// 误导用户；整理类操作的目标根只认显式配置的这个。
    #[serde(default, rename = "libraryDirectory")]
    pub library_root: Option<String>,
    #[serde(default)]
    pub plugins: Option<Vec<String>>,
    #[serde(default)]
    pub config_dir: Option<String>,
}

impl StatusReport {
    /// 协议版本是否在宿主能理解的范围内。
    ///
    /// M1 只提供判断、**不做拦截**（保持既有行为：版本不符也照常工作）。
    /// M3 冻结协议后由调用方决定是否拒绝对话。
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
            "protocolVersion": 3,
            "beetsVersion": "1.2.3",
            "pythonVersion": "3.14.7",
            "libraryPath": "C:/data/catalog.dat",
            "libraryExists": true,
            "libraryDirectory": "C:/Music",
            "plugins": ["alpha", "beta"],
            "configDir": "C:/data"
        });
        let r: StatusReport = serde_json::from_value(wire).expect("线上形状必须可解析");
        assert_eq!(r.protocol_version, Some(3));
        assert_eq!(r.tool_version.as_deref(), Some("1.2.3"));
        assert_eq!(r.runtime_version.as_deref(), Some("3.14.7"));
        assert_eq!(r.library_exists, Some(true));
        assert_eq!(r.library_root.as_deref(), Some("C:/Music"));
        assert_eq!(r.plugins.as_deref(), Some(&["alpha".to_string(), "beta".to_string()][..]));
        assert!(r.speaks(3));
        assert!(!r.speaks(2));
    }

    /// 缺字段不是错误：工具可能还没初始化完就应答了。
    #[test]
    fn status_report_tolerates_missing_fields() {
        let r: StatusReport = serde_json::from_value(serde_json::json!({})).unwrap();
        assert_eq!(r, StatusReport::default());
        assert!(!r.speaks(3));
    }

    #[test]
    fn envelope_parses_error_branch() {
        let wire = serde_json::json!({
            "ok": false,
            "error": { "code": "E_NO_TOOL", "stage": "probe", "beetsMessage": "not importable" }
        });
        let e: Envelope = serde_json::from_value(wire).unwrap();
        assert!(!e.ok);
        let err = e.error.expect("错误分支必须能取到细节");
        assert_eq!(err.code, "E_NO_TOOL");
        assert_eq!(err.stage, "probe");
        assert_eq!(err.beets_message, "not importable");
    }
}
