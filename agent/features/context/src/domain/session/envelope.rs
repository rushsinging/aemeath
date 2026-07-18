use serde::{Deserialize, Serialize};
use serde_json::Value;
use share::message::Message;
use std::path::PathBuf;
use storage::TaskSnapshot as LegacyTaskSnapshot;
use task::TaskSnapshot;

use super::{ChatSegment, PersistedWorkspaceContext, SessionMetadata};

pub const CURRENT_SESSION_SCHEMA_VERSION: u32 = 1;

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

#[derive(Clone, Serialize, Deserialize)]
pub struct CanonicalSession {
    pub id: String,
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
    /// The pre-#890 on-disk task image is the untyped storage DTO. It is upgraded
    /// to the canonical [`TaskSnapshot`] during legacy decode; it is never stored
    /// as the canonical type directly.
    #[serde(default)]
    tasks: Option<LegacyTaskSnapshot>,
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
fn upgrade_legacy_task_snapshot(
    legacy: LegacyTaskSnapshot,
) -> Result<TaskSnapshot, SessionCodecError> {
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
            Some(version) => Err(SessionCodecError::InvalidJson(format!(
                "unsupported historical schema version {version}"
            ))),
            None => Self::decode_legacy(value, upgrade_workspace),
        }
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
                committed_steps: Vec::new(),
            },
            upgraded_from_legacy: true,
        })
    }
}
