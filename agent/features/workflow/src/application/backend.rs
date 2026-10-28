//! `WorkflowToolBackend` 的内存实现（#2085）。
//!
//! create / list / query / update 完整可用；node_update / replan 为过渡
//! 占位（分别由 #2086 / #2087 实现），生产装配在开关开启前不会注册六工具。

use std::sync::Arc;

use async_trait::async_trait;

use super::aggregate::WorkflowAggregate;
use super::store::InMemoryWorkflowStore;
use crate::domain::tool_data::{
    CreateWorkflowSpec, LifecycleAction, LifecycleResultData, ListWorkflowsData,
    ListWorkflowsQuery, NodeUpdateResultData, NodeViewData, QueryWorkflowQuery, ReplanResultData,
    ReplanWorkflowCommand, UpdateNodeCommand, UpdateWorkflowCommand, WorkflowDetailData,
    WorkflowSummaryData,
};
use crate::domain::tool_error::WorkflowToolError;
use crate::domain::{NodeKind, NodeStatus};
use crate::ports::WorkflowToolBackend;

/// 内存 backend：id 与时间戳由注入的生成器提供（测试可固定序列）。
pub struct InMemoryWorkflowBackend {
    store: Arc<InMemoryWorkflowStore>,
    id_generator: Box<dyn Fn() -> String + Send + Sync>,
    clock: Box<dyn Fn() -> String + Send + Sync>,
}

impl InMemoryWorkflowBackend {
    pub fn new(
        store: Arc<InMemoryWorkflowStore>,
        id_generator: Box<dyn Fn() -> String + Send + Sync>,
        clock: Box<dyn Fn() -> String + Send + Sync>,
    ) -> Self {
        Self {
            store,
            id_generator,
            clock,
        }
    }

    fn summary_data(aggregate: &WorkflowAggregate) -> WorkflowSummaryData {
        WorkflowSummaryData {
            workflow_id: aggregate.workflow_id.clone(),
            name: aggregate.name.clone(),
            status: aggregate.status(),
            active_mode: aggregate.active_mode(),
            pending_mode: aggregate.pending_mode(),
            node_summary: crate::domain::tool_data::NodeSummaryCounts {
                pending: aggregate.node_ids_with_status(NodeStatus::Pending).count(),
                active: aggregate.node_ids_with_status(NodeStatus::Active).count(),
                completed: aggregate
                    .node_ids_with_status(NodeStatus::Completed)
                    .count(),
                failed: aggregate.node_ids_with_status(NodeStatus::Failed).count(),
                stopped: aggregate.node_ids_with_status(NodeStatus::Stopped).count(),
            },
            updated_at: aggregate.updated_at.clone(),
        }
    }

    fn detail_data(aggregate: &WorkflowAggregate, include_facts: bool) -> WorkflowDetailData {
        let _ = include_facts;
        let nodes = aggregate
            .nodes
            .iter()
            .map(|node| NodeViewData {
                node_id: node.node_id.clone(),
                kind: node.kind,
                role: node.role,
                status: aggregate
                    .node_status(&node.node_id)
                    .unwrap_or(NodeStatus::Pending),
                description: node.description.clone(),
                completion_criteria: node.completion_criteria.clone(),
                conclusion: aggregate.node_conclusion(&node.node_id).map(str::to_owned),
                evidence_refs: aggregate.node_evidence(&node.node_id).unwrap_or_default(),
                successors: aggregate.node_exits(&node.node_id).unwrap_or_default(),
            })
            .collect();
        WorkflowDetailData {
            workflow_id: aggregate.workflow_id.clone(),
            name: aggregate.name.clone(),
            objective: aggregate.objective.clone(),
            status: aggregate.status(),
            active_mode: aggregate.active_mode(),
            pending_mode: aggregate.pending_mode(),
            root_node_id: aggregate.root_node_id.clone(),
            complete_node_id: aggregate
                .nodes
                .iter()
                .find(|node| node.kind == NodeKind::Complete)
                .map(|node| node.node_id.clone())
                .unwrap_or_default(),
            nodes,
            edges: aggregate.edges.clone(),
            // facts 摘要由 Runtime 提供并透传（#2084 契约）；内存实现
            // 无执行事实来源，恒为空，include_facts 只影响协议层语义。
            facts: Vec::new(),
            allowed_actions: aggregate.allowed_actions(),
        }
    }
}

#[async_trait]
impl WorkflowToolBackend for InMemoryWorkflowBackend {
    async fn create(
        &self,
        spec: CreateWorkflowSpec,
    ) -> Result<WorkflowSummaryData, WorkflowToolError> {
        if let Some(active) = self.store.active_workflow_id() {
            let _ = active;
            return Err(WorkflowToolError::ActiveWorkflowAlreadyExists);
        }
        let aggregate = WorkflowAggregate::new(spec, (self.id_generator)(), (self.clock)())?;
        let summary = Self::summary_data(&aggregate);
        self.store.insert(aggregate)?;
        Ok(summary)
    }

    async fn list(
        &self,
        query: ListWorkflowsQuery,
    ) -> Result<ListWorkflowsData, WorkflowToolError> {
        let total = self.store.total(query.status);
        let matched = self.store.list(query.status);
        let offset = query.offset.min(total);
        let workflows = matched
            .into_iter()
            .skip(offset)
            .take(query.limit)
            .map(|agg| Self::summary_data(&agg.lock().expect("aggregate lock")))
            .collect();
        Ok(ListWorkflowsData {
            workflows,
            total,
            limit: query.limit,
            offset: query.offset,
        })
    }

    async fn query(
        &self,
        query: QueryWorkflowQuery,
    ) -> Result<WorkflowDetailData, WorkflowToolError> {
        let aggregate = self.store.get(&query.workflow_id)?;
        let aggregate = aggregate.lock().expect("aggregate lock");
        Ok(Self::detail_data(&aggregate, query.include_facts))
    }

    async fn update(
        &self,
        command: UpdateWorkflowCommand,
    ) -> Result<LifecycleResultData, WorkflowToolError> {
        let aggregate = self.store.get(&command.workflow_id)?;
        let mut aggregate = aggregate.lock().expect("aggregate lock");
        match command.action {
            LifecycleAction::Activate => aggregate.activate()?,
            LifecycleAction::Deactivate => aggregate.deactivate()?,
            LifecycleAction::Complete => {
                let summary = command.summary.clone().unwrap_or_default();
                aggregate.complete(&summary)?
            }
            LifecycleAction::Cancel => {
                let reason = command.summary.clone().unwrap_or_default();
                aggregate.cancel(&reason)?
            }
            LifecycleAction::Fail => {
                let reason = command.summary.clone().unwrap_or_default();
                aggregate.fail(&reason)?
            }
        }
        Ok(LifecycleResultData {
            workflow_id: aggregate.workflow_id.clone(),
            status: aggregate.status(),
            active_mode: aggregate.active_mode(),
            pending_mode: aggregate.pending_mode(),
        })
    }

    async fn node_update(
        &self,
        command: UpdateNodeCommand,
    ) -> Result<NodeUpdateResultData, WorkflowToolError> {
        let aggregate = self.store.get(&command.workflow_id)?;
        let mut aggregate = aggregate.lock().expect("aggregate lock");
        // Replanning 拒绝是真实语义（D1）；complete 的推进由 #2086 实现。
        if aggregate.active_mode() == Some(crate::domain::ActiveMode::Replanning) {
            return Err(WorkflowToolError::WorkflowReplanning {
                workflow_id: command.workflow_id.clone(),
            });
        }
        let node_id = command.node_id.clone();
        let outcome = aggregate.advance_node(
            &node_id,
            command.action,
            command.conclusion,
            command.evidence_refs.clone(),
            command.successors.clone(),
        )?;
        let replan_boundary = if outcome.node_status == NodeStatus::Failed {
            Some(crate::domain::tool_data::ReplanBoundary {
                trigger_node_id: node_id.clone(),
                stopped_node_ids: outcome.stopped_node_ids.clone(),
                frozen_fact_refs: command.evidence_refs,
            })
        } else {
            None
        };
        Ok(NodeUpdateResultData {
            workflow_id: aggregate.workflow_id.clone(),
            node_id,
            node_status: outcome.node_status,
            status: aggregate.status(),
            active_mode: aggregate.active_mode(),
            activated_node_ids: outcome.activated_node_ids,
            stopped_node_ids: outcome.stopped_node_ids,
            replan_boundary,
        })
    }

    async fn replan(
        &self,
        command: ReplanWorkflowCommand,
    ) -> Result<ReplanResultData, WorkflowToolError> {
        let aggregate = self.store.get(&command.workflow_id)?;
        let mut aggregate = aggregate.lock().expect("aggregate lock");
        aggregate.replan(command)
    }
}
