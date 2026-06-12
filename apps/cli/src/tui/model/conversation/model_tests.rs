use super::change::ConversationChange;
use super::intent::ConversationIntent;
use super::model::ConversationModel;
use super::tool_call::ToolCallStatus;

#[test]
fn test_conversation_observes_tool_lifecycle() {
    let mut model = ConversationModel::default();
    let changes = model.apply(ConversationIntent::StartChat {
        submission: "read file".to_string(),
    });
    assert!(changes
        .iter()
        .any(|change| matches!(change, ConversationChange::ChatStarted { .. })));

    model.apply(ConversationIntent::ObserveToolCallStart {
        id: "tool-1".to_string(),
        provider_id: None,
        name: "Read".to_string(),
        index: 0,
    });
    model.apply(ConversationIntent::ObserveToolCallUpdate {
        provider_id: Some("provider-1".to_string()),
        id: "tool-1".to_string(),
        name: "Read".to_string(),
        index: 0,
        summary: Some("Read file".to_string()),
        arguments: None,
        status: ToolCallStatus::Ready,
    });
    let changes = model.apply(ConversationIntent::ObserveToolResult {
        provider_id: "provider-1".to_string(),
        id: "tool-1".to_string(),
        tool_name: "Read".to_string(),
        output: "ok".to_string(),
        content: serde_json::json!({ "text": "test output" }),
        is_error: false,
        image_count: 0,
    });

    assert!(changes.iter().any(|change| matches!(
        change,
        ConversationChange::ToolCallCompleted { status, .. } if *status == ToolCallStatus::Success
    )));
}

#[test]
fn test_conversation_reports_orphan_tool_result() {
    let mut model = ConversationModel::default();
    model.apply(ConversationIntent::StartChat {
        submission: "read file".to_string(),
    });
    let changes = model.apply(ConversationIntent::ObserveToolResult {
        provider_id: "provider-1".to_string(),
        id: "missing".to_string(),
        tool_name: "Read".to_string(),
        output: "late".to_string(),
        content: serde_json::json!({ "text": "test output" }),
        is_error: false,
        image_count: 0,
    });
    assert!(changes.iter().any(|change| matches!(
        change,
        ConversationChange::OrphanToolResultObserved { id } if id == "missing"
    )));
}

#[test]
fn test_conversation_reused_runtime_ids_across_turns_do_not_overwrite_earlier_blocks() {
    let mut model = ConversationModel::default();
    model.apply(ConversationIntent::StartChat {
        submission: "load first skill".to_string(),
    });
    model.apply(ConversationIntent::ObserveToolCallStart {
        id: "tool-1".to_string(),
        provider_id: None,
        name: "Skill".to_string(),
        index: 0,
    });
    model.apply(ConversationIntent::ObserveToolCallUpdate {
        id: "tool-1".to_string(),
        provider_id: None,
        name: "Skill".to_string(),
        index: 0,
        arguments: Some(r#"{"skill":"superpowers:using-superpowers"}"#.to_string()),
        summary: None,
        status: ToolCallStatus::Ready,
    });
    model.apply(ConversationIntent::ObserveToolCallUpdate {
        provider_id: Some("call-using".to_string()),
        id: "tool-1".to_string(),
        name: "Skill".to_string(),
        index: 0,
        summary: Some(String::new()),
        arguments: None,
        status: ToolCallStatus::Ready,
    });
    model.apply(ConversationIntent::CompleteChat);

    model.apply(ConversationIntent::StartChat {
        submission: "load second skill".to_string(),
    });
    model.apply(ConversationIntent::ObserveToolCallStart {
        id: "tool-3".to_string(),
        provider_id: None,
        name: "Skill".to_string(),
        index: 0,
    });
    model.apply(ConversationIntent::ObserveToolCallUpdate {
        id: "tool-3".to_string(),
        provider_id: None,
        name: "Skill".to_string(),
        index: 0,
        arguments: Some(r#"{"skill":"superpowers:brainstorming"}"#.to_string()),
        summary: None,
        status: ToolCallStatus::Ready,
    });
    model.apply(ConversationIntent::ObserveToolCallUpdate {
        provider_id: Some("call-brainstorm".to_string()),
        id: "tool-3".to_string(),
        name: "Skill".to_string(),
        index: 0,
        summary: Some(String::new()),
        arguments: None,
        status: ToolCallStatus::Ready,
    });

    let summaries: Vec<_> = model
        .blocks
        .iter()
        .filter_map(|block| match block {
            super::block::ConversationBlock::ToolCall { name, summary, .. } if name == "Skill" => {
                Some(summary.as_str())
            }
            _ => None,
        })
        .collect();

    assert_eq!(summaries.len(), 2);
    assert!(summaries[0].contains("superpowers:using-superpowers"));
    assert!(summaries[1].contains("superpowers:brainstorming"));
}

#[test]
fn test_conversation_repeated_runtime_id_result_does_not_complete_previous_provider_tool() {
    let mut model = ConversationModel::default();
    model.apply(ConversationIntent::StartChat {
        submission: "load skill".to_string(),
    });
    model.apply(ConversationIntent::ObserveToolCallStart {
        id: "tool-1".to_string(),
        provider_id: Some("call-skill".to_string()),
        name: "Skill".to_string(),
        index: 0,
    });
    model.apply(ConversationIntent::ObserveToolCallUpdate {
        id: "tool-1".to_string(),
        provider_id: Some("call-skill".to_string()),
        name: "Skill".to_string(),
        index: 0,
        arguments: Some(r#"{"skill":"superpowers:using-superpowers"}"#.to_string()),
        summary: None,
        status: ToolCallStatus::Ready,
    });
    model.apply(ConversationIntent::ObserveToolCallUpdate {
        provider_id: Some("call-skill".to_string()),
        id: "tool-1".to_string(),
        name: "Skill".to_string(),
        index: 0,
        summary: Some(String::new()),
        arguments: None,
        status: ToolCallStatus::Ready,
    });
    model.apply(ConversationIntent::CompleteChat);

    model.apply(ConversationIntent::StartChat {
        submission: "read config".to_string(),
    });
    model.apply(ConversationIntent::ObserveToolResult {
        id: "tool-2".to_string(),
        provider_id: "call-read".to_string(),
        tool_name: "Read".to_string(),
        output: "//! Configuration file management".to_string(),
        content: serde_json::json!({ "text": "test output" }),
        is_error: false,
        image_count: 0,
    });
    model.apply(ConversationIntent::ObserveToolCallUpdate {
        provider_id: Some("call-read".to_string()),
        id: "tool-2".to_string(),
        name: "Read".to_string(),
        index: 0,
        summary: Some(r#"{"file_path":"agent/shared/src/config.rs"}"#.to_string()),
        arguments: None,
        status: ToolCallStatus::Ready,
    });

    let skill_result = model.chats[0].turns[0].tool_calls[0].result.as_deref();
    assert_ne!(
        skill_result,
        Some("//! Configuration file management"),
        "Read 结果不应写入上一轮 Skill"
    );
    assert!(model.blocks.iter().any(|block| matches!(
        block,
        super::block::ConversationBlock::ToolCall { id, name, .. }
            if id.as_ref() == "tool-2" && name == "Read"
    )));
    assert!(model.blocks.iter().any(|block| matches!(
        block,
        super::block::ConversationBlock::ToolResult { id, output, .. }
            if id.as_ref() == "tool-2" && output.contains("Configuration file management")
    )));
}

#[test]
fn test_conversation_binds_tool_call_by_provider_id_when_runtime_id_changed() {
    let mut model = ConversationModel::default();
    model.apply(ConversationIntent::StartChat {
        submission: "load skill".to_string(),
    });
    model.apply(ConversationIntent::ObserveToolCallStart {
        id: "call-provider-skill".to_string(),
        provider_id: Some("call-provider-skill".to_string()),
        name: "Skill".to_string(),
        index: 0,
    });
    model.apply(ConversationIntent::ObserveToolCallUpdate {
        id: "call-provider-skill".to_string(),
        provider_id: Some("call-provider-skill".to_string()),
        name: "Skill".to_string(),
        index: 0,
        arguments: Some(r#"{"skill":"superpowers:brainstorming"}"#.to_string()),
        summary: None,
        status: ToolCallStatus::Ready,
    });
    model.apply(ConversationIntent::ObserveToolCallUpdate {
        provider_id: Some("call-provider-skill".to_string()),
        id: "tool-99".to_string(),
        name: "Skill".to_string(),
        index: 0,
        summary: Some(String::new()),
        arguments: None,
        status: ToolCallStatus::Ready,
    });

    let tool_blocks: Vec<_> = model
        .blocks
        .iter()
        .filter_map(|block| match block {
            super::block::ConversationBlock::ToolCall { id, summary, .. } => {
                Some((id.as_ref(), summary.as_str()))
            }
            _ => None,
        })
        .collect();

    assert_eq!(tool_blocks.len(), 1);
    assert_eq!(tool_blocks[0].0, "call-provider-skill");
    assert!(tool_blocks[0].1.contains("superpowers:brainstorming"));
}

#[test]
fn test_conversation_preserves_streamed_args_when_tool_call_summary_is_empty() {
    let mut model = ConversationModel::default();
    model.apply(ConversationIntent::StartChat {
        submission: "load skill".to_string(),
    });
    model.apply(ConversationIntent::ObserveToolCallStart {
        id: "call-skill".to_string(),
        provider_id: None,
        name: "Skill".to_string(),
        index: 0,
    });
    model.apply(ConversationIntent::ObserveToolCallUpdate {
        id: "call-skill".to_string(),
        provider_id: None,
        name: "Skill".to_string(),
        index: 0,
        arguments: Some(r#"{"skill":"superpowers:using-superpowers"}"#.to_string()),
        summary: None,
        status: ToolCallStatus::Ready,
    });
    model.apply(ConversationIntent::ObserveToolCallUpdate {
        provider_id: Some("provider-skill".to_string()),
        id: "call-skill".to_string(),
        name: "Skill".to_string(),
        index: 0,
        summary: Some(String::new()),
        arguments: None,
        status: ToolCallStatus::Ready,
    });

    assert_eq!(
        model.chats[0].turns[0].tool_calls[0].summary.as_deref(),
        Some(r#"{"skill":"superpowers:using-superpowers"}"#)
    );
    assert!(model.blocks.iter().any(|block| matches!(
        block,
        super::block::ConversationBlock::ToolCall { summary, .. }
            if summary == r#"{"skill":"superpowers:using-superpowers"}"#
    )));
}

#[test]
fn test_conversation_keeps_distinct_task_tool_blocks_after_empty_summary_bind() {
    let mut model = ConversationModel::default();
    model.apply(ConversationIntent::StartChat {
        submission: "create tasks".to_string(),
    });
    model.apply(ConversationIntent::ObserveToolCallStart {
        id: "call-list".to_string(),
        provider_id: None,
        name: "TaskListCreate".to_string(),
        index: 0,
    });
    model.apply(ConversationIntent::ObserveToolCallUpdate {
        id: "call-list".to_string(),
        provider_id: None,
        name: "TaskListCreate".to_string(),
        index: 0,
        arguments: Some(r#"{"subject":"修复显示","summary":"排查 tool call"}"#.to_string()),
        summary: None,
        status: ToolCallStatus::Ready,
    });
    model.apply(ConversationIntent::ObserveToolCallUpdate {
        provider_id: Some("provider-list".to_string()),
        id: "call-list".to_string(),
        name: "TaskListCreate".to_string(),
        index: 0,
        summary: Some(String::new()),
        arguments: None,
        status: ToolCallStatus::Ready,
    });
    model.apply(ConversationIntent::ObserveToolCallStart {
        id: "call-task".to_string(),
        provider_id: None,
        name: "TaskCreate".to_string(),
        index: 1,
    });
    model.apply(ConversationIntent::ObserveToolCallUpdate {
        id: "call-task".to_string(),
        provider_id: None,
        name: "TaskCreate".to_string(),
        index: 1,
        arguments: Some(r#"{"subject":"写测试","description":"复现 TaskCreate 显示"}"#.to_string()),
        summary: None,
        status: ToolCallStatus::Ready,
    });
    model.apply(ConversationIntent::ObserveToolCallUpdate {
        provider_id: Some("provider-task".to_string()),
        id: "call-task".to_string(),
        name: "TaskCreate".to_string(),
        index: 1,
        summary: Some(String::new()),
        arguments: None,
        status: ToolCallStatus::Ready,
    });

    let tool_blocks: Vec<_> = model
        .blocks
        .iter()
        .filter_map(|block| match block {
            super::block::ConversationBlock::ToolCall {
                id, name, summary, ..
            } => Some((id.as_ref(), name.as_str(), summary.as_str())),
            _ => None,
        })
        .collect();

    assert_eq!(tool_blocks.len(), 2, "应保留两个独立 tool call block");
    assert!(tool_blocks.iter().any(|(id, name, summary)| {
        *id == "call-list" && *name == "TaskListCreate" && summary.contains("修复显示")
    }));
    assert!(tool_blocks.iter().any(|(id, name, summary)| {
        *id == "call-task" && *name == "TaskCreate" && summary.contains("写测试")
    }));
}

#[test]
fn test_conversation_late_tool_call_binds_existing_result() {
    let mut model = ConversationModel::default();
    model.apply(ConversationIntent::StartChat {
        submission: "read file".to_string(),
    });
    model.apply(ConversationIntent::ObserveToolCallStart {
        id: "tool-1".to_string(),
        provider_id: None,
        name: "Read".to_string(),
        index: 0,
    });
    model.apply(ConversationIntent::ObserveToolResult {
        provider_id: "provider-1".to_string(),
        id: "tool-1".to_string(),
        tool_name: "Read".to_string(),
        output: "line1\nline2".to_string(),
        content: serde_json::json!({ "text": "test output" }),
        is_error: false,
        image_count: 0,
    });
    model.apply(ConversationIntent::ObserveToolCallUpdate {
        provider_id: Some("provider-1".to_string()),
        id: "tool-1".to_string(),
        name: "Read".to_string(),
        index: 0,
        summary: Some("Read file".to_string()),
        arguments: None,
        status: ToolCallStatus::Ready,
    });

    assert!(!model.blocks.iter().any(|block| matches!(
        block,
        super::block::ConversationBlock::OrphanToolResult { id, .. } if id == "tool-1"
    )));
    assert!(model.blocks.iter().any(|block| matches!(
        block,
        super::block::ConversationBlock::ToolResult { id, .. } if id.as_ref() == "tool-1"
    )));
    assert_eq!(
        model.chats[0].turns[0].tool_calls[0].result.as_deref(),
        Some("line1\nline2")
    );
    assert_eq!(
        model.chats[0].turns[0].tool_calls[0].status,
        ToolCallStatus::Success
    );
}

#[test]
fn test_conversation_streams_text_and_thinking_into_blocks() {
    let mut model = ConversationModel::default();
    model.apply(ConversationIntent::StartChat {
        submission: "hello".to_string(),
    });
    model.apply(ConversationIntent::ObserveThinkingText {
        text: "plan".to_string(),
    });
    model.apply(ConversationIntent::ObserveAssistantText {
        text: "answer".to_string(),
    });

    assert!(model.blocks.iter().any(|block| matches!(
        block,
        super::block::ConversationBlock::Thinking { text, .. } if text == "plan"
    )));
    assert!(model.blocks.iter().any(|block| matches!(
        block,
        super::block::ConversationBlock::AssistantText { text, .. } if text == "answer"
    )));
}

#[test]
fn test_conversation_places_late_tool_call_before_pending_assistant_text() {
    let mut model = ConversationModel::default();
    model.apply(ConversationIntent::StartChat {
        submission: "check docs".to_string(),
    });
    model.apply(ConversationIntent::ObserveAssistantText {
        text: "结论先到".to_string(),
    });
    model.apply(ConversationIntent::ObserveToolCallStart {
        id: "tool-1".to_string(),
        provider_id: None,
        name: "Read".to_string(),
        index: 0,
    });
    model.apply(ConversationIntent::ObserveToolCallUpdate {
        provider_id: Some("provider-1".to_string()),
        id: "tool-1".to_string(),
        name: "Read".to_string(),
        index: 0,
        summary: Some("Read docs".to_string()),
        arguments: None,
        status: ToolCallStatus::Ready,
    });

    let tool_pos = model
        .blocks
        .iter()
        .position(|block| {
            matches!(
                block,
                super::block::ConversationBlock::ToolCall { id, .. } if id.as_ref() == "tool-1"
            )
        })
        .expect("tool block");
    let text_pos = model
        .blocks
        .iter()
        .position(|block| {
            matches!(
                block,
                super::block::ConversationBlock::AssistantText { text, .. } if text == "结论先到"
            )
        })
        .expect("assistant text block");

    assert!(
        tool_pos < text_pos,
        "后到达的 tool call 应显示在待完成文本块之前"
    );
}

#[test]
fn test_conversation_keeps_tool_after_completed_assistant_text() {
    let mut model = ConversationModel::default();
    model.apply(ConversationIntent::StartChat {
        submission: "check docs".to_string(),
    });
    model.apply(ConversationIntent::ObserveAssistantText {
        text: "已经完成的文字".to_string(),
    });
    model.apply(ConversationIntent::CompleteTextBlock);
    model.apply(ConversationIntent::ObserveToolCallStart {
        id: "tool-1".to_string(),
        provider_id: None,
        name: "Read".to_string(),
        index: 0,
    });
    model.apply(ConversationIntent::ObserveToolCallUpdate {
        provider_id: Some("provider-1".to_string()),
        id: "tool-1".to_string(),
        name: "Read".to_string(),
        index: 0,
        summary: Some("Read docs".to_string()),
        arguments: None,
        status: ToolCallStatus::Ready,
    });

    let text_pos = model
        .blocks
        .iter()
        .position(|block| matches!(
            block,
            super::block::ConversationBlock::AssistantText { text, .. } if text == "已经完成的文字"
        ))
        .expect("assistant text block");
    let tool_pos = model
        .blocks
        .iter()
        .position(|block| {
            matches!(
                block,
                super::block::ConversationBlock::ToolCall { id, .. } if id.as_ref() == "tool-1"
            )
        })
        .expect("tool block");

    assert!(text_pos < tool_pos, "已完成文本块不应被后续工具调用重排");
}

#[test]
fn test_conversation_places_tool_result_after_late_bound_tool_call() {
    let mut model = ConversationModel::default();
    model.apply(ConversationIntent::StartChat {
        submission: "read docs".to_string(),
    });
    model.apply(ConversationIntent::ObserveToolResult {
        provider_id: "provider-1".to_string(),
        id: "tool-1".to_string(),
        tool_name: "Read".to_string(),
        output: "file contents".to_string(),
        content: serde_json::json!({ "text": "test output" }),
        is_error: false,
        image_count: 0,
    });
    model.apply(ConversationIntent::ObserveToolCallStart {
        id: "tool-1".to_string(),
        provider_id: None,
        name: "Read".to_string(),
        index: 0,
    });
    model.apply(ConversationIntent::ObserveToolCallUpdate {
        provider_id: Some("provider-1".to_string()),
        id: "tool-1".to_string(),
        name: "Read".to_string(),
        index: 0,
        summary: Some("Read docs".to_string()),
        arguments: None,
        status: ToolCallStatus::Ready,
    });

    let tool_pos = model
        .blocks
        .iter()
        .position(|block| {
            matches!(
                block,
                super::block::ConversationBlock::ToolCall { id, .. } if id.as_ref() == "tool-1"
            )
        })
        .expect("tool block");
    let result_pos = model
        .blocks
        .iter()
        .position(|block| {
            matches!(
                block,
                super::block::ConversationBlock::ToolResult { id, .. } if id.as_ref() == "tool-1"
            )
        })
        .expect("tool result block");

    assert!(tool_pos < result_pos, "工具结果不应显示在工具标题之前");
}

#[test]
fn test_conversation_keeps_tool_result_after_existing_tool_call() {
    let mut model = ConversationModel::default();
    model.apply(ConversationIntent::StartChat {
        submission: "read docs".to_string(),
    });
    model.apply(ConversationIntent::ObserveToolCallStart {
        id: "tool-1".to_string(),
        provider_id: None,
        name: "Read".to_string(),
        index: 0,
    });
    model.apply(ConversationIntent::ObserveToolCallUpdate {
        provider_id: Some("provider-1".to_string()),
        id: "tool-1".to_string(),
        name: "Read".to_string(),
        index: 0,
        summary: Some("Read docs".to_string()),
        arguments: None,
        status: ToolCallStatus::Ready,
    });
    model.apply(ConversationIntent::ObserveToolResult {
        provider_id: "provider-1".to_string(),
        id: "tool-1".to_string(),
        tool_name: "Read".to_string(),
        output: "file contents".to_string(),
        content: serde_json::json!({ "text": "test output" }),
        is_error: false,
        image_count: 0,
    });

    let positions: Vec<_> = model
        .blocks
        .iter()
        .enumerate()
        .filter_map(|(index, block)| match block {
            super::block::ConversationBlock::ToolCall { id, .. }
            | super::block::ConversationBlock::ToolResult { id, .. }
                if id.as_ref() == "tool-1" =>
            {
                Some(index)
            }
            _ => None,
        })
        .collect();

    assert_eq!(positions.len(), 2);
    assert!(positions[0] < positions[1]);
}

#[test]
fn test_queue_submission_pushes_queued_user_message_block() {
    // 正常路径：排队提交经 ConversationModel 进入 QueuedUserMessage 块（取代旧
    // OutputArea::queued_messages 命令式显示路径）。
    let mut model = ConversationModel::default();
    let changes = model.apply(ConversationIntent::QueueSubmission {
        text: "排队的消息".to_string(),
    });

    assert!(changes
        .iter()
        .any(|c| matches!(c, ConversationChange::QueuedSubmissionAdded { .. })));
    assert!(model.blocks.iter().any(|block| matches!(
        block,
        super::block::ConversationBlock::QueuedUserMessage { text, .. } if text == "排队的消息"
    )));
    assert_eq!(model.queued_submissions.len(), 1);
}

#[test]
fn test_clear_queued_submissions_removes_blocks() {
    // 边界 + 清理：冲刷队列后 QueuedUserMessage 块应被全部移除。
    let mut model = ConversationModel::default();
    model.apply(ConversationIntent::QueueSubmission {
        text: "a".to_string(),
    });
    model.apply(ConversationIntent::QueueSubmission {
        text: "b".to_string(),
    });

    let changes = model.apply(ConversationIntent::ClearQueuedSubmissions);

    assert!(changes.iter().any(|c| matches!(
        c,
        ConversationChange::QueuedSubmissionsCleared { count } if *count == 2
    )));
    assert!(model.queued_submissions.is_empty());
    assert!(!model.blocks.iter().any(|block| matches!(
        block,
        super::block::ConversationBlock::QueuedUserMessage { .. }
    )));
}

#[test]
fn test_clear_queued_submissions_on_empty_is_noop() {
    // 错误/空路径：无排队项时清理返回 count=0，不 panic。
    let mut model = ConversationModel::default();
    let changes = model.apply(ConversationIntent::ClearQueuedSubmissions);

    assert!(changes.iter().any(|c| matches!(
        c,
        ConversationChange::QueuedSubmissionsCleared { count } if *count == 0
    )));
}

#[test]
fn test_conversation_keeps_tool_args_preview() {
    let mut model = ConversationModel::default();
    model.apply(ConversationIntent::StartChat {
        submission: "read file".to_string(),
    });
    model.apply(ConversationIntent::ObserveToolCallStart {
        id: "tool-1".to_string(),
        provider_id: None,
        name: "Read".to_string(),
        index: 0,
    });
    model.apply(ConversationIntent::ObserveToolCallUpdate {
        id: "tool-1".to_string(),
        provider_id: None,
        name: "Read".to_string(),
        index: 0,
        arguments: Some(r#"{"file_path":"src/main.rs"}"#.to_string()),
        summary: None,
        status: ToolCallStatus::Ready,
    });
    model.apply(ConversationIntent::ObserveToolCallUpdate {
        provider_id: Some("provider-1".to_string()),
        id: "tool-1".to_string(),
        name: "Read".to_string(),
        index: 0,
        summary: Some("Read file".to_string()),
        arguments: None,
        status: ToolCallStatus::Ready,
    });

    assert!(model.blocks.iter().any(|block| matches!(
        block,
        super::block::ConversationBlock::ToolCall { args_preview, .. }
            if args_preview.contains("src/main.rs")
    )));
}

#[test]
fn test_agent_tool_result_not_orphan_with_index_mismatch() {
    // #95 场景：LLM 返回 text + tool_use 时，ToolCallStart 用纯 tool 序号 (0)，
    // ToolCall 用 content_block index (1)。验证 Agent tool result 不因此变成 orphan。
    let mut model = ConversationModel::default();
    model.apply(ConversationIntent::StartChat {
        submission: "review code".to_string(),
    });
    // LLM 先输出 assistant text（content_block 0）
    model.apply(ConversationIntent::ObserveAssistantText {
        text: "让我来审查".to_string(),
    });
    model.apply(ConversationIntent::CompleteTextBlock);
    // ToolCallStart 用纯 tool 序号 index=0
    model.apply(ConversationIntent::ObserveToolCallStart {
        id: "tool-1".to_string(),
        provider_id: None,
        name: "Agent".to_string(),
        index: 0,
    });
    // ToolCall 用 content_block index=1（因为 text 占了 block 0）
    model.apply(ConversationIntent::ObserveToolCallUpdate {
        provider_id: Some("provider-1".to_string()),
        id: "call_agent_1".to_string(),
        name: "Agent".to_string(),
        index: 1,
        summary: Some("Review code".to_string()),
        arguments: None,
        status: ToolCallStatus::Ready,
    });
    // Agent progress（不影响绑定）
    model.apply(ConversationIntent::RecordAgentProgress {
        tool_id: "call_agent_1".to_string(),
        message: "reading files...".to_string(),
    });
    // Agent tool result
    let changes = model.apply(ConversationIntent::ObserveToolResult {
        provider_id: "provider-1".to_string(),
        id: "call_agent_1".to_string(),
        tool_name: "Agent".to_string(),
        output: "审查报告".to_string(),
        content: serde_json::json!({ "text": "test output" }),
        is_error: false,
        image_count: 0,
    });

    // result 不应是 orphan
    assert!(
        !changes
            .iter()
            .any(|c| matches!(c, ConversationChange::OrphanToolResultObserved { .. })),
        "Agent tool result 不应变成 orphan"
    );
    assert!(changes.iter().any(|c| matches!(
        c,
        ConversationChange::ToolCallCompleted { status, .. } if *status == ToolCallStatus::Success
    )));
    assert!(!model.blocks.iter().any(|block| matches!(
        block,
        super::block::ConversationBlock::OrphanToolResult { id, .. } if id == "call_agent_1"
    )));
}

#[test]
fn test_agent_tool_result_not_orphan_text_streaming_then_tool() {
    // #95 场景 B：assistant text 还在 streaming（未 CompleteTextBlock）时，
    // tool call 就到了。ToolCallStart index=0, ToolCall index=1（错位）。
    let mut model = ConversationModel::default();
    model.apply(ConversationIntent::StartChat {
        submission: "review".to_string(),
    });
    model.apply(ConversationIntent::ObserveAssistantText {
        text: "让我".to_string(),
    });
    // 不调 CompleteTextBlock — text 还在 streaming
    model.apply(ConversationIntent::ObserveToolCallStart {
        id: "tool-1".to_string(),
        provider_id: None,
        name: "Agent".to_string(),
        index: 0,
    });
    model.apply(ConversationIntent::ObserveToolCallUpdate {
        provider_id: Some("provider-1".to_string()),
        id: "call_abc".to_string(),
        name: "Agent".to_string(),
        index: 1,
        summary: Some("Review".to_string()),
        arguments: None,
        status: ToolCallStatus::Ready,
    });
    let changes = model.apply(ConversationIntent::ObserveToolResult {
        provider_id: "provider-1".to_string(),
        id: "call_abc".to_string(),
        tool_name: "Agent".to_string(),
        output: "报告".to_string(),
        content: serde_json::json!({ "text": "test output" }),
        is_error: false,
        image_count: 0,
    });

    assert!(
        !changes
            .iter()
            .any(|c| matches!(c, ConversationChange::OrphanToolResultObserved { .. })),
        "Agent result 不应因 text streaming 而变 orphan"
    );
}

#[test]
fn test_tool_result_not_orphan_when_no_tool_call_start() {
    // #95 核心场景：provider 未发送 ToolCallStart，直接发送 ToolCall + ToolResult。
    // 修复前 observe_tool_call 中 bind_tool 返回 None 导致 ToolCall block 不被创建，
    // ToolResult 到达时 complete_active_tool 找不到匹配 id → orphan。
    let mut model = ConversationModel::default();
    model.apply(ConversationIntent::StartChat {
        submission: "review code".to_string(),
    });
    // 不发送 ToolCallStart
    model.apply(ConversationIntent::ObserveToolCallUpdate {
        provider_id: Some("provider-1".to_string()),
        id: "call_agent_no_start".to_string(),
        name: "Agent".to_string(),
        index: 0,
        summary: Some("Review code".to_string()),
        arguments: None,
        status: ToolCallStatus::Ready,
    });
    let changes = model.apply(ConversationIntent::ObserveToolResult {
        provider_id: "provider-1".to_string(),
        id: "call_agent_no_start".to_string(),
        tool_name: "Agent".to_string(),
        output: "审查报告".to_string(),
        content: serde_json::json!({ "text": "test output" }),
        is_error: false,
        image_count: 0,
    });

    assert!(
        !changes
            .iter()
            .any(|c| matches!(c, ConversationChange::OrphanToolResultObserved { .. })),
        "没有 ToolCallStart 时 ToolResult 不应变 orphan（bind_tool 应自动创建占位）"
    );
    assert!(changes.iter().any(|c| matches!(
        c,
        ConversationChange::ToolCallCompleted { status, .. } if *status == ToolCallStatus::Success
    )));
}
