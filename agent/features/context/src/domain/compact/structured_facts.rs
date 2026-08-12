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
