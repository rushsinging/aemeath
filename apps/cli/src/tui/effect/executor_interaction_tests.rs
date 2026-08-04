use super::{interaction_failure_from_sdk, interaction_reply_summary, interaction_reply_to_sdk};
use crate::tui::app::App;
use crate::tui::effect::effect::Effect;
use crate::tui::model::conversation::intent::{
    CancelInteraction, ConversationIntent, InteractionCancelRejected, ShowInteraction,
    ToolCallStart, ToolCallUpdate, UpdateInteractionDraft,
};
use crate::tui::model::conversation::interaction::{
    InteractionBody, InteractionCommandFailure, InteractionDraftAction, InteractionRequest,
    UiInteractionCancelReason, UiInteractionReply, UiInteractionRequestId, UiPlanApprovalPrompt,
    UiRunId, UiStuckDiagnostic,
};
use crate::tui::model::conversation::update::ConversationUpdate;
use async_trait::async_trait;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

#[test]
fn reply_conversion_preserves_each_typed_reply() {
    assert!(matches!(
        interaction_reply_to_sdk(UiInteractionReply::ToolApproval {
            approved: true,
            reason: None,
        }),
        sdk::InteractionReply::ToolApproval(sdk::ApprovalDecision::Approve)
    ));
    assert!(matches!(
        interaction_reply_to_sdk(UiInteractionReply::ToolApproval {
            approved: false,
            reason: Some("权限不足".to_string()),
        }),
        sdk::InteractionReply::ToolApproval(sdk::ApprovalDecision::Deny { reason: Some(reason) }) if reason == "权限不足"
    ));
    assert!(matches!(
        interaction_reply_to_sdk(UiInteractionReply::PlanApproval {
            approved: true,
            reason: None,
        }),
        sdk::InteractionReply::PlanApproval(sdk::ApprovalDecision::Approve)
    ));
    assert!(matches!(
        interaction_reply_to_sdk(UiInteractionReply::UserAnswers(vec!["答案".to_string()])),
        sdk::InteractionReply::UserQuestions(answers)
            if answers == vec![sdk::UserAnswer("答案".to_string())]
    ));
    assert!(matches!(
        interaction_reply_to_sdk(UiInteractionReply::ContinueHardPause),
        sdk::InteractionReply::HardPauseContinue
    ));
}

#[test]
fn reply_summary_records_shape_without_answer_content() {
    let summary = interaction_reply_summary(&UiInteractionReply::UserAnswers(vec![
        "日料".to_string(),
        "寿司".to_string(),
    ]));

    assert_eq!(summary, "user_answers count=2 lengths=[2, 2]");
    assert!(!summary.contains("日料"));
    assert!(!summary.contains("寿司"));
}

#[test]
fn sdk_command_outcomes_map_to_retryable_tui_failures() {
    assert_eq!(
        interaction_failure_from_sdk(sdk::InteractionCommandOutcome::NotFound),
        InteractionCommandFailure::NotFound
    );
    assert_eq!(
        interaction_failure_from_sdk(sdk::InteractionCommandOutcome::RunCancelling),
        InteractionCommandFailure::RunCancelling
    );
}

#[derive(Default)]
struct RecordingInteractionClient {
    replies: Mutex<Vec<(sdk::InteractionRequestId, sdk::InteractionReply)>>,
    cancellations: Mutex<Vec<(sdk::InteractionRequestId, sdk::InteractionCancelReason)>>,
}

#[async_trait]
impl sdk::AgentClient for RecordingInteractionClient {
    fn reply_interaction(
        &self,
        request_id: &sdk::InteractionRequestId,
        reply: sdk::InteractionReply,
    ) -> sdk::InteractionCommandOutcome {
        self.replies
            .lock()
            .expect("replies")
            .push((request_id.clone(), reply));
        sdk::InteractionCommandOutcome::Accepted
    }

    fn cancel_interaction(
        &self,
        request_id: &sdk::InteractionRequestId,
        reason: sdk::InteractionCancelReason,
    ) -> sdk::InteractionCommandOutcome {
        self.cancellations
            .lock()
            .expect("cancellations")
            .push((request_id.clone(), reason));
        sdk::InteractionCommandOutcome::Accepted
    }

    async fn chat(&self, _input: sdk::ChatRequest) -> Result<sdk::ChatStream, sdk::SdkError> {
        unreachable!("interaction effect test does not start chat")
    }
}

fn install_hard_pause_interaction(app: &mut App, request_id: UiInteractionRequestId) {
    ConversationIntent::ShowInteraction(ShowInteraction {
        request: InteractionRequest {
            request_id,
            run_id: UiRunId::from("run-1"),
            tool_call_id: None,
            body: InteractionBody::HardPause(UiStuckDiagnostic {
                reason: "等待确认".to_string(),
                recent_actions: Vec::new(),
            }),
        },
    })
    .update(&mut app.model.conversation);
}

fn install_plan_approval_interaction(app: &mut App, request_id: UiInteractionRequestId) {
    ConversationIntent::ShowInteraction(ShowInteraction {
        request: InteractionRequest {
            request_id,
            run_id: UiRunId::from("run-1"),
            tool_call_id: None,
            body: InteractionBody::PlanApproval(UiPlanApprovalPrompt {
                title: "迁移计划".to_string(),
                steps: vec!["执行".to_string()],
            }),
        },
    })
    .update(&mut app.model.conversation);
}

#[tokio::test]
async fn reply_effect_calls_agent_client_once_and_completes_interaction() {
    let client = Arc::new(RecordingInteractionClient::default());
    let mut app = App::new("session".to_string(), "/tmp".into(), "model".to_string());
    app.agent_client = Some(client.clone());
    let (tx, _rx) = mpsc::channel(1);
    let request_id = UiInteractionRequestId::from("018f0000-0000-7000-8000-000000000001");
    install_hard_pause_interaction(&mut app, request_id.clone());

    app.execute_effect(
        Effect::ReplyInteraction {
            request_id: request_id.clone(),
            reply: UiInteractionReply::ContinueHardPause,
        },
        &tx,
    )
    .await;

    let replies = client.replies.lock().expect("replies");
    assert_eq!(replies.len(), 1);
    assert_eq!(replies[0].0.as_str(), request_id.as_str());
    assert!(matches!(
        replies[0].1,
        sdk::InteractionReply::HardPauseContinue
    ));
    assert!(app.model.conversation.active_interaction().is_none());
}

#[tokio::test]
async fn plan_approval_confirmation_preserves_reply_variant() {
    let mut app = App::new("session".to_string(), "/tmp".into(), "model".to_string());
    let request_id = UiInteractionRequestId::from("request-3");
    install_plan_approval_interaction(&mut app, request_id.clone());
    ConversationIntent::UpdateInteractionDraft(UpdateInteractionDraft {
        request_id: request_id.clone(),
        action: InteractionDraftAction::Approve,
    })
    .update(&mut app.model.conversation);

    let reply = ConversationIntent::ConfirmInteraction(
        crate::tui::model::conversation::intent::ConfirmInteraction {
            request_id: request_id.clone(),
        },
    )
    .update(&mut app.model.conversation)
    .into_iter()
    .find_map(|change| {
        match change {
            crate::tui::model::conversation::change::ConversationChange::InteractionReplyRequested {
                reply,
                ..
            } => Some(reply),
            _ => None,
        }
    })
    .expect("plan reply");

    assert!(matches!(
        reply,
        UiInteractionReply::PlanApproval { approved: true, .. }
    ));
}

fn start_ask_user_tool(
    app: &mut App,
    tool_call_id: crate::tui::model::conversation::ids::ToolCallId,
    chat_id: &str,
    run_id: &str,
) {
    let chat_id = crate::tui::model::conversation::ids::ChatId::new(chat_id);
    let run_id = crate::tui::model::conversation::ids::ChatRunId::new(run_id);
    ConversationIntent::ToolCallStart(ToolCallStart {
        chat_id: chat_id.clone(),
        run_id: run_id.clone(),
        id: tool_call_id.clone(),
        provider_id: Some(tool_call_id.as_str().to_string()),
        name: "AskUserQuestion".to_string(),
        index: 0,
    })
    .update(&mut app.model.conversation);
    ConversationIntent::ToolCallUpdate(ToolCallUpdate {
        chat_id,
        run_id,
        id: tool_call_id.clone(),
        provider_id: Some(tool_call_id.as_str().to_string()),
        name: "AskUserQuestion".to_string(),
        index: 0,
        arguments: Some(serde_json::json!({ "question": "确认取消？" }).to_string()),
        status: crate::tui::model::conversation::tool_call::ToolCallStatus::Running,
    })
    .update(&mut app.model.conversation);
}

fn install_ask_user_interaction(
    app: &mut App,
    request_id: UiInteractionRequestId,
    tool_call_id: crate::tui::model::conversation::ids::ToolCallId,
    chat_id: &str,
    run_id: &str,
) {
    start_ask_user_tool(app, tool_call_id.clone(), chat_id, run_id);
    ConversationIntent::ShowInteraction(ShowInteraction {
        request: InteractionRequest {
            request_id,
            run_id: UiRunId::from("run-1"),
            tool_call_id: Some(tool_call_id.as_str().to_string()),
            body: InteractionBody::UserQuestions(vec![
                crate::tui::model::conversation::interaction::UiUserQuestion {
                    prompt: "确认取消？".to_string(),
                    options: vec!["继续".to_string()],
                    allow_multi: false,
                },
            ]),
        },
    })
    .update(&mut app.model.conversation);
}

#[tokio::test]
async fn cancel_effect_calls_typed_cancel_and_completes_interaction() {
    let client = Arc::new(RecordingInteractionClient::default());
    let mut app = App::new("session".to_string(), "/tmp".into(), "model".to_string());
    app.agent_client = Some(client.clone());
    let (tx, _rx) = mpsc::channel(1);
    let request_id = UiInteractionRequestId::from("018f0000-0000-7000-8000-000000000002");
    install_hard_pause_interaction(&mut app, request_id.clone());
    ConversationIntent::CancelInteraction(CancelInteraction {
        request_id: request_id.clone(),
    })
    .update(&mut app.model.conversation);

    app.execute_effect(
        Effect::CancelInteraction {
            request_id: request_id.clone(),
            reason: UiInteractionCancelReason::UserCancelled,
        },
        &tx,
    )
    .await;

    let cancellations = client.cancellations.lock().expect("cancellations");
    assert_eq!(cancellations.len(), 1);
    assert_eq!(cancellations[0].0.as_str(), request_id.as_str());
    assert_eq!(
        cancellations[0].1,
        sdk::InteractionCancelReason::UserCancelled
    );
    assert!(app.model.conversation.active_interaction().is_none());
}

#[tokio::test]
async fn accepted_ask_user_cancel_marks_only_matching_tool_cancelled_through_executor() {
    let client = Arc::new(RecordingInteractionClient::default());
    let mut app = App::new("session".to_string(), "/tmp".into(), "model".to_string());
    app.agent_client = Some(client);
    let (tx, _rx) = mpsc::channel(1);
    let request_id = UiInteractionRequestId::from("018f0000-0000-7000-8000-000000000009");
    let matching_tool_id =
        crate::tui::model::conversation::ids::ToolCallId::new("ask-tool-matching");
    let unrelated_tool_id =
        crate::tui::model::conversation::ids::ToolCallId::new("ask-tool-unrelated");
    install_ask_user_interaction(
        &mut app,
        request_id.clone(),
        matching_tool_id.clone(),
        "chat-matching",
        "turn-matching",
    );
    start_ask_user_tool(
        &mut app,
        unrelated_tool_id.clone(),
        "chat-unrelated",
        "turn-unrelated",
    );
    ConversationIntent::CancelInteraction(CancelInteraction {
        request_id: request_id.clone(),
    })
    .update(&mut app.model.conversation);

    app.execute_effect(
        Effect::CancelInteraction {
            request_id,
            reason: UiInteractionCancelReason::UserCancelled,
        },
        &tx,
    )
    .await;

    let tool_status = |tool_call_id: &crate::tui::model::conversation::ids::ToolCallId| {
        app.model
            .conversation
            .chats
            .iter()
            .flat_map(|chat| &chat.runs)
            .flat_map(|turn| &turn.tool_calls)
            .find(|call| call.id.as_ref() == Some(tool_call_id))
            .expect("AskUserQuestion tool call should exist")
            .status
    };
    assert_eq!(
        tool_status(&matching_tool_id),
        crate::tui::model::conversation::tool_call::ToolCallStatus::Cancelled
    );
    assert_eq!(
        tool_status(&unrelated_tool_id),
        crate::tui::model::conversation::tool_call::ToolCallStatus::Running
    );
}

#[tokio::test]
async fn cancel_rejection_restores_collecting_through_cancel_result_intent() {
    let mut app = App::new("session".to_string(), "/tmp".into(), "model".to_string());
    let request_id = UiInteractionRequestId::from("request-4");
    install_hard_pause_interaction(&mut app, request_id.clone());
    ConversationIntent::CancelInteraction(CancelInteraction {
        request_id: request_id.clone(),
    })
    .update(&mut app.model.conversation);

    ConversationIntent::InteractionCancelRejected(InteractionCancelRejected {
        request_id,
        failure: InteractionCommandFailure::NotFound,
    })
    .update(&mut app.model.conversation);

    assert!(matches!(
        app.model
            .conversation
            .active_interaction()
            .expect("retryable interaction")
            .phase(),
        crate::tui::model::conversation::interaction::InteractionPhase::Collecting
    ));
}

#[tokio::test]
async fn malformed_request_id_is_rejected_without_calling_agent_client() {
    let client = Arc::new(RecordingInteractionClient::default());
    let mut app = App::new("session".to_string(), "/tmp".into(), "model".to_string());
    app.agent_client = Some(client.clone());
    let (tx, _rx) = mpsc::channel(1);
    let request_id = UiInteractionRequestId::from("not-a-uuid7");
    install_hard_pause_interaction(&mut app, request_id.clone());

    app.execute_effect(
        Effect::ReplyInteraction {
            request_id,
            reply: UiInteractionReply::ContinueHardPause,
        },
        &tx,
    )
    .await;

    assert!(client.replies.lock().expect("replies").is_empty());
    assert!(matches!(
        app.model
            .conversation
            .active_interaction()
            .expect("retryable interaction")
            .phase(),
        crate::tui::model::conversation::interaction::InteractionPhase::Collecting
    ));
}
