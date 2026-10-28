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

/// 持久化错误：IO 故障或损坏数据（fail-closed，不静默重置）。
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PersistenceError {
    #[error("persistence io error: {0}")]
    Io(String),
    #[error("persistence corrupt data at {0}")]
    Corrupt(String),
}

/// Workflow 出站持久化端口（#2083）：Session 索引 + 实体快照。
///
/// 实现方（storage 文件系统 adapter）保证单文件原子写；损坏数据以
/// [`PersistenceError::Corrupt`] 拒绝加载。
pub trait WorkflowPersistence: Send + Sync {
    /// 保存聚合快照（write-through；temp+rename 原子）。
    fn save(
        &self,
        session_id: &str,
        snapshot: &crate::domain::snapshot::WorkflowSnapshotData,
    ) -> Result<(), PersistenceError>;

    /// 加载 Session 索引内的全部聚合快照（resume 用）。
    fn load_all(
        &self,
        session_id: &str,
    ) -> Result<Vec<crate::domain::snapshot::WorkflowSnapshotData>, PersistenceError>;

    /// 清空 Session 索引与 active 指针；实体保留（/clear 语义）。
    fn clear_index(&self, session_id: &str) -> Result<(), PersistenceError>;
}

/// 当前 Active Workflow 的状态快照（#2088 提示注入用；只读、无 IO）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowStateHint {
    pub workflow_id: String,
    pub active_mode: crate::domain::ActiveMode,
}

/// Workflow 状态查询端口：供 reminder source 同步读取当前状态。
///
/// 不暴露 Graph 内容——提示只需 `workflow_id` 与模式，节点详情一律
/// 走 `workflow_query`。
pub trait WorkflowStateProbe: Send + Sync {
    /// 当前 Session 的 Active Workflow 状态；无 Active 返回 `None`。
    fn active_state(&self) -> Option<WorkflowStateHint>;
}
