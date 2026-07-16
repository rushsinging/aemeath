use super::*;

#[test]
fn test_compact_window_boundaries() {
    assert_eq!(compact_window(4), None);
    assert_eq!(compact_window(5), None);
    assert_eq!(compact_window(6), None);

    assert_eq!(
        compact_window(100),
        Some(CompactWindow {
            head_protect: 2,
            split_point: 90,
            keep_recent: 10,
        })
    );
}

#[test]
fn test_messages_selected_for_precompact_memory_uses_same_early_window_as_compact() {
    let messages = (0..10)
        .map(|idx| Message::user(format!("message-{idx}")))
        .collect::<Vec<_>>();

    let selected = messages_selected_for_precompact_memory(&messages);

    let selected_text = selected
        .iter()
        .map(Message::text_content)
        .collect::<Vec<_>>();
    assert_eq!(
        selected_text,
        vec![
            "message-0",
            "message-1",
            "message-2",
            "message-3",
            "message-4",
            "message-5",
        ]
    );
}

#[test]
fn test_messages_selected_for_precompact_memory_returns_empty_for_small_history() {
    let messages = vec![
        Message::user("one"),
        Message::user("two"),
        Message::user("three"),
        Message::user("four"),
    ];

    assert!(messages_selected_for_precompact_memory(&messages).is_empty());
}

#[test]
fn compact_prompt_preserves_user_requests_and_continuation_state() {
    assert!(COMPACT_PROMPT.contains("## User Requests"));
    assert!(COMPACT_PROMPT.contains("## Work Completed"));
    assert!(COMPACT_PROMPT.contains("## Problems / Findings"));
    assert!(COMPACT_PROMPT.contains("## Next Action"));
    assert!(COMPACT_PROMPT.contains("## Continuation Status"));
    assert!(COMPACT_PROMPT.contains("later corrections supersede"));
    assert!(COMPACT_PROMPT.contains("NEVER upgrade"));
}

#[test]
fn compact_request_contains_all_user_inputs_in_order() {
    let request = build_compact_request(
        &[
            Message::user("看看 issue 850"),
            Message::user("只分析，不实现"),
            Message::user("按 segment 汇总"),
        ],
        None,
        100_000,
    );
    let text = request[0].text_content();
    let inspect = text.find("看看 issue 850").unwrap();
    let no_implementation = text.find("只分析，不实现").unwrap();
    let by_segment = text.find("按 segment 汇总").unwrap();

    assert!(inspect < no_implementation);
    assert!(no_implementation < by_segment);
}

#[test]
fn compact_request_merges_previous_summary_without_duplicate_empty_prompt() {
    let request = build_compact_request(
        &[Message::user("继续检查 compact")],
        Some("earlier user request and completed work"),
        100_000,
    );

    assert_eq!(request.len(), 1);
    let text = request[0].text_content();
    assert_eq!(
        text.matches("You are a conversation history compactor")
            .count(),
        1
    );
    assert_eq!(text.matches("<conversation_history>").count(), 1);
    assert!(text.contains("<previous_summary>"));
    assert!(text.contains("earlier user request and completed work"));
    assert!(text.contains("继续检查 compact"));
}

#[test]
fn fallback_summary_latest_user_request_continues_without_claiming_completion() {
    let summary = build_summary_text(
        &[
            Message {
                role: Role::Assistant,
                content: vec![ContentBlock::Text {
                    text: "计划读取 issue，尚未执行".to_string(),
                }],
                metadata: None,
            },
            Message::user("只分析，不实现"),
        ],
        None,
    );

    assert!(summary.contains("## User Requests"));
    assert!(summary.contains("只分析，不实现"));
    assert!(summary.contains("## Work Completed"));
    assert!(summary.contains("Unverified assistant report"));
    assert!(!summary.contains("- 已完成"));
    assert!(summary.contains("## Problems / Findings"));
    assert!(summary.contains("## Next Action"));
    assert!(summary.contains("## Continuation Status"));
    assert!(summary.contains("Continue"));
}

#[test]
fn fallback_summary_waiting_for_approval_does_not_continue() {
    let summary = build_summary_text(
        &[Message {
            role: Role::Assistant,
            content: vec![ContentBlock::Text {
                text: "方案已给出，等待你确认后再修改".to_string(),
            }],
            metadata: None,
        }],
        None,
    );

    assert!(summary.contains("Waiting for User"));
    assert!(summary.contains("等待你确认"));
}

#[test]
fn fallback_summary_explicit_completion_report_waits_for_user_confirmation() {
    let summary = build_summary_text(
        &[Message {
            role: Role::Assistant,
            content: vec![ContentBlock::Text {
                text: "已完成代码修改并通过测试".to_string(),
            }],
            metadata: None,
        }],
        None,
    );

    assert!(summary.contains("Assistant-reported completion"));
    assert!(summary.contains("Waiting for User"));
    assert!(!summary.contains("\nCompleted —"));
}

#[test]
fn fallback_summary_negated_completion_is_not_treated_as_completed() {
    for text in [
        "work is not completed",
        "branch is not merged",
        "修改尚未完成",
        "没有合入",
    ] {
        let summary = build_summary_text(
            &[Message {
                role: Role::Assistant,
                content: vec![ContentBlock::Text {
                    text: text.to_string(),
                }],
                metadata: None,
            }],
            None,
        );

        assert!(
            !summary.contains("Assistant-reported completion"),
            "{text} must not be classified as completion"
        );
        assert!(summary.contains("Waiting for User"));
        assert!(!summary.contains("\nCompleted —"));
    }
}

#[tokio::test]
async fn second_compact_fallback_preserves_previous_summary() {
    let messages = (0..10)
        .map(|index| Message::user(format!("message-{index}")))
        .collect::<Vec<_>>();
    let cancel = CancellationToken::new();

    let result = compact_messages_with_llm(
        &messages,
        Some("first compact summary with original user request"),
        100_000,
        None,
        None,
        &cancel,
    )
    .await
    .expect("second compact should run");

    assert!(
        result
            .summary
            .contains("first compact summary with original user request"),
        "second compact must retain the previous active summary"
    );
}
