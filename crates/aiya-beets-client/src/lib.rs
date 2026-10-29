//! # aiya-beets-client
//!
//! 播放器侧的工具 client：**发现 → 起进程 → 说协议 → 消费结果**。
//!
//! ## 边界（评审硬门禁）
//!
//! 这个 crate 里**不允许**出现：
//!
//! - 解释器/虚拟环境/包管理器相关的任何字样与布局推断
//! - 任何具体工具插件名
//! - 工具自己的库文件名、配置文件格式（YAML 键名等）
//! - 工具的领域对象模型
//!
//! 换句话说：**它不知道对面是怎么装上去的、怎么配的、内部长什么样。**
//! 它只知道两件事——「用这个入口起一个进程」和「一行一个 JSON」。
//!
//! 具体怎么找到入口（环境变量、配置记录、约定路径、PATH）属于调用方的策略，
//! 由 `Launcher` 传进来。M2 把这些策略集中到 `src-tauri` 的一处，
//! 再把环境知识整体搬到工具侧。
//!
//! ## 为什么要有这层
//!
//! 现状是播放器的 `beets.rs`（1316 行）里混着环境知识和协议处理，改一行
//! 工具侧的逻辑要重发整个播放器。抽出来之后，两侧只通过协议耦合，
//! 工具可以独立发布、独立升级。
//!
//! ## 协议
//!
//! 契约真源在工具侧（`beets-tool/protocol/v2.schema.json`），本仓镜像一份
//! （`protocol/aiya-beets-v2.json`，`npm run proto:check` 守字节一致）。
//! `tests/proto_mirror.rs` 对着镜像逐字段断言本 crate 的 serde 形状——
//! 改了 `proto.rs` 的字段名而没改协议，那条测试会红。

pub mod error;
pub mod launch;
pub mod proto;
pub mod session;
pub mod tool;

pub use error::ToolError;
pub use launch::Launcher;
pub use proto::{StatusReport, PROTOCOL_VERSION};
pub use session::{EventSink, NullSink, SESSION_IDLE_TIMEOUT, TERMINAL_EVENTS};
pub use tool::{BeetsTool, StdioTool};
