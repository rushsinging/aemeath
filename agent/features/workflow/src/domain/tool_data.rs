//! Workflow Tool 的领域命令 / 查询 / 结果类型（设计文档 §5.1–§5.6 的领域形态）。
//!
//! 协议 JSON 类型定义在 `tools` crate 的 domain types；本模块承载端口语义，
//! 由 adapter 负责双向映射。

use serde::{Deserialize, Serialize};

use super::{
    ActiveMode, EdgeDefinition, NodeDefinition, NodeId, NodeKind, NodeStatus, Role, WorkflowStatus,
};

/// evidence 引用形态（设计文档 §5.7）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvidenceRef {
    pub fact_id: String,
    pub kind: EvidenceKind,
}

/// 被引用事实的类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceKind {
    ToolResult,
    SubAgentOutput,
    BackgroundTask,
    UserMessage,
}

/// 节点出口决定（设计文档 §3.2）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SuccessorDecision {
    pub node_id: NodeId,
    pub description: String,
}

/// Replan 的历史边界（设计文档 §3.6）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplanBoundary {
    pub trigger_node_id: NodeId,
    pub stopped_node_ids: Vec<NodeId>,
    pub frozen_fact_refs: Vec<EvidenceRef>,
}

/// continuation 的重入边；target 必须是既有 Pending 非 CompleteNode。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RejoinEdge {
    pub from_node_id: NodeId,
    pub target_node_id: NodeId,
}

/// 尚未生效结构的重组声明。
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct PendingRewrites {
    #[serde(default)]
    pub update: Vec<PendingNodeUpdate>,
    #[serde(default)]
    pub remove: Vec<NodeId>,
}

/// 对既有 Pending Node 的字段更新；`None` 表示不改。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingNodeUpdate {
    pub node_id: NodeId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<Role>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completion_criteria: Option<String>,
}

/// `workflow_create` 的领域命令。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateWorkflowSpec {
    pub name: String,
    pub objective: String,
    pub root_node_id: NodeId,
    pub nodes: Vec<NodeDefinition>,
    pub edges: Vec<EdgeDefinition>,
}

/// `workflow_update` 的聚合生命周期动作。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LifecycleAction {
    Activate,
    Deactivate,
    Complete,
    Cancel,
    Fail,
}

/// `workflow_update` 的领域命令。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdateWorkflowCommand {
    pub workflow_id: String,
    pub action: LifecycleAction,
    /// complete / fail / cancel 必填；activate / deactivate 禁止。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(default)]
    pub evidence_refs: Vec<EvidenceRef>,
}

/// `workflow_node_update` 的节点动作。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeAction {
    Complete,
    Fail,
}

/// `workflow_node_update` 的领域命令。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdateNodeCommand {
    pub workflow_id: String,
    pub node_id: NodeId,
    pub action: NodeAction,
    pub conclusion: String,
    #[serde(default)]
    pub evidence_refs: Vec<EvidenceRef>,
    /// 非 CompleteNode 完成时必填、≥1；fail 与 CompleteNode 完成时禁止。
    #[serde(default)]
    pub successors: Vec<SuccessorDecision>,
}

/// `workflow_replan` 的领域命令。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplanWorkflowCommand {
    pub workflow_id: String,
    pub boundary: ReplanBoundary,
    #[serde(default)]
    pub new_nodes: Vec<NodeDefinition>,
    #[serde(default)]
    pub new_edges: Vec<EdgeDefinition>,
    pub entry_node_ids: Vec<NodeId>,
    pub rejoin_edges: Vec<RejoinEdge>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending_rewrites: Option<PendingRewrites>,
}

/// `workflow_list` 的查询。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListWorkflowsQuery {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<WorkflowStatus>,
    pub limit: usize,
    pub offset: usize,
}

/// `workflow_query` 的查询。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueryWorkflowQuery {
    pub workflow_id: String,
    pub include_facts: bool,
}

/// 节点计数摘要。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct NodeSummaryCounts {
    pub pending: usize,
    pub active: usize,
    pub completed: usize,
    pub failed: usize,
    pub stopped: usize,
}

/// `workflow_list` 的单项摘要。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowSummaryData {
    pub workflow_id: String,
    pub name: String,
    pub status: WorkflowStatus,
    pub active_mode: Option<ActiveMode>,
    /// Inactive 时可能为 `Some(Replanning)`；其余状态为 `None`。
    pub pending_mode: Option<ActiveMode>,
    pub node_summary: NodeSummaryCounts,
    pub updated_at: String,
}

/// `workflow_list` 的结果。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListWorkflowsData {
    pub workflows: Vec<WorkflowSummaryData>,
    pub total: usize,
    pub limit: usize,
    pub offset: usize,
}

/// 节点视图（含结论与出口）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeViewData {
    pub node_id: NodeId,
    pub kind: NodeKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<Role>,
    pub status: NodeStatus,
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completion_criteria: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conclusion: Option<String>,
    #[serde(default)]
    pub evidence_refs: Vec<EvidenceRef>,
    #[serde(default)]
    pub successors: Vec<SuccessorDecision>,
}

/// 可引用执行事实的摘要（由 Runtime 提供，Workflow 只透传）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FactSummaryData {
    pub fact_id: String,
    pub kind: EvidenceKind,
    pub source: String,
    #[serde(default)]
    pub related_node_ids: Vec<NodeId>,
    pub at: String,
}

/// Kernel 计算的当前合法操作提示（不构成授权）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AllowedAction {
    Activate,
    Deactivate,
    Complete,
    Cancel,
    Fail,
    Replan,
    NodeComplete,
    NodeFail,
}

/// `workflow_query` 的结果。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowDetailData {
    pub workflow_id: String,
    pub name: String,
    pub objective: String,
    pub status: WorkflowStatus,
    pub active_mode: Option<ActiveMode>,
    pub pending_mode: Option<ActiveMode>,
    pub root_node_id: NodeId,
    pub complete_node_id: NodeId,
    pub nodes: Vec<NodeViewData>,
    pub edges: Vec<EdgeDefinition>,
    #[serde(default)]
    pub facts: Vec<FactSummaryData>,
    pub allowed_actions: Vec<AllowedAction>,
}

/// 生命周期操作的结果。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LifecycleResultData {
    pub workflow_id: String,
    pub status: WorkflowStatus,
    pub active_mode: Option<ActiveMode>,
    pub pending_mode: Option<ActiveMode>,
}

/// 节点更新的结果。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeUpdateResultData {
    pub workflow_id: String,
    pub node_id: NodeId,
    pub node_status: NodeStatus,
    pub status: WorkflowStatus,
    pub active_mode: Option<ActiveMode>,
    #[serde(default)]
    pub activated_node_ids: Vec<NodeId>,
    #[serde(default)]
    pub stopped_node_ids: Vec<NodeId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replan_boundary: Option<ReplanBoundary>,
}

/// Replan 的结果。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplanResultData {
    pub workflow_id: String,
    pub status: WorkflowStatus,
    pub active_mode: Option<ActiveMode>,
    #[serde(default)]
    pub activated_node_ids: Vec<NodeId>,
    #[serde(default)]
    pub applied_node_ids: Vec<NodeId>,
    #[serde(default)]
    pub retired_pending_node_ids: Vec<NodeId>,
}
