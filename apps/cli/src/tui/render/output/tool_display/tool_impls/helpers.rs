use super::super::common::truncate_ellipsis_tail;
use crate::tui::render::theme;
use ratatui::style::Style;
use ratatui::text::{Line, Span};

/// 截断路径，保留尾部（更有辨识度）。路径可含非 ASCII（如中文文件名）。
pub(super) fn truncate_path(path: &str, max_width: usize) -> String {
    truncate_ellipsis_tail(path, max_width)
}

/// 构造 `ToolDisplay` 通用 header line 模板：`<name> <path> [<suffix>]`。
pub(super) fn build_header_line(name: &str, path: &str, suffix: &str) -> Line<'static> {
    let display_path = truncate_path(path, 60);
    let mut spans = vec![Span::styled(
        name.to_string(),
        Style::default().fg(theme::ACCENT_BRIGHT),
    )];
    if !display_path.is_empty() {
        spans.push(Span::raw(" "));
        spans.push(Span::styled(display_path, Style::default().fg(theme::TEXT)));
    }
    if !suffix.is_empty() {
        spans.push(Span::styled(
            suffix.to_string(),
            Style::default().fg(theme::TEXT_MUTED),
        ));
    }
    Line::from(spans)
}

#[cfg(test)]
mod tests {
    use super::build_header_line;

    #[test]
    fn build_header_line_no_suffix() {
        let line = build_header_line("Read", "/foo/bar/baz.txt", "");
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(text, "Read /foo/bar/baz.txt");
    }

    #[test]
    fn build_header_line_with_suffix() {
        let line = build_header_line("Read", "/foo/bar/baz.txt", " (5 lines)");
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(text, "Read /foo/bar/baz.txt (5 lines)");
    }

    #[test]
    fn build_header_line_truncates_long_path() {
        let long =
            "/very/very/very/very/very/very/very/very/very/very/very/very/very/long/path/file.txt";
        let line = build_header_line("Read", long, "");
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(text.starts_with("Read "), "expected Read prefix: {text}");
        assert!(
            text.contains("..."),
            "expected ellipsis in long path: {text}"
        );
        assert!(
            text.len() < long.len() + 10,
            "long path should be truncated: {text}"
        );
    }
}
