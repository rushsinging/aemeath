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
use crate::tui::model::conversation::block::AskUserSlot;
use crate::tui::model::conversation::ids::{ChatId, ChatTurnId, ToolCallId};
use crate::tui::model::conversation::intent::{
    AnswerCurrentAskUser, InteractionCancelAccepted, InteractionReplyAccepted, ShowAskUserBatch,
    ToolCallStart, ToolCallUpdate,
};
use crate::tui::model::conversation::tool_call::ToolCallStatus;

fn tool_approval_request(id: &str) -> InteractionRequest {
    InteractionRequest {
        request_id: UiInteractionRequestId::from(id),
        run_id: UiRunId::from("run-1"),
        tool_call_id: None,
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
        tool_call_id: None,
        body,
    }
}

fn ask_user_slot(id: &str, question: &str) -> AskUserSlot {
    AskUserSlot {
        id: id.to_string(),
        question_seq: 0,
        question: question.to_string(),
        options: vec![sdk::OptionItem::title_only("日料".to_string())],
        llm_option_count: 1,
        multi_select: false,
        default: None,
        answer: None,
    }
}

fn ask_user_tool_status(model: &ConversationModel, tool_call_id: &ToolCallId) -> ToolCallStatus {
    model
        .chats
        .iter()
        .flat_map(|chat| &chat.turns)
        .flat_map(|turn| &turn.tool_calls)
        .find(|call| call.id.as_ref() == Some(tool_call_id))
        .expect("AskUserQuestion tool call should exist")
        .status
}

fn start_ask_user_tool(model: &mut ConversationModel, tool_call_id: &ToolCallId) {
    let chat_id = ChatId::new("chat-1");
    let turn_id = ChatTurnId::new("turn-1");
    model.apply(ToolCallStart {
        chat_id: chat_id.clone(),
        turn_id: turn_id.clone(),
        id: tool_call_id.clone(),
        provider_id: None,
        name: "AskUserQuestion".to_string(),
        index: 0,
    });
    model.apply(ToolCallUpdate {
        chat_id,
        turn_id,
        id: tool_call_id.clone(),
        provider_id: Some(tool_call_id.as_str().to_string()),
        name: "AskUserQuestion".to_string(),
        index: 0,
        arguments: Some(serde_json::json!({ "question": "明天想吃什么？" }).to_string()),
        status: ToolCallStatus::Running,
    });
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
fn accepted_reply_completes_only_the_ask_tool_bound_to_the_matching_request() {
    let mut model = ConversationModel::default();
    let completed_request_id = UiInteractionRequestId::from("request-completed");
    let completed_tool_id = ToolCallId::new("ask-completed");
    let unrelated_tool_id = ToolCallId::new("ask-unrelated");
    start_ask_user_tool(&mut model, &completed_tool_id);
    start_ask_user_tool(&mut model, &unrelated_tool_id);
    model.apply(ShowAskUserBatch {
        request_id: completed_request_id.clone(),
        slots: vec![ask_user_slot(completed_tool_id.as_str(), "明天想吃什么？")],
    });
    model.apply(AnswerCurrentAskUser {
        answer: "日料".to_string(),
    });
    model.restore_answered_ask_user_batch(vec![AskUserSlot {
        answer: Some("中餐".to_string()),
        ..ask_user_slot("history", "昨天吃了什么？")
    }]);
    model.show_interaction(InteractionRequest {
        request_id: completed_request_id.clone(),
        run_id: UiRunId::from("run-1"),
        tool_call_id: Some(completed_tool_id.as_str().to_string()),
        body: InteractionBody::UserQuestions(vec![UiUserQuestion {
            prompt: "明天想吃什么？".to_string(),
            options: vec!["日料".to_string()],
            allow_multi: false,
        }]),
    });
    model.update_interaction_draft(
        &completed_request_id,
        InteractionDraftAction::SetUserAnswer {
            index: 0,
            answer: "日料".to_string(),
        },
    );
    model.confirm_interaction(&completed_request_id);

    model.apply(ConversationIntent::InteractionReplyAccepted(
        InteractionReplyAccepted {
            request_id: completed_request_id.clone(),
        },
    ));

    assert_eq!(
        ask_user_tool_status(&model, &completed_tool_id),
        ToolCallStatus::Success
    );
    let completed_result = model
        .chats
        .iter()
        .flat_map(|chat| &chat.turns)
        .flat_map(|turn| &turn.tool_calls)
        .find(|call| call.id.as_ref() == Some(&completed_tool_id))
        .and_then(|call| call.result.as_ref())
        .expect("matching AskUserQuestion should have a result");
    assert_eq!(completed_result.output, "Q1: 日料");
    assert_eq!(
        completed_result.content,
        serde_json::json!({"status": "ok", "answers": ["日料"]})
    );
    assert_eq!(
        ask_user_tool_status(&model, &unrelated_tool_id),
        ToolCallStatus::Running
    );
    assert!(model.active_interaction().is_none());
}

#[test]
fn accepted_cancel_cancels_only_the_ask_tool_bound_to_the_matching_request() {
    let mut model = ConversationModel::default();
    let cancelled_request_id = UiInteractionRequestId::from("request-cancelled");
    let cancelled_tool_id = ToolCallId::new("ask-cancelled");
    let unrelated_tool_id = ToolCallId::new("ask-unrelated");
    start_ask_user_tool(&mut model, &cancelled_tool_id);
    start_ask_user_tool(&mut model, &unrelated_tool_id);
    model.apply(ShowAskUserBatch {
        request_id: cancelled_request_id.clone(),
        slots: vec![ask_user_slot(cancelled_tool_id.as_str(), "明天想吃什么？")],
    });
    model.show_interaction(InteractionRequest {
        request_id: cancelled_request_id.clone(),
        run_id: UiRunId::from("run-1"),
        tool_call_id: Some(cancelled_tool_id.as_str().to_string()),
        body: InteractionBody::UserQuestions(vec![UiUserQuestion {
            prompt: "明天想吃什么？".to_string(),
            options: vec!["日料".to_string()],
            allow_multi: false,
        }]),
    });
    model.cancel_interaction(&cancelled_request_id);

    model.apply(ConversationIntent::InteractionCancelAccepted(
        InteractionCancelAccepted {
            request_id: cancelled_request_id,
        },
    ));

    assert_eq!(
        ask_user_tool_status(&model, &cancelled_tool_id),
        ToolCallStatus::Cancelled
    );
    assert_eq!(
        ask_user_tool_status(&model, &unrelated_tool_id),
        ToolCallStatus::Running
    );
    assert!(model.active_interaction().is_none());
}

#[test]
fn rejected_reply_keeps_the_ask_tool_running() {
    let mut model = ConversationModel::default();
    let request_id = UiInteractionRequestId::from("request-rejected");
    let tool_call_id = ToolCallId::new("ask-rejected");
    start_ask_user_tool(&mut model, &tool_call_id);
    model.apply(ShowAskUserBatch {
        request_id: request_id.clone(),
        slots: vec![ask_user_slot(tool_call_id.as_str(), "明天想吃什么？")],
    });
    model.show_interaction(InteractionRequest {
        request_id: request_id.clone(),
        run_id: UiRunId::from("run-1"),
        tool_call_id: Some(tool_call_id.as_str().to_string()),
        body: InteractionBody::UserQuestions(vec![UiUserQuestion {
            prompt: "明天想吃什么？".to_string(),
            options: vec!["日料".to_string()],
            allow_multi: false,
        }]),
    });
    model.update_interaction_draft(
        &request_id,
        InteractionDraftAction::SetUserAnswer {
            index: 0,
            answer: "日料".to_string(),
        },
    );
    model.confirm_interaction(&request_id);

    model.apply(ConversationIntent::InteractionReplyRejected(
        InteractionReplyRejected {
            request_id: request_id.clone(),
            failure: InteractionCommandFailure::InvalidReply("答案无效".to_string()),
        },
    ));

    assert_eq!(
        ask_user_tool_status(&model, &tool_call_id),
        ToolCallStatus::Running
    );
    assert_eq!(
        model.active_interaction().expect("retryable").phase(),
        InteractionPhase::Collecting
    );
}
