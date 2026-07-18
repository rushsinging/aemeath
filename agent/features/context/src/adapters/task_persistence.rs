//! Context ↔ Task BC persistence adapters.
//!
//! These adapters bridge the Task BC's narrow [`TaskPersist`] port to the
//! Session persistence envelope's [`SnapshotState`] vocabulary. They own no task
//! state of their own — every capture / prepare / commit is delegated to the
//! Task port, so the aggregate never leaks across the BC boundary. All methods
//! are synchronous: the Task port's in-memory transaction contains no I/O.

use std::sync::Arc;

use task::{PreparedTaskRestore, TaskPersist, TaskSnapshot, TaskSnapshotValidationError};

use crate::domain::session::{Session, SnapshotState};

// ─── Capture-only facade (#890) ──────────────────────────────────────

/// Capture-only facade over the Task BC persistence boundary.
///
/// Published by Context so the composition root can hand Runtime an
/// [`Arc<dyn LegacyTaskCapture>`] instead of [`TaskPersist`] or
/// [`SessionTaskAdapters`]. Runtime (and any other downstream consumer) can
/// *capture* the live task image into a legacy session but has **no** restore
/// authority: `prepare_restore` / `commit_restore` / `PreparedTaskRestore`
/// never appear on this trait.
///
/// Restore authority remains inside Context; #871 will wire this participant
/// into the cross-BC prepare/commit coordinator. Runtime only receives this
/// capture-only facade.
pub trait LegacyTaskCapture: Send + Sync {
    /// Writes the Task-owned image into the legacy Session facade.
    fn capture_legacy_session(&self, session: &mut Session) -> Result<(), String>;
}

/// Composition factory: wraps the narrow [`TaskPersist`] port into an
/// [`Arc<dyn LegacyTaskCapture>`] whose only capability is legacy-session
/// capture. The returned trait object has no restore methods; #871 will wire
/// Context's restore participant into the joint coordinator.
pub fn compose_session_task_capture(persist: Arc<dyn TaskPersist>) -> Arc<dyn LegacyTaskCapture> {
    Arc::new(SessionTaskAdapters::new(persist))
}

// ─── Concrete adapter structs (Context-internal) ─────────────────────

pub struct SessionTaskAdapters {
    source: TaskSnapshotSource,
}

impl SessionTaskAdapters {
    /// Builds the Context-owned capture adapter from the Task persistence view.
    /// Restore authority is constructed separately by the session restore path.
    pub fn new(persist: Arc<dyn TaskPersist>) -> Self {
        Self {
            source: TaskSnapshotSource::new(persist),
        }
    }

    pub fn capture_legacy_session(&self, session: &mut Session) -> Result<(), String> {
        self.source.capture_legacy_session(session)
    }
}

impl LegacyTaskCapture for SessionTaskAdapters {
    fn capture_legacy_session(&self, session: &mut Session) -> Result<(), String> {
        SessionTaskAdapters::capture_legacy_session(self, session)
    }
}

/// Captures the live Task aggregate as the envelope's task snapshot slot.
///
/// Capture is *always* [`SnapshotState::Captured`], even when the aggregate is
/// empty: persisting an empty task set is a deliberate, observed image ("this
/// session had no tasks") and must stay distinguishable from
/// [`SnapshotState::Missing`] / [`SnapshotState::CapturedEmpty`], which only
/// arise for legacy data that never carried a typed snapshot.
pub struct TaskSnapshotSource {
    persist: Arc<dyn TaskPersist>,
}

impl TaskSnapshotSource {
    pub fn new(persist: Arc<dyn TaskPersist>) -> Self {
        Self { persist }
    }

    /// Collects one coherent image of the current aggregate and wraps it as
    /// `Captured`. Never `Missing` / `CapturedEmpty`.
    pub fn source(&self) -> SnapshotState<TaskSnapshot> {
        SnapshotState::Captured(self.persist.collect_snapshot())
    }

    /// Writes the Task-owned image into the legacy Session facade.
    ///
    /// Runtime can keep using the current cross-workspace/chat writer without
    /// gaining the Task persistence capability or projecting the aggregate by
    /// hand. Context owns this temporary boundary until that writer moves to
    /// [`crate::domain::session::CanonicalSession`].
    pub fn capture_legacy_session(&self, session: &mut Session) -> Result<(), String> {
        let snapshot = match self.source() {
            SnapshotState::Captured(snapshot) => snapshot,
            SnapshotState::Missing | SnapshotState::CapturedEmpty => {
                unreachable!("TaskSnapshotSource always captures a typed snapshot")
            }
        };
        session.tasks = Some(snapshot_to_legacy(snapshot)?);
        Ok(())
    }
}

fn snapshot_to_legacy(snapshot: TaskSnapshot) -> Result<storage::TaskSnapshot, String> {
    // Task owns the canonical wire. The legacy DTO is only a compatibility
    // target, so conversion deliberately passes through that wire rather than
    // duplicating a field-by-field projection in Runtime.
    let bytes = snapshot.encode().map_err(|error| error.to_string())?;
    let value: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
    let object = value
        .as_object()
        .ok_or_else(|| "task snapshot wire is not an object".to_owned())?;
    let mut tasks = object
        .get("tasks")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();
    for task in &mut tasks {
        if let Some(task) = task.as_object_mut() {
            let batch = parse_wire_u64(task.get("batch"), "tasks[].batch")?;
            task.insert("batch".into(), serde_json::json!(batch));
        }
    }
    let mut batches = object
        .get("batches")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();
    for batch in &mut batches {
        if let Some(batch) = batch.as_object_mut() {
            let id = parse_wire_u64(batch.get("id"), "batches[].id")?;
            batch.insert("id".into(), serde_json::json!(id));
        }
    }
    let legacy = serde_json::json!({
        "tasks": tasks,
        "next_id": parse_wire_u64(object.get("next_task_id"), "next_task_id")?,
        "current_batch": object
            .get("current_batch")
            .filter(|value| !value.is_null())
            .map(|value| parse_wire_u64(Some(value), "current_batch"))
            .transpose()?
            .unwrap_or(0),
        "batches": batches,
    });
    serde_json::from_value(legacy).map_err(|error| error.to_string())
}

fn parse_wire_u64(value: Option<&serde_json::Value>, field: &str) -> Result<u64, String> {
    let value = value.ok_or_else(|| format!("task snapshot wire is missing {field}"))?;
    match value {
        serde_json::Value::String(value) => value
            .parse()
            .map_err(|error| format!("invalid {field}: {error}")),
        serde_json::Value::Number(value) => value
            .as_u64()
            .ok_or_else(|| format!("invalid {field}: {value}")),
        _ => Err(format!("invalid {field}: {value}")),
    }
}

/// Restores the Task aggregate from a persisted envelope task slot.
///
/// Restore is split into a fallible [`prepare`](Self::prepare) that validates
/// without touching live state and an infallible [`commit`](Self::commit) that
/// installs the already validated candidate. The two `SnapshotState` variants
/// that carry no typed image — `Missing` and `CapturedEmpty` — both map to the
/// canonical [`TaskSnapshot::empty`], so restoring either one clears any stale
/// live tasks rather than leaving them behind.
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "#871 将其接入跨 BC restore coordinator")
)]
pub(crate) struct TaskRestoreAdapter {
    persist: Arc<dyn TaskPersist>,
}

#[cfg_attr(
    not(test),
    expect(dead_code, reason = "#871 将其接入跨 BC restore coordinator")
)]
impl TaskRestoreAdapter {
    pub(crate) fn new(persist: Arc<dyn TaskPersist>) -> Self {
        Self { persist }
    }

    /// Validates the restore candidate against every aggregate invariant and, on
    /// success, returns a single-use [`PreparedTaskRestore`] token. The live
    /// backing is neither read nor mutated: `Captured` forwards its snapshot
    /// verbatim, while `Missing` / `CapturedEmpty` prepare the canonical empty
    /// snapshot.
    pub(crate) fn prepare(
        &self,
        state: &SnapshotState<TaskSnapshot>,
    ) -> Result<PreparedTaskRestore, TaskSnapshotValidationError> {
        match state {
            SnapshotState::Captured(snapshot) => self.persist.prepare_restore(snapshot),
            SnapshotState::Missing | SnapshotState::CapturedEmpty => {
                self.persist.prepare_restore(&TaskSnapshot::empty())
            }
        }
    }

    /// Installs a previously prepared candidate, replacing the whole aggregate in
    /// one infallible step. Consuming the token by value keeps a prepared
    /// candidate single-use.
    pub(crate) fn commit(&self, prepared: PreparedTaskRestore) {
        self.persist.commit_restore(prepared);
    }
}
