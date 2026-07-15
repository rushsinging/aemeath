use serde::{Deserialize, Serialize};

use crate::{InteractionRequestId, RunId};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InteractionRequest {
    pub id: InteractionRequestId,
    pub run_id: RunId,
    pub body: InteractionRequestBody,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum InteractionRequestBody {
    UserQuestions(Vec<UserQuestion>),
    ToolApproval(ToolApprovalPrompt),
    PlanApproval(PlanApprovalPrompt),
    HardPause(StuckDiagnostic),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserQuestion {
    pub prompt: String,
    pub options: Vec<String>,
    pub allow_multi: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolApprovalPrompt {
    pub tool_name: String,
    pub args_summary: String,
    pub risk_level: RiskLevel,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanApprovalPrompt {
    pub plan_title: String,
    pub steps: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StuckDiagnostic {
    pub reason: String,
    pub recent_actions: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RiskLevel {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ApprovalDecision {
    Approve,
    Deny { reason: Option<String> },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum InteractionReply {
    UserQuestions(Vec<UserAnswer>),
    ToolApproval(ApprovalDecision),
    PlanApproval(ApprovalDecision),
    HardPauseContinue,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserAnswer(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum InteractionCancelReason {
    UserCancelled,
    RunCancelled,
    ClientDisconnected,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum InteractionReplyError {
    VariantMismatch,
    AnswerCountMismatch,
    InvalidAnswer(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum InteractionCommandOutcome {
    Accepted,
    NotFound,
    AlreadyCompleted,
    InvalidReply(InteractionReplyError),
    RunCancelling,
}
