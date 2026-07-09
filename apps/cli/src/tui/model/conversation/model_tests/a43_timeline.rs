// --- A4.3 基线测试：位置查询改读 timeline ---

/// A4.3 TDD 基线：insert_tool_call_block_before_active_text 用 timeline 去重
/// 同一 (chat, turn, id) 的 ToolCall 重复触发后，timeline 中只有 1 个 ToolCall ref。
#[test]
fn test_a43_insert_tool_call_dedup_reads_timeline() {
    let mut model = ConversationModel::default();
    let chat_id = super::ids::ChatId::new("chat-a43-dedup");
    let turn_id = super::ids::ChatTurnId::new("turn-a43-dedup");
    let tool_id = super::ids::ToolCallId::new("tool-a43-dedup");

    model.ensure_runtime_turn(chat_id.clone(), turn_id.clone());

    // ToolCallStart → 第 1 次插入
    model.apply(ToolCallStart {
        chat_id: chat_id.clone(),
        turn_id: turn_id.clone(),
        id: tool_id.clone(),
        provider_id: None,
        name: "Read".to_string(),
        index: 0,
    });

    // ToolCallUpdate with same id → 不应重复插入 ToolCall
    model.apply(ToolCallUpdate {
        chat_id: chat_id.clone(),
        turn_id: turn_id.clone(),
        id: tool_id.clone(),
        provider_id: None,
        name: "Read".to_string(),
        index: 0,
        arguments: Some(r#"{"file_path":"main.rs"}"#.to_string()),
        status: ToolCallStatus::Ready,
    });

    let tool_call_count = model
        .timeline
        .items()
        .iter()
        .filter(|item| {
            matches!(
                item,
                OutputTimelineItem::ToolCall { reference }
                    if reference.context.chat_id == chat_id
                        && reference.context.turn_id == turn_id
                        && reference.tool_call_id == tool_id
            )
        })
        .count();

    assert_eq!(
        tool_call_count, 1,
        "timeline 中相同 (chat, turn, id) 的 ToolCall 应仅出现 1 次（去重）"
    );
}

/// A4.3 TDD 基线：promote_orphan_tool_result 后 timeline 中孤儿消失、ToolResult 出现
/// 且 ToolResult 排在 ToolCall 之后（顺序等价）。
#[test]
fn test_a43_promote_orphan_timeline_ordering() {
    let mut model = ConversationModel::default();
    let chat_id = super::ids::ChatId::new("chat-a43-orphan");
    let turn_id = super::ids::ChatTurnId::new("turn-a43-orphan");
    let tool_id = super::ids::ToolCallId::new("tool-a43-orphan");

    model.ensure_runtime_turn(chat_id.clone(), turn_id.clone());

    // 先到达 ToolResult（孤儿）
    let changes = model.apply(ToolResult {
        chat_id: chat_id.clone(),
        turn_id: turn_id.clone(),
        provider_id: "prov-a43".to_string(),
        id: tool_id.clone(),
        tool_name: "Bash".to_string(),
        output: "output-a43".to_string(),
        content: serde_json::json!({}),
        is_error: false,
        image_count: 0,
    });
    assert!(
        changes
            .iter()
            .any(|c| matches!(c, ConversationChange::OrphanToolResultObserved { .. })),
        "先到达的 ToolResult 应为孤儿"
    );

    // 验证孤儿在 timeline 中
    let has_orphan = model.timeline.items().iter().any(|item| {
        matches!(item, OutputTimelineItem::OrphanToolResult { id, .. } if id == tool_id.as_ref())
    });
    assert!(has_orphan, "孤儿应出现在 timeline 中");

    // ToolCallStart → 触发 promote_orphan_tool_result
    model.apply(ToolCallStart {
        chat_id: chat_id.clone(),
        turn_id: turn_id.clone(),
        id: tool_id.clone(),
        provider_id: None,
        name: "Bash".to_string(),
        index: 0,
    });

    // ToolCallUpdate → 触发 promote_orphan_tool_result（confirm binding）
    model.apply(ToolCallUpdate {
        chat_id: chat_id.clone(),
        turn_id: turn_id.clone(),
        id: tool_id.clone(),
        provider_id: Some("prov-a43".to_string()),
        name: "Bash".to_string(),
        index: 0,
        arguments: None,
        status: ToolCallStatus::Ready,
    });

    // 孤儿应从 timeline 中消失
    let still_orphan = model.timeline.items().iter().any(|item| {
        matches!(item, OutputTimelineItem::OrphanToolResult { id, .. } if id == tool_id.as_ref())
    });
    assert!(!still_orphan, "孤儿提升后应从 timeline 移除");

    // ToolCall 和 ToolResult 均应存在于 timeline，且顺序正确
    let positions: Vec<_> = model
        .timeline
        .items()
        .iter()
        .enumerate()
        .filter_map(|(idx, item)| match item {
            OutputTimelineItem::ToolCall { reference }
            | OutputTimelineItem::ToolResult { reference }
                if reference.tool_call_id == tool_id =>
            {
                Some(idx)
            }
            _ => None,
        })
        .collect();

    assert_eq!(positions.len(), 2, "应有 ToolCall + ToolResult 各一个");
    assert!(
        positions[0] < positions[1],
        "孤儿提升后 ToolResult 应排在 ToolCall 之后（顺序等价）"
    );
}

/// A4.3 TDD 基线：update_tool_call 中已存在的 ToolCall 不重复插入 timeline。
/// 复现 model.rs:313 的查询点：existing_tool_position 改读 timeline。
#[test]
fn test_a43_update_tool_call_no_duplicate_timeline_entry() {
    let mut model = ConversationModel::default();
    let chat_id = super::ids::ChatId::new("chat-a43-dup");
    let turn_id = super::ids::ChatTurnId::new("turn-a43-dup");
    let tool_id = super::ids::ToolCallId::new("tool-a43-dup");

    model.ensure_runtime_turn(chat_id.clone(), turn_id.clone());

    // First: ToolCallStart inserts the tool
    model.apply(ToolCallStart {
        chat_id: chat_id.clone(),
        turn_id: turn_id.clone(),
        id: tool_id.clone(),
        provider_id: None,
        name: "Write".to_string(),
        index: 0,
    });

    // Multiple ToolCallUpdates with same id — should NOT create duplicate ToolCall entries
    for _ in 0..3 {
        model.apply(ToolCallUpdate {
            chat_id: chat_id.clone(),
            turn_id: turn_id.clone(),
            id: tool_id.clone(),
            provider_id: Some("prov-a43-dup".to_string()),
            name: "Write".to_string(),
            index: 0,
            arguments: Some(r#"{"file_path":"a.txt"}"#.to_string()),
            status: ToolCallStatus::Ready,
        });
    }

    let count = model
        .timeline
        .items()
        .iter()
        .filter(|item| {
            matches!(
                item,
                OutputTimelineItem::ToolCall { reference }
                    if reference.context.chat_id == chat_id
                       && reference.context.turn_id == turn_id
                       && reference.tool_call_id == tool_id
            )
        })
        .count();

    assert_eq!(
        count, 1,
        "多次 ToolCallUpdate 后 timeline 中该 ToolCall 仍只有 1 项"
    );
}
