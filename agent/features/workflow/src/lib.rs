//! # Workflow BC 领域模型与 Graph Kernel（Issue #2082）
//!
//! 本 crate 在 MVP 阶段只承载纯领域层：Graph 结构类型、封闭状态枚举与
//! 确定性 Kernel 校验。不依赖存储、Runtime 或 Tool 层；serde 派生仅为
//! 后续持久化协议（#2083）就绪，本层不做任何 IO。
//!
//! 设计真相源：`docs/design/02-modules/workflow/01-workflow-design.md`。
pub mod domain;

mod constants;

pub(crate) use constants::LOG_TARGET;

pub use domain::{
    ActiveMode, EdgeDefinition, GraphError, GraphInvariantReason, Kernel, NodeDefinition, NodeId,
    NodeKind, NodeStatus, Role, WorkflowDefinition, WorkflowStatus,
};
