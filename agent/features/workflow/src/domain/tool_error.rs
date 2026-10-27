//! Workflow Tool 的 typed business errors（设计文档 §5.8）。
//!
//! 13 个变体为 MVP 完整枚举：细粒度、稳定、可序列化，供 Runtime 透传与
//! LLM 消费。不使用单一粗分类，也不使用 `EvidenceNotFound`（Kernel 只校验
//! 引用结构，不校验实体存在）。

use serde::{Deserialize, Serialize};

use super::GraphInvariantReason;

/// Workflow Tool 业务错误。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WorkflowToolError {
    /// 创建时 Session 已存在 Active Workflow。
    #[error("active workflow already exists")]
    ActiveWorkflowAlreadyExists,
    /// 目标 Workflow 不在允许该操作的状态。
    #[error("workflow not active: {workflow_id}")]
    WorkflowNotActive { workflow_id: String },
    /// 目标 Workflow 已处于不可逆终态。
    #[error("workflow already terminal: {workflow_id}")]
    WorkflowAlreadyTerminal { workflow_id: String },
    /// 目标 Workflow 处于 Active(Replanning)，拒绝 node_update / complete。
    #[error("workflow is replanning: {workflow_id}")]
    WorkflowReplanning { workflow_id: String },
    /// 目标 Workflow 不处于 Active(Replanning)，拒绝 replan。
    #[error("workflow is not replanning: {workflow_id}")]
    WorkflowNotReplanning { workflow_id: String },
    /// 不存在合法 rejoin target，无法通过 Replan 继续。
    #[error("workflow cannot continue: {workflow_id}")]
    WorkflowCannotContinue { workflow_id: String },
    /// 目标 Node 不是当前 Active Node。
    #[error("node not active: {node_id}")]
    NodeNotActive { node_id: String },
    /// 目标 Node 属于冻结历史。
    #[error("node history frozen: {node_id}")]
    NodeHistoryFrozen { node_id: String },
    /// successors 结构非法（非直接后继、缺失、出现在禁止位置）。
    #[error("invalid successor: {subject}")]
    InvalidSuccessor { subject: String },
    /// evidence 引用结构非法（缺 id、kind 非法、形状错误）。
    /// 不表示被引用实体不存在。
    #[error("invalid evidence reference")]
    InvalidEvidenceReference,
    /// Graph 结构不变量违规，附 Kernel 细分原因。
    #[error("graph invariant violation: {reason:?}")]
    GraphInvariantViolation {
        reason: GraphInvariantReason,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        subject: Option<String>,
    },
    /// complete 条件不满足（CompleteNode 未完成或仍存在 Active Node）。
    #[error("completion conditions unsatisfied: {subject}")]
    CompletionConditionsUnsatisfied { subject: String },
    /// Workflow 已不在当前 Session 的引用集合中（如 `/clear` 之后）。
    #[error("workflow not visible: {workflow_id}")]
    WorkflowNotVisible { workflow_id: String },
}
