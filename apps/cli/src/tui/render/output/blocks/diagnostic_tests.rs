use super::*;

#[test]
fn test_diagnostic_error_uses_error_color() {
    let view = TextBlockView {
        key: "e".into(),
        text: "boom".into(),
        style: SemanticStyle::Error,
    };
    let block = render_diagnostic("e", &view, &RenderCtx::for_width(80));

    assert_eq!(block.lines[0].plain, "boom");
    assert_eq!(block.lines[0].spans[0].style.fg, Some(theme::ERROR));
}

#[test]
fn test_diagnostic_trailing_newline_emits_blank_line() {
    // 文本以 \n 结尾时追加一行尾随空行（done 提示间距，修迁移回归）。
    let view = TextBlockView {
        key: "d".into(),
        text: "✻ Sautéed for 3s\n".into(),
        style: SemanticStyle::Muted,
    };
    let block = render_diagnostic("d", &view, &RenderCtx::for_width(80));

    assert_eq!(block.lines.len(), 2, "应有提示行 + 尾随空行");
    assert_eq!(block.lines[0].plain, "✻ Sautéed for 3s");
    assert_eq!(block.lines[1].plain, "", "末行为空行间距");
}

#[test]
fn test_diagnostic_no_trailing_newline_no_extra_blank() {
    // 边界：不以 \n 结尾的普通提示不追加空行。
    let view = TextBlockView {
        key: "d".into(),
        text: "plain".into(),
        style: SemanticStyle::Muted,
    };
    let block = render_diagnostic("d", &view, &RenderCtx::for_width(80));

    assert_eq!(block.lines.len(), 1);
    assert_eq!(block.lines[0].plain, "plain");
}
