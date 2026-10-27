//! Workflow Graph 领域类型与 Kernel 校验（设计文档 §2、§3、§5.8）。
pub mod kernel;

pub use kernel::Kernel;

#[cfg(test)]
mod kernel_tests;

use serde::{Deserialize, Serialize};

/// Workflow 聚合五状态（设计文档 §2.1）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowStatus {
    Active,
    Inactive,
    Completed,
    Cancelled,
    Failed,
}

/// Active Workflow 的两种模式（设计文档 §2.1）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActiveMode {
    Executing,
    Replanning,
}

/// Node 类型三分类（设计文档 §3.1）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeKind {
    Work,
    Confirm,
    Complete,
}

/// Node 五状态（设计文档 §3.1）；Completed / Failed / Stopped 为不可逆终态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeStatus {
    Pending,
    Active,
    Completed,
    Failed,
    Stopped,
}

/// Node Role 六元封闭集合（设计文档 §3.5；MVP 仅语义标签，不驱动 Tool 裁剪）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Planner,
    Explorer,
    Architect,
    Implementer,
    Verifier,
    Reviewer,
}

/// Node 身份标识；MVP 用字符串 id，创建后不可变。
pub type NodeId = String;

/// 节点定义（静态结构；运行状态由聚合持有，此处只描述定义）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeDefinition {
    pub node_id: NodeId,
    pub kind: NodeKind,
    /// kind = work 时必填；confirm / complete 禁止。
    pub role: Option<Role>,
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completion_criteria: Option<String>,
}

/// 静态 Edge；不含业务条件，业务条件由 SuccessorDecision 承载（#2086 范围）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EdgeDefinition {
    pub from_node_id: NodeId,
    pub to_node_id: NodeId,
}

/// 一次 `workflow_create` 提交的完整 DAG 提案（设计文档 §5.1）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowDefinition {
    pub name: String,
    pub objective: String,
    pub root_node_id: NodeId,
    pub nodes: Vec<NodeDefinition>,
    pub edges: Vec<EdgeDefinition>,
}

/// Graph 结构不变量违规的细分原因（设计文档 §5.8 `GraphInvariantViolation` 附加上下文）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GraphInvariantReason {
    MissingNode,
    MultipleRoots,
    RootIsSpecialKind,
    MissingCompleteNode,
    MultipleCompleteNodes,
    CompleteNodeHasSuccessor,
    NodeWithoutSuccessor,
    UnreachableNode,
    NoPathToCompleteNode,
    CycleDetected,
    SelfLoop,
    RoleOnSpecialKind,
    WorkNodeMissingRole,
}

/// Kernel 结构校验错误；本层只产生 `GraphInvariantViolation` 语义。
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, Serialize, Deserialize)]
#[error("GraphInvariantViolation: {reason:?}")]
pub struct GraphError {
    pub reason: GraphInvariantReason,
    /// 违规对象（node id / edge 端点），供修复上下文。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject: Option<String>,
}
