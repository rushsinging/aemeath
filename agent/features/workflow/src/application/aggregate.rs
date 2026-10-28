//! Workflow 聚合：Graph 结构 + 节点状态 + 生命周期状态机（#2085）。
//!
//! 节点推进（node_update 的后继激活与 AND-join）由 #2086 承接；本文件
//! 冻结生命周期迁移、complete 条件与 allowed_actions 计算。

use std::collections::HashMap;

use crate::domain::tool_data::{AllowedAction, CreateWorkflowSpec, EvidenceRef, SuccessorDecision};
use crate::domain::tool_error::WorkflowToolError;
use crate::domain::{
    ActiveMode, EdgeDefinition, Kernel, NodeDefinition, NodeId, NodeStatus, WorkflowStatus,
};

/// 一次节点推进的结果（#2086）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdvanceOutcome {
    pub node_status: NodeStatus,
    /// 本次提交激活的后继（AND-join 全齐才出现）。
    pub activated_node_ids: Vec<NodeId>,
    /// 仅 fail 时非空：同一提交中转 Stopped 的并行节点。
    pub stopped_node_ids: Vec<NodeId>,
}

/// Workflow 聚合根（内存态；持久化由 #2083 承接）。
#[derive(Debug, Clone)]
pub struct WorkflowAggregate {
    pub workflow_id: String,
    pub name: String,
    pub objective: String,
    pub root_node_id: NodeId,
    pub nodes: Vec<NodeDefinition>,
    pub edges: Vec<EdgeDefinition>,
    node_states: HashMap<NodeId, NodeStatus>,
    node_conclusions: HashMap<NodeId, String>,
    node_evidence: HashMap<NodeId, Vec<EvidenceRef>>,
    node_exits: HashMap<NodeId, Vec<SuccessorDecision>>,
    status: WorkflowStatus,
    active_mode: Option<ActiveMode>,
    pending_mode: Option<ActiveMode>,
    pub updated_at: String,
}

impl WorkflowAggregate {
    /// Kernel 校验 + 初始化：root 为唯一初始 Active Node，其余 Pending。
    pub fn new(
        spec: CreateWorkflowSpec,
        workflow_id: impl Into<String>,
        updated_at: impl Into<String>,
    ) -> Result<Self, WorkflowToolError> {
        let definition = crate::domain::WorkflowDefinition {
            name: spec.name.clone(),
            objective: spec.objective.clone(),
            root_node_id: spec.root_node_id.clone(),
            nodes: spec.nodes.clone(),
            edges: spec.edges.clone(),
        };
        if let Err(error) = Kernel::validate(&definition) {
            return Err(WorkflowToolError::GraphInvariantViolation {
                reason: error.reason,
                subject: error.subject,
            });
        }
        let mut node_states = HashMap::new();
        for node in &spec.nodes {
            node_states.insert(
                node.node_id.clone(),
                if node.node_id == spec.root_node_id {
                    NodeStatus::Active
                } else {
                    NodeStatus::Pending
                },
            );
        }
        Ok(Self {
            workflow_id: workflow_id.into(),
            name: spec.name,
            objective: spec.objective,
            root_node_id: spec.root_node_id,
            nodes: spec.nodes,
            edges: spec.edges,
            node_states,
            node_conclusions: HashMap::new(),
            node_evidence: HashMap::new(),
            node_exits: HashMap::new(),
            status: WorkflowStatus::Active,
            active_mode: Some(ActiveMode::Executing),
            pending_mode: None,
            updated_at: updated_at.into(),
        })
    }

    pub fn status(&self) -> WorkflowStatus {
        self.status
    }

    pub fn active_mode(&self) -> Option<ActiveMode> {
        self.active_mode
    }

    /// Inactive 携带的待恢复模式；其余状态恒为 `None`。
    pub fn pending_mode(&self) -> Option<ActiveMode> {
        self.pending_mode
    }

    pub fn node_status(&self, node_id: &str) -> Option<NodeStatus> {
        self.node_states.get(node_id).copied()
    }

    pub fn node_conclusion(&self, node_id: &str) -> Option<&str> {
        self.node_conclusions.get(node_id).map(String::as_str)
    }

    pub fn node_evidence(&self, node_id: &str) -> Option<Vec<EvidenceRef>> {
        self.node_evidence.get(node_id).cloned()
    }

    /// 指定状态的节点 id（供查询视图与测试断言）。
    pub fn node_ids_with_status(&self, status: NodeStatus) -> impl Iterator<Item = &NodeId> {
        self.node_states
            .iter()
            .filter(move |(_, node_status)| **node_status == status)
            .map(|(node_id, _)| node_id)
    }

    /// 受控入口：进入 Replanning。真实触发路径（node_update fail）由 #2086
    /// 接入；本方法供其复用与测试构造。
    pub fn enter_replanning(&mut self) {
        self.active_mode = Some(ActiveMode::Replanning);
    }

    /// 受控入口：标记节点 Completed（不带结论；供测试构造最小状态）。
    #[cfg(test)]
    pub(crate) fn mark_node_completed(&mut self, node_id: &str) -> Result<(), WorkflowToolError> {
        // 测试构造器：直接设置终态以构造 complete 条件场景（E5/E6），
        // 不走 Pending → Active → Completed 状态机；#2086 提供真实推进。
        match self.node_states.get_mut(node_id) {
            Some(state) => {
                *state = NodeStatus::Completed;
                Ok(())
            }
            None => Err(WorkflowToolError::NodeNotActive {
                node_id: node_id.to_owned(),
            }),
        }
    }

    /// 受控入口：带结论与证据完成节点（#2086 的正式推进复用此语义并扩展
    /// 后继激活与 AND-join）。
    #[cfg(test)]
    pub(crate) fn complete_node_with(
        &mut self,
        node_id: &str,
        conclusion: String,
        evidence: Vec<EvidenceRef>,
    ) -> Result<(), WorkflowToolError> {
        self.mark_node_completed(node_id)?;
        self.node_conclusions.insert(node_id.to_owned(), conclusion);
        self.node_evidence.insert(node_id.to_owned(), evidence);
        Ok(())
    }

    /// 节点失败：目标 Active Node → Failed，其余并行 Active Node 原子转
    /// Stopped，Workflow 立即进入 Active(Replanning)（设计 §3.1）。
    /// 后继不激活、结论记录、完整推进语义由 #2086 扩展。
    pub fn fail_node(
        &mut self,
        node_id: &str,
        conclusion: String,
    ) -> Result<Vec<NodeId>, WorkflowToolError> {
        match self.node_states.get(node_id) {
            Some(NodeStatus::Active) => {}
            Some(_) => {
                return Err(WorkflowToolError::NodeNotActive {
                    node_id: node_id.to_owned(),
                })
            }
            None => {
                return Err(WorkflowToolError::NodeNotActive {
                    node_id: node_id.to_owned(),
                })
            }
        }
        let mut stopped = Vec::new();
        for (candidate, state) in self.node_states.iter_mut() {
            if *state == NodeStatus::Active && candidate.as_str() != node_id {
                *state = NodeStatus::Stopped;
                stopped.push(candidate.clone());
            }
        }
        self.node_states
            .insert(node_id.to_owned(), NodeStatus::Failed);
        self.node_conclusions.insert(node_id.to_owned(), conclusion);
        self.enter_replanning();
        Ok(stopped)
    }

    /// 节点推进：complete / fail + 出口决定 + 隐式 AND-join 激活（#2086）。
    ///
    /// 校验顺序固定（测试依赖）：Replanning 拒 → 节点状态 → successors
    /// 形状（fail 禁带 / complete 必填 / 直接后继 / 无重复 / CompleteNode 禁）
    /// → 状态迁移 → 激活判定。
    pub fn advance_node(
        &mut self,
        node_id: &str,
        action: crate::domain::tool_data::NodeAction,
        conclusion: String,
        evidence: Vec<EvidenceRef>,
        successors: Vec<SuccessorDecision>,
    ) -> Result<AdvanceOutcome, WorkflowToolError> {
        if self.active_mode == Some(ActiveMode::Replanning) {
            return Err(WorkflowToolError::WorkflowReplanning {
                workflow_id: self.workflow_id.clone(),
            });
        }
        match self.node_states.get(node_id) {
            Some(NodeStatus::Active) => {}
            // Stopped / Pending：无推进资格（非冻结历史语义）。
            Some(NodeStatus::Pending | NodeStatus::Stopped) | None => {
                return Err(WorkflowToolError::NodeNotActive {
                    node_id: node_id.to_owned(),
                })
            }
            // Completed / Failed：不可逆冻结历史。
            Some(_) => {
                return Err(WorkflowToolError::NodeHistoryFrozen {
                    node_id: node_id.to_owned(),
                })
            }
        }
        let is_complete_node = self
            .nodes
            .iter()
            .any(|node| node.node_id == node_id && node.kind == crate::domain::NodeKind::Complete);
        let direct_successors: Vec<&str> = self
            .edges
            .iter()
            .filter(|edge| edge.from_node_id == node_id)
            .map(|edge| edge.to_node_id.as_str())
            .collect();

        // successors 形状校验。
        let mut seen: Vec<&str> = Vec::new();
        for decision in &successors {
            if !direct_successors.contains(&decision.node_id.as_str()) {
                return Err(WorkflowToolError::InvalidSuccessor {
                    subject: decision.node_id.clone(),
                });
            }
            if seen.contains(&decision.node_id.as_str()) {
                return Err(WorkflowToolError::InvalidSuccessor {
                    subject: decision.node_id.clone(),
                });
            }
            seen.push(decision.node_id.as_str());
        }
        match action {
            crate::domain::tool_data::NodeAction::Fail => {
                if !successors.is_empty() {
                    return Err(WorkflowToolError::InvalidSuccessor {
                        subject: successors[0].node_id.clone(),
                    });
                }
                let stopped = self.fail_node(node_id, conclusion)?;
                self.node_evidence.insert(node_id.to_owned(), evidence);
                Ok(AdvanceOutcome {
                    node_status: NodeStatus::Failed,
                    activated_node_ids: Vec::new(),
                    stopped_node_ids: stopped,
                })
            }
            crate::domain::tool_data::NodeAction::Complete => {
                if is_complete_node {
                    if !successors.is_empty() {
                        return Err(WorkflowToolError::InvalidSuccessor {
                            subject: successors[0].node_id.clone(),
                        });
                    }
                } else if successors.is_empty() {
                    return Err(WorkflowToolError::InvalidSuccessor {
                        subject: node_id.to_owned(),
                    });
                }
                // 状态迁移 + 记录。
                self.node_states
                    .insert(node_id.to_owned(), NodeStatus::Completed);
                self.node_conclusions.insert(node_id.to_owned(), conclusion);
                self.node_evidence.insert(node_id.to_owned(), evidence);
                self.node_exits
                    .insert(node_id.to_owned(), successors.clone());

                // 激活判定：对每个被选中的直接后继做 AND-join 检查。
                let mut activated = Vec::new();
                if !is_complete_node {
                    for decision in &successors {
                        if self.and_join_satisfied(&decision.node_id) {
                            self.node_states
                                .insert(decision.node_id.clone(), NodeStatus::Active);
                            activated.push(decision.node_id.clone());
                        }
                    }
                }
                Ok(AdvanceOutcome {
                    node_status: NodeStatus::Completed,
                    activated_node_ids: activated,
                    stopped_node_ids: Vec::new(),
                })
            }
        }
    }

    /// 隐式 AND-join：目标的全部静态直接前驱都 Completed 且出口决定均
    /// 选择了该目标。Failed / Stopped 前驱不满足条件（D9：不消耗 join）。
    fn and_join_satisfied(&self, target_node_id: &str) -> bool {
        let predecessors: Vec<&str> = self
            .edges
            .iter()
            .filter(|edge| edge.to_node_id == target_node_id)
            .map(|edge| edge.from_node_id.as_str())
            .collect();
        if predecessors.is_empty() {
            return false;
        }
        predecessors.iter().all(|predecessor| {
            self.node_states.get(*predecessor) == Some(&NodeStatus::Completed)
                && self.node_exits.get(*predecessor).is_some_and(|exits| {
                    exits
                        .iter()
                        .any(|decision| decision.node_id == target_node_id)
                })
        })
    }

    /// 节点已提交的出口决定（查询视图消费）。
    pub fn node_exits(&self, node_id: &str) -> Option<Vec<SuccessorDecision>> {
        self.node_exits.get(node_id).cloned()
    }

    fn ensure_not_terminal(&self) -> Result<(), WorkflowToolError> {
        if matches!(
            self.status,
            WorkflowStatus::Completed | WorkflowStatus::Cancelled | WorkflowStatus::Failed
        ) {
            return Err(WorkflowToolError::WorkflowAlreadyTerminal {
                workflow_id: self.workflow_id.clone(),
            });
        }
        Ok(())
    }

    /// Inactive → Active，按 pending_mode 恢复。
    pub fn activate(&mut self) -> Result<(), WorkflowToolError> {
        self.ensure_not_terminal()?;
        if self.status != WorkflowStatus::Inactive {
            return Err(WorkflowToolError::WorkflowNotActive {
                workflow_id: self.workflow_id.clone(),
            });
        }
        self.status = WorkflowStatus::Active;
        self.active_mode = Some(self.pending_mode.unwrap_or(ActiveMode::Executing));
        Ok(())
    }

    /// Active(Executing|Replanning) → Inactive，保留 pending_mode。
    pub fn deactivate(&mut self) -> Result<(), WorkflowToolError> {
        self.ensure_not_terminal()?;
        if self.status != WorkflowStatus::Active {
            return Err(WorkflowToolError::WorkflowNotActive {
                workflow_id: self.workflow_id.clone(),
            });
        }
        // 设计 §2.1：仅 Replanning 挂起保留待恢复模式；Executing 挂起后
        // activate 默认回 Executing，pending_mode 为 None。
        self.pending_mode =
            (self.active_mode == Some(ActiveMode::Replanning)).then_some(ActiveMode::Replanning);
        self.status = WorkflowStatus::Inactive;
        self.active_mode = None;
        Ok(())
    }

    /// complete：仅 Active(Executing) 且 CompleteNode 已完成、无 Active Node。
    pub fn complete(&mut self, summary: &str) -> Result<(), WorkflowToolError> {
        self.ensure_not_terminal()?;
        if self.status != WorkflowStatus::Active {
            return Err(WorkflowToolError::WorkflowNotActive {
                workflow_id: self.workflow_id.clone(),
            });
        }
        if self.active_mode == Some(ActiveMode::Replanning) {
            return Err(WorkflowToolError::WorkflowReplanning {
                workflow_id: self.workflow_id.clone(),
            });
        }
        if summary.trim().is_empty() {
            return Err(WorkflowToolError::CompletionConditionsUnsatisfied {
                subject: "summary is required for complete".to_owned(),
            });
        }
        let complete_done = self.nodes.iter().any(|node| {
            node.kind == crate::domain::NodeKind::Complete
                && self.node_status(&node.node_id) == Some(NodeStatus::Completed)
        });
        let no_active = self
            .node_ids_with_status(NodeStatus::Active)
            .next()
            .is_none();
        if !complete_done || !no_active {
            return Err(WorkflowToolError::CompletionConditionsUnsatisfied {
                subject: if complete_done {
                    "active nodes remain".to_owned()
                } else {
                    "complete node has no conclusion".to_owned()
                },
            });
        }
        self.status = WorkflowStatus::Completed;
        self.active_mode = None;
        self.pending_mode = None;
        Ok(())
    }

    /// cancel：Active 任意模式或 Inactive 均可。
    pub fn cancel(&mut self, reason: &str) -> Result<(), WorkflowToolError> {
        self.ensure_not_terminal()?;
        if reason.trim().is_empty() {
            return Err(WorkflowToolError::CompletionConditionsUnsatisfied {
                subject: "reason is required for cancel".to_owned(),
            });
        }
        self.status = WorkflowStatus::Cancelled;
        self.active_mode = None;
        self.pending_mode = None;
        Ok(())
    }

    /// fail：仅 Active(Replanning)。
    pub fn fail(&mut self, reason: &str) -> Result<(), WorkflowToolError> {
        self.ensure_not_terminal()?;
        if self.status != WorkflowStatus::Active {
            return Err(WorkflowToolError::WorkflowNotActive {
                workflow_id: self.workflow_id.clone(),
            });
        }
        if self.active_mode != Some(ActiveMode::Replanning) {
            return Err(WorkflowToolError::WorkflowNotReplanning {
                workflow_id: self.workflow_id.clone(),
            });
        }
        if reason.trim().is_empty() {
            return Err(WorkflowToolError::CompletionConditionsUnsatisfied {
                subject: "reason is required for fail".to_owned(),
            });
        }
        self.status = WorkflowStatus::Failed;
        self.active_mode = None;
        self.pending_mode = None;
        Ok(())
    }

    /// Kernel 计算的当前合法操作提示（仅为提示，不构成授权）。
    pub fn allowed_actions(&self) -> Vec<AllowedAction> {
        match (self.status, self.active_mode) {
            (WorkflowStatus::Active, Some(ActiveMode::Executing)) => vec![
                AllowedAction::Deactivate,
                AllowedAction::Complete,
                AllowedAction::Cancel,
                AllowedAction::NodeComplete,
                AllowedAction::NodeFail,
            ],
            (WorkflowStatus::Active, Some(ActiveMode::Replanning)) => vec![
                AllowedAction::Replan,
                AllowedAction::Fail,
                AllowedAction::Cancel,
                AllowedAction::Deactivate,
            ],
            (WorkflowStatus::Inactive, _) => vec![AllowedAction::Activate, AllowedAction::Cancel],
            (WorkflowStatus::Completed | WorkflowStatus::Cancelled | WorkflowStatus::Failed, _) => {
                Vec::new()
            }
            (WorkflowStatus::Active, None) => Vec::new(),
        }
    }
}
