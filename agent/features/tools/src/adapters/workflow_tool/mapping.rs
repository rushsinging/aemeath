//! 协议类型 ↔ `workflow` 领域命令 / 结果的映射。

use crate::domain::types::workflow::{
    WorkflowCreateResult, WorkflowEdgeInput, WorkflowEdgeResult, WorkflowEvidenceRefInput,
    WorkflowFactSummaryResult, WorkflowListResult, WorkflowNodeInput, WorkflowNodeUpdateResult,
    WorkflowNodeViewResult, WorkflowPendingRewritesInput, WorkflowQueryResult,
    WorkflowRejoinEdgeInput, WorkflowReplanBoundaryInput, WorkflowReplanResult, WorkflowRoleInput,
    WorkflowSuccessorInput, WorkflowSummaryResult, WorkflowUpdateResult,
};
use workflow::{
    ActiveMode, AllowedAction, EdgeDefinition, EvidenceKind, EvidenceRef, FactSummaryData,
    LifecycleResultData, ListWorkflowsData, NodeDefinition, NodeKind, NodeStatus,
    NodeSummaryCounts, NodeUpdateResultData, NodeViewData, PendingNodeUpdate, PendingRewrites,
    RejoinEdge, ReplanBoundary, ReplanResultData, Role, SuccessorDecision, WorkflowDetailData,
    WorkflowStatus, WorkflowSummaryData, WorkflowToolError,
};

pub(super) fn status_label(status: WorkflowStatus) -> &'static str {
    match status {
        WorkflowStatus::Active => "active",
        WorkflowStatus::Inactive => "inactive",
        WorkflowStatus::Completed => "completed",
        WorkflowStatus::Cancelled => "cancelled",
        WorkflowStatus::Failed => "failed",
    }
}

pub(super) fn mode_label(mode: ActiveMode) -> &'static str {
    match mode {
        ActiveMode::Executing => "executing",
        ActiveMode::Replanning => "replanning",
    }
}

fn kind_label(kind: NodeKind) -> &'static str {
    match kind {
        NodeKind::Work => "work",
        NodeKind::Confirm => "confirm",
        NodeKind::Complete => "complete",
    }
}

pub(super) fn node_status_label(status: NodeStatus) -> &'static str {
    match status {
        NodeStatus::Pending => "pending",
        NodeStatus::Active => "active",
        NodeStatus::Completed => "completed",
        NodeStatus::Failed => "failed",
        NodeStatus::Stopped => "stopped",
    }
}

pub(super) fn role_label(role: Role) -> &'static str {
    match role {
        Role::Planner => "planner",
        Role::Explorer => "explorer",
        Role::Architect => "architect",
        Role::Implementer => "implementer",
        Role::Verifier => "verifier",
        Role::Reviewer => "reviewer",
    }
}

pub(super) fn role_from_input(role: WorkflowRoleInput) -> Role {
    match role {
        WorkflowRoleInput::Planner => Role::Planner,
        WorkflowRoleInput::Explorer => Role::Explorer,
        WorkflowRoleInput::Architect => Role::Architect,
        WorkflowRoleInput::Implementer => Role::Implementer,
        WorkflowRoleInput::Verifier => Role::Verifier,
        WorkflowRoleInput::Reviewer => Role::Reviewer,
    }
}

pub(super) fn allowed_action_label(action: AllowedAction) -> &'static str {
    match action {
        AllowedAction::Activate => "activate",
        AllowedAction::Deactivate => "deactivate",
        AllowedAction::Complete => "complete",
        AllowedAction::Cancel => "cancel",
        AllowedAction::Fail => "fail",
        AllowedAction::Replan => "replan",
        AllowedAction::NodeComplete => "node_complete",
        AllowedAction::NodeFail => "node_fail",
    }
}

pub(super) fn evidence_kind_label(kind: EvidenceKind) -> &'static str {
    match kind {
        EvidenceKind::ToolResult => "tool_result",
        EvidenceKind::SubAgentOutput => "sub_agent_output",
        EvidenceKind::BackgroundTask => "background_task",
        EvidenceKind::UserMessage => "user_message",
    }
}

pub(super) fn evidence_ref_from_input(input: &WorkflowEvidenceRefInput) -> EvidenceRef {
    EvidenceRef {
        fact_id: input.fact_id.clone(),
        kind: match input.kind {
            crate::domain::types::workflow::WorkflowEvidenceKindInput::ToolResult => {
                EvidenceKind::ToolResult
            }
            crate::domain::types::workflow::WorkflowEvidenceKindInput::SubAgentOutput => {
                EvidenceKind::SubAgentOutput
            }
            crate::domain::types::workflow::WorkflowEvidenceKindInput::BackgroundTask => {
                EvidenceKind::BackgroundTask
            }
            crate::domain::types::workflow::WorkflowEvidenceKindInput::UserMessage => {
                EvidenceKind::UserMessage
            }
        },
    }
}

pub(super) fn evidence_refs_from_input(
    refs: &Option<Vec<WorkflowEvidenceRefInput>>,
) -> Vec<EvidenceRef> {
    refs.as_deref()
        .unwrap_or_default()
        .iter()
        .map(evidence_ref_from_input)
        .collect()
}

pub(super) fn successor_from_input(input: &WorkflowSuccessorInput) -> SuccessorDecision {
    SuccessorDecision {
        node_id: input.node_id.clone(),
        description: input.description.clone(),
    }
}

pub(super) fn successors_from_input(
    successors: &Option<Vec<WorkflowSuccessorInput>>,
) -> Vec<SuccessorDecision> {
    successors
        .as_deref()
        .unwrap_or_default()
        .iter()
        .map(successor_from_input)
        .collect()
}

/// 按 nodes 下标映射生成 id（`wnd_<下标+1>`，图内唯一、稳定可测）。
///
/// LLM 协议面 NEVER 携带 node id（#2091 口径：id 由 Workflow 自动填充）；
/// 下标越界返回 `None`，由调用方转协议错误。
pub(super) fn node_id_at(index: usize) -> String {
    format!("wnd_{}", index + 1)
}

pub(super) fn edge_from_indexes(
    input: &WorkflowEdgeInput,
    node_count: usize,
) -> Option<EdgeDefinition> {
    edge_from_indexes_with(input, node_count, node_id_at)
}

fn edge_from_indexes_with(
    input: &WorkflowEdgeInput,
    node_count: usize,
    id_of: fn(usize) -> String,
) -> Option<EdgeDefinition> {
    if input.from_index >= node_count || input.to_index >= node_count {
        return None;
    }
    Some(EdgeDefinition {
        from_node_id: id_of(input.from_index),
        to_node_id: id_of(input.to_index),
    })
}

/// replan `new_nodes`：id 由系统按 `wrn_<下标+1>` 生成（LLM 不填 id）。
pub(super) fn replan_nodes_from_input(input: &[WorkflowNodeInput]) -> Vec<NodeDefinition> {
    input
        .iter()
        .enumerate()
        .map(|(index, node)| {
            let mut definition = node_from_input(node);
            definition.node_id = replan_node_id_at(index);
            definition
        })
        .collect()
}

/// replan 新节点 id：`wrn_<下标+1>`，与 create 生成的 `wnd_` 原节点命名空间区分。
pub(super) fn replan_node_id_at(index: usize) -> String {
    format!("wrn_{}", index + 1)
}

/// 解析 replan `new_edges`：下标成对出现在 `new_nodes` 内部或跨（new → rejoin target 由
/// `rejoin_edges` 表达，此处仅处理 new_nodes 内部下标）；越界直接丢弃该边由 Kernel 兜底。
pub(super) fn edges_from_indexes(
    input: &Option<Vec<WorkflowEdgeInput>>,
    node_count: usize,
) -> Vec<EdgeDefinition> {
    input
        .as_deref()
        .unwrap_or_default()
        .iter()
        .filter_map(|edge| edge_from_indexes_with(edge, node_count, replan_node_id_at))
        .collect()
}

pub(super) fn node_from_input(input: &WorkflowNodeInput) -> NodeDefinition {
    NodeDefinition {
        node_id: node_id_at(input.index),
        kind: match input.kind {
            crate::domain::types::workflow::WorkflowNodeKindInput::Work => NodeKind::Work,
            crate::domain::types::workflow::WorkflowNodeKindInput::Confirm => NodeKind::Confirm,
            crate::domain::types::workflow::WorkflowNodeKindInput::Complete => NodeKind::Complete,
        },
        role: input.role.map(role_from_input),
        description: input.description.clone(),
        completion_criteria: input.completion_criteria.clone(),
    }
}

pub(super) fn nodes_from_input(input: &[WorkflowNodeInput]) -> Vec<NodeDefinition> {
    input.iter().map(node_from_input).collect()
}

pub(super) fn boundary_from_input(input: &WorkflowReplanBoundaryInput) -> ReplanBoundary {
    ReplanBoundary {
        trigger_node_id: input.trigger_node_id.clone(),
        stopped_node_ids: input.stopped_node_ids.clone(),
        frozen_fact_refs: input
            .frozen_fact_refs
            .iter()
            .map(evidence_ref_from_input)
            .collect(),
    }
}

pub(super) fn rejoin_edges_from_input(input: &[WorkflowRejoinEdgeInput]) -> Vec<RejoinEdge> {
    input
        .iter()
        .map(|edge| RejoinEdge {
            from_node_id: edge.from_node_id.clone(),
            target_node_id: edge.target_node_id.clone(),
        })
        .collect()
}

pub(super) fn pending_rewrites_from_input(
    input: &Option<WorkflowPendingRewritesInput>,
) -> Option<PendingRewrites> {
    input.as_ref().map(|rewrites| PendingRewrites {
        update: rewrites
            .update
            .iter()
            .map(|update| PendingNodeUpdate {
                node_id: update.node_id.clone(),
                description: update.description.clone(),
                role: update.role.map(role_from_input),
                completion_criteria: update.completion_criteria.clone(),
            })
            .collect(),
        remove: rewrites.remove.clone(),
    })
}

fn node_summary_to_result(
    counts: NodeSummaryCounts,
) -> crate::domain::types::workflow::WorkflowNodeSummaryResult {
    crate::domain::types::workflow::WorkflowNodeSummaryResult {
        pending: counts.pending,
        active: counts.active,
        completed: counts.completed,
        failed: counts.failed,
        stopped: counts.stopped,
    }
}

fn summary_to_result(summary: WorkflowSummaryData) -> WorkflowSummaryResult {
    WorkflowSummaryResult {
        workflow_id: summary.workflow_id,
        name: summary.name,
        status: status_label(summary.status).to_owned(),
        active_mode: summary.active_mode.map(|mode| mode_label(mode).to_owned()),
        pending_mode: summary.pending_mode.map(|mode| mode_label(mode).to_owned()),
        node_summary: node_summary_to_result(summary.node_summary),
        updated_at: summary.updated_at,
    }
}

pub(super) fn create_result(
    summary: WorkflowSummaryData,
    root_node_id: String,
    complete_node_id: String,
    node_count: usize,
) -> WorkflowCreateResult {
    let create_summary = summary_to_result(summary);
    WorkflowCreateResult {
        workflow_id: create_summary.workflow_id,
        status: create_summary.status,
        active_mode: create_summary.active_mode,
        root_node_id,
        complete_node_id,
        node_count,
    }
}

pub(super) fn list_result(data: ListWorkflowsData) -> WorkflowListResult {
    WorkflowListResult {
        workflows: data.workflows.into_iter().map(summary_to_result).collect(),
        total: data.total,
        limit: data.limit,
        offset: data.offset,
    }
}

fn node_view_to_result(node: NodeViewData) -> WorkflowNodeViewResult {
    WorkflowNodeViewResult {
        node_id: node.node_id,
        kind: kind_label(node.kind).to_owned(),
        role: node.role.map(|role| role_label(role).to_owned()),
        status: node_status_label(node.status).to_owned(),
        description: node.description,
        completion_criteria: node.completion_criteria,
        conclusion: node.conclusion,
        evidence_refs: node
            .evidence_refs
            .into_iter()
            .map(|evidence| WorkflowEvidenceRefInput {
                fact_id: evidence.fact_id,
                kind: match evidence.kind {
                    EvidenceKind::ToolResult => {
                        crate::domain::types::workflow::WorkflowEvidenceKindInput::ToolResult
                    }
                    EvidenceKind::SubAgentOutput => {
                        crate::domain::types::workflow::WorkflowEvidenceKindInput::SubAgentOutput
                    }
                    EvidenceKind::BackgroundTask => {
                        crate::domain::types::workflow::WorkflowEvidenceKindInput::BackgroundTask
                    }
                    EvidenceKind::UserMessage => {
                        crate::domain::types::workflow::WorkflowEvidenceKindInput::UserMessage
                    }
                },
            })
            .collect(),
        successors: node
            .successors
            .into_iter()
            .map(|decision| WorkflowSuccessorInput {
                node_id: decision.node_id,
                description: decision.description,
            })
            .collect(),
    }
}

fn fact_to_result(fact: FactSummaryData) -> WorkflowFactSummaryResult {
    WorkflowFactSummaryResult {
        fact_id: fact.fact_id,
        kind: evidence_kind_label(fact.kind).to_owned(),
        source: fact.source,
        related_node_ids: fact.related_node_ids,
        at: fact.at,
    }
}

pub(super) fn query_result(data: WorkflowDetailData) -> WorkflowQueryResult {
    WorkflowQueryResult {
        workflow_id: data.workflow_id,
        name: data.name,
        objective: data.objective,
        status: status_label(data.status).to_owned(),
        active_mode: data.active_mode.map(|mode| mode_label(mode).to_owned()),
        pending_mode: data.pending_mode.map(|mode| mode_label(mode).to_owned()),
        root_node_id: data.root_node_id,
        complete_node_id: data.complete_node_id,
        nodes: data.nodes.into_iter().map(node_view_to_result).collect(),
        edges: data
            .edges
            .into_iter()
            .map(|edge| WorkflowEdgeResult {
                from_node_id: edge.from_node_id,
                to_node_id: edge.to_node_id,
            })
            .collect(),
        facts: data.facts.into_iter().map(fact_to_result).collect(),
        allowed_actions: data
            .allowed_actions
            .into_iter()
            .map(|action| allowed_action_label(action).to_owned())
            .collect(),
    }
}

pub(super) fn update_result(data: LifecycleResultData) -> WorkflowUpdateResult {
    WorkflowUpdateResult {
        workflow_id: data.workflow_id,
        status: status_label(data.status).to_owned(),
        active_mode: data.active_mode.map(|mode| mode_label(mode).to_owned()),
        pending_mode: data.pending_mode.map(|mode| mode_label(mode).to_owned()),
    }
}

pub(super) fn node_update_result(data: NodeUpdateResultData) -> WorkflowNodeUpdateResult {
    WorkflowNodeUpdateResult {
        workflow_id: data.workflow_id,
        node_id: data.node_id,
        node_status: node_status_label(data.node_status).to_owned(),
        status: status_label(data.status).to_owned(),
        active_mode: data.active_mode.map(|mode| mode_label(mode).to_owned()),
        activated_node_ids: data.activated_node_ids,
        stopped_node_ids: data.stopped_node_ids,
        replan_boundary: data.replan_boundary.map(|boundary| WorkflowReplanBoundaryInput {
            trigger_node_id: boundary.trigger_node_id,
            stopped_node_ids: boundary.stopped_node_ids,
            frozen_fact_refs: boundary
                .frozen_fact_refs
                .into_iter()
                .map(|evidence| WorkflowEvidenceRefInput {
                    fact_id: evidence.fact_id,
                    kind: match evidence.kind {
                        EvidenceKind::ToolResult => {
                            crate::domain::types::workflow::WorkflowEvidenceKindInput::ToolResult
                        }
                        EvidenceKind::SubAgentOutput => crate::domain::types::workflow::WorkflowEvidenceKindInput::SubAgentOutput,
                        EvidenceKind::BackgroundTask => crate::domain::types::workflow::WorkflowEvidenceKindInput::BackgroundTask,
                        EvidenceKind::UserMessage => crate::domain::types::workflow::WorkflowEvidenceKindInput::UserMessage,
                    },
                })
                .collect(),
        }),
    }
}

pub(super) fn replan_result(data: ReplanResultData) -> WorkflowReplanResult {
    WorkflowReplanResult {
        workflow_id: data.workflow_id,
        status: status_label(data.status).to_owned(),
        active_mode: data.active_mode.map(|mode| mode_label(mode).to_owned()),
        activated_node_ids: data.activated_node_ids,
        applied_node_ids: data.applied_node_ids,
        retired_pending_node_ids: data.retired_pending_node_ids,
    }
}

/// typed business error → LLM 可见的结构化 JSON 文本。
pub(super) fn error_text(error: &WorkflowToolError) -> String {
    serde_json::to_string(error).unwrap_or_else(|_| error.to_string())
}
