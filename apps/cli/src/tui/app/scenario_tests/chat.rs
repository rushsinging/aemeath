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

/// #1106 回归：runtime 允许发空 SystemMessage（hook 的 additional_context /
/// system_message 只判 Option 不判空串），TUI 必须不渲染——否则每条空消息
/// 各吃掉 2 行（空内容 + depth0 前置空行），在输出区堆出大片空行。
///
/// 端到端：runtime 事件 → ACL → model → view_assembler → render → 屏幕字符。
#[test]
fn empty_system_messages_from_runtime_do_not_accumulate_blank_lines() {
    fn blanks_between_anchors(empty_count: usize) -> usize {
        let mut harness = TuiScenarioHarness::new(60, 30);
        for anchor in ["ANCHORUP", "ANCHORDOWN"] {
            if anchor == "ANCHORDOWN" {
                for payload in ["", "<system-reminder></system-reminder>"]
                    .iter()
                    .cycle()
                    .take(empty_count)
                {
                    harness.runtime(UiEvent::SystemMessage((*payload).to_string()));
                }
            }
            harness.ui(UiEvent::Text {
                context: context(),
                text: anchor.to_string(),
            });
            harness.ui(UiEvent::BlockComplete {
                context: context(),
                text: anchor.to_string(),
            });
        }
        harness.ui(UiEvent::Done { context: context() });
        harness.render();

        let screen = harness.screen();
        let lines: Vec<&str> = screen.lines().collect();
        let up = lines
            .iter()
            .position(|l| l.contains("ANCHORUP"))
            .expect("上锚点应在屏幕上");
        let down = lines
            .iter()
            .position(|l| l.contains("ANCHORDOWN"))
            .expect("下锚点应在屏幕上");
        lines[up + 1..down]
            .iter()
            .filter(|l| l.trim().is_empty())
            .count()
    }

    let baseline = blanks_between_anchors(0);
    for empty_count in [1usize, 4, 8] {
        assert_eq!(
            blanks_between_anchors(empty_count),
            baseline,
            "{empty_count} 条空 SystemMessage 不应新增空行（基线 {baseline}）"
        );
    }
}
