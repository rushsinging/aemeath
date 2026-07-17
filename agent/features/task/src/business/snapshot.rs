use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::{
    Batch, BatchId, BatchStatus, Task, TaskId, TaskPriority, TaskRevision, TaskSnapshotFields,
    TaskStatus, TaskStoreState,
};

const CURRENT_SCHEMA_VERSION: u64 = 2;

/// A Task-owned, typed persistence snapshot. Runtime entities deliberately do
/// not implement serde; conversion is confined to the wire DTOs in this file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskSnapshot {
    revision: TaskRevision,
    tasks: Vec<Task>,
    next_task_id: TaskId,
    next_batch_id: BatchId,
    current_batch: Option<BatchId>,
    batches: Vec<Batch>,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum TaskSnapshotCodecError {
    #[error("invalid task snapshot JSON: {0}")]
    InvalidJson(String),
    #[error("invalid task snapshot schema version representation: {value}")]
    InvalidSchemaVersionRepresentation { value: String },
    #[error("unsupported future task snapshot schema version {version}")]
    UnsupportedFutureVersion { version: u64 },
    #[error("unsupported task snapshot schema version {version}")]
    UnsupportedVersion { version: u64 },
    #[error("invalid ID representation at {field}: {value}")]
    InvalidIdRepresentation { field: String, value: String },
    #[error("legacy next batch ID cannot be derived")]
    NextBatchIdExhausted,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum TaskSnapshotValidationError {
    #[error("zero task ID: {id}")]
    ZeroTaskId { id: TaskId },
    #[error("zero batch ID: {id}")]
    ZeroBatchId { id: BatchId },
    #[error("duplicate task ID: {id}")]
    DuplicateTaskId { id: TaskId },
    #[error("duplicate batch ID: {id}")]
    DuplicateBatchId { id: BatchId },
    #[error("persisted deleted task: {id}")]
    PersistedDeletedTask { id: TaskId },
    #[error("task {task_id} references missing batch {batch_id}")]
    InvalidBatchReference { task_id: TaskId, batch_id: BatchId },
    #[error("task {task_id} references missing dependency {dependency_id}")]
    DanglingDependency {
        task_id: TaskId,
        dependency_id: TaskId,
    },
    #[error("task {task_id} depends on itself")]
    SelfDependency { task_id: TaskId },
    #[error("duplicate dependency {dependency_id} on task {task_id}")]
    DuplicateDependencyReference {
        task_id: TaskId,
        dependency_id: TaskId,
    },
    #[error("dependency graph contains a cycle")]
    DependencyCycle,
    #[error("cross-batch dependency: {task_id} -> {blocked_by_id}")]
    CrossBatchDependency {
        task_id: TaskId,
        blocked_by_id: TaskId,
    },
    #[error("multiple active batches: {first}, {second}")]
    MultipleActiveBatches { first: BatchId, second: BatchId },
    #[error("invalid current batch: {batch_id}")]
    InvalidCurrentBatch { batch_id: BatchId },
    #[error("current batch {current:?} does not match active batch {active}")]
    CurrentBatchMismatch {
        current: Option<BatchId>,
        active: BatchId,
    },
    #[error("next task ID must exceed every persisted task ID")]
    InvalidNextTaskId,
    #[error("next batch ID must exceed every persisted batch ID")]
    InvalidNextBatchId,
    #[error("invalid timestamps for task {task_id}")]
    InvalidTaskTimestamps { task_id: TaskId },
}

/// A validated restore candidate. The aggregate state stays inside the Task BC:
/// only crate-private capture/install plumbing may construct or consume it.
pub(crate) struct PreparedTaskRestore {
    candidate: TaskStoreState,
}

impl std::fmt::Debug for PreparedTaskRestore {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let _candidate = &self.candidate;
        formatter.write_str("PreparedTaskRestore { .. }")
    }
}

impl PreparedTaskRestore {
    pub(crate) fn into_candidate(self) -> TaskStoreState {
        self.candidate
    }

    #[cfg(test)]
    pub(crate) fn candidate(&self) -> &TaskStoreState {
        &self.candidate
    }
}

impl TaskSnapshot {
    pub(crate) fn from_state(state: &TaskStoreState) -> Self {
        Self {
            revision: state.revision(),
            tasks: state
                .tasks()
                .values()
                .filter(|task| task.status() != TaskStatus::Deleted)
                .cloned()
                .map(|mut task| {
                    task.restore_blocks(Vec::new());
                    task
                })
                .collect(),
            next_task_id: state.next_task_id_for_snapshot(),
            next_batch_id: state.next_batch_id_for_snapshot(),
            current_batch: state.current_batch(),
            batches: state.batches().values().cloned().collect(),
        }
    }

    pub fn empty() -> Self {
        Self {
            revision: TaskRevision::new(0),
            tasks: Vec::new(),
            next_task_id: TaskId::new(1),
            next_batch_id: BatchId::new(1),
            current_batch: None,
            batches: Vec::new(),
        }
    }

    pub fn revision(&self) -> TaskRevision {
        self.revision
    }
    pub fn tasks(&self) -> &[Task] {
        &self.tasks
    }
    pub fn next_task_id(&self) -> TaskId {
        self.next_task_id
    }
    pub fn next_batch_id(&self) -> BatchId {
        self.next_batch_id
    }
    pub fn current_batch(&self) -> Option<BatchId> {
        self.current_batch
    }
    pub fn batches(&self) -> &[Batch] {
        &self.batches
    }

    /// Validates all aggregate invariants without exposing an installation
    /// capability outside the crate.
    pub fn validate(self) -> Result<(), TaskSnapshotValidationError> {
        self.prepare().map(drop)
    }

    /// Validates aggregate invariants and, only after every check succeeds,
    /// builds a crate-private candidate store state. The reverse `blocks` index
    /// is derived from persisted `blocked_by` edges rather than persisted
    /// separately.
    pub(crate) fn prepare(self) -> Result<PreparedTaskRestore, TaskSnapshotValidationError> {
        let mut task_indexes = HashMap::with_capacity(self.tasks.len());
        for (index, task) in self.tasks.iter().enumerate() {
            let id = task.id();
            if id.get() == 0 {
                return Err(TaskSnapshotValidationError::ZeroTaskId { id });
            }
            if task_indexes.insert(id, index).is_some() {
                return Err(TaskSnapshotValidationError::DuplicateTaskId { id });
            }
        }

        let mut batch_indexes = HashMap::with_capacity(self.batches.len());
        let mut active_batch = None;
        for (index, batch) in self.batches.iter().enumerate() {
            let id = batch.id();
            if id.get() == 0 {
                return Err(TaskSnapshotValidationError::ZeroBatchId { id });
            }
            if batch_indexes.insert(id, index).is_some() {
                return Err(TaskSnapshotValidationError::DuplicateBatchId { id });
            }
            if batch.status() == BatchStatus::Active {
                if let Some(first) = active_batch {
                    return Err(TaskSnapshotValidationError::MultipleActiveBatches {
                        first,
                        second: id,
                    });
                }
                active_batch = Some(id);
            }
        }

        for task in &self.tasks {
            let id = task.id();
            if task.status() == TaskStatus::Deleted {
                return Err(TaskSnapshotValidationError::PersistedDeletedTask { id });
            }
            if !batch_indexes.contains_key(&task.batch()) {
                return Err(TaskSnapshotValidationError::InvalidBatchReference {
                    task_id: id,
                    batch_id: task.batch(),
                });
            }
            if !valid_task_timestamps(task) {
                return Err(TaskSnapshotValidationError::InvalidTaskTimestamps { task_id: id });
            }

            let mut dependencies = HashSet::with_capacity(task.blocked_by().len());
            for &dependency_id in task.blocked_by() {
                if dependency_id == id {
                    return Err(TaskSnapshotValidationError::SelfDependency { task_id: id });
                }
                if !dependencies.insert(dependency_id) {
                    return Err(TaskSnapshotValidationError::DuplicateDependencyReference {
                        task_id: id,
                        dependency_id,
                    });
                }
                let Some(&dependency_index) = task_indexes.get(&dependency_id) else {
                    return Err(TaskSnapshotValidationError::DanglingDependency {
                        task_id: id,
                        dependency_id,
                    });
                };
                if self.tasks[dependency_index].batch() != task.batch() {
                    return Err(TaskSnapshotValidationError::CrossBatchDependency {
                        task_id: id,
                        blocked_by_id: dependency_id,
                    });
                }
            }
        }

        if dependency_graph_has_cycle(&self.tasks, &task_indexes) {
            return Err(TaskSnapshotValidationError::DependencyCycle);
        }

        if let Some(current) = self.current_batch {
            let Some(&index) = batch_indexes.get(&current) else {
                return Err(TaskSnapshotValidationError::InvalidCurrentBatch { batch_id: current });
            };
            if self.batches[index].status() != BatchStatus::Active {
                return Err(TaskSnapshotValidationError::InvalidCurrentBatch { batch_id: current });
            }
        }
        if let Some(active) = active_batch {
            if self.current_batch != Some(active) {
                return Err(TaskSnapshotValidationError::CurrentBatchMismatch {
                    current: self.current_batch,
                    active,
                });
            }
        }

        if self.next_task_id.get() == 0
            || self.tasks.iter().any(|task| task.id() >= self.next_task_id)
        {
            return Err(TaskSnapshotValidationError::InvalidNextTaskId);
        }
        if self.next_batch_id.get() == 0
            || self
                .batches
                .iter()
                .any(|batch| batch.id() >= self.next_batch_id)
        {
            return Err(TaskSnapshotValidationError::InvalidNextBatchId);
        }

        // All validation is complete. Build the candidate and its derived
        // reverse dependency index without altering persisted timestamps.
        let mut tasks: HashMap<_, _> = self
            .tasks
            .into_iter()
            .map(|task| (task.id(), task))
            .collect();
        let mut reverse: HashMap<TaskId, Vec<TaskId>> = HashMap::new();
        for task in tasks.values() {
            for &dependency in task.blocked_by() {
                reverse.entry(dependency).or_default().push(task.id());
            }
        }
        for (id, blocks) in reverse {
            tasks
                .get_mut(&id)
                .expect("validated dependency must exist")
                .restore_blocks(blocks);
        }
        let batches = self
            .batches
            .into_iter()
            .map(|batch| (batch.id(), batch))
            .collect();
        Ok(PreparedTaskRestore {
            candidate: TaskStoreState::from_snapshot(
                tasks,
                batches,
                self.next_task_id,
                self.next_batch_id,
                self.current_batch,
                self.revision,
            ),
        })
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, TaskSnapshotCodecError> {
        let value: serde_json::Value = serde_json::from_slice(bytes)
            .map_err(|error| TaskSnapshotCodecError::InvalidJson(error.to_string()))?;
        match value.get("schema_version") {
            None => decode_v1(value),
            Some(raw_version) => {
                let version = raw_version.as_u64().ok_or_else(|| {
                    TaskSnapshotCodecError::InvalidSchemaVersionRepresentation {
                        value: raw_version.to_string(),
                    }
                })?;
                match version {
                    version if version > CURRENT_SCHEMA_VERSION => {
                        Err(TaskSnapshotCodecError::UnsupportedFutureVersion { version })
                    }
                    CURRENT_SCHEMA_VERSION => decode_v2(value),
                    version => Err(TaskSnapshotCodecError::UnsupportedVersion { version }),
                }
            }
        }
    }

    /// Always emits the current canonical V2 representation.
    pub fn encode(&self) -> Result<Vec<u8>, TaskSnapshotCodecError> {
        let mut tasks: Vec<_> = self.tasks.iter().map(TaskWireV2::from).collect();
        tasks.sort_unstable_by_key(|task| parse_wire_u64(&task.id).unwrap_or(u64::MAX));
        for task in &mut tasks {
            task.tags.sort();
            task.blocked_by
                .sort_unstable_by_key(|id| parse_wire_u64(id).unwrap_or(u64::MAX));
        }
        let mut batches: Vec<_> = self.batches.iter().map(BatchWireV2::from).collect();
        batches.sort_unstable_by_key(|batch| parse_wire_u64(&batch.id).unwrap_or(u64::MAX));
        let wire = SnapshotWireV2 {
            schema_version: CURRENT_SCHEMA_VERSION,
            revision: self.revision.get().to_string(),
            tasks,
            next_task_id: self.next_task_id.get().to_string(),
            next_batch_id: self.next_batch_id.get().to_string(),
            current_batch: self.current_batch.map(|id| id.get().to_string()),
            batches,
        };
        serde_json::to_vec(&wire)
            .map_err(|error| TaskSnapshotCodecError::InvalidJson(error.to_string()))
    }
}

fn valid_task_timestamps(task: &Task) -> bool {
    let created = task.created_at();
    let updated = task.updated_at();
    if updated < created
        || task
            .started_at()
            .is_some_and(|started| started < created || started > updated)
        || task
            .completed_at()
            .is_some_and(|completed| completed < created || completed > updated)
        || matches!((task.started_at(), task.completed_at()), (Some(started), Some(completed)) if completed < started)
    {
        return false;
    }
    match task.status() {
        TaskStatus::Pending => task.started_at().is_none() && task.completed_at().is_none(),
        TaskStatus::InProgress => task.started_at().is_some() && task.completed_at().is_none(),
        TaskStatus::Completed => task.started_at().is_some() && task.completed_at().is_some(),
        TaskStatus::Deleted => false,
    }
}

fn dependency_graph_has_cycle(tasks: &[Task], indexes: &HashMap<TaskId, usize>) -> bool {
    // Kahn's algorithm avoids making validation depth depend on the native
    // stack. Build adjacency by task-slice index so traversal is deterministic
    // and never depends on HashMap iteration order.
    let mut remaining_dependencies = vec![0usize; tasks.len()];
    let mut dependents = vec![Vec::new(); tasks.len()];
    for (task_index, task) in tasks.iter().enumerate() {
        remaining_dependencies[task_index] = task.blocked_by().len();
        for dependency_id in task.blocked_by() {
            dependents[indexes[dependency_id]].push(task_index);
        }
    }

    let mut ready: Vec<usize> = remaining_dependencies
        .iter()
        .enumerate()
        .filter_map(|(index, &count)| (count == 0).then_some(index))
        .collect();
    let mut visited = 0;
    while let Some(index) = ready.pop() {
        visited += 1;
        for &dependent in &dependents[index] {
            remaining_dependencies[dependent] -= 1;
            if remaining_dependencies[dependent] == 0 {
                ready.push(dependent);
            }
        }
    }

    visited != tasks.len()
}

#[derive(Serialize, Deserialize)]
struct SnapshotWireV2 {
    schema_version: u64,
    revision: String,
    tasks: Vec<TaskWireV2>,
    next_task_id: String,
    next_batch_id: String,
    current_batch: Option<String>,
    batches: Vec<BatchWireV2>,
}

#[derive(Serialize, Deserialize)]
struct TaskWireV2 {
    id: String,
    batch: String,
    subject: String,
    description: String,
    active_form: Option<String>,
    session_id: Option<String>,
    tags: Vec<String>,
    blocked_by: Vec<String>,
    status: TaskStatus,
    priority: TaskPriority,
    created_at: u64,
    updated_at: u64,
    started_at: Option<u64>,
    completed_at: Option<u64>,
}

#[derive(Serialize, Deserialize)]
struct BatchWireV2 {
    id: String,
    summary: Option<String>,
    status: BatchStatus,
    created_at: u64,
    last_active_turn: u64,
    silence_turns: u64,
}

impl From<&Task> for TaskWireV2 {
    fn from(task: &Task) -> Self {
        Self {
            id: task.id().get().to_string(),
            batch: task.batch().get().to_string(),
            subject: task.subject().to_owned(),
            description: task.description().to_owned(),
            active_form: task.active_form().map(str::to_owned),
            session_id: task.session_id().map(str::to_owned),
            tags: task.tags().to_vec(),
            blocked_by: task.blocked_by().iter().map(ToString::to_string).collect(),
            status: task.status(),
            priority: task.priority(),
            created_at: task.created_at(),
            updated_at: task.updated_at(),
            started_at: task.started_at(),
            completed_at: task.completed_at(),
        }
    }
}

impl From<&Batch> for BatchWireV2 {
    fn from(batch: &Batch) -> Self {
        Self {
            id: batch.id().get().to_string(),
            summary: batch.summary().map(str::to_owned),
            status: batch.status(),
            created_at: batch.created_at(),
            last_active_turn: batch.last_active_turn(),
            silence_turns: batch.silence_turns(),
        }
    }
}

fn decode_v2(value: serde_json::Value) -> Result<TaskSnapshot, TaskSnapshotCodecError> {
    validate_v2_ids(&value)?;
    let wire: SnapshotWireV2 = serde_json::from_value(value)
        .map_err(|error| TaskSnapshotCodecError::InvalidJson(error.to_string()))?;
    let revision = parse_id(&wire.revision, "revision", true)?;
    let next_task_id = parse_id(&wire.next_task_id, "next_task_id", false)?;
    let next_batch_id = parse_id(&wire.next_batch_id, "next_batch_id", false)?;
    let current_batch = wire
        .current_batch
        .as_deref()
        .map(|id| parse_id(id, "current_batch", false).map(BatchId::new))
        .transpose()?;
    let tasks = wire
        .tasks
        .into_iter()
        .enumerate()
        .map(|(index, task)| task_from_v2(task, index))
        .collect::<Result<Vec<_>, _>>()?;
    let batches = wire
        .batches
        .into_iter()
        .enumerate()
        .map(|(index, batch)| batch_from_v2(batch, index))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(TaskSnapshot {
        revision: TaskRevision::new(revision),
        tasks,
        next_task_id: TaskId::new(next_task_id),
        next_batch_id: BatchId::new(next_batch_id),
        current_batch,
        batches,
    })
}

fn task_from_v2(wire: TaskWireV2, index: usize) -> Result<Task, TaskSnapshotCodecError> {
    let id = parse_id(&wire.id, &format!("tasks[{index}].id"), false)?;
    let batch = parse_id(&wire.batch, &format!("tasks[{index}].batch"), false)?;
    let blocked_by = wire
        .blocked_by
        .into_iter()
        .enumerate()
        .map(|(dependency, value)| {
            parse_id(
                &value,
                &format!("tasks[{index}].blocked_by[{dependency}]"),
                false,
            )
            .map(TaskId::new)
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Task::from_snapshot(TaskSnapshotFields {
        id: TaskId::new(id),
        batch: BatchId::new(batch),
        subject: wire.subject,
        description: wire.description,
        active_form: wire.active_form,
        session_id: wire.session_id,
        tags: wire.tags,
        blocked_by,
        status: wire.status,
        priority: wire.priority,
        created_at: wire.created_at,
        updated_at: wire.updated_at,
        started_at: wire.started_at,
        completed_at: wire.completed_at,
    }))
}

fn batch_from_v2(wire: BatchWireV2, index: usize) -> Result<Batch, TaskSnapshotCodecError> {
    let id = parse_id(&wire.id, &format!("batches[{index}].id"), false)?;
    Ok(Batch::from_snapshot(
        BatchId::new(id),
        wire.summary,
        wire.status,
        wire.created_at,
        wire.last_active_turn,
        wire.silence_turns,
    ))
}

// Validate the JSON vocabulary before serde conversion so numeric/mixed V2 IDs
// consistently receive the public typed ID error rather than a generic error.
fn validate_v2_ids(value: &serde_json::Value) -> Result<(), TaskSnapshotCodecError> {
    fn required_string<'a>(
        value: &'a serde_json::Value,
        field: &str,
        allow_zero: bool,
    ) -> Result<&'a str, TaskSnapshotCodecError> {
        let raw =
            value
                .as_str()
                .ok_or_else(|| TaskSnapshotCodecError::InvalidIdRepresentation {
                    field: field.to_owned(),
                    value: value.to_string(),
                })?;
        parse_id(raw, field, allow_zero)?;
        Ok(raw)
    }
    for (field, allow_zero) in [
        ("revision", true),
        ("next_task_id", false),
        ("next_batch_id", false),
    ] {
        if let Some(raw) = value.get(field) {
            required_string(raw, field, allow_zero)?;
        }
    }
    if let Some(raw) = value.get("current_batch") {
        if !raw.is_null() {
            required_string(raw, "current_batch", false)?;
        }
    }
    if let Some(tasks) = value.get("tasks").and_then(serde_json::Value::as_array) {
        for (i, task) in tasks.iter().enumerate() {
            for field in ["id", "batch"] {
                if let Some(raw) = task.get(field) {
                    required_string(raw, &format!("tasks[{i}].{field}"), false)?;
                }
            }
            if let Some(ids) = task.get("blocked_by").and_then(serde_json::Value::as_array) {
                for (j, raw) in ids.iter().enumerate() {
                    required_string(raw, &format!("tasks[{i}].blocked_by[{j}]"), false)?;
                }
            }
        }
    }
    if let Some(batches) = value.get("batches").and_then(serde_json::Value::as_array) {
        for (i, batch) in batches.iter().enumerate() {
            if let Some(raw) = batch.get("id") {
                required_string(raw, &format!("batches[{i}].id"), false)?;
            }
        }
    }
    Ok(())
}

#[derive(Deserialize)]
struct SnapshotWireV1 {
    #[serde(default)]
    tasks: Vec<TaskWireV1>,
    next_id: u64,
    #[serde(default)]
    current_batch: u64,
    #[serde(default)]
    batches: Vec<BatchWireV1>,
}

#[derive(Deserialize)]
struct TaskWireV1 {
    id: String,
    #[serde(default)]
    batch: u64,
    subject: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    active_form: Option<String>,
    #[serde(default)]
    session_id: Option<String>,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    blocked_by: Vec<String>,
    status: TaskStatus,
    #[serde(default)]
    priority: TaskPriority,
    #[serde(default)]
    created_at: u64,
    #[serde(default)]
    updated_at: u64,
    #[serde(default)]
    started_at: Option<u64>,
    #[serde(default)]
    completed_at: Option<u64>,
}

#[derive(Deserialize)]
struct BatchWireV1 {
    id: u64,
    #[serde(default)]
    summary: Option<String>,
    status: BatchStatus,
    created_at: u64,
    last_active_turn: u64,
    #[serde(default)]
    silence_turns: u64,
}

fn decode_v1(value: serde_json::Value) -> Result<TaskSnapshot, TaskSnapshotCodecError> {
    let wire: SnapshotWireV1 = serde_json::from_value(value)
        .map_err(|error| TaskSnapshotCodecError::InvalidJson(error.to_string()))?;
    if wire.next_id == 0 {
        return Err(invalid_id("next_id", "0"));
    }
    let max_batch = wire.batches.iter().map(|batch| batch.id).max().unwrap_or(0);
    let next_batch_id = max_batch
        .checked_add(1)
        .ok_or(TaskSnapshotCodecError::NextBatchIdExhausted)?;
    let tasks = wire
        .tasks
        .into_iter()
        .enumerate()
        .map(|(i, task)| {
            let id = parse_id(&task.id, &format!("tasks[{i}].id"), false)?;
            let blocked_by = task
                .blocked_by
                .into_iter()
                .enumerate()
                .map(|(j, raw)| {
                    parse_id(&raw, &format!("tasks[{i}].blocked_by[{j}]"), false).map(TaskId::new)
                })
                .collect::<Result<Vec<_>, _>>()?;
            let (started_at, completed_at) = upgrade_v1_execution_timestamps(
                task.status,
                task.updated_at,
                task.started_at,
                task.completed_at,
            );
            Ok(Task::from_snapshot(TaskSnapshotFields {
                id: TaskId::new(id),
                batch: BatchId::new(task.batch),
                subject: task.subject,
                description: task.description,
                active_form: task.active_form,
                session_id: task.session_id,
                tags: task.tags,
                blocked_by,
                status: task.status,
                priority: task.priority,
                created_at: task.created_at,
                updated_at: task.updated_at,
                started_at,
                completed_at,
            }))
        })
        .collect::<Result<Vec<_>, TaskSnapshotCodecError>>()?;
    let batches = wire
        .batches
        .into_iter()
        .map(|batch| {
            Batch::from_snapshot(
                BatchId::new(batch.id),
                batch.summary,
                batch.status,
                batch.created_at,
                batch.last_active_turn,
                batch.silence_turns,
            )
        })
        .collect();
    Ok(TaskSnapshot {
        revision: TaskRevision::new(0),
        tasks,
        next_task_id: TaskId::new(wire.next_id),
        next_batch_id: BatchId::new(next_batch_id),
        current_batch: (wire.current_batch != 0).then_some(BatchId::new(wire.current_batch)),
        batches,
    })
}

fn upgrade_v1_execution_timestamps(
    status: TaskStatus,
    updated_at: u64,
    started_at: Option<u64>,
    completed_at: Option<u64>,
) -> (Option<u64>, Option<u64>) {
    match status {
        TaskStatus::Pending => (None, None),
        TaskStatus::InProgress => (Some(started_at.unwrap_or(updated_at)), None),
        TaskStatus::Completed => (
            Some(started_at.unwrap_or(updated_at)),
            Some(completed_at.unwrap_or(updated_at)),
        ),
        // Deleted records remain untouched so validation rejects the tombstone
        // rather than manufacturing execution history for it.
        TaskStatus::Deleted => (started_at, completed_at),
    }
}

fn parse_wire_u64(value: &str) -> Option<u64> {
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    value.parse().ok()
}

fn parse_id(value: &str, field: &str, allow_zero: bool) -> Result<u64, TaskSnapshotCodecError> {
    let parsed = parse_wire_u64(value).ok_or_else(|| invalid_id(field, value))?;
    if (!allow_zero && parsed == 0) || (value.len() > 1 && value.starts_with('0')) {
        return Err(invalid_id(field, value));
    }
    Ok(parsed)
}

fn invalid_id(field: &str, value: &str) -> TaskSnapshotCodecError {
    TaskSnapshotCodecError::InvalidIdRepresentation {
        field: field.to_owned(),
        value: value.to_owned(),
    }
}
