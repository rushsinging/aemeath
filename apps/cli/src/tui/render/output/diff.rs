use similar::{ChangeTag, TextDiff};

use crate::tui::render::output_area::types::{SpanPart, INDENT};
use crate::tui::render::syntax::{self, language_by_extension};
use crate::tui::render::theme;
use ratatui::style::Color;

/// Diff 行号 / 高亮颜色常量。
const LINE_NUM_COLOR: Color = theme::TEXT_DIM;
const DIFF_ADD_FG: Color = theme::DIFF_ADD_FG;
const DIFF_REMOVE_FG: Color = theme::DIFF_REMOVE_FG;

/// 对比 old_content 与 new_content，生成带行号和语法高亮的 diff 输出行。
///
/// `file_ext` 用于推断语言进行语法高亮（如 `"rs"`、`"py"`），None 则不进行语法高亮。
/// 每行产出一组 `SpanPart`（着色原语），由调用方转为 `RenderedLine`。
pub fn build_diff_lines(
    old_content: &str,
    new_content: &str,
    file_ext: Option<&str>,
    out: &mut Vec<Vec<SpanPart>>,
) {
    let diff = TextDiff::from_lines(old_content, new_content);
    let changes: Vec<_> = diff.iter_all_changes().collect();

    let old_line_count = old_content.lines().count();
    let new_line_count = new_content.lines().count();
    let width = line_num_width(old_line_count.max(new_line_count));

    let syntax_ref = file_ext.and_then(language_by_extension);

    let mut old_line = 0usize;
    let mut new_line = 0usize;

    for change in &changes {
        match change.tag() {
            ChangeTag::Delete => {
                old_line += 1;
                let line_text = change.to_string();
                let line_text_trimmed = line_text.trim_end_matches('\n');
                out.push(build_delete_line(old_line, width, line_text_trimmed));
            }
            ChangeTag::Insert => {
                new_line += 1;
                let line_text = change.to_string();
                let line_text_trimmed = line_text.trim_end_matches('\n');
                out.push(build_insert_line(
                    new_line,
                    width,
                    line_text_trimmed,
                    syntax_ref.as_ref(),
                ));
            }
            ChangeTag::Equal => {
                old_line += 1;
                new_line += 1;
                let line_text = change.to_string();
                let line_text_trimmed = line_text.trim_end_matches('\n');
                out.push(build_context_line(old_line, new_line, width, line_text_trimmed));
            }
        }
    }
}

/// 构建删除行 spans：`{INDENT}{old_num}  {new_pad} | - {text}`
fn build_delete_line(old_num: usize, width: usize, text: &str) -> Vec<SpanPart> {
    let mut spans = Vec::new();
    spans.push(SpanPart::plain(INDENT.to_string(), LINE_NUM_COLOR));
    // 行号：old_num + 空格占位
    spans.push(SpanPart::plain(
        format!("{:>width$}  {:>width$} ", old_num, "", width = width),
        LINE_NUM_COLOR,
    ));
    // 分隔符 + 标记
    spans.push(SpanPart::plain("| ", LINE_NUM_COLOR));
    spans.push(SpanPart::plain("- ", DIFF_REMOVE_FG));
    // 内容
    spans.push(SpanPart::plain(text.to_string(), DIFF_REMOVE_FG));
    spans
}

/// 构建新增行 spans：`{INDENT}{old_pad}  {new_num} | + {highlighted_text}`
fn build_insert_line(
    new_num: usize,
    width: usize,
    text: &str,
    syntax_ref: Option<&syntect::parsing::SyntaxReference>,
) -> Vec<SpanPart> {
    let mut spans = Vec::new();
    spans.push(SpanPart::plain(INDENT.to_string(), LINE_NUM_COLOR));
    // 行号：空格占位 + new_num
    spans.push(SpanPart::plain(
        format!("{:>width$}  {:>width$} ", "", new_num, width = width),
        LINE_NUM_COLOR,
    ));
    // 分隔符 + 标记
    spans.push(SpanPart::plain("| ", LINE_NUM_COLOR));
    spans.push(SpanPart::plain("+ ", DIFF_ADD_FG));

    // 语法高亮内容
    if let Some(highlighted) = syntax::highlight_line(text, syntax_ref) {
        // 将高亮后的所有 span 的颜色在绿色背景上叠加：保持原始高亮颜色
        spans.extend(highlighted);
    } else {
        spans.push(SpanPart::plain(text.to_string(), DIFF_ADD_FG));
    }
    spans
}

/// 构建上下文行 spans：`{INDENT}{old_num}  {new_num} | {text}`
fn build_context_line(old_num: usize, new_num: usize, width: usize, text: &str) -> Vec<SpanPart> {
    let mut spans = Vec::new();
    spans.push(SpanPart::plain(INDENT.to_string(), LINE_NUM_COLOR));
    spans.push(SpanPart::plain(
        format!("{:>width$}  {:>width$} ", old_num, new_num, width = width),
        LINE_NUM_COLOR,
    ));
    spans.push(SpanPart::plain("| ", LINE_NUM_COLOR));
    spans.push(SpanPart::plain(format!("{INDENT}{text}"), LINE_NUM_COLOR));
    spans
}

/// 计算行号显示宽度（至少 1 位）
fn line_num_width(max_line: usize) -> usize {
    if max_line == 0 {
        return 1;
    }
    let mut w = 0;
    let mut n = max_line;
    while n > 0 {
        n /= 10;
        w += 1;
    }
    w
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_line_num_width() {
        assert_eq!(line_num_width(0), 1);
        assert_eq!(line_num_width(1), 1);
        assert_eq!(line_num_width(9), 1);
        assert_eq!(line_num_width(10), 2);
        assert_eq!(line_num_width(99), 2);
        assert_eq!(line_num_width(100), 3);
        assert_eq!(line_num_width(1000), 4);
    }

    fn line_text(spans: &[SpanPart]) -> String {
        spans.iter().map(|span| span.text.as_str()).collect()
    }

    #[test]
    fn test_build_diff_lines_basic() {
        let old = "line1\nline2\nline3\n";
        let new = "line1\nchanged\nline3\n";
        let mut out = Vec::new();
        build_diff_lines(old, new, None, &mut out);

        // 预期：context(line1) + delete(line2) + insert(changed) + context(line3)
        assert_eq!(out.len(), 4);
        // delete 行含删除标记 "- " 与原文本
        assert!(line_text(&out[1]).contains("- "));
        assert!(line_text(&out[1]).contains("line2"));
        // insert 行含新增标记 "+ " 与新文本
        assert!(line_text(&out[2]).contains("+ "));
        assert!(line_text(&out[2]).contains("changed"));
    }

    #[test]
    fn test_build_diff_lines_with_line_numbers() {
        let old = "a\nb\nc\n";
        let new = "a\nx\nc\n";
        let mut out = Vec::new();
        build_diff_lines(old, new, None, &mut out);

        // 每行都有 spans
        for spans in &out {
            assert!(!spans.is_empty());
        }

        // 删除行(第二行)：old_num=2, new_num 为空
        let full = line_text(&out[1]);
        assert!(
            full.contains("2"),
            "delete line should show old line number 2, got: {full}"
        );

        // 插入行：old_num 为空, new_num=2
        let full = line_text(&out[2]);
        assert!(
            full.contains("2"),
            "insert line should show new line number 2, got: {full}"
        );
    }

    #[test]
    fn test_build_diff_lines_with_syntax_highlight() {
        let old = "fn old() {}\n";
        let new = "fn new() {}\n";
        let mut out = Vec::new();
        build_diff_lines(old, new, Some("rs"), &mut out);

        // Insert 行（第二行）应有语法高亮
        let insert_spans = &out[1];
        assert!(line_text(insert_spans).contains("+ "));
        // 语法高亮会产生多个不同颜色的 span
        assert!(
            insert_spans.len() > 2,
            "syntax highlight should produce multiple spans"
        );
    }

    #[test]
    fn test_build_diff_lines_empty() {
        let mut out = Vec::new();
        build_diff_lines("", "", None, &mut out);
        assert!(out.is_empty());
    }
}
