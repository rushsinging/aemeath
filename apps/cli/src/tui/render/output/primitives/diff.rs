//! diff 原语：复用现有 build_diff_lines(产出 OutputLine+SpanPart) 转 RenderedLine。

use crate::tui::render::output::diff::build_diff_lines;
use crate::tui::render::output::primitives::spanparts_to_spans;
use crate::tui::render::output::rendered::RenderedLine;
use crate::tui::render::output_area::types::OutputLine;

pub fn diff(old: &str, new: &str, ext: Option<&str>, _width: u16) -> Vec<RenderedLine> {
    let mut out: Vec<OutputLine> = Vec::new();
    build_diff_lines(old, new, ext, &None, &mut out);
    out.into_iter()
        .map(|line| match line.spans {
            Some(parts) => {
                let spans = spanparts_to_spans(&parts);
                RenderedLine::new(spans)
            }
            None => RenderedLine::new(vec![ratatui::text::Span::raw(line.content)]),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_diff_emits_add_remove_with_color_and_plain() {
        let lines = diff("a\nb\n", "a\nc\n", Some("rs"), 80);
        let plains = lines
            .iter()
            .map(|line| line.plain.as_str())
            .collect::<Vec<_>>();

        assert!(
            plains
                .iter()
                .any(|plain| plain.contains('-') && plain.contains('b')),
            "应含删除行 b"
        );
        assert!(
            plains
                .iter()
                .any(|plain| plain.contains('+') && plain.contains('c')),
            "应含新增行 c"
        );
        assert!(
            lines
                .iter()
                .any(|line| line.spans.iter().any(|span| span.style.fg.is_some())),
            "至少一行带颜色 span（语义色）"
        );
    }

    #[test]
    fn test_diff_line_keeps_left_indent_not_flush_left() {
        let lines = diff("x\n", "y\n", None, 80);

        assert!(
            lines.iter().all(|line| line.plain.starts_with("  ")),
            "每行应保留两空格缩进"
        );
    }
}
