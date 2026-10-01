//! unified diff 原语：识别 LLM markdown ` ```diff ` 代码块内的统一 diff 文本，
//! 复用 `output/diff.rs` 的 `INDENT` 缩进 + `DIFF_ADD_FG`/`DIFF_REMOVE_FG` 语义色风格
//! 渲染为 `RenderedLine`（bug #61：修复 diff 行贴最左、选中高亮丢失）。
//!
//! 与 `primitives::diff::diff`（基于 `similar` 重算行号）不同：unified diff 文本自带
//! `@@ ... @@` 行号信息，按原文呈现即可，不重算行号；added 行（去前导 `+`）可选语法高亮。

use crate::tui::render::output::primitives::spanparts_to_spans;
use crate::tui::render::output::rendered::RenderedLine;
use crate::tui::render::output_area::types::{SpanPart, INDENT};
use crate::tui::render::syntax::{self, extension_from_path, language_by_extension};
use crate::tui::render::theme;
use ratatui::style::Color;

/// unified diff 行类型。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DiffLineKind {
    /// `@@ -a,b +c,d @@` hunk 头。
    Hunk,
    /// `+` 起始的新增行（但非 `+++` 文件头）。
    Added,
    /// `-` 起始的删除行（但非 `---` 文件头）。
    Removed,
    /// 文件头 `+++`/`---`/`diff `/`index ` 等元信息。
    Meta,
    /// 上下文行及其它。
    Context,
}

/// 识别单行 unified diff 类型。
fn classify(line: &str) -> DiffLineKind {
    if line.starts_with("@@") {
        DiffLineKind::Hunk
    } else if line.starts_with("+++")
        || line.starts_with("---")
        || line.starts_with("diff ")
        || line.starts_with("index ")
    {
        DiffLineKind::Meta
    } else if line.starts_with('+') {
        DiffLineKind::Added
    } else if line.starts_with('-') {
        DiffLineKind::Removed
    } else {
        DiffLineKind::Context
    }
}

/// 渲染一段 unified diff 文本为带缩进 + diff 语义色 + 正文语法高亮的渲染行。
///
/// `ext` 用于对正文做语法高亮（去掉 diff 前导符号后高亮再补回语义符号）；None 时尝试从 diff 文件头推断。
/// `_width` 预留参数（与 `primitives::diff::diff` 签名对齐），当前不换行。
pub fn render_unified_diff(text: &str, ext: Option<&str>, _width: u16) -> Vec<RenderedLine> {
    let inferred_ext = ext
        .map(str::to_string)
        .or_else(|| infer_ext_from_diff(text));
    let syntax_ref = inferred_ext.as_deref().and_then(language_by_extension);
    text.lines()
        .map(|line| render_line(line, syntax_ref.as_ref()))
        .collect()
}

fn infer_ext_from_diff(text: &str) -> Option<String> {
    text.lines().find_map(infer_ext_from_diff_line)
}

fn infer_ext_from_diff_line(line: &str) -> Option<String> {
    if let Some(path) = line
        .strip_prefix("+++ ")
        .or_else(|| line.strip_prefix("--- "))
    {
        return diff_path_extension(path);
    }

    if let Some(rest) = line.strip_prefix("diff --git ") {
        return rest.split_whitespace().rev().find_map(diff_path_extension);
    }

    None
}

fn diff_path_extension(path: &str) -> Option<String> {
    let path = path.trim();
    if path == "/dev/null" {
        return None;
    }
    let path = path
        .strip_prefix("a/")
        .or_else(|| path.strip_prefix("b/"))
        .unwrap_or(path);
    extension_from_path(path).map(str::to_string)
}

/// 单行渲染：保持 `INDENT` 缩进（修 #61 贴最左），按类型着色。
fn render_line(line: &str, syntax_ref: Option<&syntect::parsing::SyntaxReference>) -> RenderedLine {
    let kind = classify(line);
    let mut parts: Vec<SpanPart> = vec![SpanPart::plain(INDENT.to_string(), theme::TEXT_DIM)];
    match kind {
        DiffLineKind::Hunk => {
            parts.push(SpanPart::plain(line.to_string(), theme::TEXT_DIM));
        }
        DiffLineKind::Meta => {
            parts.push(SpanPart::plain(line.to_string(), theme::TEXT_MUTED));
        }
        DiffLineKind::Removed => {
            let body = line.strip_prefix('-').unwrap_or(line);
            parts.push(SpanPart::plain("-".to_string(), theme::DIFF_REMOVE_FG));
            push_removed_body(&mut parts, body);
        }
        DiffLineKind::Added => {
            // 去掉前导 '+'（单 ASCII 字节）做语法高亮，再补回 '+' 前缀语义符号。
            let body = line.strip_prefix('+').unwrap_or(line);
            parts.push(SpanPart::plain("+".to_string(), theme::DIFF_ADD_FG));
            push_highlighted_body(&mut parts, body, theme::DIFF_ADD_FG, syntax_ref);
        }
        DiffLineKind::Context => {
            let body = line.strip_prefix(' ').unwrap_or(line);
            parts.push(SpanPart::plain(" ".to_string(), theme::TEXT_DIM));
            push_highlighted_body(&mut parts, body, theme::TEXT, syntax_ref);
        }
    }
    RenderedLine::new(spanparts_to_spans(&parts))
}

fn push_removed_body(parts: &mut Vec<SpanPart>, body: &str) {
    parts.push(SpanPart::plain(body.to_string(), theme::DIFF_REMOVE_FG));
}

/// 将 body 高亮后追加；无语法引用或高亮失败时回退为 `fallback` 单色。
fn push_highlighted_body(
    parts: &mut Vec<SpanPart>,
    body: &str,
    fallback: Color,
    syntax_ref: Option<&syntect::parsing::SyntaxReference>,
) {
    if let Some(highlighted) = syntax::highlight_line(body, syntax_ref) {
        parts.extend(highlighted);
    } else {
        parts.push(SpanPart::plain(body.to_string(), fallback));
    }
}

#[cfg(test)]
#[path = "unified_diff_tests.rs"]
mod tests;
