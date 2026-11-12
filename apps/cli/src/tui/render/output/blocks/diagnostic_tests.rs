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

#[test]
fn test_diagnostic_long_url_wraps_within_width() {
    // #2196 follow-up：升级提示含长 URL，必须按可用宽度换行而不是单行溢出。
    let text = "[aemeath v0.1.1 is available (you have v0.0.0-abc); run `aemeath update` to upgrade | https://github.com/rushsinging/aemeath/releases/tag/v0.1.1]";
    let view = TextBlockView {
        key: "u".into(),
        text: text.into(),
        style: SemanticStyle::Muted,
    };
    let block = render_diagnostic("u", &view, &RenderCtx::for_width(40));

    assert!(
        block.lines.len() > 1,
        "长 URL 提示必须换行: {:?}",
        block
            .lines
            .iter()
            .map(|l| l.plain.clone())
            .collect::<Vec<_>>()
    );
    for line in block.lines.iter() {
        let width: usize = line
            .plain
            .chars()
            .map(|c| unicode_width::UnicodeWidthChar::width(c).unwrap_or(0))
            .sum();
        assert!(width <= 40, "行宽 {width} 超出 40: {:?}", line.plain);
    }
}

#[test]
fn test_diagnostic_respects_explicit_newlines() {
    let view = TextBlockView {
        key: "m".into(),
        text: "first\nsecond".into(),
        style: SemanticStyle::Muted,
    };
    let block = render_diagnostic("m", &view, &RenderCtx::for_width(80));

    assert_eq!(block.lines.len(), 2);
    assert_eq!(block.lines[0].plain, "first");
    assert_eq!(block.lines[1].plain, "second");
}
