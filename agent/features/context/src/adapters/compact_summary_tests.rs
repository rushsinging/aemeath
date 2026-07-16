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
fn fallback_summary_preserves_user_request_and_continuation_schema() {
    let summary = build_summary_text(&[
        Message::user("看看 issue 850"),
        Message {
            role: Role::Assistant,
            content: vec![ContentBlock::Text {
                text: "已读取 issue 元数据".to_string(),
            }],
            metadata: None,
        },
    ]);

    assert!(summary.contains("## User Requests"));
    assert!(summary.contains("看看 issue 850"));
    assert!(summary.contains("## Work Completed"));
    assert!(summary.contains("已读取 issue 元数据"));
    assert!(summary.contains("## Problems / Findings"));
    assert!(summary.contains("## Next Action"));
    assert!(summary.contains("## Continuation Status"));
    assert!(summary.contains("Continue"));
}
