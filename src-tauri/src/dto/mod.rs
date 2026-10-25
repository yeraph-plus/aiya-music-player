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

use std::collections::VecDeque;
use std::sync::{Mutex, MutexGuard, OnceLock};

pub mod app;
pub mod beets;
pub mod history;
pub mod library;
pub mod lyrics;
pub mod output;
pub mod playlist;

pub use app::{AppInfo, Diagnostics, ListTiming, RecentError, SpectrumToggle};
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
        let err = Self { code, message: message.into(), detail: None, retryable: false };
        record(&err);
        err
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
        let err = Self {
            code: ApiErrorCode::Busy,
            message: message.into(),
            detail: None,
            retryable: true,
        };
        record(&err);
        err
    }

    /// 没装 beets：引导用户去设置页，不是"出错"
    pub fn beets_missing(message: impl Into<String>) -> Self {
        Self::new(ApiErrorCode::BeetsMissing, message)
    }

    /// 内部错误：把技术细节塞进 `detail`，`message` 保持可读
    pub fn internal(detail: impl std::fmt::Display) -> Self {
        let err = Self {
            code: ApiErrorCode::Internal,
            message: "内部错误".into(),
            detail: Some(detail.to_string()),
            retryable: false,
        };
        record(&err);
        err
    }
}

impl From<aiya_data::DbError> for ApiError {
    fn from(e: aiya_data::DbError) -> Self {
        ApiError::internal(e)
    }
}

/* ---------------- 最近错误台账（诊断页） ---------------- */

/// 环形容量。UI 文案写的是"最多保留 30 条"，改这里要同步改那句。
const RECENT_ERROR_LIMIT: usize = 30;

/// 进程级环形缓冲。
///
/// 为什么用全局静态而不是 `AppState` 字段：采集点在 `ApiError` 的构造函数里，
/// 那里拿不到任何 `State`（它是个纯值类型，还在被 `aiya-data` 之类的
/// crate 间接构造）。挂在全局是"零散埋点"的替代方案 —— 一个点覆盖 53 个命令。
///
/// 错误路径不是热路径（正常操作一年也撞不到几次），Mutex 争用可以忽略。
static RECENT_ERRORS: OnceLock<Mutex<VecDeque<RecentError>>> = OnceLock::new();

fn recent_errors_cell() -> MutexGuard<'static, VecDeque<RecentError>> {
    RECENT_ERRORS
        .get_or_init(|| Mutex::new(VecDeque::with_capacity(RECENT_ERROR_LIMIT)))
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

fn record(err: &ApiError) {
    let mut cell = recent_errors_cell();
    if cell.len() == RECENT_ERROR_LIMIT {
        cell.pop_front();
    }
    cell.push_back(RecentError {
        at_ms: now_millis(),
        code: err.code,
        message: err.message.clone(),
        detail: err.detail.clone(),
    });
}

/// 诊断页用：按时间正序（旧 → 新）快照。
pub fn recent_errors() -> Vec<RecentError> {
    recent_errors_cell().iter().cloned().collect()
}

/// 测试用：清空台账（否则用例之间互相污染）。
#[doc(hidden)]
pub fn clear_recent_errors() {
    recent_errors_cell().clear();
}

#[cfg(test)]
mod ring_tests {
    use super::*;

    /// 环形缓冲的两个性质：容量封顶、只留最新的。
    ///
    /// 全局静态被其它测试共享（并行跑），所以**不清空、不假设总长度**，
    /// 只断言与本次写入序列相关的性质：标记开头的被挤掉了，标记结尾的还在。
    #[test]
    fn recent_errors_ring_caps_and_keeps_newest() {
        let marker = format!("ring-test-{}", std::process::id());
        let total = RECENT_ERROR_LIMIT * 2;
        for i in 0..total {
            let _ = ApiError::new(ApiErrorCode::Io, format!("{marker}-{i}"));
        }

        let got = recent_errors();
        assert!(
            got.len() <= RECENT_ERROR_LIMIT,
            "环形缓冲超过上限：{} > {}",
            got.len(),
            RECENT_ERROR_LIMIT
        );
        assert!(
            got.iter().any(|e| e.message == format!("{marker}-{}", total - 1)),
            "最新一条应留在环里"
        );
        assert!(
            !got.iter().any(|e| e.message == format!("{marker}-0")),
            "最早一条应被挤掉"
        );
        assert_eq!(
            got.iter()
                .find(|e| e.message == format!("{marker}-{}", total - 1))
                .map(|e| e.code),
            Some(ApiErrorCode::Io)
        );
    }

    /// `internal()` 走的是另一条构造路径（不经 `new`），别漏记。
    #[test]
    fn internal_errors_record_detail() {
        let marker = format!("boom-{}", std::process::id());
        let _ = ApiError::internal(&marker);
        let got = recent_errors();
        let hit = got
            .iter()
            .find(|e| e.detail.as_deref() == Some(marker.as_str()))
            .expect("internal 错误必须进台账");
        assert_eq!(hit.code, ApiErrorCode::Internal);
        assert_eq!(hit.message, "内部错误");
    }
}
