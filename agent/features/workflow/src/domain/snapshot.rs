//! Workflow 聚合持久化快照（#2083）：全部状态的 serde 形态。

use serde::{Deserialize, Serialize};

use super::tool_data::{EvidenceRef, SuccessorDecision};
use super::{ActiveMode, EdgeDefinition, NodeDefinition, NodeId, NodeStatus, WorkflowStatus};

/// 单节点的持久化状态条目。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeSnapshotEntry {
    pub node_id: NodeId,
    pub status: NodeStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conclusion: Option<String>,
    #[serde(default)]
    pub evidence_refs: Vec<EvidenceRef>,
    #[serde(default)]
    pub exits: Vec<SuccessorDecision>,
}

/// 聚合全量快照：Graph 定义 + 节点状态 + 生命周期 + 结论 / 证据 / 出口。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowSnapshotData {
    pub workflow_id: String,
    pub name: String,
    pub objective: String,
    pub root_node_id: NodeId,
    pub nodes: Vec<NodeDefinition>,
    pub edges: Vec<EdgeDefinition>,
    pub node_entries: Vec<NodeSnapshotEntry>,
    pub status: WorkflowStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_mode: Option<ActiveMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending_mode: Option<ActiveMode>,
    pub updated_at: String,
}
