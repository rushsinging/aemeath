use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};

use crate::tui::app::event::UiEvent;

use super::super::testing::TuiScenarioHarness;

fn context() -> UiTurnContext {
    UiTurnContext {
        chat_id: ChatId::new("chat-link"),
        turn_id: ChatTurnId::new("turn-link"),
    }
}

use crate::tui::app::event::UiTurnContext;
use crate::tui::model::conversation::ids::{ChatId, ChatTurnId};

#[test]
fn markdown_link_in_assistant_message_produces_clickable_linkspan() {
    let mut harness = TuiScenarioHarness::new(100, 30);
    harness.ui(UiEvent::TurnStarted { messages: vec![] });
    harness.ui(UiEvent::Text {
        context: context(),
        text: "see [example](https://example.com) here".into(),
    });
    harness.ui(UiEvent::BlockComplete {
        context: context(),
        text: "see [example](https://example.com) here".into(),
    });
    harness.ui(UiEvent::Done { context: context() });
    harness.render();

    // 检查渲染后的 document 中是否存在 links
    let has_link = harness
        .app
        .output_area
        .document()
        .iter_lines()
        .any(|line| line.links.iter().any(|ls| ls.url == "https://example.com"));

    assert!(
        has_link,
        "document should contain a LinkSpan with url https://example.com"
    );
}

#[test]
fn cmd_click_on_markdown_link_opens_url_after_full_render() {
    let mut harness = TuiScenarioHarness::new(100, 30);
    harness.ui(UiEvent::TurnStarted { messages: vec![] });
    harness.ui(UiEvent::Text {
        context: context(),
        text: "see [example](https://example.com) here".into(),
    });
    harness.ui(UiEvent::BlockComplete {
        context: context(),
        text: "see [example](https://example.com) here".into(),
    });
    harness.ui(UiEvent::Done { context: context() });
    harness.render();

    // 找到包含链接的 logic_idx 及其 link 信息
    let (logic_idx, gutter_cols, col_start, _col_end) = harness
        .app
        .output_area
        .document()
        .iter_lines()
        .enumerate()
        .find_map(|(idx, line)| {
            line.links
                .iter()
                .find(|ls| ls.url == "https://example.com")
                .map(|ls| (idx, line.gutter_cols, ls.col_start, ls.col_end))
        })
        .expect("link should exist in document");

    // 通过 screen_line_map 找到 logic_idx 对应的屏幕行
    let screen_row = harness
        .app
        .output_area
        .screen_line_map
        .iter()
        .position(|(idx, _, _)| *idx == logic_idx)
        .expect("logic_idx should be in screen_line_map") as u16;

    // 点击链接文本中间位置：屏幕列 = gutter_cols + col_start + 2
    let click_col = (gutter_cols + col_start + 2) as u16;
    let output_area = harness.app.layout.output_area_rect;
    let mouse = MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: output_area.x + click_col,
        row: output_area.y + screen_row,
        modifiers: KeyModifiers::SUPER,
    };
    let effects = harness.app.handle_mouse_event(mouse, output_area);
    assert!(
        effects.iter().any(|e| matches!(
            e,
            crate::tui::effect::effect::Effect::OpenUrl { url } if url == "https://example.com"
        )),
        "Cmd+Click on link should produce OpenUrl effect, got: {:?}",
        effects
    );
}
