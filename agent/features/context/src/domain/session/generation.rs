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

#[derive(Debug, Clone)]
pub struct DisplayHistoryStepWindow {
    session_id: String,
    generation_revision: u64,
    steps: Vec<SessionStepMember>,
}

impl DisplayHistoryStepWindow {
    pub fn new(
        session_id: impl Into<String>,
        generation_revision: u64,
        steps: Vec<SessionStepMember>,
    ) -> Self {
        Self {
            session_id: session_id.into(),
            generation_revision,
            steps,
        }
    }

    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    pub fn generation_revision(&self) -> u64 {
        self.generation_revision
    }

    pub fn steps(&self) -> &[SessionStepMember] {
        &self.steps
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DisplayHistoryStepReference {
    run_id: String,
    step_id: String,
    member_name: String,
    estimated_lines: usize,
    user_input_history: Vec<String>,
    finalize_cause: Option<crate::domain::FinalizeCause>,
    duration_ms: Option<u64>,
}

impl DisplayHistoryStepReference {
    pub fn run_id(&self) -> &str {
        &self.run_id
    }

    pub fn step_id(&self) -> &str {
        &self.step_id
    }

    pub fn member_name(&self) -> &str {
        &self.member_name
    }

    pub fn estimated_lines(&self) -> usize {
        self.estimated_lines
    }

    pub fn user_input_history(&self) -> &[String] {
        &self.user_input_history
    }

    pub fn finalize_cause(&self) -> Option<crate::domain::FinalizeCause> {
        self.finalize_cause
    }

    pub fn duration_ms(&self) -> Option<u64> {
        self.duration_ms
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DisplayHistoryStepIndex {
    session_id: String,
    generation_revision: u64,
    steps: Vec<DisplayHistoryStepReference>,
}

impl DisplayHistoryStepIndex {
    #[cfg(any(test, feature = "dev"))]
    pub fn fixture(
        session_id: impl Into<String>,
        generation_revision: u64,
        steps: Vec<(&str, &str, &str, usize)>,
    ) -> Self {
        Self {
            session_id: session_id.into(),
            generation_revision,
            steps: steps
                .into_iter()
                .map(|(run_id, step_id, member_name, estimated_lines)| {
                    DisplayHistoryStepReference {
                        run_id: run_id.to_string(),
                        step_id: step_id.to_string(),
                        member_name: member_name.to_string(),
                        estimated_lines,
                        user_input_history: Vec::new(),
                        finalize_cause: None,
                        duration_ms: None,
                    }
                })
                .collect(),
        }
    }

    pub fn from_session_and_manifest(
        session: &CanonicalSession,
        manifest: &SessionGenerationManifest,
    ) -> Self {
        let steps_by_identity = session
            .run_slices
            .iter()
            .flat_map(|slice| {
                slice
                    .steps
                    .iter()
                    .map(|step| ((slice.run_id.as_str(), step.step_id.as_str()), step))
            })
            .collect::<HashMap<_, _>>();
        Self {
            session_id: manifest.session_id.clone(),
            generation_revision: manifest.revision,
            steps: manifest
                .steps
                .iter()
                .map(|reference| {
                    let step = steps_by_identity
                        .get(&(
                            reference.cursor.run_id.as_str(),
                            reference.cursor.step_id.as_str(),
                        ))
                        .copied();
                    DisplayHistoryStepReference {
                        run_id: reference.cursor.run_id.clone(),
                        step_id: reference.cursor.step_id.clone(),
                        member_name: reference.member_name.clone(),
                        estimated_lines: step.map(estimated_step_lines).unwrap_or(1),
                        user_input_history: step.map(step_user_input_history).unwrap_or_default(),
                        finalize_cause: step
                            .and_then(|step| step.outcome.as_ref())
                            .map(|outcome| outcome.finalize_cause),
                        duration_ms: step
                            .and_then(|step| step.outcome.as_ref())
                            .and_then(|outcome| outcome.duration_ms),
                    }
                })
                .collect(),
        }
    }

    pub fn from_manifest(manifest: &SessionGenerationManifest) -> Self {
        Self {
            session_id: manifest.session_id.clone(),
            generation_revision: manifest.revision,
            steps: manifest
                .steps
                .iter()
                .map(|reference| DisplayHistoryStepReference {
                    run_id: reference.cursor.run_id.clone(),
                    step_id: reference.cursor.step_id.clone(),
                    member_name: reference.member_name.clone(),
                    estimated_lines: reference.estimated_lines,
                    user_input_history: reference.user_input_history.clone(),
                    finalize_cause: reference.finalize_cause,
                    duration_ms: reference.duration_ms,
                })
                .collect(),
        }
    }

    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    pub fn generation_revision(&self) -> u64 {
        self.generation_revision
    }

    pub fn steps(&self) -> &[DisplayHistoryStepReference] {
        &self.steps
    }
}

fn default_estimated_lines() -> usize {
    1
}

fn step_user_input_history(step: &CommittedRunStep) -> Vec<String> {
    step.accepted_input
        .iter()
        .flat_map(|input| input.messages.iter())
        .filter(|message| {
            message.role == share::message::Role::User
                && message.source() == share::message::MessageSource::User
                && !message.has_tool_results()
        })
        .map(|message| message.text_content())
        .filter(|text| !text.trim().is_empty())
        .collect()
}

fn estimated_step_lines(step: &CommittedRunStep) -> usize {
    step.accepted_input
        .iter()
        .flat_map(|input| input.messages.iter())
        .chain(
            step.outcome
                .iter()
                .flat_map(|outcome| outcome.messages.iter()),
        )
        .map(|message| message.text_content().lines().count().max(1))
        .fold(0usize, |total, lines| total.saturating_add(lines))
        .max(1)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionStepReference {
    cursor: RunStepCursor,
    member_name: String,
    #[serde(default = "default_estimated_lines")]
    estimated_lines: usize,
    #[serde(default)]
    user_input_history: Vec<String>,
    #[serde(default)]
    finalize_cause: Option<crate::domain::FinalizeCause>,
    #[serde(default)]
    duration_ms: Option<u64>,
}

impl SessionStepReference {
    pub fn cursor(&self) -> &RunStepCursor {
        &self.cursor
    }

    pub fn member_name(&self) -> &str {
        &self.member_name
    }

    pub fn estimated_lines(&self) -> usize {
        self.estimated_lines
    }

    pub fn user_input_history(&self) -> &[String] {
        &self.user_input_history
    }

    pub fn finalize_cause(&self) -> Option<crate::domain::FinalizeCause> {
        self.finalize_cause
    }

    pub fn duration_ms(&self) -> Option<u64> {
        self.duration_ms
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
                estimated_lines: default_estimated_lines(),
                user_input_history: Vec::new(),
                finalize_cause: None,
                duration_ms: None,
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

    pub fn with_step_metadata(
        mut self,
        steps: &[SessionStepMember],
    ) -> Result<Self, SessionGenerationWireError> {
        let steps_by_identity = steps
            .iter()
            .map(|member| {
                (
                    (
                        member.cursor.run_id.as_str(),
                        member.cursor.step_id.as_str(),
                    ),
                    &member.step,
                )
            })
            .collect::<HashMap<_, _>>();
        for reference in &mut self.steps {
            let step = steps_by_identity
                .get(&(
                    reference.cursor.run_id.as_str(),
                    reference.cursor.step_id.as_str(),
                ))
                .copied()
                .ok_or_else(|| {
                    SessionGenerationWireError::InvalidManifest(
                        "Session generation 缺少 Step metadata 来源".to_string(),
                    )
                })?;
            reference.estimated_lines = estimated_step_lines(step);
            reference.user_input_history = step_user_input_history(step);
            reference.finalize_cause = step.outcome.as_ref().map(|outcome| outcome.finalize_cause);
            reference.duration_ms = step
                .outcome
                .as_ref()
                .and_then(|outcome| outcome.duration_ms);
        }
        Ok(self)
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
            || self.steps.len() != expected.steps.len()
            || self
                .steps
                .iter()
                .zip(&expected.steps)
                .any(|(actual, expected)| {
                    actual.cursor != expected.cursor || actual.member_name != expected.member_name
                })
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

    pub fn compact_start_at(&self) -> Option<&RunStepCursor> {
        self.compact
            .as_ref()
            .and_then(|marker| marker.start_at.as_ref())
    }

    pub fn compact_summary(&self) -> Option<&str> {
        self.compact.as_ref().map(|marker| marker.summary.as_str())
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
pub struct SessionCommitPlan {
    changed_members: Vec<SessionMemberBytes>,
    reused_members: Vec<String>,
    removed_members: Vec<String>,
    reuse_fallbacks: Vec<SessionMemberBytes>,
    preserves_unloaded_steps: bool,
}

impl SessionCommitPlan {
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

    pub fn between_preserving_unloaded_steps(
        before: &CanonicalSession,
        after: &CanonicalSession,
        persisted_manifest: &SessionGenerationManifest,
    ) -> Result<Self, SessionGenerationWireError> {
        if before.id != persisted_manifest.session_id
            || before.revision != persisted_manifest.revision
        {
            return Err(SessionGenerationWireError::StaleSessionGeneration {
                expected_revision: before.revision,
                actual_revision: persisted_manifest.revision,
            });
        }
        let mut changes = Self::between(before, after)?;
        let after_steps = collect_steps(after)?;
        let after_by_identity = after_steps
            .iter()
            .map(|member| {
                (
                    (
                        member.cursor.run_id.as_str(),
                        member.cursor.step_id.as_str(),
                    ),
                    member,
                )
            })
            .collect::<HashMap<_, _>>();
        let mut merged_references = persisted_manifest.steps.clone();
        for reference in &mut merged_references {
            if let Some(step) = after_by_identity.get(&(
                reference.cursor.run_id.as_str(),
                reference.cursor.step_id.as_str(),
            )) {
                reference.estimated_lines = estimated_step_lines(step.step());
                reference.user_input_history = step_user_input_history(step.step());
                reference.finalize_cause = step
                    .step()
                    .outcome
                    .as_ref()
                    .map(|outcome| outcome.finalize_cause);
                reference.duration_ms = step
                    .step()
                    .outcome
                    .as_ref()
                    .and_then(|outcome| outcome.duration_ms);
            }
        }
        let persisted_identities = merged_references
            .iter()
            .map(|reference| {
                (
                    reference.cursor.run_id.clone(),
                    reference.cursor.step_id.clone(),
                )
            })
            .collect::<HashSet<_>>();
        for step in &after_steps {
            if persisted_identities
                .contains(&(step.cursor.run_id.clone(), step.cursor.step_id.clone()))
            {
                continue;
            }
            merged_references.push(SessionStepReference {
                cursor: step.cursor.clone(),
                member_name: SessionGenerationManifest::step_member_name(&step.cursor),
                estimated_lines: estimated_step_lines(step.step()),
                user_input_history: step_user_input_history(step.step()),
                finalize_cause: step
                    .step()
                    .outcome
                    .as_ref()
                    .map(|outcome| outcome.finalize_cause),
                duration_ms: step
                    .step()
                    .outcome
                    .as_ref()
                    .and_then(|outcome| outcome.duration_ms),
            });
        }
        let merged_manifest = SessionGenerationManifest {
            generation_schema_version: CURRENT_SESSION_GENERATION_SCHEMA_VERSION,
            session_schema_version: CURRENT_SESSION_SCHEMA_VERSION,
            session_id: after.id.clone(),
            revision: after.revision,
            state_member_name: SESSION_STATE_MEMBER_NAME.to_string(),
            metadata_member_name: SESSION_METADATA_MEMBER_NAME.to_string(),
            steps: merged_references,
        };
        let encoded_manifest = SessionGenerationCodec::encode_manifest(&merged_manifest)?;
        let manifest_member = changes
            .changed_members
            .iter_mut()
            .find(|member| member.name == MANIFEST_MEMBER_NAME)
            .ok_or_else(|| {
                SessionGenerationWireError::InvalidManifest(
                    "Session change set 缺少 generation manifest".to_string(),
                )
            })?;
        manifest_member.bytes = encoded_manifest;

        changes.preserves_unloaded_steps = true;
        changes.removed_members.clear();
        let changed_names = changes
            .changed_members
            .iter()
            .map(|member| member.name.as_str())
            .collect::<HashSet<_>>();
        for reference in persisted_manifest.steps() {
            if !changed_names.contains(reference.member_name())
                && !changes
                    .reused_members
                    .iter()
                    .any(|name| name == reference.member_name())
            {
                changes.reused_members.push(reference.member_name.clone());
            }
        }
        let after_steps_by_name = after_steps
            .iter()
            .map(|step| {
                (
                    SessionGenerationManifest::step_member_name(step.cursor()),
                    step,
                )
            })
            .collect::<HashMap<_, _>>();
        changes.reuse_fallbacks = changes
            .reused_members
            .iter()
            .filter_map(|name| {
                after_steps_by_name.get(name).map(|step| {
                    SessionGenerationCodec::encode_step(step)
                        .map(|bytes| SessionMemberBytes::new(name.clone(), bytes))
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let persisted_step_names = persisted_manifest
            .steps()
            .iter()
            .map(|reference| reference.member_name.as_str())
            .collect::<HashSet<_>>();
        let missing_reused_step_names = changes
            .reused_members
            .iter()
            .filter(|name| {
                after_steps_by_name.contains_key(name.as_str())
                    && !persisted_step_names.contains(name.as_str())
            })
            .cloned()
            .collect::<HashSet<_>>();
        changes
            .reused_members
            .retain(|name| !missing_reused_step_names.contains(name));
        for name in missing_reused_step_names {
            let step = after_steps_by_name.get(&name).ok_or_else(|| {
                SessionGenerationWireError::InvalidManifest(format!(
                    "Session 待写入 Step member 不存在：{name}"
                ))
            })?;
            changes.changed_members.push(SessionMemberBytes::new(
                name,
                SessionGenerationCodec::encode_step(step)?,
            ));
        }
        changes
            .changed_members
            .sort_by(|left, right| left.name.cmp(&right.name));
        changes.reused_members.sort();
        changes.reused_members.dedup();
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
        )?
        .with_step_metadata(&after_steps)?;
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
            if before.revision != 0 {
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
            if before.revision != 0 {
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
            reuse_fallbacks: Vec::new(),
            preserves_unloaded_steps: false,
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

    pub fn validate_initial_commit_boundary(
        &self,
        session_id: &str,
        expected_revision: u64,
    ) -> Result<(), SessionGenerationWireError> {
        if expected_revision != 0 {
            return Err(SessionGenerationWireError::StaleSessionGeneration {
                expected_revision,
                actual_revision: 0,
            });
        }
        self.validate_target_manifest(session_id, expected_revision)
    }

    /// 数据集被外部清空后的全量重建计划：以内存完整状态生成
    /// target revision = session.revision 的全量成员（reused 为空）。
    ///
    /// 与 [`Self::initial`] 共享生成逻辑，但配套的
    /// [`Self::validate_rebuild_boundary`] 不要求 `expected_revision == 0`，
    /// 允许内存修订号 N > 0 的长驻进程在空数据集上重建。
    pub fn complete_snapshot(
        session: &CanonicalSession,
    ) -> Result<Self, SessionGenerationWireError> {
        Self::initial(session)
    }

    /// 全量重建提交边界：不要求 `expected_revision == 0`（内存修订号可为任意
    /// N > 0），但仍要求 target manifest 恰好递增一次，维持修订号单调不变量，
    /// 重建后磁盘修订号与内存自然对齐。
    pub fn validate_rebuild_boundary(
        &self,
        session_id: &str,
        expected_revision: u64,
    ) -> Result<(), SessionGenerationWireError> {
        self.validate_target_manifest(session_id, expected_revision)
    }

    pub fn validate_commit_boundary(
        &self,
        session_id: &str,
        expected_revision: u64,
        persisted_manifest: &SessionGenerationManifest,
    ) -> Result<(), SessionGenerationWireError> {
        if persisted_manifest.session_id() != session_id {
            return Err(SessionGenerationWireError::SessionIdentityMismatch {
                before: session_id.to_string(),
                after: persisted_manifest.session_id().to_string(),
            });
        }
        if persisted_manifest.revision() != expected_revision {
            return Err(SessionGenerationWireError::StaleSessionGeneration {
                expected_revision,
                actual_revision: persisted_manifest.revision(),
            });
        }
        self.validate_target_manifest(session_id, expected_revision)
    }

    fn validate_target_manifest(
        &self,
        session_id: &str,
        expected_revision: u64,
    ) -> Result<(), SessionGenerationWireError> {
        let manifest_member = self
            .changed_members
            .iter()
            .find(|member| member.name() == MANIFEST_MEMBER_NAME)
            .ok_or_else(|| {
                SessionGenerationWireError::InvalidManifest(
                    "Session commit plan 缺少 generation manifest".to_string(),
                )
            })?;
        let target_manifest = SessionGenerationCodec::decode_manifest(manifest_member.bytes())?;
        if target_manifest.session_id() != session_id {
            return Err(SessionGenerationWireError::SessionIdentityMismatch {
                before: session_id.to_string(),
                after: target_manifest.session_id().to_string(),
            });
        }
        let target_revision = expected_revision.checked_add(1).ok_or_else(|| {
            SessionGenerationWireError::InvalidManifest(
                "Session commit expected revision 已溢出".to_string(),
            )
        })?;
        if target_manifest.revision() != target_revision {
            return Err(SessionGenerationWireError::InvalidManifest(format!(
                "Session commit target revision 必须递增一次: expected={target_revision}, actual={}",
                target_manifest.revision()
            )));
        }
        Ok(())
    }

    pub fn reconcile_persisted_steps(
        &mut self,
        persisted_manifest: &SessionGenerationManifest,
    ) -> Result<(), SessionGenerationWireError> {
        if !self.preserves_unloaded_steps {
            return Ok(());
        }
        let manifest_member = self
            .changed_members
            .iter_mut()
            .find(|member| member.name() == MANIFEST_MEMBER_NAME)
            .ok_or_else(|| {
                SessionGenerationWireError::InvalidManifest(
                    "Session commit plan 缺少 generation manifest".to_string(),
                )
            })?;
        let target_manifest = SessionGenerationCodec::decode_manifest(manifest_member.bytes())?;
        let target_by_identity = target_manifest
            .steps
            .iter()
            .map(|reference| {
                (
                    (
                        reference.cursor.run_id.as_str(),
                        reference.cursor.step_id.as_str(),
                    ),
                    reference,
                )
            })
            .collect::<HashMap<_, _>>();
        let mut merged_steps = persisted_manifest.steps.clone();
        for reference in &mut merged_steps {
            if let Some(target) = target_by_identity.get(&(
                reference.cursor.run_id.as_str(),
                reference.cursor.step_id.as_str(),
            )) {
                *reference = (*target).clone();
            }
        }
        let persisted_identities = merged_steps
            .iter()
            .map(|reference| {
                (
                    reference.cursor.run_id.clone(),
                    reference.cursor.step_id.clone(),
                )
            })
            .collect::<HashSet<_>>();
        for reference in &target_manifest.steps {
            if !persisted_identities.contains(&(
                reference.cursor.run_id.clone(),
                reference.cursor.step_id.clone(),
            )) {
                merged_steps.push(reference.clone());
            }
        }
        let merged_manifest = SessionGenerationManifest {
            steps: merged_steps,
            ..target_manifest
        };
        manifest_member.bytes = SessionGenerationCodec::encode_manifest(&merged_manifest)?;

        let changed_names = self
            .changed_members
            .iter()
            .map(|member| member.name())
            .collect::<HashSet<_>>();
        for reference in persisted_manifest.steps() {
            if !changed_names.contains(reference.member_name())
                && !self
                    .reused_members
                    .iter()
                    .any(|name| name == reference.member_name())
            {
                self.reused_members.push(reference.member_name.clone());
            }
        }
        self.reused_members.sort();
        self.reused_members.dedup();
        Ok(())
    }

    pub fn promote_reuse_fallbacks<F>(&mut self, mut has_evidence: F)
    where
        F: FnMut(&str) -> bool,
    {
        let missing_names = self
            .reused_members
            .iter()
            .filter(|name| !has_evidence(name))
            .cloned()
            .collect::<HashSet<_>>();
        self.reused_members
            .retain(|name| !missing_names.contains(name));
        for fallback in &self.reuse_fallbacks {
            if missing_names.contains(fallback.name())
                && !self
                    .changed_members
                    .iter()
                    .any(|member| member.name() == fallback.name())
            {
                self.changed_members.push(fallback.clone());
            }
        }
        self.changed_members
            .sort_by(|left, right| left.name.cmp(&right.name));
    }

    pub fn reuse_fallbacks(&self) -> &[SessionMemberBytes] {
        &self.reuse_fallbacks
    }

    pub fn preserves_unloaded_steps(&self) -> bool {
        self.preserves_unloaded_steps
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
    #[error("Session 数据集修订号已变更: expected={expected_revision}, actual={actual_revision}")]
    StaleSessionGeneration {
        expected_revision: u64,
        actual_revision: u64,
    },
    #[error("Session generation 已变更: expected={expected_revision}, actual={actual_revision}")]
    StaleDisplayHistory {
        expected_revision: u64,
        actual_revision: u64,
    },
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
