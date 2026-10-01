use super::App;
use crate::tui::render::output_area::SCROLLBAR_RESERVE_COLS;
use ratatui::layout::Rect;

#[test]
fn test_output_document_width_reserves_scrollbar_and_two_padding_columns() {
    let mut app = App::new(
        "session".to_string(),
        std::env::current_dir().unwrap(),
        "model".to_string(),
    );
    app.layout.output_area_rect = Rect::new(0, 0, 80, 20);

    assert_eq!(
        app.output_document_width(),
        80 - SCROLLBAR_RESERVE_COLS,
        "文档预换行宽度 = 终端宽度 - 滚动条预留列数"
    );
}

#[test]
fn test_output_document_width_never_underflows() {
    let mut app = App::new(
        "session".to_string(),
        std::env::current_dir().unwrap(),
        "model".to_string(),
    );
    app.layout.output_area_rect = Rect::new(0, 0, 3, 20);

    assert_eq!(app.output_document_width(), 1);
}
