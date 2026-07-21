use serde::{Deserialize, Serialize};
use serde_json::Value;
use share::message::Message;
use std::path::PathBuf;
use task::TaskSnapshot;

use super::{ChatSegment, PersistedWorkspaceContext, SessionMetadata};

pub const CURRENT_SESSION_SCHEMA_VERSION: u32 = 2;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", content = "value", rename_all = "snake_case")]
pub enum SnapshotState<T> {
    Missing,
    CapturedEmpty,
    Captured(T),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommittedStep {
    pub run_id: String,
    pub step_id: String,
    pub fingerprint: String,
    pub committed_revision: u64,
}

impl CommittedStep {
    pub fn fixture(run_id: &str, step_id: &str, fingerprint: &str, revision: u64) -> Self {
        Self {
            run_id: run_id.into(),
            step_id: step_id.into(),
            fingerprint: fingerprint.into(),
            committed_revision: revision,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcceptedInputProjection {
    pub messages: Vec<Message>,
    pub fingerprint: String,
    pub committed_revision: u64,
}

impl AcceptedInputProjection {
    pub fn new(
        messages: Vec<Message>,
        fingerprint: impl Into<String>,
        committed_revision: u64,
    ) -> Self {
        Self {
            messages,
            fingerprint: fingerprint.into(),
            committed_revision,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunStepCursor {
    pub run_id: String,
    pub step_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActiveCompactMarker {
    pub summary: String,
    /// First visible complete Step; `None` means compacted history has no visible tail yet.
    pub start_at: Option<RunStepCursor>,
    pub source_revision: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommittedRunStep {
    pub step_id: String,
    #[serde(default)]
    pub accepted_input: Option<AcceptedInputProjection>,
    #[serde(default)]
    pub outcome: Option<Vec<Message>>,
}

impl CommittedRunStep {
    pub fn accepted_only(
        step_id: impl Into<String>,
        accepted_input: AcceptedInputProjection,
    ) -> Self {
        Self {
            step_id: step_id.into(),
            accepted_input: Some(accepted_input),
            outcome: None,
        }
    }

    pub fn outcome_only(step_id: impl Into<String>, outcome: Vec<Message>) -> Self {
        Self {
            step_id: step_id.into(),
            accepted_input: None,
            outcome: Some(outcome),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommittedRunSlice {
    pub run_id: String,
    #[serde(default)]
    pub steps: Vec<CommittedRunStep>,
}

impl CommittedRunSlice {
    pub fn new(run_id: impl Into<String>, steps: Vec<CommittedRunStep>) -> Self {
        Self {
            run_id: run_id.into(),
            steps,
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct CanonicalSession {
    pub id: String,
    /// v1 / legacy read compatibility only. v2 writer never emits this field.
    #[serde(default, skip_serializing)]
    pub chats: Vec<ChatSegment>,
    pub created_at: String,
    pub updated_at: String,
    #[serde(default)]
    pub metadata: SessionMetadata,
    /// Canonical typed task image owned by the Task BC. The Task snapshot uses
    /// its own versioned `encode`/`decode` wire rather than serde on the runtime
    /// entities, so the [`SnapshotState`] slot is bridged through
    /// [`task_snapshot_state`] instead of a plain derive.
    #[serde(with = "task_snapshot_state")]
    pub tasks: SnapshotState<TaskSnapshot>,
    pub workspace: SnapshotState<PersistedWorkspaceContext>,
    pub revision: u64,
    #[serde(default)]
    pub compact: Option<ActiveCompactMarker>,
    #[serde(default)]
    pub run_slices: Vec<CommittedRunSlice>,
    #[serde(default)]
    pub committed_steps: Vec<CommittedStep>,
}

impl std::fmt::Debug for CanonicalSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CanonicalSession")
            .field("id", &self.id)
            .field("chats", &self.chats.len())
            .field("revision", &self.revision)
            .finish()
    }
}

impl PartialEq for CanonicalSession {
    fn eq(&self, other: &Self) -> bool {
        serde_json::to_value(self).ok() == serde_json::to_value(other).ok()
    }
}
impl Eq for CanonicalSession {}

impl CanonicalSession {
    pub fn append_finalized_outcome(
        &mut self,
        run_id: &str,
        step_id: &str,
        messages: Vec<Message>,
    ) {
        let cursor = RunStepCursor {
            run_id: run_id.to_string(),
            step_id: step_id.to_string(),
        };
        if let Some(slice) = self
            .run_slices
            .iter_mut()
            .find(|slice| slice.run_id == run_id)
        {
            if let Some(step) = slice.steps.iter_mut().find(|step| step.step_id == step_id) {
                step.outcome = Some(messages);
            } else {
                slice
                    .steps
                    .push(CommittedRunStep::outcome_only(step_id, messages));
            }
        } else {
            self.run_slices.push(CommittedRunSlice::new(
                run_id,
                vec![CommittedRunStep::outcome_only(step_id, messages)],
            ));
        }
        if self
            .compact
            .as_ref()
            .is_some_and(|marker| marker.start_at.is_none())
        {
            self.compact.as_mut().expect("checked above").start_at = Some(cursor);
        }
    }

    pub fn flattened_steps_from_marker(&self) -> Vec<(RunStepCursor, Vec<Message>)> {
        let start_at = self
            .compact
            .as_ref()
            .and_then(|marker| marker.start_at.as_ref());
        let mut visible = self.compact.is_none();
        let mut steps = Vec::new();
        for slice in &self.run_slices {
            for step in &slice.steps {
                if !visible
                    && start_at.is_some_and(|cursor| {
                        cursor.run_id == slice.run_id && cursor.step_id == step.step_id
                    })
                {
                    visible = true;
                }
                if visible {
                    let messages = step
                        .accepted_input
                        .iter()
                        .flat_map(|input| input.messages.iter())
                        .chain(step.outcome.iter().flat_map(|outcome| outcome.iter()))
                        .cloned()
                        .collect();
                    steps.push((
                        RunStepCursor {
                            run_id: slice.run_id.clone(),
                            step_id: step.step_id.clone(),
                        },
                        messages,
                    ));
                }
            }
        }
        steps
    }

    pub fn structured_messages(&self) -> Vec<Message> {
        self.flattened_steps_from_marker()
            .into_iter()
            .flat_map(|(_, messages)| messages)
            .collect()
    }

    pub fn active_summary(&self) -> Option<&str> {
        self.compact.as_ref().map(|marker| marker.summary.as_str())
    }

    pub fn fixture(id: &str) -> Self {
        Self {
            id: id.into(),
            chats: Vec::new(),
            created_at: "2026-01-01T00:00:00Z".into(),
            updated_at: "2026-01-01T00:00:00Z".into(),
            metadata: SessionMetadata::default(),
            tasks: SnapshotState::Missing,
            workspace: SnapshotState::Missing,
            revision: 0,
            compact: None,
            run_slices: Vec::new(),
            committed_steps: Vec::new(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct VersionedEnvelope {
    schema_version: u32,
    #[serde(flatten)]
    session: CanonicalSession,
}

#[derive(Debug, Deserialize)]
struct V1VersionedEnvelope {
    #[allow(dead_code)]
    schema_version: u32,
    id: String,
    #[serde(default)]
    chats: Vec<ChatSegment>,
    created_at: String,
    updated_at: String,
    #[serde(default)]
    metadata: SessionMetadata,
    #[serde(with = "task_snapshot_state")]
    tasks: SnapshotState<TaskSnapshot>,
    workspace: SnapshotState<PersistedWorkspaceContext>,
    #[serde(default)]
    revision: u64,
    #[serde(default)]
    committed_steps: Vec<CommittedStep>,
}

#[derive(Deserialize)]
struct LegacySession {
    id: String,
    #[serde(default)]
    messages: Vec<Message>,
    #[serde(default)]
    chats: Vec<ChatSegment>,
    created_at: String,
    updated_at: String,
    #[serde(default)]
    metadata: SessionMetadata,
    /// The pre-#890 on-disk task image remains opaque JSON here. It is upgraded
    /// through the Task BC's versioned decoder and is never interpreted by
    /// Context or Storage.
    #[serde(default)]
    tasks: Option<Value>,
    #[serde(default)]
    cwd: Option<String>,
    #[serde(default)]
    workspace: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedSession {
    pub session: CanonicalSession,
    pub upgraded_from_legacy: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum SessionCodecError {
    #[error("Session schema version {version} is newer than supported")]
    UnsupportedFutureVersion {
        version: u32,
        original_bytes: Vec<u8>,
    },
    #[error("Session JSON decode failed: {0}")]
    InvalidJson(String),
    #[error("Legacy Session cwd {cwd} conflicts with workspace identity cwd {identity_cwd}")]
    LegacyCwdIdentityConflict { cwd: String, identity_cwd: String },
    #[error("Legacy workspace path does not exist: {path}")]
    LegacyWorkspacePathNotFound { path: PathBuf },
    #[error("Legacy workspace path is not accessible: {path}")]
    LegacyWorkspacePermissionDenied { path: PathBuf },
    #[error("Legacy workspace path cannot be canonicalized: {path}")]
    LegacyWorkspaceCanonicalizeFailed { path: PathBuf },
    #[error("Git is unavailable while probing a legacy workspace")]
    LegacyWorkspaceGitUnavailable,
    #[error("Git probe failed for legacy workspace path {path} (exit code {exit_code:?})")]
    LegacyWorkspaceGitProbeFailed {
        path: PathBuf,
        exit_code: Option<i32>,
    },
    #[error("Git returned invalid output while probing legacy workspace path {path}")]
    LegacyWorkspaceInvalidGitOutput { path: PathBuf },
    #[error("Legacy workspace path belongs to a different repository: {path}")]
    LegacyWorkspaceRepositoryMismatch { path: PathBuf },
    #[error("Legacy workspace path is not stored in canonical form: {path}")]
    LegacyWorkspacePathNotCanonical { path: PathBuf },
    #[error("Legacy non-git workspace layout is invalid: {path}")]
    LegacyWorkspaceInvalidNonGitLayout { path: PathBuf },
    #[error("Legacy workspace id does not match its derived identity")]
    LegacyWorkspaceIdMismatch,
    #[error("Session JSON encode failed: {0}")]
    Encode(String),
}

/// Upgrades a pre-#890 storage task snapshot to the canonical [`TaskSnapshot`].
///
/// The two representations are *not* assumed identical: the legacy DTO is
/// re-serialized to its wire bytes and decoded through the Task BC's own
/// versioned V1 decode path, which is the single authority for interpreting
/// legacy task wire data. Any incompatibility surfaces as a typed decode error
/// rather than a silent, lossy field-by-field copy.
fn upgrade_legacy_task_snapshot(legacy: Value) -> Result<TaskSnapshot, SessionCodecError> {
    let bytes = serde_json::to_vec(&legacy)
        .map_err(|error| SessionCodecError::InvalidJson(error.to_string()))?;
    TaskSnapshot::decode(&bytes).map_err(|error| SessionCodecError::InvalidJson(error.to_string()))
}

/// serde bridge for `SnapshotState<TaskSnapshot>`.
///
/// [`TaskSnapshot`] intentionally does not implement serde on its runtime
/// entities; its canonical wire form is produced by `encode`/`decode`. This
/// module reuses the derived [`SnapshotState`] tagging by routing the captured
/// payload through a `serde_json::Value` produced by that canonical codec, so
/// the envelope stays a plain typed field while the Task BC keeps sole ownership
/// of its wire format.
mod task_snapshot_state {
    use super::{SnapshotState, TaskSnapshot, Value};
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub(super) fn serialize<S>(
        state: &SnapshotState<TaskSnapshot>,
        serializer: S,
    ) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let wire = match state {
            SnapshotState::Missing => SnapshotState::Missing,
            SnapshotState::CapturedEmpty => SnapshotState::CapturedEmpty,
            SnapshotState::Captured(snapshot) => {
                let bytes = snapshot.encode().map_err(serde::ser::Error::custom)?;
                let value: Value =
                    serde_json::from_slice(&bytes).map_err(serde::ser::Error::custom)?;
                SnapshotState::Captured(value)
            }
        };
        wire.serialize(serializer)
    }

    pub(super) fn deserialize<'de, D>(
        deserializer: D,
    ) -> Result<SnapshotState<TaskSnapshot>, D::Error>
    where
        D: Deserializer<'de>,
    {
        Ok(match SnapshotState::<Value>::deserialize(deserializer)? {
            SnapshotState::Missing => SnapshotState::Missing,
            SnapshotState::CapturedEmpty => SnapshotState::CapturedEmpty,
            SnapshotState::Captured(value) => {
                let bytes = serde_json::to_vec(&value).map_err(serde::de::Error::custom)?;
                let snapshot = TaskSnapshot::decode(&bytes).map_err(serde::de::Error::custom)?;
                SnapshotState::Captured(snapshot)
            }
        })
    }
}

pub struct SessionCodec;

impl SessionCodec {
    pub fn encode(session: &CanonicalSession) -> Result<Vec<u8>, SessionCodecError> {
        let mut canonical = session.clone();
        canonical.revision = canonical
            .committed_steps
            .iter()
            .map(|step| step.committed_revision)
            .max()
            .unwrap_or(canonical.revision)
            .max(canonical.revision);
        serde_json::to_vec_pretty(&VersionedEnvelope {
            schema_version: CURRENT_SESSION_SCHEMA_VERSION,
            session: canonical,
        })
        .map_err(|error| SessionCodecError::Encode(error.to_string()))
    }

    pub(crate) fn decode_with_workspace_upgrade<F>(
        bytes: &[u8],
        upgrade_workspace: F,
    ) -> Result<DecodedSession, SessionCodecError>
    where
        F: FnOnce(
            Option<String>,
            Option<Value>,
        ) -> Result<(Option<PersistedWorkspaceContext>, bool), SessionCodecError>,
    {
        let value: Value = serde_json::from_slice(bytes)
            .map_err(|error| SessionCodecError::InvalidJson(error.to_string()))?;
        match value.get("schema_version").and_then(Value::as_u64) {
            Some(version) if version > u64::from(CURRENT_SESSION_SCHEMA_VERSION) => {
                Err(SessionCodecError::UnsupportedFutureVersion {
                    version: version as u32,
                    original_bytes: bytes.to_vec(),
                })
            }
            Some(version) if version == u64::from(CURRENT_SESSION_SCHEMA_VERSION) => {
                let envelope: VersionedEnvelope = serde_json::from_value(value)
                    .map_err(|error| SessionCodecError::InvalidJson(error.to_string()))?;
                Ok(DecodedSession {
                    session: envelope.session,
                    upgraded_from_legacy: false,
                })
            }
            Some(1) => {
                let legacy: V1VersionedEnvelope = serde_json::from_value(value)
                    .map_err(|error| SessionCodecError::InvalidJson(error.to_string()))?;
                let run_slices = Self::synthetic_run_slices(&legacy.chats);
                let compact = Self::marker_from_chats(&legacy.chats, &run_slices, legacy.revision);
                Ok(DecodedSession {
                    session: CanonicalSession {
                        id: legacy.id,
                        chats: legacy.chats,
                        created_at: legacy.created_at,
                        updated_at: legacy.updated_at,
                        metadata: legacy.metadata,
                        tasks: legacy.tasks,
                        workspace: legacy.workspace,
                        revision: legacy.revision,
                        compact,
                        run_slices,
                        committed_steps: legacy.committed_steps,
                    },
                    upgraded_from_legacy: true,
                })
            }
            Some(version) => Err(SessionCodecError::InvalidJson(format!(
                "unsupported historical schema version {version}"
            ))),
            None => Self::decode_legacy(value, upgrade_workspace),
        }
    }

    fn synthetic_run_slices(chats: &[ChatSegment]) -> Vec<CommittedRunSlice> {
        chats
            .iter()
            .map(|segment| {
                let run_id = format!("legacy:{}", segment.id);
                let step_id = format!("synthetic:{}", segment.id);
                let step = match segment.kind {
                    super::SegmentKind::Normal => CommittedRunStep::accepted_only(
                        step_id,
                        AcceptedInputProjection::new(segment.messages.clone(), run_id.clone(), 0),
                    ),
                    super::SegmentKind::Compact => {
                        CommittedRunStep::outcome_only(step_id, segment.messages.clone())
                    }
                };
                CommittedRunSlice::new(run_id, vec![step])
            })
            .collect()
    }

    fn marker_from_chats(
        chats: &[ChatSegment],
        run_slices: &[CommittedRunSlice],
        source_revision: u64,
    ) -> Option<ActiveCompactMarker> {
        let compact = chats
            .iter()
            .rfind(|segment| segment.kind == super::SegmentKind::Compact)?;
        let start_at = run_slices
            .iter()
            .find(|slice| slice.run_id == format!("legacy:{}", compact.id))
            .and_then(|slice| slice.steps.first())
            .map(|step| RunStepCursor {
                run_id: format!("legacy:{}", compact.id),
                step_id: step.step_id.clone(),
            });
        Some(ActiveCompactMarker {
            summary: compact.summary.clone().unwrap_or_default(),
            start_at,
            source_revision,
        })
    }

    fn decode_legacy<F>(
        value: Value,
        upgrade_workspace: F,
    ) -> Result<DecodedSession, SessionCodecError>
    where
        F: FnOnce(
            Option<String>,
            Option<Value>,
        ) -> Result<(Option<PersistedWorkspaceContext>, bool), SessionCodecError>,
    {
        let mut legacy: LegacySession = serde_json::from_value(value)
            .map_err(|error| SessionCodecError::InvalidJson(error.to_string()))?;
        if legacy.chats.is_empty() && !legacy.messages.is_empty() {
            let mut segment = ChatSegment::normal(None);
            segment.messages = std::mem::take(&mut legacy.messages);
            legacy.chats.push(segment);
        }
        let (workspace, captured_workspace) = upgrade_workspace(legacy.cwd, legacy.workspace)?;
        let tasks = match legacy.tasks {
            Some(legacy_tasks) => {
                SnapshotState::Captured(upgrade_legacy_task_snapshot(legacy_tasks)?)
            }
            None if captured_workspace => SnapshotState::CapturedEmpty,
            None => SnapshotState::Missing,
        };
        let run_slices = Self::synthetic_run_slices(&legacy.chats);
        let compact = Self::marker_from_chats(&legacy.chats, &run_slices, 0);
        Ok(DecodedSession {
            session: CanonicalSession {
                id: legacy.id,
                chats: legacy.chats,
                created_at: legacy.created_at,
                updated_at: legacy.updated_at,
                metadata: legacy.metadata,
                tasks,
                workspace: workspace.map_or(SnapshotState::Missing, SnapshotState::Captured),
                revision: 0,
                compact,
                run_slices,
                committed_steps: Vec::new(),
            },
            upgraded_from_legacy: true,
        })
    }
}
