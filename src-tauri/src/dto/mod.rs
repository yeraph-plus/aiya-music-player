//! IPC DTO：跨进程边界的**唯一**形状来源（docs/17）。
//!
//! 为什么要有这一层（而不是继续 `json!` 手拼）：
//! - `json!` 拼出来的字段名/嵌套形状没有任何编译期约束，TS 侧只能手抄
//!   （`ui/contracts` 1301 行全靠人肉同步，已出过两次事）；
//! - 这里的每个结构体都 `derive(TS)`，由 `npm run gen:types` 生成到
//!   `ui/src/lib/ipc/generated/`，字段一改，TS 侧跟着变，`git diff` 会红。
//!
//! 约定（docs/17 §3）：
//! - 字段 Rust 侧 snake_case，wire 上一律 camelCase；
//! - 时间**一律毫秒**（`*Ms`），id 一律 `String`，缺值用 `Option` → `null`；
//! - 整数一律 `#[ts(type = "number")]`：ts-rs 默认把 `i64/u64` 映射成 `bigint`，
//!   而 serde_json 发的是 JSON number，JS 侧 `JSON.parse` 得到的是 `number`。
//!   `Option<i64>` 必须写成 `#[ts(type = "number | null")]`——`ts(type)` 会覆盖
//!   整个类型，连 `| null` 都要自己带上。
//!
//! 模块划分按"谁在哪个页面用"：
//! - [`app`]：关于页/诊断页（含输出模式的子结构在 [`output`]）
//! - [`library`]：曲库列表/扫描/选择/移除
//! - [`playlist`]：歌单
//! - [`lyrics`]：歌词
//! - [`history`]：播放历史
//! - [`beets`]：beets 环境与导入回执
//!
//! **参数为什么不在这里**：命令参数出错必须映射成 `invalidArg`，而 Tauri 的
//! 参数反序列化发生在命令体之前——用强类型参数会把"参数不合法"变成 Tauri
//! 自己的一句话 reject，绕过本模块的错误分类（docs/17 §3.2）。所以
//! query / settings 这类复合参数仍收 `Json` 并在命令体里解析。

use serde::{Deserialize, Serialize};
use ts_rs::TS;

pub mod app;
pub mod beets;
pub mod history;
pub mod library;
pub mod lyrics;
pub mod output;
pub mod playlist;

pub use app::{AppInfo, Diagnostics, SpectrumToggle};
pub use library::{LibraryStats, TrackDetail};
pub use output::{AudioModeEvent, AudioModeInfo, DesiredOutput, MediaKeysInfo, PowerInfo, StreamsInfo};

/// 命令失败的统一形状。替代 `{ok:false, error:{code,message}}` 信封。
///
/// 为什么不再用信封：错误本来就有两条通道（Tauri 自己 reject + `ok:false`），
/// UI 每处要判两次；而且信封逼着命令返回 `Value`，typed struct 永远进不来。
/// 现在命令返回 `Result<T, ApiError>`，Tauri 把 `Err` 序列化后直接 reject。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct ApiError {
    pub code: ApiErrorCode,
    /// 面向用户的一句话，可直接显示
    pub message: String,
    /// 面向诊断页/日志的技术细节，不进 UI 文案。
    ///
    /// 不用 `skip_serializing_if`：字段缺席与 `null` 在 TS 侧是两种类型
    /// （`detail?: string` vs `detail: string | null`），ts-rs 生成的是后者，
    /// 于是"省一个 null"会让类型与运行期对不上。宁可多两个字节。
    pub detail: Option<String>,
    /// UI 是否值得提供"重试"（设备断开、扫描冲突 = true；参数错 = false）
    pub retryable: bool,
}

/// 错误分类。刻意比旧码少：旧实现里 `E_INTERNAL` 一个码占了 26/41，
/// 等于没有分类；这里按"UI 该怎么处理"来分，而不是按"哪段代码出错"来分。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub enum ApiErrorCode {
    /// 兜底：没预料到的内部错误
    Internal,
    /// 目标不存在（曲目/歌单/文件）
    NotFound,
    /// 参数非法（含形状不符、名字不合规、路径不合规）
    InvalidArg,
    /// 正忙（扫描进行中等），`retryable` 通常为 true
    Busy,
    /// IO 失败：文件被占用、磁盘满、权限
    Io,
    /// 找不到 beets（Python 侧车未安装/未初始化）
    BeetsMissing,
    /// 设备/格式不支持（如必须独占但设备拒绝）
    Unsupported,
    /// 音频设备丢失（拔声卡、驱动崩）
    DeviceLost,
}

impl ApiError {
    pub fn new(code: ApiErrorCode, message: impl Into<String>) -> Self {
        Self { code, message: message.into(), detail: None, retryable: false }
    }

    /// 参数非法：不重试
    pub fn invalid_arg(message: impl Into<String>) -> Self {
        Self::new(ApiErrorCode::InvalidArg, message)
    }

    /// 目标不存在：不重试（重试也是同一个结果，除非用户先改了数据）
    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(ApiErrorCode::NotFound, message)
    }

    /// 正忙：`retryable = true`——等对方结束再点一次确实可能成功
    pub fn busy(message: impl Into<String>) -> Self {
        Self {
            code: ApiErrorCode::Busy,
            message: message.into(),
            detail: None,
            retryable: true,
        }
    }

    /// 没装 beets：引导用户去设置页，不是"出错"
    pub fn beets_missing(message: impl Into<String>) -> Self {
        Self::new(ApiErrorCode::BeetsMissing, message)
    }

    /// 内部错误：把技术细节塞进 `detail`，`message` 保持可读
    pub fn internal(detail: impl std::fmt::Display) -> Self {
        Self {
            code: ApiErrorCode::Internal,
            message: "内部错误".into(),
            detail: Some(detail.to_string()),
            retryable: false,
        }
    }
}

impl From<aiya_data::DbError> for ApiError {
    fn from(e: aiya_data::DbError) -> Self {
        ApiError::internal(e)
    }
}
