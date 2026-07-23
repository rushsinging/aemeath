use super::change::ConversationChange;
use super::model::ConversationModel;

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub(crate) struct UiInteractionRequestId(String);

impl From<&str> for UiInteractionRequestId {
    fn from(value: &str) -> Self {
        Self(value.to_string())
    }
}

impl UiInteractionRequestId {
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub(crate) struct UiRunId(String);

impl From<&str> for UiRunId {
    fn from(value: &str) -> Self {
        Self(value.to_string())
    }
}

impl UiRunId {
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct InteractionRequest {
    pub(crate) request_id: UiInteractionRequestId,
    pub(crate) run_id: UiRunId,
    pub(crate) body: InteractionBody,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum InteractionBody {
    UserQuestions(Vec<UiUserQuestion>),
    ToolApproval(UiApprovalPrompt),
    PlanApproval(UiPlanApprovalPrompt),
    HardPause(UiStuckDiagnostic),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct UiUserQuestion {
    pub(crate) prompt: String,
    pub(crate) options: Vec<String>,
    pub(crate) allow_multi: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct UiApprovalPrompt {
    pub(crate) title: String,
    pub(crate) detail: String,
    pub(crate) risk: UiRiskLevel,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct UiPlanApprovalPrompt {
    pub(crate) title: String,
    pub(crate) steps: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct UiStuckDiagnostic {
    pub(crate) reason: String,
    pub(crate) recent_actions: Vec<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum UiRiskLevel {
    Low,
    Medium,
    High,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum InteractionDraft {
    UserAnswers(Vec<String>),
    Approval {
        approved: Option<bool>,
        reason: Option<String>,
    },
    HardPause {
        continue_run: bool,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum InteractionDraftAction {
    Approve,
    Deny { reason: Option<String> },
    SetUserAnswer { index: usize, answer: String },
    ContinueHardPause,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum UiInteractionReply {
    UserAnswers(Vec<String>),
    ToolApproval {
        approved: bool,
        reason: Option<String>,
    },
    PlanApproval {
        approved: bool,
        reason: Option<String>,
    },
    ContinueHardPause,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum InteractionCommandFailure {
    InvalidRequestId(String),
    InvalidReply(String),
    NotFound,
    AlreadyCompleted,
    RunCancelling,
    CommandClientUnavailable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum UiInteractionCancelReason {
    UserCancelled,
}

impl InteractionDraft {
    fn for_body(body: &InteractionBody) -> Self {
        match body {
            InteractionBody::UserQuestions(questions) => {
                Self::UserAnswers(vec![String::new(); questions.len()])
            }
            InteractionBody::ToolApproval(_) | InteractionBody::PlanApproval(_) => Self::Approval {
                approved: None,
                reason: None,
            },
            InteractionBody::HardPause(_) => Self::HardPause { continue_run: true },
        }
    }

    pub(crate) fn is_approved(&self) -> bool {
        matches!(
            self,
            Self::Approval {
                approved: Some(true),
                ..
            }
        )
    }

    fn apply(&mut self, action: InteractionDraftAction) -> bool {
        match (self, action) {
            (Self::Approval { approved, reason }, InteractionDraftAction::Approve) => {
                *approved = Some(true);
                *reason = None;
                true
            }
            (
                Self::Approval {
                    approved,
                    reason: draft_reason,
                },
                InteractionDraftAction::Deny { reason },
            ) => {
                *approved = Some(false);
                *draft_reason = reason;
                true
            }
            (
                Self::UserAnswers(answers),
                InteractionDraftAction::SetUserAnswer { index, answer },
            ) => {
                let Some(slot) = answers.get_mut(index) else {
                    return false;
                };
                *slot = answer;
                true
            }
            (Self::HardPause { continue_run }, InteractionDraftAction::ContinueHardPause) => {
                *continue_run = true;
                true
            }
            _ => false,
        }
    }

    fn reply(&self) -> Option<UiInteractionReply> {
        match self {
            Self::UserAnswers(answers) if answers.iter().all(|answer| !answer.is_empty()) => {
                Some(UiInteractionReply::UserAnswers(answers.clone()))
            }
            Self::Approval {
                approved: Some(true),
                ..
            } => Some(UiInteractionReply::ToolApproval {
                approved: true,
                reason: None,
            }),
            Self::Approval {
                approved: Some(false),
                reason,
            } => Some(UiInteractionReply::ToolApproval {
                approved: false,
                reason: reason.clone(),
            }),
            Self::HardPause { continue_run: true } => Some(UiInteractionReply::ContinueHardPause),
            _ => None,
        }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum InteractionPhase {
    Collecting,
    Confirming,
    ReplyPending,
    CancelPending,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct InteractionState {
    request: InteractionRequest,
    draft: InteractionDraft,
    phase: InteractionPhase,
}

impl InteractionState {
    fn new(request: InteractionRequest) -> Self {
        let draft = InteractionDraft::for_body(&request.body);
        Self {
            request,
            draft,
            phase: InteractionPhase::Collecting,
        }
    }

    pub(crate) fn request_id(&self) -> &UiInteractionRequestId {
        &self.request.request_id
    }

    pub(crate) fn run_id(&self) -> &UiRunId {
        &self.request.run_id
    }

    pub(crate) fn body(&self) -> &InteractionBody {
        &self.request.body
    }

    pub(crate) fn draft(&self) -> &InteractionDraft {
        &self.draft
    }

    pub(crate) fn phase(&self) -> InteractionPhase {
        self.phase
    }

    fn reply(&self) -> Option<UiInteractionReply> {
        match (&self.request.body, &self.draft) {
            (InteractionBody::UserQuestions(_), InteractionDraft::UserAnswers(answers))
                if answers.iter().all(|answer| !answer.is_empty()) =>
            {
                Some(UiInteractionReply::UserAnswers(answers.clone()))
            }
            (
                InteractionBody::ToolApproval(_),
                InteractionDraft::Approval {
                    approved: Some(approved),
                    reason,
                },
            ) => Some(UiInteractionReply::ToolApproval {
                approved: *approved,
                reason: reason.clone(),
            }),
            (
                InteractionBody::PlanApproval(_),
                InteractionDraft::Approval {
                    approved: Some(approved),
                    reason,
                },
            ) => Some(UiInteractionReply::PlanApproval {
                approved: *approved,
                reason: reason.clone(),
            }),
            (InteractionBody::HardPause(_), InteractionDraft::HardPause { continue_run: true }) => {
                Some(UiInteractionReply::ContinueHardPause)
            }
            _ => None,
        }
    }

    fn update_draft(&mut self, action: InteractionDraftAction) -> bool {
        if !matches!(
            self.phase,
            InteractionPhase::Collecting | InteractionPhase::Confirming
        ) {
            return false;
        }
        self.draft.apply(action)
    }

    fn confirm(&mut self) -> Option<UiInteractionReply> {
        if !matches!(
            self.phase,
            InteractionPhase::Collecting | InteractionPhase::Confirming
        ) {
            return None;
        }
        let reply = self.reply()?;
        self.phase = InteractionPhase::ReplyPending;
        Some(reply)
    }

    fn cancel(&mut self) -> bool {
        if !matches!(
            self.phase,
            InteractionPhase::Collecting | InteractionPhase::Confirming
        ) {
            return false;
        }
        self.phase = InteractionPhase::CancelPending;
        true
    }

    fn restore_collecting(&mut self) {
        self.phase = InteractionPhase::Collecting;
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub(crate) struct UiRunStepId(String);

impl From<&str> for UiRunStepId {
    fn from(value: &str) -> Self {
        Self(value.to_string())
    }
}

impl UiRunStepId {
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AgentRunStepPhase {
    Running,
    Completed,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct AgentRunStepState {
    step_id: UiRunStepId,
    phase: AgentRunStepPhase,
    tool_reference: Option<String>,
}

impl AgentRunStepState {
    fn new(step_id: UiRunStepId, tool_reference: Option<String>) -> Self {
        Self {
            step_id,
            phase: AgentRunStepPhase::Running,
            tool_reference,
        }
    }

    pub(crate) fn step_id(&self) -> &UiRunStepId {
        &self.step_id
    }

    pub(crate) fn phase(&self) -> AgentRunStepPhase {
        self.phase
    }

    pub(crate) fn tool_reference(&self) -> Option<&str> {
        self.tool_reference.as_deref()
    }

    fn complete(&mut self) -> bool {
        if self.phase != AgentRunStepPhase::Running {
            return false;
        }
        self.phase = AgentRunStepPhase::Completed;
        true
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AgentRunPhase {
    Running,
    AwaitingUser,
    Cancelling,
    Cancelled,
    Completed,
    Failed,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct AgentRunState {
    run_id: UiRunId,
    phase: AgentRunPhase,
    steps: Vec<AgentRunStepState>,
}

impl AgentRunState {
    pub(super) fn new(run_id: UiRunId) -> Self {
        Self {
            run_id,
            phase: AgentRunPhase::Running,
            steps: Vec::new(),
        }
    }

    pub(crate) fn run_id(&self) -> &UiRunId {
        &self.run_id
    }

    pub(crate) fn phase(&self) -> AgentRunPhase {
        self.phase
    }

    pub(crate) fn steps(&self) -> &[AgentRunStepState] {
        &self.steps
    }

    pub(super) fn start_step(
        &mut self,
        step_id: UiRunStepId,
        tool_reference: Option<String>,
    ) -> bool {
        if self.steps.iter().any(|step| step.step_id == step_id) {
            return false;
        }
        self.steps
            .push(AgentRunStepState::new(step_id, tool_reference));
        true
    }

    pub(super) fn complete_step(&mut self, step_id: &UiRunStepId) -> bool {
        self.steps
            .iter_mut()
            .find(|step| &step.step_id == step_id)
            .is_some_and(AgentRunStepState::complete)
    }

    pub(super) fn transition_to(&mut self, phase: AgentRunPhase) -> bool {
        let allowed = matches!(
            (self.phase, phase),
            (AgentRunPhase::Running, AgentRunPhase::AwaitingUser)
                | (AgentRunPhase::AwaitingUser, AgentRunPhase::Running)
                | (
                    AgentRunPhase::Running | AgentRunPhase::AwaitingUser,
                    AgentRunPhase::Cancelling
                )
                | (AgentRunPhase::Cancelling, AgentRunPhase::Cancelled)
                | (
                    AgentRunPhase::Running,
                    AgentRunPhase::Completed | AgentRunPhase::Failed
                )
        );
        if allowed {
            self.phase = phase;
        }
        allowed
    }
}

impl ConversationModel {
    pub(crate) fn active_interaction(&self) -> Option<&InteractionState> {
        self.active_interaction.as_ref()
    }

    pub(crate) fn show_interaction(
        &mut self,
        request: InteractionRequest,
    ) -> Vec<ConversationChange> {
        if let Some(active) = self.active_interaction.as_ref() {
            return vec![ConversationChange::InteractionConflict {
                active_request_id: active.request_id().clone(),
                received_request_id: request.request_id,
            }];
        }
        let request_id = request.request_id.clone();
        self.active_interaction = Some(InteractionState::new(request));
        vec![ConversationChange::InteractionShown { request_id }]
    }

    pub(super) fn update_interaction_draft(
        &mut self,
        request_id: &UiInteractionRequestId,
        action: InteractionDraftAction,
    ) -> Vec<ConversationChange> {
        let Some(interaction) = self.active_interaction.as_mut() else {
            return Vec::new();
        };
        if interaction.request_id() != request_id || !interaction.update_draft(action) {
            return Vec::new();
        }
        vec![ConversationChange::InteractionUpdated {
            request_id: request_id.clone(),
        }]
    }

    pub(super) fn confirm_interaction(
        &mut self,
        request_id: &UiInteractionRequestId,
    ) -> Vec<ConversationChange> {
        let Some(interaction) = self.active_interaction.as_mut() else {
            return Vec::new();
        };
        if interaction.request_id() != request_id {
            return Vec::new();
        }
        let Some(reply) = interaction.confirm() else {
            return Vec::new();
        };
        vec![ConversationChange::InteractionReplyRequested {
            request_id: request_id.clone(),
            reply,
        }]
    }

    pub(super) fn cancel_interaction(
        &mut self,
        request_id: &UiInteractionRequestId,
    ) -> Vec<ConversationChange> {
        let Some(interaction) = self.active_interaction.as_mut() else {
            return Vec::new();
        };
        if interaction.request_id() != request_id || !interaction.cancel() {
            return Vec::new();
        }
        vec![ConversationChange::InteractionCancelRequested {
            request_id: request_id.clone(),
        }]
    }

    pub(super) fn accept_interaction(
        &mut self,
        request_id: &UiInteractionRequestId,
    ) -> Vec<ConversationChange> {
        let Some(interaction) = self.active_interaction.as_ref() else {
            return Vec::new();
        };
        if interaction.request_id() != request_id {
            return Vec::new();
        }
        self.active_interaction = None;
        vec![ConversationChange::InteractionCompleted {
            request_id: request_id.clone(),
        }]
    }

    pub(super) fn reject_interaction_reply(
        &mut self,
        request_id: &UiInteractionRequestId,
        failure: InteractionCommandFailure,
    ) -> Vec<ConversationChange> {
        let Some(interaction) = self.active_interaction.as_mut() else {
            return Vec::new();
        };
        if interaction.request_id() != request_id {
            return Vec::new();
        }
        interaction.restore_collecting();
        vec![ConversationChange::InteractionCommandRejected {
            request_id: request_id.clone(),
            failure,
        }]
    }

    pub(super) fn reject_interaction_cancel(
        &mut self,
        request_id: &UiInteractionRequestId,
        failure: InteractionCommandFailure,
    ) -> Vec<ConversationChange> {
        self.reject_interaction_reply(request_id, failure)
    }
}
