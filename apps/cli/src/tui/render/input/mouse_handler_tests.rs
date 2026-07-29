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
        root_group_block_counts: Vec::new(),
        block_line_ends: Vec::new(),
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

fn scroll_up() -> MouseEvent {
    MouseEvent {
        kind: MouseEventKind::ScrollUp,
        column: 1,
        row: 0,
        modifiers: KeyModifiers::NONE,
    }
}

fn scroll_down() -> MouseEvent {
    MouseEvent {
        kind: MouseEventKind::ScrollDown,
        column: 1,
        row: 0,
        modifiers: KeyModifiers::NONE,
    }
}

#[test]
fn consecutive_mouse_scroll_up_events_load_multiple_viewport_sized_batches() {
    let mut app = app_with_link();
    app.view_state.output.last_visible_height = 20;
    app.view_state.output.source_total_lines = 5_000;

    app.handle_mouse_event(scroll_up(), Rect::new(0, 0, 80, 10));
    assert_eq!(app.view_state.output.render_line_limit(), 1_015);

    app.handle_mouse_event(scroll_up(), Rect::new(0, 0, 80, 10));
    assert_eq!(app.view_state.output.render_line_limit(), 1_030);
}

#[test]
fn mouse_scroll_down_moves_older_history_window_toward_latest() {
    let mut app = app_with_link();
    app.view_state.output.auto_scroll = false;
    app.view_state.output.scroll_offset = 2;
    app.view_state.output.render_line_limit = 3_000;
    app.view_state.output.source_total_lines = 10_000;
    app.view_state.output.history_window_tail_offset = 1_200;

    let effects = app.handle_mouse_event(scroll_down(), Rect::new(0, 0, 80, 10));

    assert!(effects.is_empty());
    assert_eq!(app.view_state.output.history_window_tail_offset, 1_185);
    assert_eq!(app.view_state.output.scroll_offset, 14);
    assert!(!app.view_state.output.auto_scroll);
}

#[test]
fn mouse_scroll_down_at_latest_bottom_restores_initial_window_and_marks_output_dirty() {
    let mut app = app_with_link();
    app.view_state.output.auto_scroll = false;
    app.view_state.output.scroll_offset = 3;
    app.view_state.output.render_line_limit = 3_000;
    app.view_state.output.source_total_lines = 10_000;
    app.view_state.output.history_window_tail_offset = 0;
    app.view_state.dirty.clear_output();

    let effects = app.handle_mouse_event(scroll_down(), Rect::new(0, 0, 80, 10));

    assert!(effects.is_empty());
    assert_eq!(app.view_state.output.scroll_offset, 0);
    assert!(app.view_state.output.auto_scroll);
    assert_eq!(app.view_state.output.render_line_limit(), 1_000);
    assert!(app.view_state.dirty.output);
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
