use std::{path::PathBuf, rc::Rc};

use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::{layout::Rect, text::Span};

use crate::tui::{
    app::App,
    effect::effect::Effect,
    render::output::rendered::{LinkSpan, RenderedBlock, RenderedDocument, RenderedLine},
};

fn app_with_link() -> App {
    let mut app = App::new(
        "test-session".into(),
        PathBuf::from("/workspace"),
        "test-model".into(),
    );
    let area = Rect::new(0, 0, 80, 10);
    app.layout.output_area_rect = area;
    app.output_area.replace_document(RenderedDocument {
        blocks: vec![RenderedBlock {
            block_id: "link".into(),
            lines: Rc::new(vec![RenderedLine::with_plain_and_links(
                vec![Span::raw("docs")],
                "docs".into(),
                vec![LinkSpan {
                    col_start: 0,
                    col_end: 4,
                    url: "https://example.com/docs".into(),
                }],
            )]),
        }],
    });
    app.output_area.screen_line_map = vec![(0, sdk::CharIdx::new(0), sdk::CharIdx::new(4))];
    app
}

fn left_click(modifiers: KeyModifiers) -> MouseEvent {
    MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 1,
        row: 0,
        modifiers,
    }
}

#[test]
fn ordinary_click_on_link_begins_output_selection_without_opening_url() {
    let mut app = app_with_link();
    let effects = app.handle_mouse_event(left_click(KeyModifiers::NONE), Rect::new(0, 0, 80, 10));

    assert!(effects.is_empty());
    assert!(app.view_state.output.is_selecting());
}

#[test]
fn super_click_on_link_opens_url_without_starting_selection() {
    let mut app = app_with_link();
    let effects = app.handle_mouse_event(left_click(KeyModifiers::SUPER), Rect::new(0, 0, 80, 10));

    assert_eq!(
        effects,
        vec![Effect::OpenUrl {
            url: "https://example.com/docs".into(),
        }]
    );
    assert!(!app.view_state.output.is_selecting());
}

#[test]
fn control_click_on_link_opens_url_without_starting_selection() {
    let mut app = app_with_link();
    let effects =
        app.handle_mouse_event(left_click(KeyModifiers::CONTROL), Rect::new(0, 0, 80, 10));

    assert_eq!(
        effects,
        vec![Effect::OpenUrl {
            url: "https://example.com/docs".into(),
        }]
    );
    assert!(!app.view_state.output.is_selecting());
}

#[test]
fn alt_click_on_link_opens_url_without_starting_selection() {
    let mut app = app_with_link();
    let effects = app.handle_mouse_event(left_click(KeyModifiers::ALT), Rect::new(0, 0, 80, 10));

    assert_eq!(
        effects,
        vec![Effect::OpenUrl {
            url: "https://example.com/docs".into(),
        }]
    );
    assert!(!app.view_state.output.is_selecting());
}

#[test]
fn plain_click_on_link_starts_selection_without_opening_url() {
    let mut app = app_with_link();
    let effects = app.handle_mouse_event(left_click(KeyModifiers::NONE), Rect::new(0, 0, 80, 10));

    assert!(effects.is_empty());
    assert!(app.view_state.output.is_selecting());
}
