use crate::tui::display::theme;
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
        OutputBlockView::UserMessage(text) => text_lines("You: ", text),
        OutputBlockView::AssistantMessage(text) => text_lines("", text),
        OutputBlockView::DiagnosticNotice(text) | OutputBlockView::SystemNotice(text) => {
            text_lines("", text)
        }
        OutputBlockView::ToolCall(tool) => tool_lines(tool),
        OutputBlockView::Separator => vec![Line::raw("")],
    }
}

fn text_lines(prefix: &str, text: &TextBlockView) -> Vec<Line<'static>> {
    if text.text.is_empty() {
        return vec![Line::from(Span::styled(
            prefix.to_string(),
            style(text.style),
        ))];
    }
    text.text
        .lines()
        .enumerate()
        .map(|(index, line)| {
            let rendered = if index == 0 {
                format!("{prefix}{line}")
            } else {
                line.to_string()
            };
            Line::from(Span::styled(rendered, style(text.style)))
        })
        .collect()
}

fn tool_lines(tool: &ToolCallBlockView) -> Vec<Line<'static>> {
    let mut lines = vec![Line::from(Span::styled(
        format!("{} {}", tool.icon, tool.title),
        style(tool.style),
    ))];
    if let Some(summary) = &tool.summary {
        lines.push(Line::from(Span::styled(
            format!("  │ {summary}"),
            style(SemanticStyle::Muted),
        )));
    }
    if let Some(args) = &tool.args_preview {
        lines.push(Line::from(Span::styled(
            format!("  │ {args}"),
            style(SemanticStyle::Muted),
        )));
    }
    if let Some(result) = &tool.result_summary {
        lines.push(Line::from(Span::styled(
            format!("  └ {result}"),
            style(tool.style),
        )));
    }
    lines
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

        assert_eq!(lines[0].spans[0].content.as_ref(), "You: hello");
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
}
