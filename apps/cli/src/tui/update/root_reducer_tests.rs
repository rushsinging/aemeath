use super::*;
use crate::tui::model::conversation::intent::{
    ConversationIntent, SetCompactProgress, StartChat, ToolCallStart, ToolCallUpdate,
};

#[test]
fn test_reduce_agent_event_tool_call_updates_conversation() {
    let mut model = TuiModel::default();
    model.conversation.apply(StartChat {
        submission: "read".to_string(),
    });
    let chat_id = crate::tui::model::conversation::ids::ChatId::new("session-1");
    let turn_id = crate::tui::model::conversation::ids::ChatTurnId::new("turn-1");
    model
        .conversation
        .ensure_runtime_turn(chat_id.clone(), turn_id.clone());
    reduce_agent_event(
        &mut model,
        AgentEventMapping {
            conversation: vec![ConversationIntent::ToolCallStart(ToolCallStart {
                chat_id: chat_id.clone(),
                turn_id: turn_id.clone(),
                id: crate::tui::model::conversation::ids::ToolCallId::new("tool-1"),
                provider_id: Some("provider-1".to_string()),
                name: "Read".to_string(),
                index: 0,
                model_id: None,
                role: None,
            })],
            ..Default::default()
        },
    );
    reduce_agent_event(
        &mut model,
        AgentEventMapping {
            conversation: vec![ConversationIntent::ToolCallUpdate(ToolCallUpdate {
                chat_id: chat_id.clone(),
                turn_id: turn_id.clone(),
                id: crate::tui::model::conversation::ids::ToolCallId::new("tool-1"),
                provider_id: Some("provider-1".to_string()),
                name: "Read".to_string(),
                index: 0,
                arguments: None,
                status: crate::tui::model::conversation::tool_call::ToolCallStatus::Ready,
            })],
            ..Default::default()
        },
    );

    let expected_tool_id = crate::tui::model::conversation::ids::ToolCallId::new("tool-1");
    assert!(model.conversation.timeline.items().iter().any(|item| matches!(
        item,
        crate::tui::model::output_timeline::OutputTimelineItem::ToolCall { reference } if reference.tool_call_id == expected_tool_id
    )));
}

#[test]
fn test_reduce_agent_event_applies_tool_patch_atomically_with_single_render_request() {
    let mut model = TuiModel::default();
    let chat_id = crate::tui::model::conversation::ids::ChatId::new("chat-atomic");
    let turn_id = crate::tui::model::conversation::ids::ChatTurnId::new("turn-atomic");

    let result = reduce_agent_event(
        &mut model,
        AgentEventMapping {
            conversation: vec![ConversationIntent::ToolCallUpdate(ToolCallUpdate {
                chat_id: chat_id.clone(),
                turn_id: turn_id.clone(),
                id: crate::tui::model::conversation::ids::ToolCallId::new("tool-atomic"),
                provider_id: Some("provider-atomic".to_string()),
                name: "Read".to_string(),
                index: 0,
                arguments: Some(r#"{"file_path":"src/lib.rs"}"#.to_string()),
                status: crate::tui::model::conversation::tool_call::ToolCallStatus::Ready,
            })],
            effects: vec![Effect::RequestRender],
            ..Default::default()
        },
    );

    assert!(result.dirty.output);
    assert_eq!(
        result
            .effects
            .iter()
            .filter(|effect| matches!(effect, Effect::RequestRender))
            .count(),
        1
    );
    let expected_tool_id = crate::tui::model::conversation::ids::ToolCallId::new("tool-atomic");
    assert!(model
        .conversation
        .timeline
        .items()
        .iter()
        .any(|item| matches!(
            item,
            crate::tui::model::output_timeline::OutputTimelineItem::ToolCall { reference }
                if reference.context.chat_id == chat_id
                    && reference.context.turn_id == turn_id
                    && reference.tool_call_id == expected_tool_id
        )));
}

/// Bug #540：compact progress 嵌入 spinner 行（output 区），dirty 归类必须为 output_dirty。
#[test]
fn test_set_compact_progress_marks_output_dirty_not_status_only() {
    let mut model = TuiModel::default();
    let result = reduce_agent_event(
        &mut model,
        AgentEventMapping {
            conversation: vec![ConversationIntent::SetCompactProgress(SetCompactProgress {
                stage: "summarizing".into(),
                current: Some(2),
                total: Some(10),
            })],
            ..Default::default()
        },
    );
    assert!(
        result.dirty.output,
        "SetCompactProgress 必须 mark output_dirty（进度条嵌在 spinner 行）"
    );
    assert_eq!(
        model
            .conversation
            .runtime
            .compact_progress
            .as_ref()
            .map(|p| p.stage.as_str()),
        Some("summarizing"),
        "apply 后 model 应保存 progress 状态"
    );
}
