use super::{extract_invocation_context, invocation_mapping_log_summary};
use crate::ports::{
    CompactionDecision, ContextWindow, DecisionReason, SessionRevision, SystemBlock, TokenBudget,
    Urgency,
};
use provider::RequestSystemBlock;
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
            context_size: 200_000,
            effective_window: 180_000,
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

#[test]
fn continuation_checkpoint_system_block_is_consumed_verbatim_once() {
    let checkpoint = "## Immutable Constraints\n- review only\n\n## Current Objective\n- inspect resume\n\n## Committed Facts\n- persisted\n\n## Uncommitted Working Set\n- none\n\n## Open Decisions / Risks\n- dynamic state\n\n## Resume Cursor\n- Next action: revalidate once\n\n## Required Revalidation\n- revalidate git\n\n## Archived Milestones\n- baseline\n\n## Continuation Status\nContinue";
    let mut context_window = window(vec![]);
    context_window.system_blocks = vec![SystemBlock {
        kind: "active_summary".to_string(),
        content: checkpoint.to_string(),
        cacheable: true,
        cache_break: true,
    }];

    let invocation = extract_invocation_context(&context_window);

    assert_eq!(invocation.system_blocks.len(), 1);
    assert_eq!(
        invocation.system_blocks[0],
        RequestSystemBlock::Cacheable(checkpoint.to_string())
    );
}
