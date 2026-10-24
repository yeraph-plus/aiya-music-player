//! beets 环境探测与导入回执。
//!
//! `BeetsStatus` 从 `beets.rs` 搬到这里：它是设置页的数据源，是**契约**，
//! 不是 beets 子进程的内部账本。放一起的好处是 `grep dto::` 就能看全
//! IPC 形状；`beets.rs` 里 `pub use` 回来，调用点不用改。

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// 设置页 beets 状态区数据源。
///
/// 全部字段可空是刻意的：探测失败时不是"没数据"，而是"哪一步拿到了什么"
/// 都要显示——设置页要能告诉用户是没装 Python、没装 beets、还是配置目录
/// 不存在。`error_code` / `error_message` 只在失败时填。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct BeetsStatus {
    pub found: bool,
    pub interpreter: Option<String>,
    pub script_path: Option<String>,
    #[ts(type = "number | null")]
    pub protocol_version: Option<u32>,
    pub beets_version: Option<String>,
    pub python_version: Option<String>,
    pub library_path: Option<String>,
    pub library_exists: Option<bool>,
    pub plugins: Option<Vec<String>>,
    pub config_dir: Option<String>,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
}

/// "任务已启动"（preview / apply / sync 三个命令共用）。
///
/// 这些命令是**异步**的：真正进度走 `beets:import` 事件流，返回值只表示
/// "接单了"。所以这里恒为 true——保留字段而不是返回 `()`，是为了将来能
/// 表达"已有会话在跑，这次被忽略"而不改签名。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct JobStarted {
    pub started: bool,
}

/// 取消请求的回执：`false` 表示当时没有可取消的会话（幂等，不算错）。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct CancelRequested {
    pub requested: bool,
}
