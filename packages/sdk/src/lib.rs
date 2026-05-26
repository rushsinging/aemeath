//! AgentClient SDK — CLI 与 Agent Runtime 之间的唯一通信契约。
//!
//! `packages/sdk` 只放 trait + 公共类型，零业务依赖。
//! 实现在 `agent/runtime`。

pub mod bootstrap;
pub mod change_set;
pub mod chat;
pub mod client;
pub mod error;
pub mod models;
pub mod project;
pub mod session;
pub mod types;

pub use bootstrap::ChatBootstrapArgs;
pub use change_set::ChangeSet;
pub use chat::{ChatEvent, ChatInput, ChatResult, ChatStream};
pub use client::AgentClient;
pub use error::SdkError;
pub use models::ModelSummary;
pub use project::ProjectContext;
pub use session::{SessionSnapshot, SessionSummary};
pub use types::{CostInfo, PermissionPrompt, StatusInfo, TaskSummary};
