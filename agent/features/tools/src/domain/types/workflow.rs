//! 六个 Workflow Tool 的协议类型（设计文档 §5.1–§5.7）。
//!
//! LLM 面向的 schema 载体；执行时由 adapter 映射为 `workflow` crate 的
//! 领域命令 / 查询，再由 `WorkflowToolBackend` 完成业务校验与提交。

use serde::{Deserialize, Serialize};

/// 节点类型三分类。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowNodeKindInput {
    /// 普通业务工作节点；root 也属于此类。
    Work,
    /// 通用确认语义节点。
    Confirm,
    /// 唯一正常拓扑出口；不执行可能失败的业务工作。
    Complete,
}

/// 节点角色六元封闭集合（MVP 仅语义标签，不驱动 Tool 裁剪）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowRoleInput {
    Planner,
    Explorer,
    Architect,
    Implementer,
    Verifier,
    Reviewer,
}

/// 证据引用类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowEvidenceKindInput {
    ToolResult,
    SubAgentOutput,
    BackgroundTask,
    UserMessage,
}

/// Workflow 聚合状态（仅用于列表过滤）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowStatusInput {
    Active,
    Inactive,
    Completed,
    Cancelled,
    Failed,
}

/// 聚合生命周期动作。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowLifecycleActionInput {
    /// Inactive → Active，按 pending_mode 恢复模式。
    Activate,
    /// Active(Executing|Replanning) → Inactive，保留 pending_mode。
    Deactivate,
    /// 仅 Active(Executing) 允许；需交付摘要。
    Complete,
    /// Active 任意模式或 Inactive → Cancelled；需理由。
    Cancel,
    /// 仅 Active(Replanning) 允许；需失败原因。
    Fail,
}

/// 节点动作。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowNodeActionInput {
    Complete,
    Fail,
}

/// 证据引用。
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct WorkflowEvidenceRefInput {
    /// 被引用执行事实的 id（形状合法即可，不要求实体存在）。
    pub fact_id: String,
    pub kind: WorkflowEvidenceKindInput,
}

/// 节点出口决定。
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct WorkflowSuccessorInput {
    /// 必须是当前节点的静态 Edge 直接指向的后继。
    pub node_id: String,
    /// 为何进入该后继、后继要继续什么。
    pub description: String,
}

/// 节点定义。
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct WorkflowNodeInput {
    /// 节点序号（0 基，即 nodes 数组下标）；真正的 node id 由 Workflow 自动生成。
    #[serde(default)]
    pub index: usize,
    pub kind: WorkflowNodeKindInput,
    /// kind = work 时必填；confirm / complete 禁止。
    #[serde(default)]
    pub role: Option<WorkflowRoleInput>,
    /// 节点要做什么 / 确认什么 / 完成定义。
    pub description: String,
    #[serde(default)]
    pub completion_criteria: Option<String>,
}

/// 静态 Edge；不含业务条件。
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct WorkflowEdgeInput {
    /// 起点在 nodes 数组中的下标（LLM 不填 node id，由系统映射）。
    pub from_index: usize,
    /// 终点在 nodes 数组中的下标。
    pub to_index: usize,
}

/// Replan 的历史边界。
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct WorkflowReplanBoundaryInput {
    /// 触发 Replanning 的 Failed Node。
    pub trigger_node_id: String,
    /// 同一提交中被转为 Stopped 的其他并行 Active Node。
    #[serde(default)]
    pub stopped_node_ids: Vec<String>,
    #[serde(default)]
    pub frozen_fact_refs: Vec<WorkflowEvidenceRefInput>,
}

/// continuation 的重入边。
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct WorkflowRejoinEdgeInput {
    /// continuation 出口在 new_nodes 中的下标（新节点 id 由系统生成）。
    pub from_index: usize,
    /// 既有 Pending 非 CompleteNode，不得是本轮新建节点或 CompleteNode。
    pub target_node_id: String,
}

/// 尚未生效结构的字段更新。
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct WorkflowPendingNodeUpdateInput {
    pub node_id: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub role: Option<WorkflowRoleInput>,
    #[serde(default)]
    pub completion_criteria: Option<String>,
}

/// 尚未生效结构的重组声明。
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, Default)]
#[serde(default)]
pub struct WorkflowPendingRewritesInput {
    pub update: Vec<WorkflowPendingNodeUpdateInput>,
    pub remove: Vec<String>,
}

/// `workflow_create` 输入。
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct WorkflowCreateInput {
    /// 本次调用意图（仅用于 TUI 卡片标题语义化展示，不参与校验）。
    #[serde(default)]
    pub goal: Option<String>,
    /// 简短名称。
    pub name: String,
    /// 完成目标。
    pub objective: String,
    /// root 在 nodes 数组中的下标（node id 由系统自动生成，LLM 不填 id）。
    pub root_index: usize,
    /// 完整 DAG 的节点集合。
    pub nodes: Vec<WorkflowNodeInput>,
    /// 完整 DAG 的边集合。
    pub edges: Vec<WorkflowEdgeInput>,
}

/// `workflow_list` 输入。
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, Default)]
#[serde(default)]
pub struct WorkflowListInput {
    /// 本次调用意图（仅用于 TUI 卡片标题语义化展示，不参与校验）。
    #[serde(default)]
    pub goal: Option<String>,
    /// 按状态过滤；省略返回全部。
    pub status: Option<WorkflowStatusInput>,
    /// 分页大小，默认 20，上限 100。
    pub limit: Option<usize>,
    /// 分页起点，默认 0。
    pub offset: Option<usize>,
}

/// `workflow_query` 输入。
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct WorkflowQueryInput {
    /// 本次调用意图（仅用于 TUI 卡片标题语义化展示，不参与校验）。
    #[serde(default)]
    pub goal: Option<String>,
    pub workflow_id: String,
    /// 是否附带执行事实摘要；默认 false。
    #[serde(default)]
    pub include_facts: Option<bool>,
}

/// `workflow_update` 输入。
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct WorkflowUpdateInput {
    /// 本次调用意图（仅用于 TUI 卡片标题语义化展示，不参与校验）。
    #[serde(default)]
    pub goal: Option<String>,
    pub workflow_id: String,
    pub action: WorkflowLifecycleActionInput,
    /// complete 必填交付摘要；fail 必填失败原因；cancel 必填理由；其他动作禁止。
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub evidence_refs: Option<Vec<WorkflowEvidenceRefInput>>,
}

/// `workflow_node_update` 输入。
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct WorkflowNodeUpdateInput {
    /// 本次调用意图（仅用于 TUI 卡片标题语义化展示，不参与校验）。
    #[serde(default)]
    pub goal: Option<String>,
    pub workflow_id: String,
    pub node_id: String,
    pub action: WorkflowNodeActionInput,
    pub conclusion: String,
    #[serde(default)]
    pub evidence_refs: Option<Vec<WorkflowEvidenceRefInput>>,
    /// complete 且非 CompleteNode 时必填、至少一个；fail 与 CompleteNode 完成时禁止。
    #[serde(default)]
    pub successors: Option<Vec<WorkflowSuccessorInput>>,
}

/// `workflow_replan` 输入。
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct WorkflowReplanInput {
    /// 本次调用意图（仅用于 TUI 卡片标题语义化展示，不参与校验）。
    #[serde(default)]
    pub goal: Option<String>,
    pub workflow_id: String,
    pub boundary: WorkflowReplanBoundaryInput,
    /// continuation 新增节点；仅重组 Pending 时可为空。
    #[serde(default)]
    pub new_nodes: Option<Vec<WorkflowNodeInput>>,
    #[serde(default)]
    pub new_edges: Option<Vec<WorkflowEdgeInput>>,
    /// continuation 入口在 new_nodes 中的下标，至少一个（id 由系统生成）。
    pub entry_indexes: Vec<usize>,
    /// 汇入原 Graph 的重入边，至少一条。
    pub rejoin_edges: Vec<WorkflowRejoinEdgeInput>,
    #[serde(default)]
    pub pending_rewrites: Option<WorkflowPendingRewritesInput>,
}

/// 节点计数摘要。
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, Default)]
#[serde(default)]
pub struct WorkflowNodeSummaryResult {
    pub pending: usize,
    pub active: usize,
    pub completed: usize,
    pub failed: usize,
    pub stopped: usize,
}

/// Workflow 列表摘要项。
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, Default)]
#[serde(default)]
pub struct WorkflowSummaryResult {
    pub workflow_id: String,
    pub name: String,
    pub status: String,
    pub active_mode: Option<String>,
    pub pending_mode: Option<String>,
    pub node_summary: WorkflowNodeSummaryResult,
    pub updated_at: String,
}

/// `workflow_create` 结果。
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, Default)]
#[serde(default)]
pub struct WorkflowCreateResult {
    pub workflow_id: String,
    /// 节点 index → 系统生成的真实 node id（LLM 后续引用的唯一来源）。
    #[serde(default)]
    pub node_ids: Vec<String>,
    pub status: String,
    pub active_mode: Option<String>,
    pub root_node_id: String,
    pub complete_node_id: String,
    pub node_count: usize,
}

/// `workflow_list` 结果。
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, Default)]
#[serde(default)]
pub struct WorkflowListResult {
    pub workflows: Vec<WorkflowSummaryResult>,
    /// 满足过滤条件的总数，不受分页影响。
    pub total: usize,
    pub limit: usize,
    pub offset: usize,
}

/// 节点视图。
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, Default)]
#[serde(default)]
pub struct WorkflowNodeViewResult {
    pub node_id: String,
    pub kind: String,
    pub role: Option<String>,
    pub status: String,
    pub description: String,
    pub completion_criteria: Option<String>,
    pub conclusion: Option<String>,
    pub evidence_refs: Vec<WorkflowEvidenceRefInput>,
    pub successors: Vec<WorkflowSuccessorInput>,
}

/// 边视图。
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, Default)]
#[serde(default)]
pub struct WorkflowEdgeResult {
    pub from_node_id: String,
    pub to_node_id: String,
}

/// 执行事实摘要。
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, Default)]
#[serde(default)]
pub struct WorkflowFactSummaryResult {
    pub fact_id: String,
    pub kind: String,
    pub source: String,
    pub related_node_ids: Vec<String>,
    pub at: String,
}

/// `workflow_query` 结果。
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, Default)]
#[serde(default)]
pub struct WorkflowQueryResult {
    pub workflow_id: String,
    pub name: String,
    pub objective: String,
    pub status: String,
    pub active_mode: Option<String>,
    pub pending_mode: Option<String>,
    pub root_node_id: String,
    pub complete_node_id: String,
    pub nodes: Vec<WorkflowNodeViewResult>,
    pub edges: Vec<WorkflowEdgeResult>,
    pub facts: Vec<WorkflowFactSummaryResult>,
    /// 当前合法操作提示；仅为建议，不构成授权。
    pub allowed_actions: Vec<String>,
}

/// `workflow_update` 结果。
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, Default)]
#[serde(default)]
pub struct WorkflowUpdateResult {
    pub workflow_id: String,
    pub status: String,
    pub active_mode: Option<String>,
    pub pending_mode: Option<String>,
}

/// `workflow_node_update` 结果。
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, Default)]
#[serde(default)]
pub struct WorkflowNodeUpdateResult {
    pub workflow_id: String,
    pub node_id: String,
    pub node_status: String,
    pub status: String,
    pub active_mode: Option<String>,
    pub activated_node_ids: Vec<String>,
    pub stopped_node_ids: Vec<String>,
    pub replan_boundary: Option<WorkflowReplanBoundaryInput>,
}

/// `workflow_replan` 结果。
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, Default)]
#[serde(default)]
pub struct WorkflowReplanResult {
    pub workflow_id: String,
    pub status: String,
    pub active_mode: Option<String>,
    pub activated_node_ids: Vec<String>,
    pub applied_node_ids: Vec<String>,
    pub retired_pending_node_ids: Vec<String>,
}
