use super::*;

#[cfg(test)]
pub(crate) fn set_test_status_text(bar: &mut StatusBar, status: &str) {
    bar.status = status.to_string();
    bar.status_type = StatusType::Normal;
    bar.input_tokens = 0;
    bar.output_tokens = 0;
    bar.last_input_tokens = 0;
    bar.session_id = None;
    bar.api_calls = 0;
    bar.model = None;
    bar.context_size = 0;
    bar.tps = 0.0;
    bar.thinking = false;
}

#[test]
fn test_status_bar_selection_maps_cjk_screen_col_to_char_index() {
    let mut bar = StatusBar::new();
    set_test_status_text(&mut bar, "你好a");
    let prefix_width = " Think:OFF │ ".chars().count() as u16;

    bar.start_selection(prefix_width + 2);
    bar.update_selection(prefix_width + 6);

    assert_eq!(bar.get_selected_text(), Some("好a ".to_string()));
}

#[test]
fn test_status_bar_selection_maps_emoji_screen_col_to_char_index() {
    let mut bar = StatusBar::new();
    set_test_status_text(&mut bar, "a🚀b");
    let prefix_width = " Think:OFF │ ".chars().count() as u16;

    bar.start_selection(prefix_width + 1);
    bar.update_selection(prefix_width + 4);

    assert_eq!(bar.get_selected_text(), Some("🚀b".to_string()));
}

#[test]
fn test_status_bar_render_uses_status_background() {
    let bar = StatusBar::new();
    let area = Rect::new(0, 0, 40, 1);
    let mut buf = Buffer::empty(area);

    bar.render(area, &mut buf);

    assert_eq!(buf.cell((0, 0)).unwrap().style().bg, Some(theme::STATUS_BG));
    assert_eq!(
        buf.cell((39, 0)).unwrap().style().bg,
        Some(theme::STATUS_BG)
    );
}
