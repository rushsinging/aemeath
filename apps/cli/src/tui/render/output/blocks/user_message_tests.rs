use super::*;
use crate::tui::view_model::style::SemanticStyle;

#[test]
fn test_user_message_renders_raw_text_with_user_style() {
    // 前导 `> ` marker 现由 gutter 注入；组件只渲染原文，使用 USER 前景和 USER_BG 背景。
    let view = TextBlockView {
        key: "u".into(),
        text: "hello".into(),
        style: SemanticStyle::Normal,
    };
    let block = render_user_message("u", &view, &RenderCtx::for_width(80));

    assert_eq!(block.lines[0].plain, "hello");
    assert_eq!(block.lines[0].spans[0].style.fg, Some(theme::USER));
    assert_eq!(block.lines[0].spans[0].style.bg, Some(theme::USER_BG));
}

#[test]
fn test_user_message_multiline_renders_each_line_without_self_prefix() {
    let view = TextBlockView {
        key: "u".into(),
        text: "a\nb".into(),
        style: SemanticStyle::Normal,
    };
    let block = render_user_message("u", &view, &RenderCtx::for_width(80));

    assert_eq!(block.lines[0].plain, "a");
    assert_eq!(block.lines[1].plain, "b");
    assert_eq!(block.lines[0].spans[0].style.bg, Some(theme::USER_BG));
    assert_eq!(block.lines[1].spans[0].style.bg, Some(theme::USER_BG));
}

#[test]
fn test_user_message_blank_line_has_no_filler_span() {
    let view = TextBlockView {
        key: "u".into(),
        text: "a\n\nb".into(),
        style: SemanticStyle::Normal,
    };
    let block = render_user_message("u", &view, &RenderCtx::for_width(80));

    assert_eq!(block.lines[0].plain, "a");
    assert_eq!(block.lines[1].plain, "");
    assert!(block.lines[1].spans.is_empty());
    assert_eq!(block.lines[2].plain, "b");
}

#[test]
fn test_user_message_wraps_long_line_to_render_width() {
    let view = TextBlockView {
        key: "u".into(),
        text: "abcdef".into(),
        style: SemanticStyle::Normal,
    };
    let block = render_user_message("u", &view, &RenderCtx::for_width(4));

    assert_eq!(
        block
            .lines
            .iter()
            .map(|line| line.plain.as_str())
            .collect::<Vec<_>>(),
        vec!["abcd", "ef"]
    );
    assert!(block.lines.iter().all(|line| line
        .spans
        .iter()
        .all(|span| span.style.bg == Some(theme::USER_BG))));
}

#[test]
fn test_user_message_expands_tab_to_four_spaces() {
    // issue #1670 复现：TSV 回显 tab 被吞成一整行。
    let view = TextBlockView {
        key: "u".into(),
        text: "wanaka_session\t9892\t78".into(),
        style: SemanticStyle::Normal,
    };
    let block = render_user_message("u", &view, &RenderCtx::for_width(80));
    assert_eq!(block.lines[0].plain, "wanaka_session    9892    78");
}
