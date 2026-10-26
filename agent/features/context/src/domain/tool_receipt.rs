use super::{SessionId, ToolOutcomeKindData};
use serde::{Deserialize, Serialize};
use share::ids::{RunId, RunStepId};

/// 清理确认状态：与 `tools` 的同名 PL 同一语义，直接复用避免双类型转换。
pub use tools::CleanupConfirmation;

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

/// 调用 id 回退规则单一实现：provider 优先、runtime 兜底。
/// to_step_receipt 与 resume 修复共用，规则变更只改此处。
pub(crate) fn effective_call_id(identity: &ToolCallIdentityData) -> &str {
    identity
        .provider_call_id
        .as_deref()
        .unwrap_or(&identity.runtime_call_id)
}

impl ToolCallReceiptData {
    pub fn to_step_receipt(&self) -> Option<super::RunStepReceiptData> {
        let ToolCallState::Terminal(terminal) = &self.state else {
            return None;
        };
        let call_id = effective_call_id(&self.identity);
        Some(super::RunStepReceiptData {
            call_id: call_id.to_string(),
            index: self.identity.call_index,
            outcome: terminal.outcome,
            agent: self.identity.agent,
            summary: Some(terminal.safe_reason.clone()),
            artifact_refs: Vec::new(),
            possible_side_effects: terminal.possible_side_effects().to_vec(),
            unfinished_call_ids: terminal.unfinished_call_ids().to_vec(),
        })
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
    ) -> Result<ToolReceiptMutationReceiptData, ToolReceiptMutationError> {
        if self.identity != mutation.identity {
            return Err(ToolReceiptMutationError::IdentityMismatch);
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
                return Err(ToolReceiptMutationError::TerminalStateConflict {
                    call_id: self.identity.runtime_call_id.clone(),
                });
            }
            (current, next) if current == &next => {
                return Ok(ToolReceiptMutationReceiptData {
                    receipt: self,
                    changed: false,
                });
            }
            _ => return Err(ToolReceiptMutationError::InvalidTransition),
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
