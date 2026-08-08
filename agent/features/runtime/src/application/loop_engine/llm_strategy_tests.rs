use super::{extract_invocation_context, invocation_mapping_log_summary};
use crate::ports::{
    CompactionDecision, ContextWindow, DecisionReason, SessionRevision, TokenBudget, Urgency,
};
use share::message::{Message, MessageMetadata, MessageSource, Role};

fn window(messages: Vec<Message>) -> ContextWindow {
    ContextWindow {
        backing_revision: SessionRevision::new(1),
        system_blocks: vec![],
        messages: messages.into(),
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

#[test]
fn invocation_context_preserves_messages_without_task_reminder_decoration() {
    let messages = vec![
        Message::user("original"),
        Message::system_generated_user("generated"),
        Message::user("latest"),
    ];

    let context = extract_invocation_context(&window(messages));

    assert_eq!(context.messages_for_api[0].text_content(), "original");
    assert_eq!(context.messages_for_api[1].text_content(), "generated");
    assert_eq!(context.messages_for_api[2].text_content(), "latest");
    assert!(!context
        .messages_for_api
        .iter()
        .any(|message| message.text_content().contains("<task-reminder>")));
}

#[test]
fn invocation_mapping_log_summary_reports_mechanical_field_counts() {
    let context = extract_invocation_context(&window(vec![
        Message::user("original"),
        Message::system_generated_user("<system-reminder>body</system-reminder>"),
    ]));

    let summary = invocation_mapping_log_summary(&context);

    assert_eq!(summary.messages, 2);
    assert_eq!(summary.system_blocks, 0);
    assert_eq!(summary.tool_schemas, 0);
    assert_eq!(summary.reminder_messages, 1);
}

#[test]
fn invocation_context_preserves_non_user_message_sources() {
    let stop_hook = Message {
        role: Role::User,
        content: vec![share::message::ContentBlock::Text {
            text: "hook".into(),
        }],
        metadata: Some(MessageMetadata {
            source: MessageSource::Hook,
            hook_notice: None,
            skill_request: None,
        }),
    };

    let context = extract_invocation_context(&window(vec![stop_hook]));

    assert_eq!(context.messages_for_api[0].text_content(), "hook");
}
