use std::cmp::Ordering;
use std::fmt;

use serde::{Deserialize, Serialize};
use thiserror::Error;

macro_rules! numeric_id {
    ($name:ident) => {
        #[derive(
            Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
        )]
        #[serde(transparent)]
        pub struct $name(u64);
        impl $name {
            pub const fn new(value: u64) -> Self {
                Self(value)
            }
            pub const fn get(self) -> u64 {
                self.0
            }
        }
        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(f)
            }
        }
    };
}

numeric_id!(TaskId);
numeric_id!(BatchId);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Pending,
    InProgress,
    Completed,
    Deleted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BatchStatus {
    Active,
    Paused,
    Archived,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskPriority {
    #[default]
    Normal,
    Low,
    High,
    Urgent,
}

impl Ord for TaskPriority {
    fn cmp(&self, other: &Self) -> Ordering {
        fn rank(priority: TaskPriority) -> u8 {
            match priority {
                TaskPriority::Low => 0,
                TaskPriority::Normal => 1,
                TaskPriority::High => 2,
                TaskPriority::Urgent => 3,
            }
        }
        rank(*self).cmp(&rank(*other))
    }
}

impl PartialOrd for TaskPriority {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum TaskCommandError {
    #[error("任务标题不能为空")]
    InvalidTaskSubject,
    #[error("批次摘要不能为空")]
    InvalidBatchSummary,
    #[error("非法任务状态迁移：{from:?} -> {to:?}")]
    IllegalTransition { from: TaskStatus, to: TaskStatus },
    #[error("删除只能通过聚合删除命令执行")]
    DeletedOnlyViaDelete,
    #[error("批次 {id} 不允许从 {from:?} 迁移到 {to:?}")]
    IllegalBatchTransition {
        id: BatchId,
        from: BatchStatus,
        to: BatchStatus,
    },
    #[error("任务不存在：{id}")]
    TaskNotFound { id: TaskId },
    #[error("批次不存在：{id}")]
    BatchNotFound { id: BatchId },
    #[error("当前没有 active 批次")]
    NoActiveBatch,
    #[error("依赖边会形成环：{task_id} -> {blocked_by_id}")]
    DependencyCycle {
        task_id: TaskId,
        blocked_by_id: TaskId,
    },
    #[error("禁止跨批次依赖：{task_id} -> {blocked_by_id}")]
    CrossBatchDependency {
        task_id: TaskId,
        blocked_by_id: TaskId,
    },
    #[error("任务 {id} 被前置任务阻塞：{blocked_by:?}")]
    TaskBlocked { id: TaskId, blocked_by: Vec<TaskId> },
    #[error("批次 {active} 已经 active，不能恢复批次 {requested}")]
    ActiveBatchConflict { active: BatchId, requested: BatchId },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskEvent {
    TaskCreated {
        task_id: TaskId,
    },
    TaskStatusChanged {
        task_id: TaskId,
        from: TaskStatus,
        to: TaskStatus,
    },
    TaskDependencyAdded {
        task_id: TaskId,
        blocked_by_id: TaskId,
    },
    TaskDependencyRemoved {
        task_id: TaskId,
        blocked_by_id: TaskId,
    },
    TaskDeleted {
        task_id: TaskId,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskCommandResult<T> {
    pub value: T,
    pub events: Vec<TaskEvent>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskCreateSpec {
    subject: String,
    description: String,
    active_form: Option<String>,
    priority: TaskPriority,
}
impl TaskCreateSpec {
    pub fn try_new(
        subject: String,
        description: String,
        active_form: Option<String>,
        priority: TaskPriority,
    ) -> Result<Self, TaskCommandError> {
        if subject.trim().is_empty() {
            return Err(TaskCommandError::InvalidTaskSubject);
        }
        Ok(Self {
            subject,
            description,
            active_form,
            priority,
        })
    }
    pub fn subject(&self) -> &str {
        &self.subject
    }
    pub fn description(&self) -> &str {
        &self.description
    }
    pub fn active_form(&self) -> Option<&str> {
        self.active_form.as_deref()
    }
    pub fn priority(&self) -> TaskPriority {
        self.priority
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BatchCreateSpec {
    summary: String,
}
impl BatchCreateSpec {
    pub fn try_new(summary: String) -> Result<Self, TaskCommandError> {
        if summary.trim().is_empty() {
            return Err(TaskCommandError::InvalidBatchSummary);
        }
        Ok(Self { summary })
    }
    pub fn summary(&self) -> &str {
        &self.summary
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Task {
    id: TaskId,
    batch: BatchId,
    subject: String,
    description: String,
    active_form: Option<String>,
    session_id: Option<String>,
    tags: Vec<String>,
    blocked_by: Vec<TaskId>,
    blocks: Vec<TaskId>,
    status: TaskStatus,
    priority: TaskPriority,
    created_at: u64,
    updated_at: u64,
    started_at: Option<u64>,
    completed_at: Option<u64>,
}
impl Task {
    pub fn create(
        id: TaskId,
        batch: BatchId,
        spec: TaskCreateSpec,
        timestamp: u64,
    ) -> TaskCommandResult<Self> {
        let task = Self {
            id,
            batch,
            subject: spec.subject,
            description: spec.description,
            active_form: spec.active_form,
            session_id: None,
            tags: Vec::new(),
            blocked_by: Vec::new(),
            blocks: Vec::new(),
            status: TaskStatus::Pending,
            priority: spec.priority,
            created_at: timestamp,
            updated_at: timestamp,
            started_at: None,
            completed_at: None,
        };
        TaskCommandResult {
            value: task,
            events: vec![TaskEvent::TaskCreated { task_id: id }],
        }
    }
    #[cfg(test)]
    pub(crate) fn with_status(
        id: TaskId,
        batch: BatchId,
        status: TaskStatus,
        timestamp: u64,
    ) -> Self {
        Self {
            id,
            batch,
            subject: "任务".into(),
            description: String::new(),
            active_form: None,
            session_id: None,
            tags: Vec::new(),
            blocked_by: Vec::new(),
            blocks: Vec::new(),
            status,
            priority: TaskPriority::Normal,
            created_at: timestamp,
            updated_at: timestamp,
            started_at: (status != TaskStatus::Pending).then_some(timestamp),
            completed_at: (status == TaskStatus::Completed).then_some(timestamp),
        }
    }
    pub fn id(&self) -> TaskId {
        self.id
    }
    pub fn batch(&self) -> BatchId {
        self.batch
    }
    pub fn subject(&self) -> &str {
        &self.subject
    }
    pub fn description(&self) -> &str {
        &self.description
    }
    pub fn active_form(&self) -> Option<&str> {
        self.active_form.as_deref()
    }
    pub fn session_id(&self) -> Option<&str> {
        self.session_id.as_deref()
    }
    pub fn tags(&self) -> &[String] {
        &self.tags
    }
    pub fn blocked_by(&self) -> &[TaskId] {
        &self.blocked_by
    }
    pub fn blocks(&self) -> &[TaskId] {
        &self.blocks
    }
    pub(crate) fn add_blocked_by(&mut self, id: TaskId, updated_at: u64) {
        if !self.blocked_by.contains(&id) {
            self.blocked_by.push(id);
            self.blocked_by.sort_unstable();
            self.updated_at = updated_at;
        }
    }
    pub(crate) fn add_blocks(&mut self, id: TaskId, updated_at: u64) {
        if !self.blocks.contains(&id) {
            self.blocks.push(id);
            self.blocks.sort_unstable();
            self.updated_at = updated_at;
        }
    }
    pub(crate) fn remove_blocked_by(&mut self, id: TaskId, updated_at: u64) -> bool {
        let old_len = self.blocked_by.len();
        self.blocked_by.retain(|existing| *existing != id);
        if self.blocked_by.len() != old_len {
            self.updated_at = updated_at;
            true
        } else {
            false
        }
    }
    pub(crate) fn remove_blocks(&mut self, id: TaskId, updated_at: u64) -> bool {
        let old_len = self.blocks.len();
        self.blocks.retain(|existing| *existing != id);
        if self.blocks.len() != old_len {
            self.updated_at = updated_at;
            true
        } else {
            false
        }
    }
    pub(crate) fn mark_deleted(&mut self, updated_at: u64) {
        self.status = TaskStatus::Deleted;
        self.updated_at = updated_at;
    }
    pub fn status(&self) -> TaskStatus {
        self.status
    }
    pub fn priority(&self) -> TaskPriority {
        self.priority
    }
    pub fn set_priority(&mut self, priority: TaskPriority, updated_at: u64) {
        self.priority = priority;
        self.updated_at = updated_at;
    }
    pub fn add_tag(&mut self, tag: String, updated_at: u64) {
        if !self.tags.contains(&tag) {
            self.tags.push(tag);
            self.updated_at = updated_at;
        }
    }
    pub fn remove_tag(&mut self, tag: &str, updated_at: u64) {
        let old_len = self.tags.len();
        self.tags.retain(|existing| existing != tag);
        if self.tags.len() != old_len {
            self.updated_at = updated_at;
        }
    }
    pub fn created_at(&self) -> u64 {
        self.created_at
    }
    pub fn updated_at(&self) -> u64 {
        self.updated_at
    }
    pub fn started_at(&self) -> Option<u64> {
        self.started_at
    }
    pub fn completed_at(&self) -> Option<u64> {
        self.completed_at
    }
    pub fn transition_to(
        &mut self,
        to: TaskStatus,
        updated_at: u64,
    ) -> Result<TaskCommandResult<Self>, TaskCommandError> {
        let from = self.status;
        if to == TaskStatus::Deleted {
            return Err(TaskCommandError::DeletedOnlyViaDelete);
        }
        if !matches!(
            (from, to),
            (
                TaskStatus::Pending,
                TaskStatus::InProgress | TaskStatus::Completed
            ) | (
                TaskStatus::InProgress,
                TaskStatus::Pending | TaskStatus::Completed
            )
        ) {
            return Err(TaskCommandError::IllegalTransition { from, to });
        }
        self.status = to;
        self.updated_at = updated_at;
        if matches!(to, TaskStatus::InProgress | TaskStatus::Completed) && self.started_at.is_none()
        {
            self.started_at = Some(updated_at);
        }
        if to == TaskStatus::Completed {
            self.completed_at = Some(updated_at);
        }
        Ok(TaskCommandResult {
            value: self.clone(),
            events: vec![TaskEvent::TaskStatusChanged {
                task_id: self.id,
                from,
                to,
            }],
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Batch {
    id: BatchId,
    summary: Option<String>,
    status: BatchStatus,
    created_at: u64,
    last_active_turn: u64,
    silence_turns: u64,
}
impl Batch {
    pub fn create(id: BatchId, spec: BatchCreateSpec, created_at: u64) -> Self {
        Self {
            id,
            summary: Some(spec.summary),
            status: BatchStatus::Active,
            created_at,
            last_active_turn: 0,
            silence_turns: 0,
        }
    }
    #[cfg(test)]
    pub(crate) fn with_status(id: BatchId, status: BatchStatus, silence_turns: u64) -> Self {
        Self {
            id,
            summary: Some("批次".into()),
            status,
            created_at: 0,
            last_active_turn: 0,
            silence_turns,
        }
    }
    pub fn id(&self) -> BatchId {
        self.id
    }
    pub fn summary(&self) -> Option<&str> {
        self.summary.as_deref()
    }
    pub fn status(&self) -> BatchStatus {
        self.status
    }
    pub fn created_at(&self) -> u64 {
        self.created_at
    }
    pub fn last_active_turn(&self) -> u64 {
        self.last_active_turn
    }
    pub fn silence_turns(&self) -> u64 {
        self.silence_turns
    }
    pub fn transition_to(&mut self, to: BatchStatus) -> Result<(), TaskCommandError> {
        let from = self.status;
        if !matches!(
            (from, to),
            (
                BatchStatus::Active,
                BatchStatus::Paused | BatchStatus::Archived
            ) | (
                BatchStatus::Paused,
                BatchStatus::Active | BatchStatus::Archived
            ) | (BatchStatus::Archived, BatchStatus::Archived)
        ) {
            return Err(TaskCommandError::IllegalBatchTransition {
                id: self.id,
                from,
                to,
            });
        }
        self.status = to;
        Ok(())
    }
}

#[cfg(test)]
#[path = "model_tests.rs"]
mod tests;
