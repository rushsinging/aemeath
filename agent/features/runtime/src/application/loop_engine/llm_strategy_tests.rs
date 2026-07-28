use super::extract_invocation_context;
use crate::ports::{
    CompactionDecision, ContextWindow, DecisionReason, SessionRevision, TokenBudget, Urgency,
};
use share::message::{ContentBlock, Message, MessageMetadata, MessageSource, Role};

fn window(messages: Vec<Message>, reminder: bool) -> ContextWindow {
    ContextWindow {
        backing_revision: SessionRevision::new(1),
        system_blocks: vec![],
        messages: messages.into(),
        invocation_reminder: reminder.then(|| {
            crate::ports::InvocationReminder::from_task_snapshot(
                &crate::ports::TaskReminderSnapshot {
                    task_list_id: Some("1".to_string()),
                    summary: Some("tasks".to_string()),
                    pending: 1,
                    in_progress: 0,
                },
                &crate::ports::Language::new("en"),
            )
            .expect("unfinished snapshot")
        }),
        tool_schemas: vec![],
        token_estimation: TokenBudget::default(),
        compaction_decision: CompactionDecision {
            needed: false,
            urgency: Urgency::None,
            decision_token_count: 0,
            threshold: 1,
            reason: DecisionReason::HeuristicFallback,
        },
    }
}

fn message_text(message: &Message) -> Vec<&str> {
    message
        .content
        .iter()
        .filter_map(|block| match block {
            ContentBlock::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect()
}

#[test]
fn reminder_decorates_only_last_real_user_message_copy() {
    let original = Message::user("original");
    let window = window(
        vec![
            original.clone(),
            Message::system_generated_user("generated"),
            Message::user("latest"),
        ],
        true,
    );

    let first = extract_invocation_context(&window);
    let second = extract_invocation_context(&window);

    assert_eq!(message_text(&first.messages_for_api[0]), vec!["original"]);
    assert_eq!(message_text(&first.messages_for_api[1]), vec!["generated"]);
    assert_eq!(
        message_text(&first.messages_for_api[2]),
        vec!["latest\n\n<task-reminder>\nCurrent task list #1 \"tasks\" has 1 pending and 0 in_progress tasks. If it is relevant to the latest user request, call TaskListGet for details; otherwise prioritize the latest request.\n</task-reminder>"]
    );
    assert_eq!(
        message_text(&second.messages_for_api[2]),
        vec!["latest\n\n<task-reminder>\nCurrent task list #1 \"tasks\" has 1 pending and 0 in_progress tasks. If it is relevant to the latest user request, call TaskListGet for details; otherwise prioritize the latest request.\n</task-reminder>"]
    );
    assert_eq!(message_text(&window.messages[2]), vec!["latest"]);
}

#[test]
fn reminder_appends_text_block_when_real_user_has_no_text() {
    let user = Message {
        role: Role::User,
        content: vec![ContentBlock::base64_image(
            "data".to_string(),
            "image/png".to_string(),
        )],
        metadata: None,
    };
    let context = extract_invocation_context(&window(vec![user], true));

    assert_eq!(
        message_text(&context.messages_for_api[0]),
        vec!["<task-reminder>\nCurrent task list #1 \"tasks\" has 1 pending and 0 in_progress tasks. If it is relevant to the latest user request, call TaskListGet for details; otherwise prioritize the latest request.\n</task-reminder>"]
    );
}

#[test]
fn reminder_is_not_injected_without_real_user_message() {
    let stop_hook = Message {
        role: Role::User,
        content: vec![ContentBlock::Text {
            text: "hook".into(),
        }],
        metadata: Some(MessageMetadata {
            source: MessageSource::StopHook,
            stop_hook: None,
        }),
    };
    let context = extract_invocation_context(&window(
        vec![Message::system_generated_user("generated"), stop_hook],
        true,
    ));

    assert_eq!(
        message_text(&context.messages_for_api[0]),
        vec!["generated"]
    );
    assert_eq!(message_text(&context.messages_for_api[1]), vec!["hook"]);
}
