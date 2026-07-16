use crate::tui::app::event::{UiEvent, UiTurnContext};
use crate::tui::model::conversation::ids::{ChatId, ChatTurnId};

use super::super::testing::TuiScenarioHarness;

fn context() -> UiTurnContext {
    UiTurnContext {
        chat_id: ChatId::new("chat-p0"),
        turn_id: ChatTurnId::new("turn-p0"),
    }
}

#[test]
fn streaming_has_representative_thinking_and_completed_snapshots() {
    let mut harness = TuiScenarioHarness::new(100, 30);
    harness.ui(UiEvent::TurnStarted { messages: vec![] });
    harness.ui(UiEvent::Thinking {
        context: context(),
        text: "Inspecting the repository".into(),
    });
    harness.render();
    assert!(harness.screen().contains("Inspecting the repository"));
    insta::assert_snapshot!("chat_streaming__thinking__100x30", harness.screen());

    harness.ui(UiEvent::Text {
        context: context(),
        text: "The result is ready.".into(),
    });
    harness.ui(UiEvent::BlockComplete {
        context: context(),
        text: "The result is ready.".into(),
    });
    harness.ui(UiEvent::Done { context: context() });
    harness.render();
    assert!(harness.screen().contains("The result is ready."));
    insta::assert_snapshot!("chat_streaming__completed__100x30", harness.screen());
    harness.assert_idle();
}

#[test]
fn tool_lifecycle_binds_result_to_call_and_renders_stable_states() {
    let mut harness = TuiScenarioHarness::new(100, 30);
    let id = sdk::ids::ToolCallId::new("read-1");
    harness.ui(UiEvent::ToolCallStart {
        context: context(),
        id: id.clone(),
        provider_id: Some("provider-read-1".into()),
        name: "Read".into(),
        index: 0,
    });
    harness.ui(UiEvent::ToolCallUpdate {
        context: context(),
        id: id.clone(),
        provider_id: Some("provider-read-1".into()),
        name: "Read".into(),
        index: 0,
        arguments_delta: None,
        arguments: Some(serde_json::json!({"file_path":"Cargo.toml"})),
        status: sdk::ToolCallStatusView::Ready,
    });
    harness.render();
    assert!(harness.screen().contains("Read"));
    insta::assert_snapshot!("tool_read__running__100x30", harness.screen());

    harness.ui(UiEvent::ToolResult {
        context: context(),
        id,
        provider_id: "provider-read-1".into(),
        tool_name: "Read".into(),
        output: "[workspace]\nmembers = []".into(),
        content: serde_json::json!({"text":"[workspace]\nmembers = []"}),
        is_error: false,
        images: vec![],
    });
    harness.render();
    assert!(harness.screen().contains("Read"));
    insta::assert_snapshot!("tool_read__completed__100x30", harness.screen());
    harness.assert_idle();
}
