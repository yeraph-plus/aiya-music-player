//! 工具门面：宿主只用这一个 trait 说话。
//!
//! 现在的实现是「每次操作起一个短进程」（`StdioTool`）。将来若要换成常驻
//! worker 或本地 socket，只需另写一个 impl，`src-tauri` 的调用点一个字不用改
//! ——这正是 M1 把接口先抽出来的目的。

use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;

use crate::error::ToolError;
use crate::launch::Launcher;
use crate::proto::StatusReport;
use crate::session::{self, EventSink};

/// 与外部整理工具的唯一接口面。
pub trait BeetsTool: Send + Sync {
    /// 启动描述（低层调用——比如自己接管 stdout 做批量收集——需要它）。
    fn launcher(&self) -> &Launcher;

    /// 一次性体检：工具版本、协议版本、数据根、能力清单。
    ///
    /// 与会话共用进程级互斥：已有会话在跑时返回 `Busy`（它同样在起 beets
    /// 进程读同一个 sqlite 库，不能绕过会话槽）。
    fn status(&self, timeout: Duration) -> Result<StatusReport, ToolError>;

    /// 起一个会话：先占会话位，再把请求写进去，事件逐行转发给 `sink`。
    ///
    /// 互斥是**进程级**的：同一时刻只允许一个会话，避免两个操作同时改同一份
    /// 数据（实测过并发写标签的后果）。`on_exit` 在会话真正结束后调用。
    fn start_session(
        &self,
        request: Value,
        channel: &'static str,
        sink: Arc<dyn EventSink>,
        on_exit: Box<dyn FnOnce() + Send + 'static>,
    ) -> Result<(), ToolError>;
}

/// 「每次操作起一个短进程」的实现。
///
/// 低频操作（导入、整理）不值得养常驻进程：每次多花的启动开销在秒级操作的
/// 噪声里，换来的是"没有常驻状态可泄漏"。
#[derive(Debug, Clone)]
pub struct StdioTool {
    launcher: Launcher,
}

impl StdioTool {
    pub fn new(launcher: Launcher) -> Self {
        Self { launcher }
    }
}

impl BeetsTool for StdioTool {
    fn launcher(&self) -> &Launcher {
        &self.launcher
    }

    fn status(&self, timeout: Duration) -> Result<StatusReport, ToolError> {
        session::query_status(&self.launcher, timeout)
    }

    fn start_session(
        &self,
        request: Value,
        channel: &'static str,
        sink: Arc<dyn EventSink>,
        on_exit: Box<dyn FnOnce() + Send + 'static>,
    ) -> Result<(), ToolError> {
        // 顺序不可换：占位必须在 spawn 之前（见 session 模块注释第 3 条）
        if !session::reserve() {
            return Err(ToolError::Busy);
        }
        let result = (|| {
            let (child, stdin) = session::spawn_session(&self.launcher, &request)?;
            // acquire 返回本次会话的世代号：此后所有清理都带世代，旧 reader
            // 的迟到 release 靠世代不匹配变成无害 no-op（否则它会偷走本会话
            // 的 stdin 并清掉 busy 位，两个 beets 进程并发写同一 sqlite）。
            let gen = session::acquire(stdin)?;
            session::spawn_reader(child, gen, sink, channel, on_exit)
        })();
        if result.is_err() {
            // 走到这里的失败要么发生在 acquire 之前（槽必然还空着），要么
            // spawn_reader 已在自己的错误路径里按世代清过槽——两种情况
            // release_unacquired 都是安全的：只在槽空时把 busy 位还回去。
            session::release_unacquired();
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::{NullSink, cancel};

    fn tool() -> StdioTool {
        StdioTool::new(Launcher::new("definitely-not-a-real-program-xyz"))
    }

    /// 起不来的程序要报 `Spawn`，不能 panic，也不能把会话位吃掉。
    #[test]
    fn status_on_missing_program_is_spawn_error() {
        let _g = crate::session::test_lock();
        let err = tool().status(Duration::from_millis(100)).unwrap_err();
        assert!(matches!(err, ToolError::Spawn { .. }), "实际 {err:?}");
    }

    #[test]
    fn failed_session_does_not_leak_slot() {
        let _g = crate::session::test_lock();
        let err = tool()
            .start_session(
                serde_json::json!({"id": 1, "cmd": "status"}),
                "ch",
                Arc::new(NullSink),
                Box::new(|| {}),
            )
            .unwrap_err();
        assert!(matches!(err, ToolError::Spawn { .. }), "实际 {err:?}");
        // 失败路径必须把位子还回来，否则后面所有操作都报"忙"
        assert!(!cancel(), "失败后不该有活跃会话");
        assert!(session::reserve(), "失败后位子必须能重新占");
        session::release_unacquired();
    }
}
