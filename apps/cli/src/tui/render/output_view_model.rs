use crate::tui::output_area::tool_display::{format_tool_call, lookup_display};
use crate::tui::output_area::INDENT;
use crate::tui::render::theme;
use crate::tui::view_model::{
    OutputBlockView, OutputViewModel, SemanticStyle, TextBlockView, ToolCallBlockView,
};
use ratatui::text::{Line, Span};

pub(crate) fn output_view_model_lines(view_model: &OutputViewModel) -> Vec<Line<'static>> {
    view_model
        .blocks
        .iter()
        .flat_map(block_lines)
        .collect::<Vec<_>>()
}

fn block_lines(block: &OutputBlockView) -> Vec<Line<'static>> {
    match block {
        OutputBlockView::UserMessage(text) => text_lines("> ", "  ", text),
        OutputBlockView::AssistantMessage(text) => text_lines("", "", text),
        OutputBlockView::ThinkingMessage(text) => text_lines("💭 ", "💭 ", text),
        OutputBlockView::DiagnosticNotice(text) | OutputBlockView::SystemNotice(text) => {
            text_lines("", "", text)
        }
        OutputBlockView::ToolCall(tool) => tool_lines(tool),
        OutputBlockView::Separator => vec![Line::raw("")],
    }
}

fn text_lines(
    first_line_prefix: &str,
    continuation_prefix: &str,
    text: &TextBlockView,
) -> Vec<Line<'static>> {
    if text.text.is_empty() {
        return vec![Line::from(Span::styled(
            first_line_prefix.to_string(),
            style(text.style),
        ))];
    }
    text.text
        .lines()
        .enumerate()
        .map(|(index, line)| {
            let rendered = if index == 0 {
                format!("{first_line_prefix}{line}")
            } else {
                format!("{continuation_prefix}{line}")
            };
            Line::from(Span::styled(rendered, style(text.style)))
        })
        .collect()
}

fn tool_lines(tool: &ToolCallBlockView) -> Vec<Line<'static>> {
    let raw_summary = tool.summary.as_deref().unwrap_or_default();
    let (mut header, details) = format_tool_call(&tool.title, raw_summary);
    header = header.replacen('●', tool.icon.as_str(), 1);
    let mut lines = vec![Line::from(Span::styled(header, style(tool.style)))];

    for detail in details {
        lines.push(Line::from(Span::styled(
            format!("{INDENT}{detail}"),
            style(SemanticStyle::Muted),
        )));
    }

    if let Some(activity) = &tool.activity_summary {
        lines.push(Line::from(Span::styled(
            format!("{INDENT}{activity}"),
            style(SemanticStyle::Muted),
        )));
    }

    if let Some(result) = &tool.result_summary {
        push_tool_result_lines(tool, result, &mut lines);
    }
    lines
}

fn push_tool_result_lines(tool: &ToolCallBlockView, result: &str, lines: &mut Vec<Line<'static>>) {
    let Some(display) = lookup_display(&tool.title) else {
        lines.push(Line::from(Span::styled(
            format!("{INDENT}{result}"),
            style(tool.style),
        )));
        return;
    };
    let max_lines = display.result_max_lines();
    if max_lines > 0 {
        let result_style = style(map_result_style(display.result_style(), tool.style));
        let total = result.lines().count();
        for line in result.lines().take(max_lines) {
            lines.push(Line::from(Span::styled(
                format!("{INDENT}{line}"),
                result_style,
            )));
        }
        if total > max_lines {
            lines.push(Line::from(Span::styled(
                format!("{INDENT}... ({} lines omitted)", total - max_lines),
                result_style,
            )));
        }
    }

    for summary in display.format_result_summary(
        result,
        matches!(
            tool.semantic_status,
            crate::tui::view_model::ToolSemanticStatus::Error
        ),
    ) {
        lines.push(Line::from(Span::styled(
            format!("{INDENT}{summary}"),
            style(tool.style),
        )));
    }
}

fn map_result_style(
    line_style: crate::tui::output_area::LineStyle,
    fallback: SemanticStyle,
) -> SemanticStyle {
    match line_style {
        crate::tui::output_area::LineStyle::Error
        | crate::tui::output_area::LineStyle::ToolCallError => SemanticStyle::Error,
        crate::tui::output_area::LineStyle::ToolCallSuccess => SemanticStyle::Success,
        crate::tui::output_area::LineStyle::ToolCallRunning => SemanticStyle::Running,
        crate::tui::output_area::LineStyle::System
        | crate::tui::output_area::LineStyle::Thinking => SemanticStyle::Muted,
        crate::tui::output_area::LineStyle::Assistant
        | crate::tui::output_area::LineStyle::Normal
        | crate::tui::output_area::LineStyle::DiffAdd
        | crate::tui::output_area::LineStyle::DiffRemove
        | crate::tui::output_area::LineStyle::AskUser
        | crate::tui::output_area::LineStyle::User => fallback,
    }
}

fn style(style: SemanticStyle) -> ratatui::style::Style {
    match style {
        SemanticStyle::Normal => ratatui::style::Style::default().fg(theme::TEXT),
        SemanticStyle::Muted => ratatui::style::Style::default().fg(theme::TEXT_MUTED),
        SemanticStyle::Success => ratatui::style::Style::default().fg(theme::SUCCESS),
        SemanticStyle::Warning => ratatui::style::Style::default().fg(theme::WARNING),
        SemanticStyle::Error => ratatui::style::Style::default().fg(theme::ERROR),
        SemanticStyle::Running | SemanticStyle::Accent => {
            ratatui::style::Style::default().fg(theme::ACCENT)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::view_model::{OutputBlockView, TextBlockView};

    #[test]
    fn test_output_view_model_lines_renders_user_prefix() {
        let vm = OutputViewModel {
            blocks: vec![OutputBlockView::UserMessage(TextBlockView {
                key: "u1".to_string(),
                text: "hello".to_string(),
                style: SemanticStyle::Normal,
            })],
            version: 1,
            follow_tail_hint: true,
        };

        let lines = output_view_model_lines(&vm);

        assert_eq!(lines[0].spans[0].content.as_ref(), "> hello");
    }

    #[test]
    fn test_output_view_model_lines_handles_multiline() {
        let vm = OutputViewModel {
            blocks: vec![OutputBlockView::AssistantMessage(TextBlockView {
                key: "a1".to_string(),
                text: "a\nb".to_string(),
                style: SemanticStyle::Normal,
            })],
            version: 1,
            follow_tail_hint: true,
        };

        let lines = output_view_model_lines(&vm);

        assert_eq!(lines.len(), 2);
        assert_eq!(lines[1].spans[0].content.as_ref(), "b");
    }

    #[test]
    fn test_output_view_model_lines_renders_separator() {
        let vm = OutputViewModel {
            blocks: vec![OutputBlockView::Separator],
            version: 1,
            follow_tail_hint: true,
        };

        let lines = output_view_model_lines(&vm);

        assert!(lines[0].spans.is_empty() || lines[0].spans[0].content.is_empty());
    }

    #[test]
    fn test_output_view_model_lines_renders_grep_tool_result_with_tool_format() {
        let vm = OutputViewModel {
            blocks: vec![OutputBlockView::ToolCall(ToolCallBlockView {
                key: "tool".to_string(),
                chat_id: Some("chat-1".to_string()),
                turn_id: Some("turn-1".to_string()),
                tool_call_id: Some("grep-1".to_string()),
                title: "Grep".to_string(),
                icon: "✓".to_string(),
                semantic_status: crate::tui::view_model::ToolSemanticStatus::Success,
                style: SemanticStyle::Success,
                args_preview: Some("bug 76".to_string()),
                summary: Some(r#"{"pattern":"76","path":"docs/bug/active.md"}"#.to_string()),
                activity_summary: None,
                result_summary: Some(
                    "/tmp/docs/bug/active.md:18:match\n/tmp/docs/bug/active.md:19:next\n/tmp/docs/bug/active.md:20:more\n/tmp/docs/bug/active.md:21:more\n/tmp/docs/bug/active.md:22:more\n/tmp/docs/bug/active.md:23:omitted".to_string(),
                ),
                collapsible: true,
                collapsed: false,
            })],
            version: 1,
            follow_tail_hint: true,
        };

        let rendered = output_view_model_lines(&vm)
            .into_iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<Vec<_>>()
                    .join("")
            })
            .collect::<Vec<_>>();

        assert_eq!(rendered[0], "✓ Grep /76/");
        assert_eq!(rendered[1], "  in docs/bug/active.md");
        assert!(rendered
            .iter()
            .any(|line| line == "  /tmp/docs/bug/active.md:18:match"));
        assert!(rendered
            .iter()
            .any(|line| line == "  ... (1 lines omitted)"));
        assert!(rendered.iter().any(|line| line == "  ✓ Grep completed"));
    }
}
