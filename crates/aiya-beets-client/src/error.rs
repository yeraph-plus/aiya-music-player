//! 错误分类。
//!
//! 为什么不在 crate 里用 `String` 传递错误：调用方（播放器）要对不同失败
//! 给出不同 UX——"没装"要引导安装、"版本不符"要提示升级、"卡死"要给重试、
//! "业务错误"要把工具自己的诊断原样显示。字符串做不到这个分派。

use std::time::Duration;

/// 与外部工具交互时可能出现的失败。
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ToolError {
    /// 进程起不来（路径错、无执行权限、被安全软件拦）。
    #[error("无法启动外部工具 {program}: {message}")]
    Spawn { program: String, message: String },

    /// 工具在超时内没有任何应答。调用方负责收尸，这里只报事实。
    #[error("外部工具在 {timeout:?} 内没有响应")]
    Timeout { timeout: Duration },

    /// 收到的不是合法 NDJSON 信封（协议漂移或输出被污染）。
    #[error("协议解析失败：{0}")]
    Protocol(String),

    /// 工具明确回了 `ok: false`。`code` / `stage` 由工具定义，宿主不解释，
    /// 原样透传给上层——这正是"宿主不认识工具内部语义"的体现。
    #[error("[{stage}] {code}: {message}")]
    Rejected { code: String, stage: String, message: String },

    /// 已有会话在跑（会话槽被占）。并发保护的一部分，不算异常。
    #[error("已有任务在执行（可先取消）")]
    Busy,

    /// 进程管道等 IO 层失败。
    #[error("{0}")]
    Io(String),
}

impl ToolError {
    /// 给 UI 看的稳定错误码。与 `Rejected` 里工具自带的 code 区分开：
    /// 这里只有"宿主侧"的失败，工具侧的原样透传。
    pub fn code(&self) -> &'static str {
        match self {
            ToolError::Spawn { .. } => "E_SPAWN",
            ToolError::Timeout { .. } => "E_TIMEOUT",
            ToolError::Protocol(_) => "E_PROTOCOL",
            ToolError::Rejected { .. } => "E_TOOL_REJECTED",
            ToolError::Busy => "E_BUSY",
            ToolError::Io(_) => "E_IO",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_are_distinct_and_stable() {
        let cases = [
            (ToolError::Spawn { program: "x".into(), message: "y".into() }, "E_SPAWN"),
            (ToolError::Timeout { timeout: Duration::from_secs(1) }, "E_TIMEOUT"),
            (ToolError::Protocol("bad".into()), "E_PROTOCOL"),
            (
                ToolError::Rejected { code: "E_X".into(), stage: "s".into(), message: "m".into() },
                "E_TOOL_REJECTED",
            ),
            (ToolError::Busy, "E_BUSY"),
            (ToolError::Io("z".into()), "E_IO"),
        ];
        for (err, expected) in cases {
            assert_eq!(err.code(), expected, "错误码漂移：{err}");
            // Display 必须可用且非空（UI 直接显示它）
            assert!(!err.to_string().is_empty());
        }
    }

    /// 工具的 code/stage 必须能在 Display 里看到：排障时用户贴的就是这句话。
    #[test]
    fn rejected_display_carries_tool_detail() {
        let e = ToolError::Rejected {
            code: "E_NO_DIRECTORY".into(),
            stage: "move".into(),
            message: "没有配置 directory".into(),
        };
        let s = e.to_string();
        assert!(s.contains("E_NO_DIRECTORY") && s.contains("move") && s.contains("没有配置"));
    }
}
