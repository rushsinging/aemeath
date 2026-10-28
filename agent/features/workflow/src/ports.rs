//! Workflow BC 的入站端口（主端口）：六 Tool 的驱动接口。
//!
//! `tools` crate 的六个 adapter 只做协议解析与转发，业务校验（生命周期
//! 状态机、Kernel 不变量、Replan 校验）由实现本端口的 Application 层承担。
//! MVP 的 Application 实现随 #2085–#2087 交付；本文件只冻结契约。

use async_trait::async_trait;

use crate::domain::tool_data::{
    CreateWorkflowSpec, LifecycleResultData, ListWorkflowsData, ListWorkflowsQuery,
    NodeUpdateResultData, QueryWorkflowQuery, ReplanResultData, ReplanWorkflowCommand,
    UpdateNodeCommand, UpdateWorkflowCommand, WorkflowDetailData, WorkflowSummaryData,
};
use crate::domain::tool_error::WorkflowToolError;

/// 六个 Workflow Tool 的共享执行端口。
#[async_trait]
pub trait WorkflowToolBackend: Send + Sync {
    /// 一次提交完整 DAG；成功即 Active，root 成为唯一初始 Active Node。
    async fn create(
        &self,
        spec: CreateWorkflowSpec,
    ) -> Result<WorkflowSummaryData, WorkflowToolError>;

    /// 只读列举当前 Session 可见的 Workflow 摘要。
    async fn list(&self, query: ListWorkflowsQuery)
        -> Result<ListWorkflowsData, WorkflowToolError>;

    /// 只读查询指定 Workflow 详情。
    async fn query(
        &self,
        query: QueryWorkflowQuery,
    ) -> Result<WorkflowDetailData, WorkflowToolError>;

    /// 聚合生命周期：activate / deactivate / complete / cancel / fail。
    async fn update(
        &self,
        command: UpdateWorkflowCommand,
    ) -> Result<LifecycleResultData, WorkflowToolError>;

    /// 单节点：complete / fail。
    async fn node_update(
        &self,
        command: UpdateNodeCommand,
    ) -> Result<NodeUpdateResultData, WorkflowToolError>;

    /// Active(Replanning) 中的 continuation subgraph 提交。
    async fn replan(
        &self,
        command: ReplanWorkflowCommand,
    ) -> Result<ReplanResultData, WorkflowToolError>;
}
