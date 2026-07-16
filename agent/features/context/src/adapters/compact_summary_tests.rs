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
        vec!["message-2", "message-3", "message-4", "message-5"]
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
