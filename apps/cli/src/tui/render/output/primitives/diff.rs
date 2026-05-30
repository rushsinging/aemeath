//! diff 原语：复用现有 build_diff_lines(产出 SpanPart) 转 RenderedLine。

use crate::tui::render::output::diff::build_diff_lines;
use crate::tui::render::output::primitives::spanparts_to_spans;
use crate::tui::render::output::rendered::RenderedLine;
use crate::tui::render::output_area::types::SpanPart;

pub fn diff(old: &str, new: &str, ext: Option<&str>, _width: u16) -> Vec<RenderedLine> {
    let mut out: Vec<Vec<SpanPart>> = Vec::new();
    build_diff_lines(old, new, ext, &mut out);
    out.into_iter()
        .map(|parts| RenderedLine::new(spanparts_to_spans(&parts)))
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
    fn test_diff_line_no_leading_block_indent() {
        // 块缩进由 gutter 注入（#60/#63）：diff 行不再自拼行首 INDENT，删除行从行号区起。
        let lines = diff("a\nb\n", "a\nc\n", Some("rs"), 80);
        let del = lines
            .iter()
            .find(|line| line.plain.contains("- ") && line.plain.contains('b'))
            .expect("删除行存在");

        assert!(
            !del.plain.starts_with("  "),
            "删除行不应自拼行首块缩进（由 gutter 注入），got: {:?}",
            del.plain
        );
    }
}
