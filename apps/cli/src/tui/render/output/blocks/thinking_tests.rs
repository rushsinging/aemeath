use super::*;
use crate::tui::view_model::style::SemanticStyle;

#[test]
fn test_thinking_renders_plain_text_without_self_bulb_prefix() {
    // 💭 marker 现由 gutter 注入（顶格）；组件只渲染原文，不再自拼 💭 前缀。
    let view = TextBlockView {
        key: "t".into(),
        text: "ponder".into(),
        style: SemanticStyle::Muted,
    };
    let block = render_thinking("t", &view, &RenderCtx::for_width(80));

    assert_eq!(block.lines[0].plain, "ponder");
    assert!(
        !block.lines[0].plain.contains('💭'),
        "💭 不应进入 plain（由 gutter 注入）"
    );
    assert_eq!(block.lines[0].spans[0].style.fg, Some(theme::THINKING));
}

#[test]
fn test_thinking_skips_blank_lines() {
    let view = TextBlockView {
        key: "t".into(),
        text: "a\n\n\nb".into(),
        style: SemanticStyle::Muted,
    };
    let block = render_thinking("t", &view, &RenderCtx::for_width(80));

    assert!(block.lines.iter().all(|line| !line.plain.trim().is_empty()));
}

#[test]
fn test_thinking_wraps_long_reasoning_line_to_render_width() {
    let text = "The user is greeting me in Chinese. According to the system reminder, I should use Chinese for thinking and response.";
    let view = TextBlockView {
        key: "t".into(),
        text: text.into(),
        style: SemanticStyle::Muted,
    };
    let block = render_thinking("t", &view, &RenderCtx::for_width(16));

    assert!(block.lines.len() > 1, "长 reasoning 行应按渲染宽度拆成多行");
    assert!(block
        .lines
        .iter()
        .all(|line| line.plain.chars().count() <= 16));
    let combined_plain = block
        .lines
        .iter()
        .map(|line| line.plain.as_str())
        .collect::<String>();
    assert_eq!(combined_plain, text);
    assert!(block
        .lines
        .iter()
        .flat_map(|line| line.spans.iter())
        .all(|span| span.style.fg == Some(theme::THINKING)));
}

#[test]
fn test_thinking_renders_markdown_markers_as_plain_text() {
    let text = "# plan: keep **stars**, _underscores_, `ticks`, and *asterisks*";
    let view = TextBlockView {
        key: "t".into(),
        text: text.into(),
        style: SemanticStyle::Muted,
    };
    let block = render_thinking("t", &view, &RenderCtx::for_width(120));

    assert_eq!(block.lines.len(), 1);
    assert_eq!(block.lines[0].plain, text);
    assert_eq!(block.lines[0].spans.len(), 1);
    assert_eq!(block.lines[0].spans[0].content.as_ref(), text);
    assert_eq!(block.lines[0].spans[0].style.fg, Some(theme::THINKING));
}
