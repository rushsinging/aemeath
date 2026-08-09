use super::{extract_invocation_context, invocation_mapping_log_summary};
use crate::ports::{
    CompactionDecision, ContextWindow, DecisionReason, SessionRevision, SystemBlock, TokenBudget,
    Urgency,
};
use provider::RequestSystemBlock;
use share::message::{ContentBlock, Message, MessageMetadata, MessageSource, Role};

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
fn bounded_tool_result_maps_to_identical_provider_bytes_without_mutating_window() {
    let preview = "<persisted-output>bounded preview</persisted-output>";
    let canonical_message = Message {
        role: Role::User,
        content: vec![ContentBlock::ToolResult {
            tool_use_id: "tool".to_string(),
            content: serde_json::json!({
                "text": preview,
                "truncated": true,
                "original_chars": 50_001,
                "original_bytes": 50_001,
                "omitted_chars": 47_501,
                "blob": {
                    "status": "unavailable",
                    "reason": "write_failed"
                }
            }),
            is_error: false,
            text: Some(preview.to_string()),
        }],
        metadata: None,
    };
    let window = window(vec![canonical_message]);
    let canonical_bytes = serde_json::to_vec(&window.messages[0]).unwrap();

    let first = extract_invocation_context(&window);
    let second = extract_invocation_context(&window);
    let first_provider_bytes = serde_json::to_vec(&first.messages_for_api[0]).unwrap();
    let second_provider_bytes = serde_json::to_vec(&second.messages_for_api[0]).unwrap();

    assert_eq!(first_provider_bytes, second_provider_bytes);
    assert_eq!(
        serde_json::to_vec(&window.messages[0]).unwrap(),
        canonical_bytes
    );
    let [ContentBlock::ToolResult { content, text, .. }] =
        first.messages_for_api[0].content.as_slice()
    else {
        panic!("expected provider tool result view");
    };
    assert_eq!(content.as_str(), Some(preview));
    assert!(text.is_none());
    let provider_json = String::from_utf8(first_provider_bytes).unwrap();
    assert!(!provider_json.contains("write_failed"));
    assert!(!provider_json.contains("FULL_PAYLOAD_SENTINEL"));
}

#[test]
fn structured_l2_l3_window_maps_to_identical_provider_bytes_without_rewriting_placeholders() {
    let messages = vec![Message {
        role: Role::Assistant,
        content: vec![
            ContentBlock::ToolUse {
                id: "read-call".into(),
                name: "Read".into(),
                input: serde_json::json!({"file_path": "/repo/src/lib.rs"}),
            },
            ContentBlock::ToolResult {
                tool_use_id: "read-call".into(),
                content: serde_json::json!({
                    "aemeath_context": {
                        "kind": "superseded_exploration",
                        "path": "/repo/src/lib.rs",
                        "tool": "Read"
                    }
                }),
                is_error: false,
                text: Some("[Superseded tool result: Read /repo/src/lib.rs]".into()),
            },
            ContentBlock::ToolUse {
                id: "search-call".into(),
                name: "WebSearch".into(),
                input: serde_json::json!({"query": "context"}),
            },
            ContentBlock::ToolResult {
                tool_use_id: "search-call".into(),
                content: serde_json::json!({
                    "aemeath_context": {
                        "kind": "microcompacted_exploration",
                        "tool": "WebSearch"
                    }
                }),
                is_error: false,
                text: Some("[Microcompacted tool result: WebSearch]".into()),
            },
        ],
        metadata: None,
    }];
    let window = window(messages);
    let window_bytes = serde_json::to_vec(&window.messages).unwrap();

    let first = extract_invocation_context(&window);
    let second = extract_invocation_context(&window);
    let first_bytes = serde_json::to_vec(&first.messages_for_api).unwrap();
    let second_bytes = serde_json::to_vec(&second.messages_for_api).unwrap();

    assert_eq!(first_bytes, second_bytes);
    assert_eq!(serde_json::to_vec(&window.messages).unwrap(), window_bytes);
    let provider_json = String::from_utf8(first_bytes).unwrap();
    assert!(provider_json.contains("[Superseded tool result: Read /repo/src/lib.rs]"));
    assert!(provider_json.contains("[Microcompacted tool result: WebSearch]"));
    assert!(!provider_json.contains("superseded_exploration"));
    assert!(!provider_json.contains("microcompacted_exploration"));
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
