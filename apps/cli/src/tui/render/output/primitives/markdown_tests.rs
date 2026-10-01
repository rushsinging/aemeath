use super::*;
use ratatui::style::{Modifier, Style};

#[test]
fn test_markdown_bold_sets_modifier_and_plain_strips_markers() {
    let lines = markdown("a **b** c", Style::default(), 80);

    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0].plain, "a b c");
    assert!(lines[0].spans.iter().any(
        |span| span.content.as_ref() == "b" && span.style.add_modifier.contains(Modifier::BOLD)
    ));
}

#[test]
fn test_markdown_wraps_by_width() {
    let lines = markdown("aaaa bbbb", Style::default(), 4);

    assert!(lines.len() >= 2, "超宽应换行");
}

#[test]
fn test_markdown_plain_invariant_matches_spans_visible_text() {
    let lines = markdown("`code` and *em*", Style::default(), 80);

    for line in &lines {
        let visible = line
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>();
        assert!(!line.plain.contains('`'));
        assert!(!visible.is_empty() || line.plain.is_empty());
    }
}

#[test]
fn test_markdown_blockquote_renders_bar_and_dim() {
    let lines = markdown("> hello", Style::default(), 80);

    assert_eq!(lines.len(), 1);
    assert!(lines[0].plain.starts_with("│ "), "应以竖线开头");
    assert!(lines[0].plain.ends_with("hello"));
    // 首个 span 是弱化色竖线。
    assert_eq!(lines[0].spans[0].style.fg, Some(theme::TEXT_DIM));
}

#[test]
fn test_markdown_blockquote_nested_two_bars() {
    let lines = markdown("> > deep", Style::default(), 80);

    assert!(lines[0].plain.starts_with("│ │ "), "两层引用两根竖线");
    assert!(lines[0].plain.ends_with("deep"));
}

#[test]
fn test_markdown_blockquote_keeps_inline_bold() {
    let lines = markdown("> see **this**", Style::default(), 80);

    assert!(lines[0].plain.contains("see this"));
    assert!(lines[0]
        .spans
        .iter()
        .any(|s| s.content.as_ref() == "this" && s.style.add_modifier.contains(Modifier::BOLD)));
}

#[test]
fn test_markdown_unordered_list_renders_bullet() {
    let lines = markdown("- item", Style::default(), 80);

    assert!(lines[0].plain.starts_with("• "), "无序项应渲染圆点");
    assert!(lines[0].plain.ends_with("item"));
    assert_eq!(lines[0].spans[0].style.fg, Some(theme::ACCENT));
}

#[test]
fn test_markdown_nested_list_preserves_indent() {
    let lines = markdown("  - nested", Style::default(), 80);

    assert!(
        lines[0].plain.starts_with("  • "),
        "嵌套项应保留缩进再加圆点, got: {:?}",
        lines[0].plain
    );
}

#[test]
fn test_markdown_ordered_list_keeps_number() {
    let lines = markdown("1. first", Style::default(), 80);

    assert!(
        lines[0].plain.starts_with("1. "),
        "有序项保留序号, got: {:?}",
        lines[0].plain
    );
    assert!(lines[0].plain.ends_with("first"));
}

#[test]
fn test_markdown_list_item_keeps_inline_code() {
    let lines = markdown("- use `cargo`", Style::default(), 80);

    assert!(lines[0].plain.contains("use cargo"));
    assert!(lines[0]
        .spans
        .iter()
        .any(|s| s.content.as_ref() == "cargo" && s.style.fg == Some(theme::CODE)));
}

#[test]
fn test_markdown_dash_without_space_is_not_list() {
    // 边界：`-` 后无空格不算列表标记，原样走 inline。
    let lines = markdown("-notalist", Style::default(), 80);

    assert!(!lines[0].plain.starts_with("• "));
    assert!(lines[0].plain.contains("-notalist"));
}

#[test]
fn markdown_link_survives_render_pipeline() {
    let lines = markdown(
        "see [example](https://example.com) here",
        Style::default(),
        80,
    );
    assert_eq!(lines.len(), 1);
    let line = &lines[0];
    assert!(
        line.links.iter().any(|ls| ls.url == "https://example.com"),
        "links should survive markdown() render: got {:?}",
        line.links
    );
}

#[test]
fn markdown_link_survives_gutter() {
    use crate::tui::render::output::gutter::apply_gutter;
    use crate::tui::view_model::output::{OutputBlockKind, TextBlockView};
    use crate::tui::view_model::style::SemanticStyle;

    let lines = markdown(
        "see [example](https://example.com) here",
        Style::default(),
        80,
    );
    let view = TextBlockView {
        key: "t".into(),
        text: "".into(),
        style: SemanticStyle::Normal,
    };
    let gutted = apply_gutter(&OutputBlockKind::AssistantMessage(view), 0, lines);
    assert!(
        gutted[0]
            .links
            .iter()
            .any(|ls| ls.url == "https://example.com"),
        "links should survive gutter: got {:?}",
        gutted[0].links
    );
}

#[test]
fn test_markdown_multiline_mixed_blocks() {
    // 多行混合：引用 + 列表 + 普通行各成独立渲染行。
    let lines = markdown("> quote\n- item\nplain", Style::default(), 80);

    assert_eq!(lines.len(), 3);
    assert!(lines[0].plain.starts_with("│ "));
    assert!(lines[1].plain.starts_with("• "));
    assert_eq!(lines[2].plain, "plain");
}
