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

pub fn reduce_compact_facts(
    batch: CompactFactBatch,
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
