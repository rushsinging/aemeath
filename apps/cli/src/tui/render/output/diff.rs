use similar::{ChangeTag, TextDiff};

use crate::tui::render::output_area::types::{SpanPart, INDENT};
use crate::tui::render::syntax::{language_by_extension, SyntaxHighlighter};
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
    build_diff_lines_from(old_content, new_content, 1, 1, file_ext, out);
}

/// 对比 old_content 与 new_content，生成从真实文件行号开始的 diff 输出行。
pub fn build_diff_lines_from(
    old_content: &str,
    new_content: &str,
    old_start: usize,
    new_start: usize,
    file_ext: Option<&str>,
    out: &mut Vec<Vec<SpanPart>>,
) {
    #[cfg(test)]
    let started = std::time::Instant::now();
    #[cfg(test)]
    let initial_output_lines = out.len();
    let old_start = old_start.max(1);
    let new_start = new_start.max(1);
    let diff = TextDiff::from_lines(old_content, new_content);
    let changes: Vec<_> = diff.iter_all_changes().collect();

    let old_line_count = old_content.lines().count();
    let new_line_count = new_content.lines().count();
    let max_old_line = old_start.saturating_add(old_line_count.saturating_sub(1));
    let max_new_line = new_start.saturating_add(new_line_count.saturating_sub(1));
    let width = line_num_width(max_old_line.max(max_new_line));

    let syntax_ref = file_ext.and_then(language_by_extension);
    let mut syntax_highlighter = syntax_ref.as_ref().map(SyntaxHighlighter::new);

    let mut old_line = old_start - 1;
    let mut new_line = new_start - 1;

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
                    syntax_highlighter.as_mut(),
                ));
            }
            ChangeTag::Equal => {
                old_line += 1;
                new_line += 1;
                let line_text = change.to_string();
                let line_text_trimmed = line_text.trim_end_matches('\n');
                out.push(build_context_line(
                    old_line,
                    new_line,
                    width,
                    line_text_trimmed,
                    syntax_highlighter.as_mut(),
                ));
            }
        }
    }
    #[cfg(test)]
    crate::tui::render::performance::record_diff_build(
        out.len().saturating_sub(initial_output_lines),
        started.elapsed(),
    );
}

/// 构建删除行 spans：`{old_num}  {new_pad} | - {highlighted_text}`（块缩进由 gutter 注入，#60/#63）。
fn build_delete_line(old_num: usize, width: usize, text: &str) -> Vec<SpanPart> {
    let mut spans = Vec::new();
    // 行号：old_num + 空格占位
    spans.push(SpanPart::plain(
        format!("{:>width$}  {:>width$} ", old_num, "", width = width),
        LINE_NUM_COLOR,
    ));
    // 分隔符 + 标记
    spans.push(SpanPart::plain("| ", LINE_NUM_COLOR));
    spans.push(SpanPart::plain("- ", DIFF_REMOVE_FG));
    push_deleted_text(&mut spans, text);
    spans
}

fn push_deleted_text(spans: &mut Vec<SpanPart>, text: &str) {
    spans.push(SpanPart::plain(text.to_string(), DIFF_REMOVE_FG));
}

/// 构建新增行 spans：`{old_pad}  {new_num} | + {highlighted_text}`（块缩进由 gutter 注入）。
fn build_insert_line(
    new_num: usize,
    width: usize,
    text: &str,
    syntax_highlighter: Option<&mut SyntaxHighlighter<'_>>,
) -> Vec<SpanPart> {
    let mut spans = Vec::new();
    // 行号：空格占位 + new_num
    spans.push(SpanPart::plain(
        format!("{:>width$}  {:>width$} ", "", new_num, width = width),
        LINE_NUM_COLOR,
    ));
    // 分隔符 + 标记
    spans.push(SpanPart::plain("| ", LINE_NUM_COLOR));
    spans.push(SpanPart::plain("+ ", DIFF_ADD_FG));

    push_highlighted_text(&mut spans, text, DIFF_ADD_FG, syntax_highlighter);
    spans
}

fn push_highlighted_text(
    spans: &mut Vec<SpanPart>,
    text: &str,
    fallback: Color,
    syntax_highlighter: Option<&mut SyntaxHighlighter<'_>>,
) {
    if let Some(highlighter) = syntax_highlighter {
        if let Some(highlighted) = highlighter.highlight_line(text) {
            spans.extend(highlighted);
            return;
        }
    }
    spans.push(SpanPart::plain(text.to_string(), fallback));
}

/// 构建上下文行 spans：`{old_num}  {new_num} | {INDENT}{highlighted_text}`（行首块缩进由 gutter 注入，
/// 内容前 INDENT 保留以与 `+ ` / `- ` 标记列对齐）。
fn build_context_line(
    old_num: usize,
    new_num: usize,
    width: usize,
    text: &str,
    syntax_highlighter: Option<&mut SyntaxHighlighter<'_>>,
) -> Vec<SpanPart> {
    let mut spans = Vec::new();
    spans.push(SpanPart::plain(
        format!("{:>width$}  {:>width$} ", old_num, new_num, width = width),
        LINE_NUM_COLOR,
    ));
    spans.push(SpanPart::plain("| ", LINE_NUM_COLOR));
    spans.push(SpanPart::plain(INDENT.to_string(), LINE_NUM_COLOR));
    push_highlighted_text(&mut spans, text, LINE_NUM_COLOR, syntax_highlighter);
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
#[path = "diff_tests.rs"]
mod tests;
