use super::{SessionId, ToolOutcomeKind};
use sdk::{RunId, RunStepId};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CleanupConfirmation {
    Confirmed,
    Unconfirmed,
    NotApplicable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolCallIdentity {
    pub session_id: SessionId,
    pub run_id: RunId,
    pub step_id: RunStepId,
    pub runtime_call_id: String,
    pub provider_call_id: Option<String>,
    pub tool_name: String,
    pub call_index: usize,
    pub agent: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolTerminalReceipt {
    pub outcome: ToolOutcomeKind,
    pub safe_reason: String,
    possible_side_effects: Vec<String>,
    unfinished_call_ids: Vec<String>,
    pub cleanup: CleanupConfirmation,
}

impl ToolTerminalReceipt {
    pub fn new(
        outcome: ToolOutcomeKind,
        safe_reason: impl Into<String>,
        cleanup: CleanupConfirmation,
    ) -> Self {
        Self {
            outcome,
            safe_reason: safe_reason.into(),
            possible_side_effects: Vec::new(),
            unfinished_call_ids: Vec::new(),
            cleanup,
        }
    }

    pub fn with_possible_side_effect(mut self, effect: impl Into<String>) -> Self {
        self.possible_side_effects.push(effect.into());
        self
    }

    pub fn with_unfinished_call(mut self, call_id: impl Into<String>) -> Self {
        self.unfinished_call_ids.push(call_id.into());
        self
    }

    pub fn possible_side_effects(&self) -> &[String] {
        &self.possible_side_effects
    }

    pub fn unfinished_call_ids(&self) -> &[String] {
        &self.unfinished_call_ids
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ToolCallState {
    Pending,
    Running,
    Terminal(ToolTerminalReceipt),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolCallReceipt {
    pub identity: ToolCallIdentity,
    pub input_preview: String,
    pub state: ToolCallState,
}

impl ToolCallReceipt {
    pub fn pending(identity: ToolCallIdentity, input_preview: impl Into<String>) -> Self {
        Self {
            identity,
            input_preview: input_preview.into(),
            state: ToolCallState::Pending,
        }
    }

    pub fn advance(
        self,
        mutation: ToolReceiptMutation,
    ) -> Result<ToolReceiptMutationReceipt, ToolReceiptMutationError> {
        if self.identity != mutation.identity {
            return Err(ToolReceiptMutationError::IdentityMismatch);
        }

        let next = match (&self.state, mutation.next) {
            (ToolCallState::Pending, ToolCallState::Running) => ToolCallState::Running,
            (
                ToolCallState::Pending | ToolCallState::Running,
                ToolCallState::Terminal(terminal),
            ) => ToolCallState::Terminal(terminal),
            (ToolCallState::Running, ToolCallState::Running) => {
                return Ok(ToolReceiptMutationReceipt {
                    receipt: self,
                    changed: false,
                });
            }
            (ToolCallState::Terminal(current), ToolCallState::Terminal(next))
                if current == &next =>
            {
                return Ok(ToolReceiptMutationReceipt {
                    receipt: self,
                    changed: false,
                });
            }
            (ToolCallState::Terminal(_), _) => {
                return Err(ToolReceiptMutationError::TerminalStateConflict {
                    call_id: self.identity.runtime_call_id.clone(),
                });
            }
            (current, next) if current == &next => {
                return Ok(ToolReceiptMutationReceipt {
                    receipt: self,
                    changed: false,
                });
            }
            _ => return Err(ToolReceiptMutationError::InvalidTransition),
        };

        Ok(ToolReceiptMutationReceipt {
            receipt: Self {
                state: next,
                ..self
            },
            changed: true,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolReceiptMutation {
    pub identity: ToolCallIdentity,
    pub input_preview: Option<String>,
    pub next: ToolCallState,
}

impl ToolReceiptMutation {
    pub fn pending(identity: ToolCallIdentity, input_preview: impl Into<String>) -> Self {
        Self {
            identity,
            input_preview: Some(input_preview.into()),
            next: ToolCallState::Pending,
        }
    }

    pub fn running(identity: ToolCallIdentity) -> Self {
        Self {
            identity,
            input_preview: None,
            next: ToolCallState::Running,
        }
    }

    pub fn terminal(identity: ToolCallIdentity, terminal: ToolTerminalReceipt) -> Self {
        Self {
            identity,
            input_preview: None,
            next: ToolCallState::Terminal(terminal),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolReceiptMutationReceipt {
    pub receipt: ToolCallReceipt,
    pub changed: bool,
}

#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum ToolReceiptMutationError {
    #[error("Tool receipt 对应 Session 不存在：{0}")]
    SessionNotFound(SessionId),
    #[error("Tool receipt 持久化失败：{0}")]
    Storage(String),
    #[error("Tool receipt identity 不匹配")]
    IdentityMismatch,
    #[error("Tool receipt 状态转换非法")]
    InvalidTransition,
    #[error("Tool receipt 已终态，禁止覆盖：{call_id}")]
    TerminalStateConflict { call_id: String },
}
