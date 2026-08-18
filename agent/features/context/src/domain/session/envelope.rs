use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;
use share::message::Message;
use std::ops::Deref;
use std::path::PathBuf;
use std::sync::Arc;
use task::TaskSnapshot;

use crate::domain::{FinalizeCause, StepReceipt, ToolCallReceipt, ToolReceiptMutation};

use super::{ChatSegment, PersistedWorkspaceContext, SessionMetadata};

pub const CURRENT_SESSION_SCHEMA_VERSION: u32 = 6;

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

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CommittedStepLedger {
    entries: Arc<[Arc<CommittedStep>]>,
}

impl CommittedStepLedger {
    pub fn from_steps(steps: Vec<CommittedStep>) -> Self {
        Self {
            entries: steps.into_iter().map(Arc::new).collect(),
        }
    }

    pub fn entries(&self) -> &[Arc<CommittedStep>] {
        &self.entries
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn iter(&self) -> std::slice::Iter<'_, Arc<CommittedStep>> {
        self.entries.iter()
    }

    pub fn find(&self, run_id: &str, step_id: &str) -> Option<&CommittedStep> {
        self.entries
            .iter()
            .find(|entry| entry.run_id == run_id && entry.step_id == step_id)
            .map(Arc::as_ref)
    }

    pub fn append(&self, step: CommittedStep) -> Self {
        let mut entries = self.entries.to_vec();
        entries.push(Arc::new(step));
        Self {
            entries: entries.into(),
        }
    }

    pub fn cleared(&self) -> Self {
        Self::default()
    }
}

impl std::ops::Index<usize> for CommittedStepLedger {
    type Output = CommittedStep;

    fn index(&self, index: usize) -> &Self::Output {
        &self.entries[index]
    }
}

impl Serialize for CommittedStepLedger {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.entries
            .iter()
            .map(Arc::as_ref)
            .collect::<Vec<_>>()
            .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for CommittedStepLedger {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Vec::<CommittedStep>::deserialize(deserializer).map(Self::from_steps)
    }
}

impl From<Vec<CommittedStep>> for CommittedStepLedger {
    fn from(steps: Vec<CommittedStep>) -> Self {
        Self::from_steps(steps)
    }
}

impl FromIterator<CommittedStep> for CommittedStepLedger {
    fn from_iter<I: IntoIterator<Item = CommittedStep>>(steps: I) -> Self {
        Self::from_steps(steps.into_iter().collect())
    }
}

#[derive(Debug, Clone, Default)]
pub struct CommittedStepMessages(Arc<[Message]>);

impl CommittedStepMessages {
    pub fn as_arc(&self) -> Arc<[Message]> {
        Arc::clone(&self.0)
    }
}

impl From<Vec<Message>> for CommittedStepMessages {
    fn from(messages: Vec<Message>) -> Self {
        Self(messages.into())
    }
}

impl Deref for CommittedStepMessages {
    type Target = [Message];

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Serialize for CommittedStepMessages {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.0.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for CommittedStepMessages {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Vec::<Message>::deserialize(deserializer).map(Self::from)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcceptedInputRecord {
    pub messages: CommittedStepMessages,
    pub fingerprint: String,
    pub committed_revision: u64,
}

impl AcceptedInputRecord {
    pub fn new(
        messages: Vec<Message>,
        fingerprint: impl Into<String>,
        committed_revision: u64,
    ) -> Self {
        Self {
            messages: messages.into(),
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
pub struct FinalizedOutcomeRecord {
    pub finalize_cause: FinalizeCause,
    #[serde(default)]
    pub duration_ms: Option<u64>,
    pub messages: CommittedStepMessages,
    pub receipts: Vec<StepReceipt>,
    pub api_input_tokens: Option<u64>,
    pub fingerprint: String,
    pub committed_revision: u64,
}

impl FinalizedOutcomeRecord {
    pub fn compatibility(messages: Vec<Message>) -> Self {
        Self {
            finalize_cause: FinalizeCause::Completed,
            duration_ms: None,
            messages: messages.into(),
            receipts: Vec::new(),
            api_input_tokens: None,
            fingerprint: String::new(),
            committed_revision: 0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommittedRunStep {
    pub step_id: String,
    #[serde(default)]
    pub accepted_input: Option<AcceptedInputRecord>,
    #[serde(default)]
    pub outcome: Option<FinalizedOutcomeRecord>,
    #[serde(default)]
    pub tool_receipts: Vec<ToolCallReceipt>,
}

impl CommittedRunStep {
    pub fn accepted_only(step_id: impl Into<String>, accepted_input: AcceptedInputRecord) -> Self {
        Self {
            step_id: step_id.into(),
            accepted_input: Some(accepted_input),
            outcome: None,
            tool_receipts: Vec::new(),
        }
    }

    pub fn outcome_only(step_id: impl Into<String>, outcome: FinalizedOutcomeRecord) -> Self {
        Self {
            step_id: step_id.into(),
            accepted_input: None,
            outcome: Some(outcome),
            tool_receipts: Vec::new(),
        }
    }

    pub fn compatibility_outcome_only(step_id: impl Into<String>, messages: Vec<Message>) -> Self {
        Self::outcome_only(step_id, FinalizedOutcomeRecord::compatibility(messages))
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

#[derive(Debug, Clone)]
pub(crate) struct RestoreStepSource {
    pub cursor: RunStepCursor,
    pub message_segments: Vec<Arc<[Message]>>,
    pub tool_receipts: Vec<ToolCallReceipt>,
    pub finalize_cause: Option<FinalizeCause>,
    pub duration_ms: Option<u64>,
}

fn step_message_segments(step: &CommittedRunStep) -> Vec<Arc<[Message]>> {
    step.accepted_input
        .iter()
        .map(|input| input.messages.as_arc())
        .chain(step.outcome.iter().map(|outcome| outcome.messages.as_arc()))
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillLoadRecord {
    pub scope: tools::SkillLoadScope,
    pub skill_name: String,
    pub revision: String,
}

impl std::ops::Index<usize> for SessionHistory {
    type Output = CommittedRunSlice;

    fn index(&self, index: usize) -> &Self::Output {
        &self.slices[index]
    }
}

impl Serialize for SessionHistory {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.slices
            .iter()
            .map(Arc::as_ref)
            .collect::<Vec<_>>()
            .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for SessionHistory {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Vec::<CommittedRunSlice>::deserialize(deserializer).map(Self::from_slices)
    }
}

impl From<Vec<CommittedRunSlice>> for SessionHistory {
    fn from(slices: Vec<CommittedRunSlice>) -> Self {
        Self::from_slices(slices)
    }
}

impl FromIterator<CommittedRunSlice> for SessionHistory {
    fn from_iter<I: IntoIterator<Item = CommittedRunSlice>>(slices: I) -> Self {
        Self::from_slices(slices.into_iter().collect())
    }
}

impl<'history> IntoIterator for &'history SessionHistory {
    type Item = &'history Arc<CommittedRunSlice>;
    type IntoIter = std::slice::Iter<'history, Arc<CommittedRunSlice>>;

    fn into_iter(self) -> Self::IntoIter {
        self.slices.iter()
    }
}

#[cfg(test)]
#[path = "envelope_tests.rs"]
mod tests;

#[derive(Clone, Debug, Default)]
pub struct SessionHistory {
    slices: Arc<[Arc<CommittedRunSlice>]>,
}

impl SessionHistory {
    pub fn from_slices(slices: Vec<CommittedRunSlice>) -> Self {
        Self {
            slices: slices.into_iter().map(Arc::new).collect(),
        }
    }

    pub fn slices(&self) -> &[Arc<CommittedRunSlice>] {
        &self.slices
    }

    pub fn len(&self) -> usize {
        self.slices.len()
    }

    pub fn is_empty(&self) -> bool {
        self.slices.is_empty()
    }

    pub fn iter(&self) -> std::slice::Iter<'_, Arc<CommittedRunSlice>> {
        self.slices.iter()
    }

    pub fn append_accepted_input(
        &self,
        run_id: &str,
        step_id: &str,
        accepted_input: AcceptedInputRecord,
    ) -> Self {
        self.replace_or_append_step(run_id, step_id, |step| {
            step.accepted_input = Some(accepted_input);
        })
    }

    pub fn append_finalized_outcome(
        &self,
        run_id: &str,
        step_id: &str,
        outcome: FinalizedOutcomeRecord,
    ) -> Self {
        self.replace_or_append_step(run_id, step_id, |step| {
            step.outcome = Some(outcome);
        })
    }

    pub fn accepted_input(&self, run_id: &str, step_id: &str) -> Option<&AcceptedInputRecord> {
        self.step(run_id, step_id)?.accepted_input.as_ref()
    }

    pub fn tool_receipt(&self, mutation: &ToolReceiptMutation) -> Option<&ToolCallReceipt> {
        self.step(
            mutation.identity.run_id.as_ref(),
            mutation.identity.step_id.as_str(),
        )?
        .tool_receipts
        .iter()
        .find(|receipt| receipt.identity == mutation.identity)
    }

    pub fn step_receipts(&self, run_id: &str, step_id: &str) -> Vec<StepReceipt> {
        let mut receipts = self
            .step(run_id, step_id)
            .map(|step| {
                step.tool_receipts
                    .iter()
                    .filter_map(ToolCallReceipt::to_step_receipt)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        receipts.sort_by_key(StepReceipt::index);
        receipts
    }

    pub fn advance_tool_receipt(
        &self,
        mutation: ToolReceiptMutation,
    ) -> Result<
        (Self, crate::domain::ToolReceiptMutationReceipt),
        crate::domain::ToolReceiptMutationError,
    > {
        if let Some(receipt) = self.tool_receipt(&mutation) {
            let advanced = receipt.clone().advance(mutation.clone())?;
            if !advanced.changed {
                return Ok((self.clone(), advanced));
            }
            let updated_receipt = advanced.receipt.clone();
            let updated = self.replace_or_append_step(
                mutation.identity.run_id.as_ref(),
                mutation.identity.step_id.as_str(),
                |step| {
                    let receipt_index = step
                        .tool_receipts
                        .iter()
                        .position(|receipt| receipt.identity == mutation.identity)
                        .expect("existing receipt must remain in the same step");
                    step.tool_receipts[receipt_index] = updated_receipt;
                },
            );
            return Ok((updated, advanced));
        }

        let input_preview = mutation.input_preview.clone().unwrap_or_default();
        let mut receipt = ToolCallReceipt::pending(mutation.identity.clone(), input_preview);
        if mutation.next != crate::domain::ToolCallState::Pending {
            receipt = receipt.advance(mutation.clone())?.receipt;
        }
        let advanced = crate::domain::ToolReceiptMutationReceipt {
            receipt: receipt.clone(),
            changed: true,
        };
        let updated = self.replace_or_append_step(
            mutation.identity.run_id.as_ref(),
            mutation.identity.step_id.as_str(),
            |step| step.tool_receipts.push(receipt),
        );
        Ok((updated, advanced))
    }

    pub fn cleared(&self) -> Self {
        Self::default()
    }

    fn step(&self, run_id: &str, step_id: &str) -> Option<&CommittedRunStep> {
        self.slices
            .iter()
            .find(|slice| slice.run_id == run_id)?
            .steps
            .iter()
            .find(|step| step.step_id == step_id)
    }

    fn replace_or_append_step(
        &self,
        run_id: &str,
        step_id: &str,
        update: impl FnOnce(&mut CommittedRunStep),
    ) -> Self {
        let mut slices = self.slices.to_vec();
        if let Some(slice_index) = slices.iter().position(|slice| slice.run_id == run_id) {
            let mut slice = (*slices[slice_index]).clone();
            if let Some(step) = slice.steps.iter_mut().find(|step| step.step_id == step_id) {
                update(step);
            } else {
                let mut step = CommittedRunStep {
                    step_id: step_id.to_string(),
                    accepted_input: None,
                    outcome: None,
                    tool_receipts: Vec::new(),
                };
                update(&mut step);
                slice.steps.push(step);
            }
            slices[slice_index] = Arc::new(slice);
        } else {
            let mut step = CommittedRunStep {
                step_id: step_id.to_string(),
                accepted_input: None,
                outcome: None,
                tool_receipts: Vec::new(),
            };
            update(&mut step);
            slices.push(Arc::new(CommittedRunSlice::new(run_id, vec![step])));
        }
        Self {
            slices: slices.into(),
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
    pub run_slices: SessionHistory,
    #[serde(default)]
    pub committed_steps: CommittedStepLedger,
    #[serde(default)]
    pub skill_load_records: Vec<SkillLoadRecord>,
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
    pub fn loaded_skill_revision(
        &self,
        scope: &tools::SkillLoadScope,
        skill_name: &str,
    ) -> Option<&str> {
        self.skill_load_records
            .iter()
            .find(|record| &record.scope == scope && record.skill_name == skill_name)
            .map(|record| record.revision.as_str())
    }

    pub fn compare_and_record_skill(
        &mut self,
        scope: &tools::SkillLoadScope,
        skill_name: &str,
        revision: &str,
    ) -> tools::SkillLoadDecision {
        if let Some(record) = self
            .skill_load_records
            .iter_mut()
            .find(|record| &record.scope == scope && record.skill_name == skill_name)
        {
            if record.revision == revision {
                return tools::SkillLoadDecision::AlreadyLoaded;
            }
            record.revision = revision.to_string();
            return tools::SkillLoadDecision::Updated;
        }
        self.skill_load_records.push(SkillLoadRecord {
            scope: scope.clone(),
            skill_name: skill_name.to_string(),
            revision: revision.to_string(),
        });
        self.skill_load_records.sort_by(|left, right| {
            (&left.scope, &left.skill_name).cmp(&(&right.scope, &right.skill_name))
        });
        tools::SkillLoadDecision::Fresh
    }

    pub fn step_receipts(&self, run_id: &str, step_id: &str) -> Vec<StepReceipt> {
        self.run_slices.step_receipts(run_id, step_id)
    }

    pub fn tool_receipt(&self, mutation: &ToolReceiptMutation) -> Option<&ToolCallReceipt> {
        self.run_slices.tool_receipt(mutation)
    }

    pub fn advance_tool_receipt(
        &mut self,
        mutation: ToolReceiptMutation,
    ) -> Result<bool, crate::domain::ToolReceiptMutationError> {
        let (history, advanced) = self.run_slices.advance_tool_receipt(mutation)?;
        if advanced.changed {
            self.run_slices = history;
        }
        Ok(advanced.changed)
    }

    pub fn append_accepted_input(
        &mut self,
        run_id: &str,
        step_id: &str,
        accepted_input: AcceptedInputRecord,
    ) {
        let cursor = RunStepCursor {
            run_id: run_id.to_string(),
            step_id: step_id.to_string(),
        };
        self.run_slices = self
            .run_slices
            .append_accepted_input(run_id, step_id, accepted_input);
        if self
            .compact
            .as_ref()
            .is_some_and(|marker| marker.start_at.is_none())
        {
            self.compact.as_mut().expect("checked above").start_at = Some(cursor);
        }
    }

    pub fn accepted_input(&self, run_id: &str, step_id: &str) -> Option<&AcceptedInputRecord> {
        self.run_slices.accepted_input(run_id, step_id)
    }

    pub fn append_finalized_outcome(
        &mut self,
        run_id: &str,
        step_id: &str,
        outcome: FinalizedOutcomeRecord,
    ) {
        let cursor = RunStepCursor {
            run_id: run_id.to_string(),
            step_id: step_id.to_string(),
        };
        self.run_slices = self
            .run_slices
            .append_finalized_outcome(run_id, step_id, outcome);
        if self
            .compact
            .as_ref()
            .is_some_and(|marker| marker.start_at.is_none())
        {
            self.compact.as_mut().expect("checked above").start_at = Some(cursor);
        }
    }

    pub(crate) fn all_restore_steps(&self) -> Vec<RestoreStepSource> {
        self.run_slices
            .iter()
            .flat_map(|slice| {
                slice.steps.iter().map(|step| RestoreStepSource {
                    cursor: RunStepCursor {
                        run_id: slice.run_id.clone(),
                        step_id: step.step_id.clone(),
                    },
                    message_segments: step_message_segments(step),
                    tool_receipts: step.tool_receipts.clone(),
                    finalize_cause: step.outcome.as_ref().map(|outcome| outcome.finalize_cause),
                    duration_ms: step
                        .outcome
                        .as_ref()
                        .and_then(|outcome| outcome.duration_ms),
                })
            })
            .collect()
    }

    pub(crate) fn restore_steps_from_marker(&self) -> Vec<RestoreStepSource> {
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
                    steps.push(RestoreStepSource {
                        cursor: RunStepCursor {
                            run_id: slice.run_id.clone(),
                            step_id: step.step_id.clone(),
                        },
                        message_segments: step_message_segments(step),
                        tool_receipts: step.tool_receipts.clone(),
                        finalize_cause: step.outcome.as_ref().map(|outcome| outcome.finalize_cause),
                        duration_ms: step
                            .outcome
                            .as_ref()
                            .and_then(|outcome| outcome.duration_ms),
                    });
                }
            }
        }
        steps
    }

    pub fn all_persisted_steps(&self) -> Vec<(RunStepCursor, Vec<Message>)> {
        self.all_restore_steps()
            .into_iter()
            .map(|step| {
                (
                    step.cursor,
                    step.message_segments
                        .iter()
                        .flat_map(|segment| segment.iter().cloned())
                        .collect(),
                )
            })
            .collect()
    }

    pub fn flattened_steps_from_marker(&self) -> Vec<(RunStepCursor, Vec<Message>)> {
        self.restore_steps_from_marker()
            .into_iter()
            .map(|step| {
                (
                    step.cursor,
                    step.message_segments
                        .iter()
                        .flat_map(|segment| segment.iter().cloned())
                        .collect(),
                )
            })
            .collect()
    }

    pub fn visible_history(&self) -> SessionHistory {
        let start_at = self
            .compact
            .as_ref()
            .and_then(|marker| marker.start_at.as_ref());
        let mut visible = self.compact.is_none();
        let mut slices = Vec::new();
        for slice in &self.run_slices {
            let mut visible_steps = Vec::new();
            for step in &slice.steps {
                if !visible
                    && start_at.is_some_and(|cursor| {
                        cursor.run_id == slice.run_id && cursor.step_id == step.step_id
                    })
                {
                    visible = true;
                }
                if visible {
                    visible_steps.push(step.clone());
                }
            }
            if !visible_steps.is_empty() {
                slices.push(CommittedRunSlice::new(slice.run_id.clone(), visible_steps));
            }
        }
        SessionHistory::from_slices(slices)
    }

    pub fn visible_message_steps(&self) -> Vec<CommittedStepMessages> {
        self.visible_history()
            .iter()
            .flat_map(|slice| slice.steps.iter())
            .flat_map(|step| {
                step.accepted_input
                    .iter()
                    .map(|input| input.messages.clone())
                    .chain(step.outcome.iter().map(|outcome| outcome.messages.clone()))
            })
            .collect()
    }

    pub fn structured_messages(&self) -> Vec<Message> {
        self.visible_message_steps()
            .into_iter()
            .flat_map(|messages| messages.iter().cloned().collect::<Vec<_>>())
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
            run_slices: Default::default(),
            committed_steps: Default::default(),
            skill_load_records: Vec::new(),
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
struct V2VersionedEnvelope {
    #[allow(dead_code)]
    schema_version: u32,
    #[serde(flatten)]
    session: V2CanonicalSession,
}

#[derive(Debug, Deserialize)]
struct V2CanonicalSession {
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
    compact: Option<ActiveCompactMarker>,
    #[serde(default)]
    run_slices: Vec<V2CommittedRunSlice>,
    #[serde(default)]
    committed_steps: Vec<CommittedStep>,
}

#[derive(Debug, Deserialize)]
struct V2CommittedRunSlice {
    run_id: String,
    #[serde(default)]
    steps: Vec<V2CommittedRunStep>,
}

#[derive(Debug, Deserialize)]
struct V2CommittedRunStep {
    step_id: String,
    #[serde(default)]
    accepted_input: Option<AcceptedInputRecord>,
    #[serde(default)]
    outcome: Option<Vec<Message>>,
}

impl From<V2CanonicalSession> for CanonicalSession {
    fn from(session: V2CanonicalSession) -> Self {
        Self {
            id: session.id,
            chats: session.chats,
            created_at: session.created_at,
            updated_at: session.updated_at,
            metadata: session.metadata,
            tasks: session.tasks,
            workspace: session.workspace,
            revision: session.revision,
            compact: session.compact,
            run_slices: session
                .run_slices
                .into_iter()
                .map(|slice| {
                    CommittedRunSlice::new(
                        slice.run_id,
                        slice
                            .steps
                            .into_iter()
                            .map(|step| CommittedRunStep {
                                step_id: step.step_id,
                                accepted_input: step.accepted_input,
                                outcome: step.outcome.map(FinalizedOutcomeRecord::compatibility),
                                tool_receipts: Vec::new(),
                            })
                            .collect(),
                    )
                })
                .collect(),
            committed_steps: session.committed_steps.into(),
            skill_load_records: Vec::new(),
        }
    }
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
pub(super) mod task_snapshot_state {
    use super::{SnapshotState, TaskSnapshot, Value};
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub(in crate::domain::session) fn serialize<S>(
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

    pub(in crate::domain::session) fn deserialize<'de, D>(
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
            Some(5) | Some(4) | Some(3) => {
                let envelope: VersionedEnvelope = serde_json::from_value(value)
                    .map_err(|error| SessionCodecError::InvalidJson(error.to_string()))?;
                Ok(DecodedSession {
                    session: envelope.session,
                    upgraded_from_legacy: true,
                })
            }
            Some(2) => {
                let envelope: V2VersionedEnvelope = serde_json::from_value(value)
                    .map_err(|error| SessionCodecError::InvalidJson(error.to_string()))?;
                Ok(DecodedSession {
                    session: envelope.session.into(),
                    upgraded_from_legacy: true,
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
                        run_slices: run_slices.into(),
                        committed_steps: legacy.committed_steps.into(),
                        skill_load_records: Vec::new(),
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
                        AcceptedInputRecord::new(segment.messages.clone(), run_id.clone(), 0),
                    ),
                    super::SegmentKind::Compact => CommittedRunStep::compatibility_outcome_only(
                        step_id,
                        segment.messages.clone(),
                    ),
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
                run_slices: run_slices.into(),
                committed_steps: Default::default(),
                skill_load_records: Vec::new(),
            },
            upgraded_from_legacy: true,
        })
    }
}
