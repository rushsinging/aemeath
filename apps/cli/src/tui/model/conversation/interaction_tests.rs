use super::intent::{
    ConfirmInteraction, ConversationIntent, InteractionReplyRejected, ShowInteraction,
    UpdateInteractionDraft,
};
use super::interaction::{
    InteractionBody, InteractionCommandFailure, InteractionDraftAction, InteractionPhase,
    InteractionRequest, UiApprovalPrompt, UiInteractionRequestId, UiPlanApprovalPrompt,
    UiRiskLevel, UiRunId, UiStuckDiagnostic, UiUserQuestion,
};
use super::model::ConversationModel;
use super::update::ConversationUpdate;

fn tool_approval_request(id: &str) -> InteractionRequest {
    InteractionRequest {
        request_id: UiInteractionRequestId::from(id),
        run_id: UiRunId::from("run-1"),
        body: InteractionBody::ToolApproval(UiApprovalPrompt {
            title: "Bash".to_string(),
            detail: "rm -rf build".to_string(),
            risk: UiRiskLevel::High,
        }),
    }
}

fn interaction_request_for(body: InteractionBody) -> InteractionRequest {
    InteractionRequest {
        request_id: UiInteractionRequestId::from("request-body"),
        run_id: UiRunId::from("run-1"),
        body,
    }
}

#[test]
fn show_interaction_stores_first_request_in_collecting_phase() {
    let mut model = ConversationModel::default();
    let request = tool_approval_request("request-1");

    model.show_interaction(request.clone());

    let interaction = model.active_interaction().expect("active interaction");
    assert_eq!(interaction.request_id(), &request.request_id);
    assert_eq!(interaction.phase(), InteractionPhase::Collecting);
}

#[test]
fn show_interaction_initializes_typed_drafts_for_all_bodies() {
    let bodies = vec![
        InteractionBody::UserQuestions(vec![UiUserQuestion {
            prompt: "继续？".to_string(),
            options: vec!["是".to_string()],
            allow_multi: false,
        }]),
        InteractionBody::ToolApproval(UiApprovalPrompt {
            title: "Bash".to_string(),
            detail: "cargo test".to_string(),
            risk: UiRiskLevel::Low,
        }),
        InteractionBody::PlanApproval(UiPlanApprovalPrompt {
            title: "迁移计划".to_string(),
            steps: vec!["实现".to_string()],
        }),
        InteractionBody::HardPause(UiStuckDiagnostic {
            reason: "需要确认".to_string(),
            recent_actions: Vec::new(),
        }),
    ];

    for body in bodies {
        let mut model = ConversationModel::default();
        let request = interaction_request_for(body);
        model.show_interaction(request.clone());
        assert_eq!(
            model
                .active_interaction()
                .expect("interaction")
                .request_id(),
            &request.request_id
        );
    }
}

#[test]
fn show_interaction_rejects_second_request_without_replacing_active_request() {
    let mut model = ConversationModel::default();
    let first = tool_approval_request("request-1");
    let second = tool_approval_request("request-2");
    model.show_interaction(first.clone());

    let changes = model.show_interaction(second.clone());

    assert!(changes
        .iter()
        .any(|change| change.is_interaction_conflict()));
    assert_eq!(
        model
            .active_interaction()
            .expect("first request retained")
            .request_id(),
        &first.request_id
    );
}

#[test]
fn confirm_interaction_requests_reply_without_changing_runtime_phase() {
    let mut model = ConversationModel::default();
    let request = tool_approval_request("request-1");
    let before_runtime = model.runtime.clone();
    ConversationIntent::ShowInteraction(ShowInteraction {
        request: request.clone(),
    })
    .update(&mut model);
    ConversationIntent::UpdateInteractionDraft(UpdateInteractionDraft {
        request_id: request.request_id.clone(),
        action: InteractionDraftAction::Approve,
    })
    .update(&mut model);

    let changes = ConversationIntent::ConfirmInteraction(ConfirmInteraction {
        request_id: request.request_id.clone(),
    })
    .update(&mut model);

    assert!(changes
        .iter()
        .any(|change| change.is_interaction_reply_requested()));
    assert_eq!(
        model
            .active_interaction()
            .expect("interaction kept pending")
            .phase(),
        InteractionPhase::ReplyPending
    );
    assert_eq!(model.runtime, before_runtime);
}

#[test]
fn rejected_reply_restores_collecting_phase_and_preserves_draft() {
    let mut model = ConversationModel::default();
    let request = tool_approval_request("request-1");
    ConversationIntent::ShowInteraction(ShowInteraction {
        request: request.clone(),
    })
    .update(&mut model);
    ConversationIntent::UpdateInteractionDraft(UpdateInteractionDraft {
        request_id: request.request_id.clone(),
        action: InteractionDraftAction::Approve,
    })
    .update(&mut model);
    ConversationIntent::ConfirmInteraction(ConfirmInteraction {
        request_id: request.request_id.clone(),
    })
    .update(&mut model);

    ConversationIntent::InteractionReplyRejected(InteractionReplyRejected {
        request_id: request.request_id.clone(),
        failure: InteractionCommandFailure::InvalidReply("审批结果不匹配".to_string()),
    })
    .update(&mut model);

    let interaction = model
        .active_interaction()
        .expect("interaction remains retryable");
    assert_eq!(interaction.phase(), InteractionPhase::Collecting);
    assert!(interaction.draft().is_approved());
}
