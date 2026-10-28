//! Session 级内存存储：至多一个 Active Workflow（#2085；持久化由 #2083 承接）。

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use super::aggregate::WorkflowAggregate;
use crate::domain::tool_error::WorkflowToolError;
use crate::domain::WorkflowStatus;

/// Session 工作表：id → 聚合；`active_id` 与聚合状态保持一致。
#[derive(Default)]
pub struct InMemoryWorkflowStore {
    workflows: Mutex<HashMap<String, Arc<Mutex<WorkflowAggregate>>>>,
}

impl InMemoryWorkflowStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// 当前 Active Workflow 的 id（至多一个）。
    pub fn active_workflow_id(&self) -> Option<String> {
        let workflows = self.workflows.lock().expect("store lock");
        workflows
            .iter()
            .find(|(_, agg)| agg.lock().expect("aggregate lock").status() == WorkflowStatus::Active)
            .map(|(id, _)| id.clone())
    }

    /// 插入新聚合；若已存在同 id 或已有 Active 则拒绝。
    pub fn insert(&self, aggregate: WorkflowAggregate) -> Result<(), WorkflowToolError> {
        let mut workflows = self.workflows.lock().expect("store lock");
        if workflows.contains_key(&aggregate.workflow_id) {
            return Err(WorkflowToolError::WorkflowNotVisible {
                workflow_id: aggregate.workflow_id.clone(),
            });
        }
        if aggregate.status() == WorkflowStatus::Active {
            if let Some(active) = workflows
                .iter()
                .find(|(_, agg)| {
                    agg.lock().expect("aggregate lock").status() == WorkflowStatus::Active
                })
                .map(|(id, _)| id.clone())
            {
                let _ = active;
                return Err(WorkflowToolError::ActiveWorkflowAlreadyExists);
            }
        }
        let id = aggregate.workflow_id.clone();
        workflows.insert(id, Arc::new(Mutex::new(aggregate)));
        Ok(())
    }

    /// 按引用集合判定可见性：不存在即 `WorkflowNotVisible`。
    pub fn get(
        &self,
        workflow_id: &str,
    ) -> Result<Arc<Mutex<WorkflowAggregate>>, WorkflowToolError> {
        let workflows = self.workflows.lock().expect("store lock");
        workflows
            .get(workflow_id)
            .cloned()
            .ok_or(WorkflowToolError::WorkflowNotVisible {
                workflow_id: workflow_id.to_owned(),
            })
    }

    /// 摘要列表（按 id 排序保证确定性）。
    pub fn list(&self, status: Option<WorkflowStatus>) -> Vec<Arc<Mutex<WorkflowAggregate>>> {
        let workflows = self.workflows.lock().expect("store lock");
        let mut matched: Vec<_> = workflows
            .values()
            .filter(|agg| {
                let agg = agg.lock().expect("aggregate lock");
                status.is_none_or(|wanted| agg.status() == wanted)
            })
            .cloned()
            .collect();
        matched.sort_by(|a, b| {
            let a = a.lock().expect("aggregate lock");
            let b = b.lock().expect("aggregate lock");
            a.workflow_id.cmp(&b.workflow_id)
        });
        matched
    }

    /// 符合过滤的总数。
    pub fn total(&self, status: Option<WorkflowStatus>) -> usize {
        let workflows = self.workflows.lock().expect("store lock");
        workflows
            .values()
            .filter(|agg| {
                let agg = agg.lock().expect("aggregate lock");
                status.is_none_or(|wanted| agg.status() == wanted)
            })
            .count()
    }
}
