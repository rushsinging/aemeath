use serde::{Deserialize, Serialize};
use serde_json::Value;
use share::message::Message;
use storage::TaskSnapshot;

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
    #[serde(default)]
    tasks: Option<TaskSnapshot>,
    #[serde(default)]
    workspace: Option<PersistedWorkspaceContext>,
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
    #[error("Session JSON encode failed: {0}")]
    Encode(String),
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

    pub fn decode(bytes: &[u8]) -> Result<DecodedSession, SessionCodecError> {
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
            None => Self::decode_legacy(value),
        }
    }

    fn decode_legacy(value: Value) -> Result<DecodedSession, SessionCodecError> {
        let mut legacy: LegacySession = serde_json::from_value(value)
            .map_err(|error| SessionCodecError::InvalidJson(error.to_string()))?;
        if legacy.chats.is_empty() && !legacy.messages.is_empty() {
            let mut segment = ChatSegment::normal(None);
            segment.messages = std::mem::take(&mut legacy.messages);
            legacy.chats.push(segment);
        }
        Ok(DecodedSession {
            session: CanonicalSession {
                id: legacy.id,
                chats: legacy.chats,
                created_at: legacy.created_at,
                updated_at: legacy.updated_at,
                metadata: legacy.metadata,
                tasks: legacy
                    .tasks
                    .map_or(SnapshotState::Missing, SnapshotState::Captured),
                workspace: legacy
                    .workspace
                    .map_or(SnapshotState::Missing, SnapshotState::Captured),
                revision: 0,
                committed_steps: Vec::new(),
            },
            upgraded_from_legacy: true,
        })
    }
}
