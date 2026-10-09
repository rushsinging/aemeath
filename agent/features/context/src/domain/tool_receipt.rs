use super::{SessionId, ToolOutcomeKindData};
use serde::{Deserialize, Serialize};
use share::error::DomainError;
use share::ids::{RunId, RunStepId};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CleanupConfirmation {
    Confirmed,
    Unconfirmed,
    NotApplicable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolCallIdentityData {
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
pub struct ToolTerminalReceiptData {
    pub outcome: ToolOutcomeKindData,
    pub safe_reason: String,
    possible_side_effects: Vec<String>,
    unfinished_call_ids: Vec<String>,
    pub cleanup: CleanupConfirmation,
}

impl ToolTerminalReceiptData {
    pub fn new(
        outcome: ToolOutcomeKindData,
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
    /// 已转后台：前台等待结束但执行未完成，terminal 推迟到真实完成时写入。
    /// step finalize 允许携带该状态挂起；恢复时按后台进程失效语义对账。
    Backgrounded,
    Terminal(ToolTerminalReceiptData),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolCallReceiptData {
    pub identity: ToolCallIdentityData,
    pub input_preview: String,
    pub state: ToolCallState,
}

impl ToolCallReceiptData {
    pub fn to_step_receipt(&self) -> Option<super::StepReceiptData> {
        let ToolCallState::Terminal(terminal) = &self.state else {
            return None;
        };
        let call_id = self
            .identity
            .provider_call_id
            .as_deref()
            .unwrap_or(&self.identity.runtime_call_id);
        let receipt = if self.identity.agent {
            super::StepReceiptData::agent(call_id, self.identity.call_index, terminal.outcome)
        } else {
            super::StepReceiptData::tool(call_id, self.identity.call_index, terminal.outcome)
        }
        .with_summary(terminal.safe_reason.clone());
        let receipt = terminal
            .possible_side_effects()
            .iter()
            .fold(receipt, |receipt, effect| {
                receipt.with_possible_side_effect(effect.clone())
            });
        Some(
            terminal
                .unfinished_call_ids()
                .iter()
                .fold(receipt, |receipt, call_id| {
                    receipt.with_unfinished_call(call_id.clone())
                }),
        )
    }

    pub fn pending(identity: ToolCallIdentityData, input_preview: impl Into<String>) -> Self {
        Self {
            identity,
            input_preview: input_preview.into(),
            state: ToolCallState::Pending,
        }
    }

    pub fn advance(
        self,
        mutation: ToolReceiptMutationData,
    ) -> Result<ToolReceiptMutationReceiptData, DomainError> {
        if self.identity != mutation.identity {
            return Err(
                DomainError::invalid("context", "Tool receipt identity 不匹配")
                    .with_code(crate::error_codes::IDENTITY_MISMATCH),
            );
        }

        let next = match (&self.state, mutation.next) {
            (ToolCallState::Pending, ToolCallState::Running) => ToolCallState::Running,
            (
                ToolCallState::Pending | ToolCallState::Running,
                ToolCallState::Terminal(terminal),
            ) => ToolCallState::Terminal(terminal),
            (ToolCallState::Running, ToolCallState::Backgrounded) => ToolCallState::Backgrounded,
            (ToolCallState::Backgrounded, ToolCallState::Terminal(terminal)) => {
                ToolCallState::Terminal(terminal)
            }
            (ToolCallState::Running, ToolCallState::Running) => {
                return Ok(ToolReceiptMutationReceiptData {
                    receipt: self,
                    changed: false,
                });
            }
            (ToolCallState::Terminal(current), ToolCallState::Terminal(next))
                if current == &next =>
            {
                return Ok(ToolReceiptMutationReceiptData {
                    receipt: self,
                    changed: false,
                });
            }
            (ToolCallState::Terminal(_), _) => {
                let call_id = self.identity.runtime_call_id.clone();
                return Err(DomainError::invalid(
                    "context",
                    format!("Tool receipt 已终态，禁止覆盖：{call_id}"),
                )
                .with_code(crate::error_codes::TERMINAL_STATE_CONFLICT));
            }
            (current, next) if current == &next => {
                return Ok(ToolReceiptMutationReceiptData {
                    receipt: self,
                    changed: false,
                });
            }
            _ => {
                return Err(DomainError::invalid("context", "Tool receipt 状态转换非法")
                    .with_code(crate::error_codes::INVALID_TRANSITION))
            }
        };

        Ok(ToolReceiptMutationReceiptData {
            receipt: Self {
                state: next,
                ..self
            },
            changed: true,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolReceiptMutationData {
    pub identity: ToolCallIdentityData,
    pub input_preview: Option<String>,
    pub next: ToolCallState,
}

impl ToolReceiptMutationData {
    pub fn pending(identity: ToolCallIdentityData, input_preview: impl Into<String>) -> Self {
        Self {
            identity,
            input_preview: Some(input_preview.into()),
            next: ToolCallState::Pending,
        }
    }

    pub fn running(identity: ToolCallIdentityData) -> Self {
        Self {
            identity,
            input_preview: None,
            next: ToolCallState::Running,
        }
    }

    pub fn backgrounded(identity: ToolCallIdentityData) -> Self {
        Self {
            identity,
            input_preview: None,
            next: ToolCallState::Backgrounded,
        }
    }

    pub fn terminal(identity: ToolCallIdentityData, terminal: ToolTerminalReceiptData) -> Self {
        Self {
            identity,
            input_preview: None,
            next: ToolCallState::Terminal(terminal),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolReceiptMutationReceiptData {
    pub receipt: ToolCallReceiptData,
    pub changed: bool,
}
