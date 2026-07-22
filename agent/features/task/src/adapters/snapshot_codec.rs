use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::domain::{
    Batch, BatchId, BatchStatus, Task, TaskId, TaskPriority, TaskRevision, TaskSnapshot,
    TaskSnapshotFields, TaskStatus,
};

const CURRENT_SCHEMA_VERSION: u64 = 2;

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

impl TaskSnapshot {
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
        let mut tasks: Vec<_> = self.tasks().iter().map(TaskWireV2::from).collect();
        tasks.sort_unstable_by_key(|task| parse_wire_u64(&task.id).unwrap_or(u64::MAX));
        for task in &mut tasks {
            task.tags.sort();
            task.blocked_by
                .sort_unstable_by_key(|id| parse_wire_u64(id).unwrap_or(u64::MAX));
        }
        let mut batches: Vec<_> = self.batches().iter().map(BatchWireV2::from).collect();
        batches.sort_unstable_by_key(|batch| parse_wire_u64(&batch.id).unwrap_or(u64::MAX));
        let wire = SnapshotWireV2 {
            schema_version: CURRENT_SCHEMA_VERSION,
            revision: self.revision().get().to_string(),
            tasks,
            next_task_id: self.next_task_id().get().to_string(),
            next_batch_id: self.next_batch_id().get().to_string(),
            current_batch: self.current_batch().map(|id| id.get().to_string()),
            batches,
        };
        serde_json::to_vec(&wire)
            .map_err(|error| TaskSnapshotCodecError::InvalidJson(error.to_string()))
    }
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
    #[serde(default)]
    seq: u64,
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
            seq: task.seq(),
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
    let mut legacy_seq_by_batch = std::collections::HashMap::<BatchId, u64>::new();
    let mut tasks = tasks;
    tasks.sort_unstable_by_key(Task::id);
    for task in &mut tasks {
        if task.seq() == 0 {
            let seq = legacy_seq_by_batch.entry(task.batch()).or_insert(1);
            task.set_seq_for_restore(*seq);
            *seq = seq.checked_add(1).unwrap_or(u64::MAX);
        }
    }
    let batches = wire
        .batches
        .into_iter()
        .enumerate()
        .map(|(index, batch)| batch_from_v2(batch, index))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(TaskSnapshot::from_decoded_parts(
        TaskRevision::new(revision),
        tasks,
        TaskId::new(next_task_id),
        BatchId::new(next_batch_id),
        current_batch,
        batches,
    ))
}

fn task_from_v2(mut wire: TaskWireV2, index: usize) -> Result<Task, TaskSnapshotCodecError> {
    if wire.status == TaskStatus::Pending && wire.completed_at.is_none() {
        wire.started_at = None;
    }
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
        seq: wire.seq,
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
                seq: id,
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
    Ok(TaskSnapshot::from_decoded_parts(
        TaskRevision::new(0),
        tasks,
        TaskId::new(wire.next_id),
        BatchId::new(next_batch_id),
        (wire.current_batch != 0).then_some(BatchId::new(wire.current_batch)),
        batches,
    ))
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
