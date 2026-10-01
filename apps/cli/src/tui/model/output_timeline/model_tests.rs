use super::*;

fn chat() -> ChatId {
    ChatId::new("chat-1")
}

fn turn() -> ChatRunId {
    ChatRunId::new("turn-1")
}

#[test]
fn test_push_tool_call_ref_is_idempotent_for_same_context() {
    let mut model = OutputTimelineModel::default();
    model.push_tool_call_ref(chat(), turn(), ToolCallId::new("tool-1"));
    model.push_tool_call_ref(chat(), turn(), ToolCallId::new("tool-1"));
    assert_eq!(model.items().len(), 1);
}

#[test]
fn test_push_tool_call_ref_allows_same_id_different_turn() {
    let mut model = OutputTimelineModel::default();
    model.push_tool_call_ref(chat(), ChatRunId::new("turn-a"), ToolCallId::new("tool-1"));
    model.push_tool_call_ref(chat(), ChatRunId::new("turn-b"), ToolCallId::new("tool-1"));
    assert_eq!(model.items().len(), 2);
}

#[test]
fn test_move_tool_result_after_tool_call_reorders_matching_context_only() {
    let mut model = OutputTimelineModel::default();
    model.push(OutputTimelineItem::ToolResult {
        reference: TimelineToolCallRef::new(chat(), turn(), ToolCallId::new("tool-1")),
    });
    model.push_tool_call_ref(chat(), turn(), ToolCallId::new("tool-1"));
    model.move_tool_result_after_tool_call(&chat(), &turn(), &ToolCallId::new("tool-1"));
    assert!(matches!(
        model.items()[0],
        OutputTimelineItem::ToolCall { .. }
    ));
    assert!(matches!(
        model.items()[1],
        OutputTimelineItem::ToolResult { .. }
    ));
}

#[test]
fn tool_ref_index_stays_consistent_across_push_and_move() {
    let mut model = OutputTimelineModel::default();
    let chat = ChatId::new("chat-1");
    let turn = ChatRunId::new("turn-1");
    let tool = ToolCallId::new("tool-1");

    model.push_tool_call_ref(chat.clone(), turn.clone(), tool.clone());
    assert!(model.contains_tool_call(&chat, &turn, tool.as_ref()));
    assert!(!model.contains_tool_result(&chat, &turn, tool.as_ref()));

    model.push_tool_result_ref(chat.clone(), turn.clone(), tool.clone());
    assert!(model.contains_tool_call(&chat, &turn, tool.as_ref()));
    assert!(model.contains_tool_result(&chat, &turn, tool.as_ref()));

    // move 只搬移位置不增删，索引必须保持（remove+insert 后仍命中）。
    model.move_tool_result_after_tool_call(&chat, &turn, &tool);
    assert!(model.contains_tool_call(&chat, &turn, tool.as_ref()));
    assert!(model.contains_tool_result(&chat, &turn, tool.as_ref()));
}

#[test]
fn tool_ref_index_rebuilds_after_retain() {
    let mut model = OutputTimelineModel::default();
    let chat = ChatId::new("chat-1");
    let turn = ChatRunId::new("turn-1");
    let keep_tool = ToolCallId::new("tool-keep");
    let drop_tool = ToolCallId::new("tool-drop");

    model.push_tool_call_ref(chat.clone(), turn.clone(), keep_tool.clone());
    model.push_tool_call_ref(chat.clone(), turn.clone(), drop_tool.clone());
    assert!(model.contains_tool_call(&chat, &turn, drop_tool.as_ref()));

    model.retain(|item| {
        !matches!(item, OutputTimelineItem::ToolCall { reference }
                if reference.tool_call_id == drop_tool)
    });

    assert!(model.contains_tool_call(&chat, &turn, keep_tool.as_ref()));
    assert!(!model.contains_tool_call(&chat, &turn, drop_tool.as_ref()));
}

#[test]
fn orphan_ids_index_tracks_pushed_and_retained_items() {
    let mut model = OutputTimelineModel::default();

    model.push(OutputTimelineItem::OrphanToolResult {
        id: "orphan-1".to_string(),
        tool_name: "Bash".to_string(),
        output: "out".to_string(),
        content: serde_json::json!({}),
        is_error: false,
        duration_ms: None,
    });
    assert!(model.contains_orphan("orphan-1"));

    model.retain(|item| !matches!(item, OutputTimelineItem::OrphanToolResult { .. }));
    assert!(!model.contains_orphan("orphan-1"));
}
