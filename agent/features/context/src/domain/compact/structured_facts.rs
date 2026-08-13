use super::{CheckpointError, CheckpointSections, ContinuationCheckpoint, ContinuationStatus};
use serde::{Deserialize, Deserializer, Serialize};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompactFactSource {
    MainUser,
    AssistantReport,
    ToolInvocation,
    ToolResult,
    SystemGenerated,
    SubagentInstruction,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConstraintScope {
    Session,
    Task,
    Phase,
    ToolCall,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConstraintLifecycle {
    Persistent,
    UntilTaskEnd,
    UntilPhaseEnd,
    UntilToolCallEnd,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConstraintAction {
    Grant,
    Restrict,
    Revoke,
    Supersede,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompactFactKind {
    Constraint,
    Objective,
    CommittedFact,
    WorkingSet,
    Risk,
    ResumeCandidate,
    Revalidation,
    Milestone,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConstraintMetadata {
    scope: ConstraintScope,
    lifecycle: ConstraintLifecycle,
    action: ConstraintAction,
}

impl ConstraintMetadata {
    pub const fn new(
        scope: ConstraintScope,
        lifecycle: ConstraintLifecycle,
        action: ConstraintAction,
    ) -> Self {
        Self {
            scope,
            lifecycle,
            action,
        }
    }

    pub const fn scope(&self) -> ConstraintScope {
        self.scope
    }

    pub const fn lifecycle(&self) -> ConstraintLifecycle {
        self.lifecycle
    }

    pub const fn action(&self) -> ConstraintAction {
        self.action
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CompactFact {
    sequence: u64,
    source: CompactFactSource,
    kind: CompactFactKind,
    text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    constraint: Option<ConstraintMetadata>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CompactFactWire {
    sequence: u64,
    source: CompactFactSource,
    kind: CompactFactKind,
    text: String,
    constraint: Option<ConstraintMetadata>,
}

impl<'de> Deserialize<'de> for CompactFact {
    fn deserialize<DeserializerType>(
        deserializer: DeserializerType,
    ) -> Result<Self, DeserializerType::Error>
    where
        DeserializerType: Deserializer<'de>,
    {
        use serde::de::Error;

        let wire = CompactFactWire::deserialize(deserializer)?;
        Self::new(
            wire.sequence,
            wire.source,
            wire.kind,
            wire.text,
            wire.constraint,
        )
        .map_err(DeserializerType::Error::custom)
    }
}

impl CompactFact {
    pub fn new(
        sequence: u64,
        source: CompactFactSource,
        kind: CompactFactKind,
        text: impl Into<String>,
        constraint: Option<ConstraintMetadata>,
    ) -> Result<Self, CompactFactError> {
        let text = text.into();
        if text.trim().is_empty() {
            return Err(CompactFactError::EmptyText);
        }
        match (kind, constraint.is_some()) {
            (CompactFactKind::Constraint, false) => {
                return Err(CompactFactError::MissingConstraintMetadata)
            }
            (CompactFactKind::Constraint, true) | (_, false) => {}
            (_, true) => return Err(CompactFactError::UnexpectedConstraintMetadata),
        }
        Ok(Self {
            sequence,
            source,
            kind,
            text,
            constraint,
        })
    }

    pub fn constraint(
        sequence: u64,
        source: CompactFactSource,
        text: impl Into<String>,
        constraint: ConstraintMetadata,
    ) -> Result<Self, CompactFactError> {
        Self::new(
            sequence,
            source,
            CompactFactKind::Constraint,
            text,
            Some(constraint),
        )
    }

    pub const fn sequence(&self) -> u64 {
        self.sequence
    }

    pub const fn source(&self) -> CompactFactSource {
        self.source
    }

    pub const fn kind(&self) -> CompactFactKind {
        self.kind
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub const fn constraint_metadata(&self) -> Option<&ConstraintMetadata> {
        self.constraint.as_ref()
    }

    pub fn normalize_scope(mut self) -> Self {
        if self.source != CompactFactSource::MainUser {
            if let Some(constraint) = &mut self.constraint {
                if constraint.scope == ConstraintScope::Session {
                    constraint.scope = ConstraintScope::Unknown;
                    constraint.lifecycle = ConstraintLifecycle::Unknown;
                }
            }
        }
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompactTaskBatchStatus {
    Active,
    Paused,
    Archived,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompactTaskStatus {
    Pending,
    InProgress,
    Completed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompactTaskItem {
    sequence: u64,
    subject: String,
    status: CompactTaskStatus,
    blocked_by_sequences: Vec<u64>,
}

impl CompactTaskItem {
    pub fn pending(
        sequence: u64,
        subject: impl Into<String>,
        blocked_by_sequences: Vec<u64>,
    ) -> Self {
        Self::new(
            sequence,
            subject,
            CompactTaskStatus::Pending,
            blocked_by_sequences,
        )
    }

    pub fn in_progress(sequence: u64, subject: impl Into<String>) -> Self {
        Self::new(sequence, subject, CompactTaskStatus::InProgress, Vec::new())
    }

    pub fn completed(sequence: u64, subject: impl Into<String>) -> Self {
        Self::new(sequence, subject, CompactTaskStatus::Completed, Vec::new())
    }

    pub fn new(
        sequence: u64,
        subject: impl Into<String>,
        status: CompactTaskStatus,
        blocked_by_sequences: Vec<u64>,
    ) -> Self {
        Self {
            sequence,
            subject: subject.into(),
            status,
            blocked_by_sequences,
        }
    }

    pub const fn sequence(&self) -> u64 {
        self.sequence
    }

    pub fn subject(&self) -> &str {
        &self.subject
    }

    pub const fn status(&self) -> &CompactTaskStatus {
        &self.status
    }

    pub fn blocked_by_sequences(&self) -> &[u64] {
        &self.blocked_by_sequences
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompactTaskSnapshot {
    revision: u64,
    batch_id: u64,
    batch_summary: String,
    batch_status: CompactTaskBatchStatus,
    items: Vec<CompactTaskItem>,
}

impl CompactTaskSnapshot {
    pub fn active(
        revision: u64,
        batch_id: u64,
        batch_summary: impl Into<String>,
        items: Vec<CompactTaskItem>,
    ) -> Self {
        Self::new(
            revision,
            batch_id,
            batch_summary,
            CompactTaskBatchStatus::Active,
            items,
        )
    }

    pub fn paused(
        revision: u64,
        batch_id: u64,
        batch_summary: impl Into<String>,
        items: Vec<CompactTaskItem>,
    ) -> Self {
        Self::new(
            revision,
            batch_id,
            batch_summary,
            CompactTaskBatchStatus::Paused,
            items,
        )
    }

    pub fn new(
        revision: u64,
        batch_id: u64,
        batch_summary: impl Into<String>,
        batch_status: CompactTaskBatchStatus,
        mut items: Vec<CompactTaskItem>,
    ) -> Self {
        items.sort_by_key(CompactTaskItem::sequence);
        Self {
            revision,
            batch_id,
            batch_summary: batch_summary.into(),
            batch_status,
            items,
        }
    }

    pub const fn revision(&self) -> u64 {
        self.revision
    }

    pub const fn batch_id(&self) -> u64 {
        self.batch_id
    }

    pub fn batch_summary(&self) -> &str {
        &self.batch_summary
    }

    pub const fn batch_status(&self) -> &CompactTaskBatchStatus {
        &self.batch_status
    }

    pub fn items(&self) -> &[CompactTaskItem] {
        &self.items
    }

    pub fn render_companion(&self) -> String {
        let completed = self
            .items
            .iter()
            .filter(|item| item.status == CompactTaskStatus::Completed)
            .count();
        let mut lines = vec![format!(
            "Batch #{} — Tasks: {completed}/{}",
            self.batch_id,
            self.items.len()
        )];
        for item in &self.items {
            let icon = match item.status {
                CompactTaskStatus::Pending => "□",
                CompactTaskStatus::InProgress => "■",
                CompactTaskStatus::Completed => "✓",
            };
            let blocked_by = if item.blocked_by_sequences.is_empty() {
                String::new()
            } else {
                format!(
                    " (blocked by {})",
                    item.blocked_by_sequences
                        .iter()
                        .map(|sequence| format!("#{sequence}"))
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            };
            lines.push(format!(
                "{icon} [task:{} seq:{}] {}{blocked_by}",
                item.sequence, item.sequence, item.subject
            ));
        }
        lines.join("\n")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompactFactBatch {
    facts: Vec<CompactFact>,
}

impl CompactFactBatch {
    pub fn new(facts: Vec<CompactFact>) -> Self {
        Self { facts }
    }

    pub fn facts(&self) -> &[CompactFact] {
        &self.facts
    }

    pub fn into_facts(self) -> Vec<CompactFact> {
        self.facts
    }
}

pub fn reconcile_checkpoint_with_task_snapshot(
    checkpoint: ContinuationCheckpoint,
    task_snapshot: Option<&CompactTaskSnapshot>,
) -> Result<ContinuationCheckpoint, CheckpointError> {
    let Some(task_snapshot) =
        task_snapshot.filter(|snapshot| task_snapshot_is_authoritative(snapshot))
    else {
        return Ok(checkpoint);
    };
    let mut wire = checkpoint.to_wire();
    let in_progress = task_snapshot
        .items
        .iter()
        .find(|item| item.status == CompactTaskStatus::InProgress)
        .expect("authoritative task snapshot requires exactly one in-progress item");
    let completed_subjects = task_snapshot
        .items
        .iter()
        .filter(|item| item.status == CompactTaskStatus::Completed)
        .map(|item| normalize_for_comparison(item.subject()))
        .collect::<Vec<_>>();

    wire.resume_cursor.next_action = in_progress.subject().to_string();
    wire.uncommitted_working_set
        .retain(|line| !contradicts_completed_work(line, &completed_subjects));
    wire.open_decisions_and_risks
        .retain(|line| !contradicts_completed_work(line, &completed_subjects));
    wire.required_revalidation
        .retain(|line| !contradicts_completed_work(line, &completed_subjects));
    for item in task_snapshot
        .items
        .iter()
        .filter(|item| item.status == CompactTaskStatus::Pending)
    {
        let dependency = if item.blocked_by_sequences().is_empty() {
            String::new()
        } else {
            format!(
                " (blocked by {})",
                item.blocked_by_sequences()
                    .iter()
                    .map(|sequence| format!("task {sequence}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        };
        wire.uncommitted_working_set.push(format!(
            "Pending task {}: {}{dependency}",
            item.sequence(),
            item.subject()
        ));
    }
    ContinuationCheckpoint::try_from(wire)
}

fn task_snapshot_is_authoritative(snapshot: &CompactTaskSnapshot) -> bool {
    snapshot.batch_status == CompactTaskBatchStatus::Active
        && !snapshot.batch_summary.trim().is_empty()
        && snapshot
            .items
            .iter()
            .filter(|item| item.status == CompactTaskStatus::InProgress)
            .count()
            == 1
}

pub fn reduce_compact_facts(
    batch: CompactFactBatch,
) -> Result<ContinuationCheckpoint, CheckpointError> {
    reduce_compact_facts_with_task_snapshot(batch, None)
}

pub fn reduce_compact_facts_with_task_snapshot(
    batch: CompactFactBatch,
    task_snapshot: Option<&CompactTaskSnapshot>,
) -> Result<ContinuationCheckpoint, CheckpointError> {
    let mut indexed_facts = batch
        .into_facts()
        .into_iter()
        .enumerate()
        .collect::<Vec<_>>();
    indexed_facts.sort_by_key(|(original_index, fact)| (fact.sequence(), *original_index));

    let mut immutable_constraints = Vec::new();
    let mut current_objective = None;
    let mut committed_facts = Vec::new();
    let mut working_set = Vec::new();
    let mut risks = Vec::new();
    let mut next_action = None;
    let mut revalidation = Vec::new();
    let mut milestones = Vec::new();

    for (_, fact) in indexed_facts {
        let fact = fact.normalize_scope();
        match fact.kind() {
            CompactFactKind::Constraint => {
                let metadata = fact
                    .constraint_metadata()
                    .expect("validated constraint fact must have metadata");
                if fact.source() == CompactFactSource::MainUser
                    && metadata.scope() == ConstraintScope::Session
                    && metadata.lifecycle() == ConstraintLifecycle::Persistent
                {
                    match metadata.action() {
                        ConstraintAction::Grant
                        | ConstraintAction::Restrict
                        | ConstraintAction::Supersede => {
                            if metadata.action() == ConstraintAction::Supersede {
                                immutable_constraints.clear();
                            }
                            immutable_constraints.push(as_fact_bullet(fact.text()));
                        }
                        ConstraintAction::Revoke => immutable_constraints.clear(),
                    }
                } else {
                    risks.push(format!(
                        "- scope unverified ({:?}/{:?}): {}",
                        metadata.scope(),
                        metadata.lifecycle(),
                        fact.text()
                    ));
                }
            }
            CompactFactKind::Objective if fact.source() == CompactFactSource::MainUser => {
                current_objective = Some(as_fact_bullet(fact.text()));
            }
            CompactFactKind::Objective => {
                risks.push(format!("- unverified objective: {}", fact.text()));
            }
            CompactFactKind::CommittedFact if fact.source() == CompactFactSource::ToolResult => {
                committed_facts.push(as_fact_bullet(fact.text()));
            }
            CompactFactKind::CommittedFact => {
                risks.push(format!("- unverified fact: {}", fact.text()));
            }
            CompactFactKind::WorkingSet => working_set.push(as_fact_bullet(fact.text())),
            CompactFactKind::Risk => risks.push(as_fact_bullet(fact.text())),
            CompactFactKind::ResumeCandidate if fact.source() == CompactFactSource::MainUser => {
                next_action = Some(fact.text().to_string());
            }
            CompactFactKind::ResumeCandidate => {
                risks.push(format!("- unverified next action: {}", fact.text()));
            }
            CompactFactKind::Revalidation => revalidation.push(as_fact_bullet(fact.text())),
            CompactFactKind::Milestone => milestones.push(as_fact_bullet(fact.text())),
        }
    }

    if let Some(task_snapshot) =
        task_snapshot.filter(|snapshot| task_snapshot_is_authoritative(snapshot))
    {
        let in_progress = task_snapshot
            .items
            .iter()
            .find(|item| item.status == CompactTaskStatus::InProgress)
            .expect("active task reconciliation requires exactly one in-progress item");
        next_action = Some(in_progress.subject().to_string());

        let completed_subjects = task_snapshot
            .items
            .iter()
            .filter(|item| item.status == CompactTaskStatus::Completed)
            .map(|item| normalize_for_comparison(item.subject()))
            .collect::<Vec<_>>();
        working_set.retain(|line| !contradicts_completed_work(line, &completed_subjects));
        risks.retain(|line| !contradicts_completed_work(line, &completed_subjects));
        revalidation.retain(|line| !contradicts_completed_work(line, &completed_subjects));

        for item in task_snapshot
            .items
            .iter()
            .filter(|item| item.status == CompactTaskStatus::Pending)
        {
            let dependency = if item.blocked_by_sequences().is_empty() {
                String::new()
            } else {
                format!(
                    " (blocked by {})",
                    item.blocked_by_sequences()
                        .iter()
                        .map(|sequence| format!("task {sequence}"))
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            };
            working_set.push(format!(
                "- Pending task {}: {}{dependency}",
                item.sequence(),
                item.subject()
            ));
        }
    }

    let current_objective = current_objective
        .unwrap_or_else(|| "- Revalidate the latest user objective before continuing.".to_string());
    let next_action = next_action
        .unwrap_or_else(|| "Revalidate the latest user objective before continuing.".to_string());
    let status = if current_objective.contains("Revalidate the latest user objective") {
        ContinuationStatus::WaitingForUser
    } else {
        ContinuationStatus::Continue
    };

    ContinuationCheckpoint::from_sections(CheckpointSections {
        immutable_constraints,
        current_objective: vec![current_objective],
        committed_facts,
        uncommitted_working_set: working_set,
        open_decisions_and_risks: risks,
        resume_cursor_lines: Vec::new(),
        next_action,
        required_revalidation: revalidation,
        archived_milestones: milestones,
        status,
        status_reason: Some(match status {
            ContinuationStatus::Continue => "a main-user objective remains active.".to_string(),
            ContinuationStatus::WaitingForUser => {
                "no active main-user objective could be established.".to_string()
            }
            ContinuationStatus::Completed => unreachable!("fact reducer never infers completion"),
        }),
    })
}

fn contradicts_completed_work(line: &str, completed_subjects: &[String]) -> bool {
    let normalized_line = normalize_for_comparison(line);
    let reports_missing_evidence = [
        "no reliable evidence",
        "no evidence",
        "not completed",
        "未完成",
        "无可靠证据",
        "没有可靠证据",
        "尚无证据",
    ]
    .iter()
    .any(|marker| normalized_line.contains(marker));
    reports_missing_evidence
        && (!completed_subjects.is_empty()
            && (normalized_line.contains("completed")
                || normalized_line.contains("完成")
                || completed_subjects.iter().any(|subject| {
                    subject
                        .split_whitespace()
                        .filter(|word| word.len() >= 4)
                        .any(|word| normalized_line.contains(word))
                })))
}

fn normalize_for_comparison(source: &str) -> String {
    source
        .trim_start_matches("- ")
        .to_lowercase()
        .replace(['`', '.', ',', ':', ';', '(', ')'], " ")
}

fn as_fact_bullet(source: &str) -> String {
    if source.trim_start().starts_with("- ") {
        source.trim().to_string()
    } else {
        format!("- {}", source.trim())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompactFactError {
    EmptyText,
    MissingConstraintMetadata,
    UnexpectedConstraintMetadata,
}

impl fmt::Display for CompactFactError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyText => write!(formatter, "compact fact text must not be empty"),
            Self::MissingConstraintMetadata => {
                write!(
                    formatter,
                    "constraint metadata is required for constraint facts"
                )
            }
            Self::UnexpectedConstraintMetadata => write!(
                formatter,
                "constraint metadata is only allowed for constraint facts"
            ),
        }
    }
}

#[cfg(test)]
#[path = "structured_facts_tests.rs"]
mod tests;
