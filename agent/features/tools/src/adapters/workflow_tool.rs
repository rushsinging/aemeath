//! 六个 Workflow Tool 的 adapter（#2084）。
//!
//! 职责边界：解析协议输入、映射为 `workflow` 领域命令、转发
//! [`WorkflowToolBackend`]、把领域结果 / 业务错误映射回协议载荷。
//! 工具自身不做业务校验——生命周期状态机、Kernel 不变量与 Replan 校验
//! 都在 backend 实现侧（#2085–#2087）。

mod mapping;

#[cfg(test)]
mod tests;

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;

use crate::domain::types::workflow::{
    WorkflowCreateInput, WorkflowCreateResult, WorkflowListInput, WorkflowListResult,
    WorkflowNodeKindInput, WorkflowNodeUpdateInput, WorkflowNodeUpdateResult, WorkflowQueryInput,
    WorkflowQueryResult, WorkflowReplanInput, WorkflowReplanResult, WorkflowUpdateInput,
    WorkflowUpdateResult,
};
use crate::domain::types::ToolSchema;
use crate::domain::{ToolExecutionContext, TypedTool, TypedToolResult};
use workflow::{
    CreateWorkflowSpec, ListWorkflowsQuery, QueryWorkflowQuery, ReplanWorkflowCommand,
    UpdateNodeCommand, UpdateWorkflowCommand, WorkflowToolBackend, WorkflowToolError,
};

/// 解析失败时的统一报错（协议层错误，不是业务错误）。
fn business_error<T: serde::Serialize + Send + 'static>(
    error: &WorkflowToolError,
) -> TypedToolResult<T> {
    TypedToolResult::error(mapping::error_text(error))
}

/// `workflow_create`
pub struct WorkflowCreateTool {
    pub backend: Arc<dyn WorkflowToolBackend>,
}

#[async_trait]
impl TypedTool for WorkflowCreateTool {
    type Output = WorkflowCreateResult;

    fn name(&self) -> &str {
        "workflow_create"
    }
    fn description(&self) -> &str {
        share::i18n::tools::workflow::workflow_create("en")
    }
    fn description_for(&self, lang: &str) -> std::borrow::Cow<'_, str> {
        std::borrow::Cow::Borrowed(share::i18n::tools::workflow::workflow_create(lang))
    }
    fn input_schema(&self) -> Value {
        WorkflowCreateInput::data_schema()
    }
    fn data_schema(&self) -> Value {
        WorkflowCreateResult::data_schema()
    }
    fn is_concurrency_safe(&self) -> bool {
        false
    }

    async fn call(
        &self,
        input: Value,
        _ctx: &ToolExecutionContext,
    ) -> TypedToolResult<Self::Output> {
        let parsed: WorkflowCreateInput = match serde_json::from_value(input) {
            Ok(value) => value,
            Err(error) => {
                return TypedToolResult::error(format!("workflow_create: invalid input: {error}"));
            }
        };
        let node_count = parsed.nodes.len();
        let complete_node_id = parsed
            .nodes
            .iter()
            .find(|node| node.kind == WorkflowNodeKindInput::Complete)
            .map(|node| mapping::node_id_at(node.index))
            .unwrap_or_default();
        let root_node_id = mapping::node_id_at(parsed.root_index);
        if parsed.root_index >= node_count {
            return TypedToolResult::error("workflow_create: root_index 超出 nodes 下标范围");
        }
        let mut edges = Vec::with_capacity(parsed.edges.len());
        for edge in &parsed.edges {
            match mapping::edge_from_indexes(edge, node_count) {
                Some(edge) => edges.push(edge),
                None => return TypedToolResult::error("workflow_create: edge 下标超出 nodes 范围"),
            }
        }
        let spec = CreateWorkflowSpec {
            name: parsed.name,
            objective: parsed.objective,
            root_node_id: root_node_id.clone(),
            nodes: mapping::nodes_from_input(&parsed.nodes),
            edges,
        };
        match self.backend.create(spec).await {
            Ok(summary) => {
                let workflow_id = summary.workflow_id.clone();
                let text = format!("workflow_create: {workflow_id} created (active/executing)");
                TypedToolResult::success(
                    text,
                    mapping::create_result(summary, root_node_id, complete_node_id, node_count),
                )
            }
            Err(error) => business_error(&error),
        }
    }
}

/// `workflow_list`
pub struct WorkflowListTool {
    pub backend: Arc<dyn WorkflowToolBackend>,
}

#[async_trait]
impl TypedTool for WorkflowListTool {
    type Output = WorkflowListResult;

    fn name(&self) -> &str {
        "workflow_list"
    }
    fn description(&self) -> &str {
        share::i18n::tools::workflow::workflow_list("en")
    }
    fn description_for(&self, lang: &str) -> std::borrow::Cow<'_, str> {
        std::borrow::Cow::Borrowed(share::i18n::tools::workflow::workflow_list(lang))
    }
    fn input_schema(&self) -> Value {
        WorkflowListInput::data_schema()
    }
    fn data_schema(&self) -> Value {
        WorkflowListResult::data_schema()
    }
    fn is_read_only(&self) -> bool {
        true
    }
    fn is_concurrency_safe(&self) -> bool {
        true
    }

    async fn call(
        &self,
        input: Value,
        _ctx: &ToolExecutionContext,
    ) -> TypedToolResult<Self::Output> {
        let parsed: WorkflowListInput = match serde_json::from_value(input) {
            Ok(value) => value,
            Err(error) => {
                return TypedToolResult::error(format!("workflow_list: invalid input: {error}"));
            }
        };
        let limit = parsed.limit.unwrap_or(20).min(100);
        let offset = parsed.offset.unwrap_or(0);
        let status = parsed.status.map(|status| match status {
            crate::domain::types::workflow::WorkflowStatusInput::Active => {
                workflow::WorkflowStatus::Active
            }
            crate::domain::types::workflow::WorkflowStatusInput::Inactive => {
                workflow::WorkflowStatus::Inactive
            }
            crate::domain::types::workflow::WorkflowStatusInput::Completed => {
                workflow::WorkflowStatus::Completed
            }
            crate::domain::types::workflow::WorkflowStatusInput::Cancelled => {
                workflow::WorkflowStatus::Cancelled
            }
            crate::domain::types::workflow::WorkflowStatusInput::Failed => {
                workflow::WorkflowStatus::Failed
            }
        });
        let query = ListWorkflowsQuery {
            status,
            limit,
            offset,
        };
        match self.backend.list(query).await {
            Ok(data) => {
                let text = format!(
                    "workflow_list: {} of {} workflows",
                    data.workflows.len(),
                    data.total
                );
                TypedToolResult::success(text, mapping::list_result(data))
            }
            Err(error) => business_error(&error),
        }
    }
}

/// `workflow_query`
pub struct WorkflowQueryTool {
    pub backend: Arc<dyn WorkflowToolBackend>,
}

#[async_trait]
impl TypedTool for WorkflowQueryTool {
    type Output = WorkflowQueryResult;

    fn name(&self) -> &str {
        "workflow_query"
    }
    fn description(&self) -> &str {
        share::i18n::tools::workflow::workflow_query("en")
    }
    fn description_for(&self, lang: &str) -> std::borrow::Cow<'_, str> {
        std::borrow::Cow::Borrowed(share::i18n::tools::workflow::workflow_query(lang))
    }
    fn input_schema(&self) -> Value {
        WorkflowQueryInput::data_schema()
    }
    fn data_schema(&self) -> Value {
        WorkflowQueryResult::data_schema()
    }
    fn is_read_only(&self) -> bool {
        true
    }
    fn is_concurrency_safe(&self) -> bool {
        true
    }

    async fn call(
        &self,
        input: Value,
        _ctx: &ToolExecutionContext,
    ) -> TypedToolResult<Self::Output> {
        let parsed: WorkflowQueryInput = match serde_json::from_value(input) {
            Ok(value) => value,
            Err(error) => {
                return TypedToolResult::error(format!("workflow_query: invalid input: {error}"));
            }
        };
        let query = QueryWorkflowQuery {
            workflow_id: parsed.workflow_id,
            include_facts: parsed.include_facts.unwrap_or(false),
        };
        match self.backend.query(query).await {
            Ok(data) => {
                let text = format!(
                    "workflow_query: {} ({} nodes)",
                    data.workflow_id,
                    data.nodes.len()
                );
                TypedToolResult::success(text, mapping::query_result(data))
            }
            Err(error) => business_error(&error),
        }
    }
}

/// `workflow_update`
pub struct WorkflowUpdateTool {
    pub backend: Arc<dyn WorkflowToolBackend>,
}

#[async_trait]
impl TypedTool for WorkflowUpdateTool {
    type Output = WorkflowUpdateResult;

    fn name(&self) -> &str {
        "workflow_update"
    }
    fn description(&self) -> &str {
        share::i18n::tools::workflow::workflow_update("en")
    }
    fn description_for(&self, lang: &str) -> std::borrow::Cow<'_, str> {
        std::borrow::Cow::Borrowed(share::i18n::tools::workflow::workflow_update(lang))
    }
    fn input_schema(&self) -> Value {
        WorkflowUpdateInput::data_schema()
    }
    fn data_schema(&self) -> Value {
        WorkflowUpdateResult::data_schema()
    }
    fn is_concurrency_safe(&self) -> bool {
        false
    }

    async fn call(
        &self,
        input: Value,
        _ctx: &ToolExecutionContext,
    ) -> TypedToolResult<Self::Output> {
        let parsed: WorkflowUpdateInput = match serde_json::from_value(input) {
            Ok(value) => value,
            Err(error) => {
                return TypedToolResult::error(format!("workflow_update: invalid input: {error}"));
            }
        };
        let action = match parsed.action {
            crate::domain::types::workflow::WorkflowLifecycleActionInput::Activate => {
                workflow::LifecycleAction::Activate
            }
            crate::domain::types::workflow::WorkflowLifecycleActionInput::Deactivate => {
                workflow::LifecycleAction::Deactivate
            }
            crate::domain::types::workflow::WorkflowLifecycleActionInput::Complete => {
                workflow::LifecycleAction::Complete
            }
            crate::domain::types::workflow::WorkflowLifecycleActionInput::Cancel => {
                workflow::LifecycleAction::Cancel
            }
            crate::domain::types::workflow::WorkflowLifecycleActionInput::Fail => {
                workflow::LifecycleAction::Fail
            }
        };
        let command = UpdateWorkflowCommand {
            workflow_id: parsed.workflow_id,
            action,
            summary: parsed.summary,
            evidence_refs: mapping::evidence_refs_from_input(&parsed.evidence_refs),
        };
        match self.backend.update(command).await {
            Ok(data) => {
                let text = format!(
                    "workflow_update: {} is {}",
                    data.workflow_id,
                    status_text(data.status)
                );
                TypedToolResult::success(text, mapping::update_result(data))
            }
            Err(error) => business_error(&error),
        }
    }
}

fn status_text(status: workflow::WorkflowStatus) -> &'static str {
    mapping::status_label(status)
}

/// `workflow_node_update`
pub struct WorkflowNodeUpdateTool {
    pub backend: Arc<dyn WorkflowToolBackend>,
}

#[async_trait]
impl TypedTool for WorkflowNodeUpdateTool {
    type Output = WorkflowNodeUpdateResult;

    fn name(&self) -> &str {
        "workflow_node_update"
    }
    fn description(&self) -> &str {
        share::i18n::tools::workflow::workflow_node_update("en")
    }
    fn description_for(&self, lang: &str) -> std::borrow::Cow<'_, str> {
        std::borrow::Cow::Borrowed(share::i18n::tools::workflow::workflow_node_update(lang))
    }
    fn input_schema(&self) -> Value {
        WorkflowNodeUpdateInput::data_schema()
    }
    fn data_schema(&self) -> Value {
        WorkflowNodeUpdateResult::data_schema()
    }
    fn is_concurrency_safe(&self) -> bool {
        false
    }

    async fn call(
        &self,
        input: Value,
        _ctx: &ToolExecutionContext,
    ) -> TypedToolResult<Self::Output> {
        let parsed: WorkflowNodeUpdateInput = match serde_json::from_value(input) {
            Ok(value) => value,
            Err(error) => {
                return TypedToolResult::error(format!(
                    "workflow_node_update: invalid input: {error}"
                ));
            }
        };
        let action = match parsed.action {
            crate::domain::types::workflow::WorkflowNodeActionInput::Complete => {
                workflow::NodeAction::Complete
            }
            crate::domain::types::workflow::WorkflowNodeActionInput::Fail => {
                workflow::NodeAction::Fail
            }
        };
        let command = UpdateNodeCommand {
            workflow_id: parsed.workflow_id,
            node_id: parsed.node_id,
            action,
            conclusion: parsed.conclusion,
            evidence_refs: mapping::evidence_refs_from_input(&parsed.evidence_refs),
            successors: mapping::successors_from_input(&parsed.successors),
        };
        match self.backend.node_update(command).await {
            Ok(data) => {
                let text = format!(
                    "workflow_node_update: {} is {}",
                    data.node_id,
                    mapping::node_status_label(data.node_status)
                );
                TypedToolResult::success(text, mapping::node_update_result(data))
            }
            Err(error) => business_error(&error),
        }
    }
}

/// `workflow_replan`
pub struct WorkflowReplanTool {
    pub backend: Arc<dyn WorkflowToolBackend>,
}

#[async_trait]
impl TypedTool for WorkflowReplanTool {
    type Output = WorkflowReplanResult;

    fn name(&self) -> &str {
        "workflow_replan"
    }
    fn description(&self) -> &str {
        share::i18n::tools::workflow::workflow_replan("en")
    }
    fn description_for(&self, lang: &str) -> std::borrow::Cow<'_, str> {
        std::borrow::Cow::Borrowed(share::i18n::tools::workflow::workflow_replan(lang))
    }
    fn input_schema(&self) -> Value {
        WorkflowReplanInput::data_schema()
    }
    fn data_schema(&self) -> Value {
        WorkflowReplanResult::data_schema()
    }
    fn is_concurrency_safe(&self) -> bool {
        false
    }

    async fn call(
        &self,
        input: Value,
        _ctx: &ToolExecutionContext,
    ) -> TypedToolResult<Self::Output> {
        let parsed: WorkflowReplanInput = match serde_json::from_value(input) {
            Ok(value) => value,
            Err(error) => {
                return TypedToolResult::error(format!("workflow_replan: invalid input: {error}"));
            }
        };
        let parsed_new_nodes = parsed.new_nodes.unwrap_or_default();
        let command = ReplanWorkflowCommand {
            workflow_id: parsed.workflow_id,
            boundary: mapping::boundary_from_input(&parsed.boundary),
            new_nodes: mapping::replan_nodes_from_input(&parsed_new_nodes),
            new_edges: mapping::edges_from_indexes(&parsed.new_edges, parsed_new_nodes.len()),
            entry_node_ids: parsed.entry_node_ids,
            rejoin_edges: mapping::rejoin_edges_from_input(&parsed.rejoin_edges),
            pending_rewrites: mapping::pending_rewrites_from_input(&parsed.pending_rewrites),
        };
        match self.backend.replan(command).await {
            Ok(data) => {
                let text = format!(
                    "workflow_replan: {} resumed ({} entry activated)",
                    data.workflow_id,
                    data.activated_node_ids.len()
                );
                TypedToolResult::success(text, mapping::replan_result(data))
            }
            Err(error) => business_error(&error),
        }
    }
}
