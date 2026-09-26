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

numeric_id!(TaskIdData);
numeric_id!(BatchIdData);
numeric_id!(TaskRevisionData);

#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("任务 ID 必须是非零十进制整数：{value}")]
pub struct TaskIdParseError {
    value: String,
}

impl TaskIdData {
    /// Parses a TaskData Tool wire identifier. Aggregate internals may still
    /// construct zero IDs solely to validate malformed persisted snapshots.
    pub fn parse_tool_input(value: &str) -> Result<Self, TaskIdParseError> {
        let id = value.parse::<u64>().map_err(|_| TaskIdParseError {
            value: value.to_owned(),
        })?;
        if id == 0 || (value.len() > 1 && value.starts_with('0')) {
            return Err(TaskIdParseError {
                value: value.to_owned(),
            });
        }
        Ok(Self::new(id))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatusData {
    Pending,
    InProgress,
    Completed,
    Deleted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BatchStatusData {
    Active,
    Paused,
    Archived,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskPriorityData {
    #[default]
    Normal,
    Low,
    High,
    Urgent,
}

impl Ord for TaskPriorityData {
    fn cmp(&self, other: &Self) -> Ordering {
        fn rank(priority: TaskPriorityData) -> u8 {
            match priority {
                TaskPriorityData::Low => 0,
                TaskPriorityData::Normal => 1,
                TaskPriorityData::High => 2,
                TaskPriorityData::Urgent => 3,
            }
        }
        rank(*self).cmp(&rank(*other))
    }
}

impl PartialOrd for TaskPriorityData {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub(crate) enum TaskCommandError {
    #[error("任务标题不能为空")]
    InvalidTaskSubject,
    #[error("批次摘要不能为空")]
    InvalidBatchSummary,
    #[error("任务 ID 已耗尽")]
    TaskIdExhausted,
    #[error("批次 ID 已耗尽")]
    BatchIdExhausted,
    #[error("修订号已耗尽")]
    RevisionExhausted,
    #[error("非法任务状态迁移：{from:?} -> {to:?}")]
    IllegalTransition {
        from: TaskStatusData,
        to: TaskStatusData,
    },
    #[error("删除只能通过聚合删除命令执行")]
    DeletedOnlyViaDelete,
    #[error("批次 {id} 不允许从 {from:?} 迁移到 {to:?}")]
    IllegalBatchTransition {
        id: BatchIdData,
        from: BatchStatusData,
        to: BatchStatusData,
    },
    #[error("任务不存在：{id}")]
    TaskNotFound { id: TaskIdData },
    #[error("批次不存在：{id}")]
    BatchNotFound { id: BatchIdData },
    #[error("当前没有 active 批次")]
    NoActiveBatch,
    #[error("依赖边会形成环：{task_id} -> {blocked_by_id}")]
    DependencyCycle {
        task_id: TaskIdData,
        blocked_by_id: TaskIdData,
    },
    #[error("禁止跨批次依赖：{task_id} -> {blocked_by_id}")]
    CrossBatchDependency {
        task_id: TaskIdData,
        blocked_by_id: TaskIdData,
    },
    #[error("任务 {task_id} 的依赖列表包含重复任务：{blocked_by_id}")]
    DuplicateDependency {
        task_id: TaskIdData,
        blocked_by_id: TaskIdData,
    },
    #[error("任务 {id} 被前置任务阻塞：{blocked_by:?}")]
    TaskBlocked {
        id: TaskIdData,
        blocked_by: Vec<TaskIdData>,
    },
    #[error("批次 {active} 已经 active，不能恢复批次 {requested}")]
    ActiveBatchConflict {
        active: BatchIdData,
        requested: BatchIdData,
    },
    #[error("批次 {id} 当前状态为 {status:?}，只有 active 批次才能记录轮次")]
    BatchNotActive {
        id: BatchIdData,
        status: BatchStatusData,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskEventData {
    TaskCreated {
        task_id: TaskIdData,
    },
    TaskStatusChanged {
        task_id: TaskIdData,
        from: TaskStatusData,
        to: TaskStatusData,
    },
    TaskDependencyAdded {
        task_id: TaskIdData,
        blocked_by_id: TaskIdData,
    },
    TaskDependencyRemoved {
        task_id: TaskIdData,
        blocked_by_id: TaskIdData,
    },
    TaskPriorityChanged {
        task_id: TaskIdData,
        from: TaskPriorityData,
        to: TaskPriorityData,
    },
    TaskSubjectChanged {
        task_id: TaskIdData,
    },
    TaskDescriptionChanged {
        task_id: TaskIdData,
    },
    TaskTagAdded {
        task_id: TaskIdData,
        tag: String,
    },
    TaskTagRemoved {
        task_id: TaskIdData,
        tag: String,
    },
    TaskDeleted {
        task_id: TaskIdData,
    },
    /// The complete TaskData aggregate was reset atomically.
    ///
    /// A non-empty reset emits exactly one event and advances the aggregate
    /// revision exactly once. Resetting an already-empty aggregate is an
    /// idempotent no-op (no event and no revision).
    TaskStoreCleared {
        task_count: usize,
        batch_count: usize,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskCommandResultData<T> {
    pub value: T,
    pub events: Vec<TaskEventData>,
    revision: Option<TaskRevisionData>,
}

impl<T> TaskCommandResultData<T> {
    pub fn revision(&self) -> Option<TaskRevisionData> {
        self.revision
    }

    pub(crate) fn uncommitted(value: T, events: Vec<TaskEventData>) -> Self {
        Self {
            value,
            events,
            revision: None,
        }
    }

    pub(crate) fn commit(&mut self, revision: TaskRevisionData) {
        self.revision = Some(revision);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskCreateSpecData {
    subject: String,
    description: String,
    active_form: Option<String>,
    priority: TaskPriorityData,
}
impl TaskCreateSpecData {
    pub fn try_new(
        subject: String,
        description: String,
        active_form: Option<String>,
        priority: TaskPriorityData,
    ) -> Result<Self, share::error::DomainError> {
        if subject.trim().is_empty() {
            return Err(TaskCommandError::InvalidTaskSubject.into());
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
    pub fn priority(&self) -> TaskPriorityData {
        self.priority
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BatchCreateSpecData {
    summary: String,
}
impl BatchCreateSpecData {
    pub fn try_new(summary: String) -> Result<Self, share::error::DomainError> {
        if summary.trim().is_empty() {
            return Err(TaskCommandError::InvalidBatchSummary.into());
        }
        Ok(Self { summary })
    }
    pub fn summary(&self) -> &str {
        &self.summary
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskData {
    id: TaskIdData,
    batch: BatchIdData,
    seq: u64,
    subject: String,
    description: String,
    active_form: Option<String>,
    session_id: Option<String>,
    tags: Vec<String>,
    blocked_by: Vec<TaskIdData>,
    blocks: Vec<TaskIdData>,
    status: TaskStatusData,
    priority: TaskPriorityData,
    created_at: u64,
    updated_at: u64,
    started_at: Option<u64>,
    completed_at: Option<u64>,
}
pub(crate) struct TaskSnapshotFields {
    pub(crate) id: TaskIdData,
    pub(crate) batch: BatchIdData,
    pub(crate) seq: u64,
    pub(crate) subject: String,
    pub(crate) description: String,
    pub(crate) active_form: Option<String>,
    pub(crate) session_id: Option<String>,
    pub(crate) tags: Vec<String>,
    pub(crate) blocked_by: Vec<TaskIdData>,
    pub(crate) status: TaskStatusData,
    pub(crate) priority: TaskPriorityData,
    pub(crate) created_at: u64,
    pub(crate) updated_at: u64,
    pub(crate) started_at: Option<u64>,
    pub(crate) completed_at: Option<u64>,
}

impl TaskData {
    pub(crate) fn create(
        id: TaskIdData,
        batch: BatchIdData,
        seq: u64,
        spec: TaskCreateSpecData,
        timestamp: u64,
    ) -> TaskCommandResultData<Self> {
        let task = Self {
            id,
            batch,
            seq,
            subject: spec.subject,
            description: spec.description,
            active_form: spec.active_form,
            session_id: None,
            tags: Vec::new(),
            blocked_by: Vec::new(),
            blocks: Vec::new(),
            status: TaskStatusData::Pending,
            priority: spec.priority,
            created_at: timestamp,
            updated_at: timestamp,
            started_at: None,
            completed_at: None,
        };
        TaskCommandResultData::uncommitted(task, vec![TaskEventData::TaskCreated { task_id: id }])
    }
    #[cfg(test)]
    pub(crate) fn with_status(
        id: TaskIdData,
        batch: BatchIdData,
        status: TaskStatusData,
        timestamp: u64,
    ) -> Self {
        Self {
            id,
            batch,
            seq: id.get(),
            subject: "任务".into(),
            description: String::new(),
            active_form: None,
            session_id: None,
            tags: Vec::new(),
            blocked_by: Vec::new(),
            blocks: Vec::new(),
            status,
            priority: TaskPriorityData::Normal,
            created_at: timestamp,
            updated_at: timestamp,
            started_at: (status != TaskStatusData::Pending).then_some(timestamp),
            completed_at: (status == TaskStatusData::Completed).then_some(timestamp),
        }
    }
    pub(crate) fn from_snapshot(fields: TaskSnapshotFields) -> Self {
        Self {
            id: fields.id,
            batch: fields.batch,
            seq: fields.seq,
            subject: fields.subject,
            description: fields.description,
            active_form: fields.active_form,
            session_id: fields.session_id,
            tags: fields.tags,
            blocked_by: fields.blocked_by,
            blocks: Vec::new(),
            status: fields.status,
            priority: fields.priority,
            created_at: fields.created_at,
            updated_at: fields.updated_at,
            started_at: fields.started_at,
            completed_at: fields.completed_at,
        }
    }
    pub fn id(&self) -> TaskIdData {
        self.id
    }
    pub fn batch(&self) -> BatchIdData {
        self.batch
    }
    pub fn seq(&self) -> u64 {
        self.seq
    }
    pub(crate) fn set_seq_for_restore(&mut self, seq: u64) {
        self.seq = seq;
    }
    pub fn subject(&self) -> &str {
        &self.subject
    }
    pub(crate) fn set_subject(
        &mut self,
        subject: String,
        updated_at: u64,
    ) -> Result<TaskCommandResultData<Self>, share::error::DomainError> {
        if subject.trim().is_empty() {
            return Err(TaskCommandError::InvalidTaskSubject.into());
        }
        if self.subject == subject {
            return Ok(TaskCommandResultData::uncommitted(self.clone(), Vec::new()));
        }
        self.subject = subject;
        self.updated_at = updated_at;
        Ok(TaskCommandResultData::uncommitted(
            self.clone(),
            vec![TaskEventData::TaskSubjectChanged { task_id: self.id }],
        ))
    }
    pub fn description(&self) -> &str {
        &self.description
    }
    pub(crate) fn set_description(
        &mut self,
        description: String,
        updated_at: u64,
    ) -> TaskCommandResultData<Self> {
        if self.description == description {
            return TaskCommandResultData::uncommitted(self.clone(), Vec::new());
        }
        self.description = description;
        self.updated_at = updated_at;
        TaskCommandResultData::uncommitted(
            self.clone(),
            vec![TaskEventData::TaskDescriptionChanged { task_id: self.id }],
        )
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
    pub fn blocked_by(&self) -> &[TaskIdData] {
        &self.blocked_by
    }
    pub fn blocks(&self) -> &[TaskIdData] {
        &self.blocks
    }
    /// Restores the derived reverse dependency index without changing the
    /// persisted task timestamps. Snapshot validation calls this only after all
    /// `blocked_by` edges have been accepted.
    pub(crate) fn restore_blocks(&mut self, mut blocks: Vec<TaskIdData>) {
        blocks.sort_unstable();
        self.blocks = blocks;
    }
    pub(crate) fn add_blocked_by(&mut self, id: TaskIdData, updated_at: u64) {
        if !self.blocked_by.contains(&id) {
            self.blocked_by.push(id);
            self.blocked_by.sort_unstable();
            self.updated_at = updated_at;
        }
    }
    pub(crate) fn add_blocks(&mut self, id: TaskIdData, updated_at: u64) {
        if !self.blocks.contains(&id) {
            self.blocks.push(id);
            self.blocks.sort_unstable();
            self.updated_at = updated_at;
        }
    }
    pub(crate) fn remove_blocked_by(&mut self, id: TaskIdData, updated_at: u64) -> bool {
        let old_len = self.blocked_by.len();
        self.blocked_by.retain(|existing| *existing != id);
        if self.blocked_by.len() != old_len {
            self.updated_at = updated_at;
            true
        } else {
            false
        }
    }
    pub(crate) fn remove_blocks(&mut self, id: TaskIdData, updated_at: u64) -> bool {
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
        self.status = TaskStatusData::Deleted;
        self.updated_at = updated_at;
    }
    pub fn status(&self) -> TaskStatusData {
        self.status
    }
    pub fn priority(&self) -> TaskPriorityData {
        self.priority
    }
    pub(crate) fn set_priority(&mut self, priority: TaskPriorityData, updated_at: u64) {
        if self.priority == priority {
            return;
        }
        self.priority = priority;
        self.updated_at = updated_at;
    }
    pub(crate) fn add_tag(&mut self, tag: String, updated_at: u64) {
        if !self.tags.contains(&tag) {
            self.tags.push(tag);
            self.updated_at = updated_at;
        }
    }
    pub(crate) fn remove_tag(&mut self, tag: &str, updated_at: u64) {
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
    pub(crate) fn reopen_from_completed(
        &mut self,
        to: TaskStatusData,
        updated_at: u64,
    ) -> Result<TaskCommandResultData<Self>, share::error::DomainError> {
        let from = self.status;
        if from != TaskStatusData::Completed
            || !matches!(to, TaskStatusData::Pending | TaskStatusData::InProgress)
        {
            return Err(TaskCommandError::IllegalTransition { from, to }.into());
        }
        self.status = to;
        self.updated_at = updated_at;
        self.completed_at = None;
        self.started_at = (to == TaskStatusData::InProgress).then_some(updated_at);
        Ok(TaskCommandResultData::uncommitted(
            self.clone(),
            vec![TaskEventData::TaskStatusChanged {
                task_id: self.id,
                from,
                to,
            }],
        ))
    }

    pub(crate) fn transition_to(
        &mut self,
        to: TaskStatusData,
        updated_at: u64,
    ) -> Result<TaskCommandResultData<Self>, share::error::DomainError> {
        let from = self.status;
        if to == TaskStatusData::Deleted {
            return Err(TaskCommandError::DeletedOnlyViaDelete.into());
        }
        if !matches!(
            (from, to),
            (
                TaskStatusData::Pending,
                TaskStatusData::InProgress | TaskStatusData::Completed
            ) | (
                TaskStatusData::InProgress,
                TaskStatusData::Pending | TaskStatusData::Completed
            )
        ) {
            return Err(TaskCommandError::IllegalTransition { from, to }.into());
        }
        self.status = to;
        self.updated_at = updated_at;
        if to == TaskStatusData::Pending {
            self.started_at = None;
            self.completed_at = None;
        } else if matches!(to, TaskStatusData::InProgress | TaskStatusData::Completed)
            && self.started_at.is_none()
        {
            self.started_at = Some(updated_at);
        }
        if to == TaskStatusData::Completed {
            self.completed_at = Some(updated_at);
        }
        Ok(TaskCommandResultData::uncommitted(
            self.clone(),
            vec![TaskEventData::TaskStatusChanged {
                task_id: self.id,
                from,
                to,
            }],
        ))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskViewData {
    id: TaskIdData,
    subject: String,
    description: String,
    status: TaskStatusData,
    blocked_by: Vec<String>,
    priority: TaskPriorityData,
    created_at: u64,
    updated_at: u64,
    session_id: Option<String>,
    batch: BatchIdData,
}

impl TaskViewData {
    pub fn from_task(task: &TaskData, blocked_by: Vec<String>) -> Self {
        Self {
            id: task.id(),
            subject: task.subject.clone(),
            description: task.description.clone(),
            status: task.status,
            blocked_by,
            priority: task.priority,
            created_at: task.created_at,
            updated_at: task.updated_at,
            session_id: task.session_id.clone(),
            batch: task.batch,
        }
    }
}

impl From<&TaskData> for TaskViewData {
    fn from(task: &TaskData) -> Self {
        Self::from_task(
            task,
            task.blocked_by().iter().map(ToString::to_string).collect(),
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BatchData {
    id: BatchIdData,
    summary: Option<String>,
    status: BatchStatusData,
    created_at: u64,
    last_active_turn: u64,
    silence_turns: u64,
}
impl BatchData {
    pub(crate) fn create(id: BatchIdData, spec: BatchCreateSpecData, created_at: u64) -> Self {
        Self {
            id,
            summary: Some(spec.summary),
            status: BatchStatusData::Active,
            created_at,
            last_active_turn: 0,
            silence_turns: 0,
        }
    }
    #[cfg(test)]
    pub(crate) fn with_status(
        id: BatchIdData,
        status: BatchStatusData,
        silence_turns: u64,
    ) -> Self {
        Self {
            id,
            summary: Some("批次".into()),
            status,
            created_at: 0,
            last_active_turn: 0,
            silence_turns,
        }
    }
    pub(crate) fn from_snapshot(
        id: BatchIdData,
        summary: Option<String>,
        status: BatchStatusData,
        created_at: u64,
        last_active_turn: u64,
        silence_turns: u64,
    ) -> Self {
        Self {
            id,
            summary,
            status,
            created_at,
            last_active_turn,
            silence_turns,
        }
    }
    pub fn id(&self) -> BatchIdData {
        self.id
    }
    pub fn summary(&self) -> Option<&str> {
        self.summary.as_deref()
    }
    pub fn status(&self) -> BatchStatusData {
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
    /// Records a turn outcome for this batch. Only an `Active` batch may
    /// record turns; `Paused`/`Archived` batches reject the call with a typed
    /// error and are left completely unchanged. Returns `Ok(true)` when the
    /// call produced an actual state change, or `Ok(false)` when the request
    /// was already reflected by the current state (idempotent no-op: the
    /// same active turn with `silence_turns` already `0`, or a silent turn
    /// once `silence_turns` has already saturated at `u64::MAX`).
    pub(crate) fn record_turn(
        &mut self,
        turn: u64,
        active: bool,
    ) -> Result<bool, share::error::DomainError> {
        if self.status != BatchStatusData::Active {
            return Err(TaskCommandError::BatchNotActive {
                id: self.id,
                status: self.status,
            }
            .into());
        }
        if active {
            if self.last_active_turn == turn && self.silence_turns == 0 {
                return Ok(false);
            }
            self.last_active_turn = turn;
            self.silence_turns = 0;
        } else {
            if self.silence_turns == u64::MAX {
                return Ok(false);
            }
            self.silence_turns = self.silence_turns.saturating_add(1);
        }
        Ok(true)
    }
    pub(crate) fn reopen(&mut self) -> Result<(), share::error::DomainError> {
        if self.status != BatchStatusData::Archived {
            return Err(TaskCommandError::IllegalBatchTransition {
                id: self.id,
                from: self.status,
                to: BatchStatusData::Active,
            }
            .into());
        }
        self.status = BatchStatusData::Active;
        Ok(())
    }

    pub(crate) fn transition_to(
        &mut self,
        to: BatchStatusData,
    ) -> Result<(), share::error::DomainError> {
        let from = self.status;
        if !matches!(
            (from, to),
            (
                BatchStatusData::Active,
                BatchStatusData::Paused | BatchStatusData::Archived
            ) | (
                BatchStatusData::Paused,
                BatchStatusData::Active | BatchStatusData::Archived
            ) | (BatchStatusData::Archived, BatchStatusData::Archived)
        ) {
            return Err(TaskCommandError::IllegalBatchTransition {
                id: self.id,
                from,
                to,
            }
            .into());
        }
        self.status = to;
        Ok(())
    }
}

#[cfg(test)]
#[path = "model_tests.rs"]
mod tests;

// ─── DomainError 折叠层（跨界唯一错误）────────────────────────────────

impl From<TaskCommandError> for share::error::DomainError {
    fn from(inner: TaskCommandError) -> Self {
        // 命令错误全部为校验/状态非法（无 IO 类别）。
        let message = inner.to_string();
        share::error::DomainError::invalid("task", message).with_source(std::sync::Arc::new(inner))
    }
}
