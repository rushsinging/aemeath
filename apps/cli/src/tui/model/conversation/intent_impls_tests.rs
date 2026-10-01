use super::*;
use crate::tui::adapter::runtime_view::{TuiChatMessage, TuiContentBlock, TuiMessageSource};
use crate::tui::model::conversation::tool_call::ToolCallStatus;
use crate::tui::model::output_timeline::OutputTimelineItem;

fn runtime_status(
    revision: u64,
    heartbeat_sequence: u64,
) -> crate::tui::adapter::runtime_status::TuiRuntimeStatus {
    crate::tui::adapter::runtime_status::TuiRuntimeStatus {
        session_id: "session-1".to_string(),
        revision,
        heartbeat_sequence,
        context_budget: crate::tui::adapter::runtime_status::TuiContextBudget {
            context_size: 200_000,
            effective_window: 180_000,
            decision_token_count: revision,
            threshold: 144_000,
            usage_permille: revision as u32,
            compaction_needed: false,
            source:
                crate::tui::adapter::runtime_status::TuiContextDecisionSource::ActualProviderUsage,
        },
    }
}

#[test]
fn runtime_status_rejects_stale_revision_and_accepts_newer_heartbeat() {
    let mut model = ConversationModel::default();
    ReplaceRuntimeStatus(runtime_status(5, 0)).update(&mut model);
    ReplaceRuntimeStatus(runtime_status(3, 0)).update(&mut model);
    assert_eq!(model.runtime.runtime_status.as_ref().unwrap().revision, 5);

    ReplaceRuntimeStatus(runtime_status(5, 1)).update(&mut model);
    assert_eq!(
        model
            .runtime
            .runtime_status
            .as_ref()
            .unwrap()
            .heartbeat_sequence,
        1
    );
}

fn ask_tool_use(id: &str, question: &str) -> TuiContentBlock {
    TuiContentBlock::ToolUse {
        id: id.to_string(),
        name: "AskUserQuestion".to_string(),
        input: serde_json::json!({ "question": question }),
    }
}

fn ask_result(id: &str, answer: serde_json::Value) -> TuiChatMessage {
    TuiChatMessage {
        role: "user".to_string(),
        content: vec![TuiContentBlock::ToolResult {
            tool_use_id: id.to_string(),
            content: answer,
            is_error: false,
            text: None,
        }],
        input_id: None,
        source: TuiMessageSource::User,
        hook_notice: None,
        skill_request: None,
    }
}

#[test]
fn resume_projects_completed_step_terminal_notice_when_duration_is_known() {
    let mut model = ConversationModel::default();

    ResumeConversation {
        steps: vec![crate::tui::adapter::runtime_view::TuiResumedSessionStep {
            run_id: "completed-run".into(),
            step_id: "completed-step".into(),
            messages: vec![TuiChatMessage::user_text("completed question")],
            finalize_cause: Some(
                crate::tui::adapter::runtime_view::TuiResumedStepFinalizeCause::Completed,
            ),
            duration_ms: Some(125_000),
        }],
    }
    .update(&mut model);

    assert_eq!(
        model
            .timeline
            .items()
            .iter()
            .filter(|item| matches!(
                item,
                OutputTimelineItem::System { text, .. }
                    if text.starts_with('✻') && text.ends_with("for 2m 5s")
            ))
            .count(),
        1
    );
    assert!(!model.timeline.items().iter().any(|item| matches!(
        item,
        OutputTimelineItem::System { text, .. }
            if text.contains("Completed") || text.contains("Cancelled") || text.contains("终止")
    )));
}

#[test]
fn resume_legacy_completed_step_without_duration_still_has_terminal_notice() {
    let mut model = ConversationModel::default();

    ResumeConversation {
        steps: vec![crate::tui::adapter::runtime_view::TuiResumedSessionStep {
            run_id: "legacy-completed-run".into(),
            step_id: "legacy-completed-step".into(),
            messages: vec![TuiChatMessage::user_text("legacy completed question")],
            finalize_cause: Some(
                crate::tui::adapter::runtime_view::TuiResumedStepFinalizeCause::Completed,
            ),
            duration_ms: None,
        }],
    }
    .update(&mut model);

    assert_eq!(
        model
            .timeline
            .items()
            .iter()
            .filter(|item| matches!(
                item,
                OutputTimelineItem::System { text, .. }
                    if text.starts_with("✻ ")
                        && !text.contains(" for ")
                        && !text.contains("Completed")
            ))
            .count(),
        1
    );
}

#[test]
fn resume_projects_cancelled_step_terminal_notice() {
    let mut model = ConversationModel::default();

    ResumeConversation {
        steps: vec![crate::tui::adapter::runtime_view::TuiResumedSessionStep {
            run_id: "cancelled-run".into(),
            step_id: "cancelled-step".into(),
            messages: vec![TuiChatMessage::user_text("cancelled question")],
            finalize_cause: Some(
                crate::tui::adapter::runtime_view::TuiResumedStepFinalizeCause::UserCancelledStep,
            ),
            duration_ms: Some(125_000),
        }],
    }
    .update(&mut model);

    assert_eq!(
        model
            .timeline
            .items()
            .iter()
            .filter(|item| matches!(
                item,
                OutputTimelineItem::System { text, .. }
                    if text == "✻ Cancelled, ran 2m 5s"
            ))
            .count(),
        1
    );
    assert!(!model.timeline.items().iter().any(|item| matches!(
        item,
        OutputTimelineItem::System { text, .. }
            if text.contains("Completed") || text.contains(" for ") || text.contains("终止")
    )));
}

#[test]
fn resume_projects_reconstructed_unfinished_bash_as_error_and_terminated_notice() {
    let mut model = ConversationModel::default();
    let assistant = TuiChatMessage {
        role: "assistant".to_string(),
        content: vec![TuiContentBlock::ToolUse {
            id: "provider-call-1".to_string(),
            name: "Bash".to_string(),
            input: serde_json::json!({"command": "sleep 180"}),
        }],
        input_id: None,
        source: TuiMessageSource::User,
        hook_notice: None,
        skill_request: None,
    };
    let result = TuiChatMessage {
        role: "user".to_string(),
        content: vec![TuiContentBlock::ToolResult {
            tool_use_id: "provider-call-1".to_string(),
            content: serde_json::json!({"outcome": "CancellationUnconfirmed"}),
            is_error: true,
            text: Some("cleanup could not be confirmed".to_string()),
        }],
        input_id: None,
        source: TuiMessageSource::SystemGenerated,
        hook_notice: None,
        skill_request: None,
    };

    ResumeConversation {
        steps: vec![crate::tui::adapter::runtime_view::TuiResumedSessionStep {
            run_id: "terminated-run".into(),
            step_id: "running-tool-step".into(),
            messages: vec![assistant, result],
            finalize_cause: Some(
                crate::tui::adapter::runtime_view::TuiResumedStepFinalizeCause::RunTerminated,
            ),
            duration_ms: None,
        }],
    }
    .update(&mut model);

    let chat_id =
        crate::tui::model::conversation::ids::ChatId::from_legacy_or_new("terminated-run");
    let run_id =
        crate::tui::model::conversation::ids::ChatRunId::from_legacy_or_new("running-tool-step");
    let turn = model
        .chats
        .iter()
        .find(|chat| chat.id == chat_id)
        .and_then(|chat| chat.runs.iter().find(|turn| turn.id == run_id))
        .expect("恢复后应存在终止 Step");
    let call = turn.tool_calls.first().expect("恢复后应存在 Bash ToolCall");
    assert_eq!(call.name, "Bash");
    assert_eq!(call.status, ToolCallStatus::Error);
    assert!(model
        .timeline
        .items()
        .iter()
        .any(|item| matches!(item, OutputTimelineItem::ToolCall { .. })));
    assert!(model
        .timeline
        .items()
        .iter()
        .any(|item| matches!(item, OutputTimelineItem::ToolResult { .. })));
    assert_eq!(
        model
            .timeline
            .items()
            .iter()
            .filter(|item| matches!(
                item,
                OutputTimelineItem::System { text, .. } if text == "此 Run 已终止"
            ))
            .count(),
        1
    );
    assert!(!model.timeline.items().iter().any(|item| matches!(
        item,
        OutputTimelineItem::System { text, .. }
            if text.contains("Completed") || text.contains("Cancelled") || text.contains(" for ")
    )));
}

#[test]
fn resume_conversation_equality_compares_step_identity_and_messages() {
    let first = ResumeConversation {
        steps: vec![crate::tui::adapter::runtime_view::TuiResumedSessionStep {
            run_id: "run-1".into(),
            step_id: "step-1".into(),
            messages: vec![TuiChatMessage::user_text("first")],
            finalize_cause: None,
            duration_ms: None,
        }],
    };
    let different = ResumeConversation {
        steps: vec![crate::tui::adapter::runtime_view::TuiResumedSessionStep {
            run_id: "run-2".into(),
            step_id: "step-1".into(),
            messages: vec![TuiChatMessage::user_text("second")],
            finalize_cause: None,
            duration_ms: None,
        }],
    };

    assert_ne!(first, different);
}

#[test]
fn resume_restores_answered_ask_batches_in_assistant_message_order() {
    let assistant_one = TuiChatMessage {
        role: "assistant".to_string(),
        content: vec![ask_tool_use("ask-1", "第一问")],
        input_id: None,
        source: TuiMessageSource::User,
        hook_notice: None,
        skill_request: None,
    };
    let assistant_two = TuiChatMessage {
        role: "assistant".to_string(),
        content: vec![ask_tool_use("ask-2", "第二问")],
        input_id: None,
        source: TuiMessageSource::User,
        hook_notice: None,
        skill_request: None,
    };
    let mut model = ConversationModel::default();

    ResumeConversation {
        steps: vec![crate::tui::adapter::runtime_view::TuiResumedSessionStep {
            run_id: "history-run".into(),
            step_id: "history-step".into(),
            messages: vec![
                assistant_one,
                ask_result("ask-1", serde_json::json!({ "answer": "答案一" })),
                assistant_two,
                ask_result("ask-2", serde_json::json!("答案二")),
            ],
            finalize_cause: None,
            duration_ms: None,
        }],
    }
    .update(&mut model);

    let restored = model
        .timeline
        .items()
        .iter()
        .filter_map(|item| match item {
            OutputTimelineItem::AskUserBatch {
                slots,
                completion: crate::tui::model::conversation::block::AskUserCompletion::Answered,
                ..
            } => Some((slots[0].question.as_str(), slots[0].answer.as_deref())),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        restored,
        vec![("第一问", Some("答案一")), ("第二问", Some("答案二"))]
    );
}
#[test]
fn resume_excludes_llm_only_messages_from_user_history() {
    let user = TuiChatMessage::user_text("user question");
    let hook_notice = TuiChatMessage {
        role: "user".to_string(),
        content: vec![TuiContentBlock::text(
            "<system-reminder>blocked by hook</system-reminder>",
        )],
        input_id: None,
        source: TuiMessageSource::Hook,
        hook_notice: None,
        skill_request: None,
    };
    let system_generated = TuiChatMessage::system_generated_user_text(
        "<system-reminder>Skill loaded</system-reminder>",
    );
    let assistant = TuiChatMessage::assistant_text("assistant reply");
    let mut model = ConversationModel::default();

    ResumeConversation {
        steps: vec![crate::tui::adapter::runtime_view::TuiResumedSessionStep {
            run_id: "history-run".into(),
            step_id: "history-step".into(),
            messages: vec![user, hook_notice, system_generated, assistant],
            finalize_cause: None,
            duration_ms: None,
        }],
    }
    .update(&mut model);

    let user_messages = model
        .timeline
        .items()
        .iter()
        .filter_map(|item| match item {
            OutputTimelineItem::UserMessage { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(user_messages, ["user question"]);
    assert!(!model.timeline.items().iter().any(|item| match item {
        OutputTimelineItem::UserMessage { text, .. } => text.contains("system-reminder"),
        _ => false,
    }));
}
