use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{
    envelope::task_snapshot_state, ActiveCompactMarker, CanonicalSession, ChatSegment,
    CommittedRunStep, CommittedStepLedger, PersistedWorkspaceContext, RunStepCursor,
    SessionHistory, SessionMetadata, SkillLoadRecord, SnapshotState,
    CURRENT_SESSION_SCHEMA_VERSION,
};
use task::TaskSnapshot;

pub const CURRENT_SESSION_GENERATION_SCHEMA_VERSION: u32 = 1;
const MANIFEST_MEMBER_NAME: &str = "manifest.json";
const SESSION_STATE_MEMBER_NAME: &str = "session-state.json";
const SESSION_METADATA_MEMBER_NAME: &str = "metadata.json";
const SESSION_TASK_MEMBER_NAME: &str = "task-state.json";
const SESSION_WORKSPACE_MEMBER_NAME: &str = "workspace-state.json";
const SESSION_RECEIPT_MEMBER_NAME: &str = "receipt-ledger.json";
const SESSION_SKILL_MEMBER_NAME: &str = "skill-loads.json";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionStepReference {
    cursor: RunStepCursor,
    member_name: String,
}

impl SessionStepReference {
    pub fn cursor(&self) -> &RunStepCursor {
        &self.cursor
    }

    pub fn member_name(&self) -> &str {
        &self.member_name
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionGenerationManifest {
    generation_schema_version: u32,
    session_schema_version: u32,
    session_id: String,
    revision: u64,
    state_member_name: String,
    metadata_member_name: String,
    steps: Vec<SessionStepReference>,
}

impl SessionGenerationManifest {
    pub fn new(
        session_id: impl Into<String>,
        revision: u64,
        step_cursors: Vec<RunStepCursor>,
    ) -> Result<Self, SessionGenerationWireError> {
        let mut identities = HashSet::with_capacity(step_cursors.len());
        let mut steps = Vec::with_capacity(step_cursors.len());
        for cursor in step_cursors {
            if !identities.insert((cursor.run_id.clone(), cursor.step_id.clone())) {
                return Err(SessionGenerationWireError::DuplicateStepIdentity {
                    run_id: cursor.run_id,
                    step_id: cursor.step_id,
                });
            }
            steps.push(SessionStepReference {
                member_name: Self::step_member_name(&cursor),
                cursor,
            });
        }
        Ok(Self {
            generation_schema_version: CURRENT_SESSION_GENERATION_SCHEMA_VERSION,
            session_schema_version: CURRENT_SESSION_SCHEMA_VERSION,
            session_id: session_id.into(),
            revision,
            state_member_name: SESSION_STATE_MEMBER_NAME.to_string(),
            metadata_member_name: SESSION_METADATA_MEMBER_NAME.to_string(),
            steps,
        })
    }

    pub fn generation_schema_version(&self) -> u32 {
        self.generation_schema_version
    }

    pub fn session_schema_version(&self) -> u32 {
        self.session_schema_version
    }

    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn state_member_name(&self) -> &str {
        &self.state_member_name
    }

    pub fn metadata_member_name(&self) -> &str {
        &self.metadata_member_name
    }

    pub fn task_member_name() -> &'static str {
        SESSION_TASK_MEMBER_NAME
    }

    pub fn workspace_member_name() -> &'static str {
        SESSION_WORKSPACE_MEMBER_NAME
    }

    pub fn receipt_member_name() -> &'static str {
        SESSION_RECEIPT_MEMBER_NAME
    }

    pub fn skill_member_name() -> &'static str {
        SESSION_SKILL_MEMBER_NAME
    }

    pub fn steps(&self) -> &[SessionStepReference] {
        &self.steps
    }

    pub fn manifest_member_name() -> &'static str {
        MANIFEST_MEMBER_NAME
    }

    pub fn step_member_name(cursor: &RunStepCursor) -> String {
        format!(
            "step-{}-{}.json",
            encode_identity_component(&cursor.run_id),
            encode_identity_component(&cursor.step_id)
        )
    }

    fn validate(&self) -> Result<(), SessionGenerationWireError> {
        let expected = Self::new(
            self.session_id.clone(),
            self.revision,
            self.steps
                .iter()
                .map(|reference| reference.cursor.clone())
                .collect(),
        )?;
        if self.generation_schema_version != CURRENT_SESSION_GENERATION_SCHEMA_VERSION
            || self.session_schema_version != CURRENT_SESSION_SCHEMA_VERSION
            || self.state_member_name != SESSION_STATE_MEMBER_NAME
            || self.metadata_member_name != SESSION_METADATA_MEMBER_NAME
            || self.steps != expected.steps
        {
            return Err(SessionGenerationWireError::InvalidManifest(
                "Session generation manifest 引用不一致".to_string(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMetadataMember {
    id: String,
    created_at: String,
    updated_at: String,
    metadata: SessionMetadata,
    revision: u64,
}

impl SessionMetadataMember {
    fn from_session(session: &CanonicalSession) -> Self {
        Self {
            id: session.id.clone(),
            created_at: session.created_at.clone(),
            updated_at: session.updated_at.clone(),
            metadata: session.metadata.clone(),
            revision: session.revision,
        }
    }

    pub fn session_id(&self) -> &str {
        &self.id
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionStateMember {
    id: String,
    #[serde(default)]
    chats: Vec<ChatSegment>,
    #[serde(with = "task_snapshot_state")]
    tasks: SnapshotState<TaskSnapshot>,
    workspace: SnapshotState<PersistedWorkspaceContext>,
    #[serde(default)]
    compact: Option<ActiveCompactMarker>,
    #[serde(default)]
    committed_steps: CommittedStepLedger,
    #[serde(default)]
    skill_load_records: Vec<SkillLoadRecord>,
}

impl SessionStateMember {
    pub fn from_session(session: &CanonicalSession) -> Self {
        Self {
            id: session.id.clone(),
            chats: session.chats.clone(),
            tasks: session.tasks.clone(),
            workspace: session.workspace.clone(),
            compact: session.compact.clone(),
            committed_steps: session.committed_steps.clone(),
            skill_load_records: session.skill_load_records.clone(),
        }
    }

    pub fn session_id(&self) -> &str {
        &self.id
    }

    pub fn into_session(
        self,
        metadata: SessionMetadataMember,
        run_slices: SessionHistory,
    ) -> CanonicalSession {
        CanonicalSession {
            id: self.id,
            chats: self.chats,
            created_at: metadata.created_at,
            updated_at: metadata.updated_at,
            metadata: metadata.metadata,
            tasks: self.tasks,
            workspace: self.workspace,
            revision: metadata.revision,
            compact: self.compact,
            run_slices,
            committed_steps: self.committed_steps,
            skill_load_records: self.skill_load_records,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionMemberBytes {
    name: String,
    bytes: Vec<u8>,
}

impl SessionMemberBytes {
    fn new(name: impl Into<String>, bytes: Vec<u8>) -> Self {
        Self {
            name: name.into(),
            bytes,
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionChangeSet {
    changed_members: Vec<SessionMemberBytes>,
    reused_members: Vec<String>,
    removed_members: Vec<String>,
}

impl SessionChangeSet {
    pub fn initial(session: &CanonicalSession) -> Result<Self, SessionGenerationWireError> {
        let empty = CanonicalSession {
            id: session.id.clone(),
            chats: Vec::new(),
            created_at: session.created_at.clone(),
            updated_at: session.created_at.clone(),
            metadata: SessionMetadata::default(),
            tasks: SnapshotState::Missing,
            workspace: SnapshotState::Missing,
            revision: 0,
            compact: None,
            run_slices: Default::default(),
            committed_steps: Default::default(),
            skill_load_records: Vec::new(),
        };
        let mut changes = Self::between(&empty, session)?;
        for (name, bytes) in [
            (
                SESSION_METADATA_MEMBER_NAME,
                SessionGenerationCodec::encode_metadata(&SessionMetadataMember::from_session(
                    session,
                ))?,
            ),
            (
                SESSION_STATE_MEMBER_NAME,
                SessionGenerationCodec::encode_state(&SessionStateMember::from_session(session))?,
            ),
        ] {
            if !changes
                .changed_members
                .iter()
                .any(|member| member.name == name)
            {
                changes
                    .changed_members
                    .push(SessionMemberBytes::new(name, bytes));
            }
        }
        changes
            .changed_members
            .sort_by(|left, right| left.name.cmp(&right.name));
        Ok(changes)
    }

    pub fn between(
        before: &CanonicalSession,
        after: &CanonicalSession,
    ) -> Result<Self, SessionGenerationWireError> {
        if before.id != after.id {
            return Err(SessionGenerationWireError::SessionIdentityMismatch {
                before: before.id.clone(),
                after: after.id.clone(),
            });
        }

        let before_steps = collect_steps(before)?;
        let after_steps = collect_steps(after)?;
        let after_manifest = SessionGenerationManifest::new(
            after.id.clone(),
            after.revision,
            after_steps
                .iter()
                .map(|step| step.cursor().clone())
                .collect(),
        )?;
        let before_metadata = SessionMetadataMember::from_session(before);
        let after_metadata = SessionMetadataMember::from_session(after);
        let before_state = SessionStateMember::from_session(before);
        let after_state = SessionStateMember::from_session(after);
        let mut changed_members = vec![SessionMemberBytes::new(
            SessionGenerationManifest::manifest_member_name(),
            SessionGenerationCodec::encode_manifest(&after_manifest)?,
        )];
        let mut reused_members = Vec::new();

        let before_state_bytes = SessionGenerationCodec::encode_state(&before_state)?;
        let after_state_bytes = SessionGenerationCodec::encode_state(&after_state)?;
        if before_state_bytes == after_state_bytes {
            if before.revision != 0 && !after_steps.is_empty() {
                reused_members.push(SESSION_STATE_MEMBER_NAME.to_string());
            }
        } else {
            changed_members.push(SessionMemberBytes::new(
                SESSION_STATE_MEMBER_NAME,
                after_state_bytes,
            ));
        }

        let before_metadata_bytes = SessionGenerationCodec::encode_metadata(&before_metadata)?;
        let after_metadata_bytes = SessionGenerationCodec::encode_metadata(&after_metadata)?;
        if before_metadata_bytes == after_metadata_bytes {
            if before.revision != 0 && !after_steps.is_empty() {
                reused_members.push(SESSION_METADATA_MEMBER_NAME.to_string());
            }
        } else {
            changed_members.push(SessionMemberBytes::new(
                SESSION_METADATA_MEMBER_NAME,
                after_metadata_bytes,
            ));
        }

        let before_by_name = before_steps
            .iter()
            .map(|step| {
                (
                    SessionGenerationManifest::step_member_name(step.cursor()),
                    step,
                )
            })
            .collect::<HashMap<_, _>>();
        for after_step in &after_steps {
            let member_name = SessionGenerationManifest::step_member_name(after_step.cursor());
            if before_by_name
                .get(&member_name)
                .is_some_and(|before_step| step_wire_equal(before_step, after_step))
            {
                reused_members.push(member_name);
            } else {
                changed_members.push(SessionMemberBytes::new(
                    member_name,
                    SessionGenerationCodec::encode_step(after_step)?,
                ));
            }
        }

        let after_names = after_steps
            .iter()
            .map(|step| SessionGenerationManifest::step_member_name(step.cursor()))
            .collect::<HashSet<_>>();
        let mut removed_members = before_steps
            .iter()
            .map(|step| SessionGenerationManifest::step_member_name(step.cursor()))
            .filter(|name| !after_names.contains(name))
            .collect::<Vec<_>>();

        changed_members.sort_by(|left, right| left.name.cmp(&right.name));
        reused_members.sort();
        removed_members.sort();
        Ok(Self {
            changed_members,
            reused_members,
            removed_members,
        })
    }

    pub fn changed_members(&self) -> &[SessionMemberBytes] {
        &self.changed_members
    }

    pub fn reused_members(&self) -> &[String] {
        &self.reused_members
    }

    pub fn removed_members(&self) -> &[String] {
        &self.removed_members
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionStepMember {
    cursor: RunStepCursor,
    step: CommittedRunStep,
}

impl SessionStepMember {
    pub fn new(
        cursor: RunStepCursor,
        step: CommittedRunStep,
    ) -> Result<Self, SessionGenerationWireError> {
        if cursor.step_id != step.step_id {
            return Err(SessionGenerationWireError::StepIdentityMismatch {
                cursor_step_id: cursor.step_id,
                member_step_id: step.step_id,
            });
        }
        Ok(Self { cursor, step })
    }

    pub fn cursor(&self) -> &RunStepCursor {
        &self.cursor
    }

    pub fn step(&self) -> &CommittedRunStep {
        &self.step
    }

    fn validate(&self) -> Result<(), SessionGenerationWireError> {
        if self.cursor.step_id != self.step.step_id {
            return Err(SessionGenerationWireError::StepIdentityMismatch {
                cursor_step_id: self.cursor.step_id.clone(),
                member_step_id: self.step.step_id.clone(),
            });
        }
        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SessionGenerationWireError {
    #[error("Session generation schema version {version} is newer than supported")]
    UnsupportedFutureVersion {
        version: u32,
        original_bytes: Vec<u8>,
    },
    #[error("Session generation manifest 包含重复 Step identity: {run_id}/{step_id}")]
    DuplicateStepIdentity { run_id: String, step_id: String },
    #[error(
        "Session step member identity 不一致: cursor={cursor_step_id}, member={member_step_id}"
    )]
    StepIdentityMismatch {
        cursor_step_id: String,
        member_step_id: String,
    },
    #[error("Session identity 不一致: before={before}, after={after}")]
    SessionIdentityMismatch { before: String, after: String },
    #[error("Session generation manifest 无效: {0}")]
    InvalidManifest(String),
    #[error("Session generation JSON 解码失败: {0}")]
    InvalidJson(String),
    #[error("Session generation JSON 编码失败: {0}")]
    Encode(String),
}

pub struct SessionGenerationCodec;

impl SessionGenerationCodec {
    pub fn encode_manifest(
        manifest: &SessionGenerationManifest,
    ) -> Result<Vec<u8>, SessionGenerationWireError> {
        manifest.validate()?;
        serde_json::to_vec_pretty(manifest)
            .map_err(|error| SessionGenerationWireError::Encode(error.to_string()))
    }

    pub fn decode_manifest(
        bytes: &[u8],
    ) -> Result<SessionGenerationManifest, SessionGenerationWireError> {
        let value: Value = serde_json::from_slice(bytes)
            .map_err(|error| SessionGenerationWireError::InvalidJson(error.to_string()))?;
        if let Some(version) = value
            .get("generation_schema_version")
            .and_then(Value::as_u64)
        {
            if version > u64::from(CURRENT_SESSION_GENERATION_SCHEMA_VERSION) {
                return Err(SessionGenerationWireError::UnsupportedFutureVersion {
                    version: version as u32,
                    original_bytes: bytes.to_vec(),
                });
            }
            if version != u64::from(CURRENT_SESSION_GENERATION_SCHEMA_VERSION) {
                return Err(SessionGenerationWireError::InvalidManifest(format!(
                    "不支持历史 generation schema version {version}"
                )));
            }
        }
        let manifest: SessionGenerationManifest = serde_json::from_value(value)
            .map_err(|error| SessionGenerationWireError::InvalidJson(error.to_string()))?;
        manifest.validate()?;
        Ok(manifest)
    }

    pub fn encode_metadata(
        metadata: &SessionMetadataMember,
    ) -> Result<Vec<u8>, SessionGenerationWireError> {
        serde_json::to_vec(metadata)
            .map_err(|error| SessionGenerationWireError::Encode(error.to_string()))
    }

    pub fn decode_metadata(
        bytes: &[u8],
    ) -> Result<SessionMetadataMember, SessionGenerationWireError> {
        serde_json::from_slice(bytes)
            .map_err(|error| SessionGenerationWireError::InvalidJson(error.to_string()))
    }

    pub fn encode_state(state: &SessionStateMember) -> Result<Vec<u8>, SessionGenerationWireError> {
        serde_json::to_vec_pretty(state)
            .map_err(|error| SessionGenerationWireError::Encode(error.to_string()))
    }

    pub fn decode_state(bytes: &[u8]) -> Result<SessionStateMember, SessionGenerationWireError> {
        serde_json::from_slice(bytes)
            .map_err(|error| SessionGenerationWireError::InvalidJson(error.to_string()))
    }

    pub fn encode_step(member: &SessionStepMember) -> Result<Vec<u8>, SessionGenerationWireError> {
        member.validate()?;
        serde_json::to_vec(member)
            .map_err(|error| SessionGenerationWireError::Encode(error.to_string()))
    }

    pub fn decode_step(bytes: &[u8]) -> Result<SessionStepMember, SessionGenerationWireError> {
        let member: SessionStepMember = serde_json::from_slice(bytes)
            .map_err(|error| SessionGenerationWireError::InvalidJson(error.to_string()))?;
        member.validate()?;
        Ok(member)
    }
}

fn collect_steps(
    session: &CanonicalSession,
) -> Result<Vec<SessionStepMember>, SessionGenerationWireError> {
    session
        .run_slices
        .iter()
        .flat_map(|slice| {
            slice.steps.iter().cloned().map(|step| {
                SessionStepMember::new(
                    RunStepCursor {
                        run_id: slice.run_id.clone(),
                        step_id: step.step_id.clone(),
                    },
                    step,
                )
            })
        })
        .collect()
}

fn step_wire_equal(left: &SessionStepMember, right: &SessionStepMember) -> bool {
    left.cursor == right.cursor
        && serde_json::to_vec(&left.step).ok() == serde_json::to_vec(&right.step).ok()
}

fn encode_identity_component(identity: &str) -> String {
    let mut encoded = String::with_capacity(identity.len() * 2);
    for byte in identity.as_bytes() {
        use std::fmt::Write as _;
        write!(&mut encoded, "{byte:02x}").expect("writing to String cannot fail");
    }
    encoded
}

#[cfg(test)]
#[path = "generation_tests.rs"]
mod tests;
