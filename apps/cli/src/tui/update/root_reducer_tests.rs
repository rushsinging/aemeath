use super::*;
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};

fn key(code: KeyCode) -> TuiMsg {
    TuiMsg::TerminalKey(KeyEvent {
        code,
        modifiers: KeyModifiers::NONE,
        kind: KeyEventKind::Press,
        state: KeyEventState::NONE,
    })
}

#[test]
fn test_update_key_enter_idle_spawns_chat() {
    let mut model = TuiModel::default();
    let mut view_state = AppViewState::default();
    update(&mut model, &mut view_state, key(KeyCode::Char('h')));
    let result = update(&mut model, &mut view_state, key(KeyCode::Enter));
    assert!(matches!(
        result.effects.last(),
        Some(Effect::SpawnAgentChat { .. })
    ));
}

#[test]
fn test_update_key_enter_running_queues_submission() {
    let mut model = TuiModel::default();
    model.conversation.apply(ConversationIntent::StartChat {
        submission: "old".to_string(),
    });
    let mut view_state = AppViewState::default();
    update(&mut model, &mut view_state, key(KeyCode::Char('n')));
    update(&mut model, &mut view_state, key(KeyCode::Enter));
    assert_eq!(model.conversation.queued_submissions.len(), 1);
}

fn test_turn_context() -> crate::tui::app::event::UiTurnContext {
    crate::tui::app::event::UiTurnContext {
        chat_id: crate::tui::model::conversation::ids::ChatId::new("chat-test"),
        turn_id: crate::tui::model::conversation::ids::ChatTurnId::new("turn-test"),
    }
}

#[test]
fn test_update_agent_text_marks_output_dirty() {
    let mut model = TuiModel::default();
    let mut view_state = AppViewState::default();
    let result = update(
        &mut model,
        &mut view_state,
        TuiMsg::AgentEvent(crate::tui::app::event::UiEvent::Text {
            context: test_turn_context(),
            text: "hi".into(),
        }),
    );
    assert!(result.dirty.output);
}

#[test]
fn test_update_agent_text_persists_output_dirty_until_render_pipeline_refreshes() {
    let mut model = TuiModel::default();
    let mut view_state = AppViewState::default();
    let result = update(
        &mut model,
        &mut view_state,
        TuiMsg::AgentEvent(crate::tui::app::event::UiEvent::Text {
            context: test_turn_context(),
            text: "hi".into(),
        }),
    );

    assert!(result.dirty.output);
    assert!(view_state.dirty.output);
}

#[test]
fn test_reduce_agent_event_tool_call_updates_conversation() {
    let mut model = TuiModel::default();
    model.conversation.apply(ConversationIntent::StartChat {
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
            conversation: vec![ConversationIntent::ObserveToolCallStart {
                chat_id: chat_id.clone(),
                turn_id: turn_id.clone(),
                id: crate::tui::model::conversation::ids::ToolCallId::new("tool-1"),
                provider_id: Some("provider-1".to_string()),
                name: "Read".to_string(),
                index: 0,
            }],
            ..Default::default()
        },
    );
    reduce_agent_event(
        &mut model,
        AgentEventMapping {
            conversation: vec![ConversationIntent::ObserveToolCallUpdate {
                chat_id: chat_id.clone(),
                turn_id: turn_id.clone(),
                id: crate::tui::model::conversation::ids::ToolCallId::new("tool-1"),
                provider_id: Some("provider-1".to_string()),
                name: "Read".to_string(),
                index: 0,
                arguments: None,
                status: crate::tui::model::conversation::tool_call::ToolCallStatus::Ready,
            }],
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
fn test_reduce_agent_event_applies_tool_patch_and_spinner_atomically_with_single_render_request() {
    let mut model = TuiModel::default();
    let chat_id = crate::tui::model::conversation::ids::ChatId::new("chat-atomic");
    let turn_id = crate::tui::model::conversation::ids::ChatTurnId::new("turn-atomic");

    let result = reduce_agent_event(
        &mut model,
        AgentEventMapping {
            conversation: vec![ConversationIntent::ObserveToolCallUpdate {
                chat_id: chat_id.clone(),
                turn_id: turn_id.clone(),
                id: crate::tui::model::conversation::ids::ToolCallId::new("tool-atomic"),
                provider_id: Some("provider-atomic".to_string()),
                name: "Read".to_string(),
                index: 0,
                arguments: Some(r#"{"file_path":"src/lib.rs"}"#.to_string()),
                status: crate::tui::model::conversation::tool_call::ToolCallStatus::Ready,
            }],
            runtime: vec![
                crate::tui::model::runtime::intent::RuntimeIntent::SetSpinnerPhase(
                    crate::tui::model::runtime::spinner::SpinnerPhase::Generating,
                ),
            ],
            effects: vec![Effect::RequestRender],
            ..Default::default()
        },
    );

    assert!(result.dirty.output);
    assert!(result.dirty.status);
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

#[test]
fn test_up_key_selects_completion_when_visible() {
    use crate::tui::model::input::completion_item::CompletionItem;

    let mut model = TuiModel::default();
    model.input.apply(InputIntent::InsertChar('/'));
    model.input.apply(InputIntent::SetCompletions {
        query: "/".to_string(),
        items: vec![
            CompletionItem::new("/help", "/help"),
            CompletionItem::new("/exit", "/exit"),
        ],
    });
    assert!(model.input.completion.visible);
    assert_eq!(model.input.completion.selected_index, Some(0));

    let mut view_state = AppViewState::default();
    update(&mut model, &mut view_state, key(KeyCode::Down));
    assert_eq!(
        model.input.completion.selected_index,
        Some(1),
        "Down 在补全可见时应选择下一项"
    );

    update(&mut model, &mut view_state, key(KeyCode::Up));
    assert_eq!(
        model.input.completion.selected_index,
        Some(0),
        "Up 在补全可见时应选择上一项"
    );
}

#[test]
fn test_update_terminal_resize_updates_layout_view_state() {
    let mut model = TuiModel::default();
    let mut view_state = AppViewState::default();

    let result = update(
        &mut model,
        &mut view_state,
        TuiMsg::TerminalResize {
            width: 100,
            height: 40,
        },
    );

    assert_eq!(view_state.layout.terminal_width, 100);
    assert_eq!(view_state.layout.terminal_height, 40);
    assert!(result.dirty.output);
    assert!(result.dirty.status);
    assert!(result.dirty.input);
    assert!(result.dirty.dialog);
    assert!(matches!(result.effects.as_slice(), [Effect::RequestRender]));
}

#[test]
fn test_up_down_history_when_completion_hidden() {
    let mut model = TuiModel::default();
    model.input.apply(InputIntent::ReplaceHistory(vec![
        "first".to_string(),
        "second".to_string(),
    ]));
    assert!(!model.input.completion.visible);

    let mut view_state = AppViewState::default();
    update(&mut model, &mut view_state, key(KeyCode::Up));
    assert_eq!(
        model.input.document.buffer, "second",
        "Up 在补全不可见时应翻历史"
    );
}
