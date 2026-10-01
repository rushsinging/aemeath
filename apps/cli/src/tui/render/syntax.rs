//! TUI 模式下的语法高亮封装。
//!
//! 基于 syntect，将代码行高亮为 `Vec<SpanPart>` 供 ratatui 渲染。

use super::constants::SYNTAX_SET;
use super::theme::constants::THEME;

use syntect::easy::HighlightLines;

use crate::tui::render::output_area::SpanPart;

/// 从文件扩展名推断 syntect 语言，失败返回 None。
pub fn language_by_extension(ext: &str) -> Option<syntect::parsing::SyntaxReference> {
    SYNTAX_SET.find_syntax_by_extension(ext).cloned()
}

/// 从 Markdown fenced code info string 推断 syntect 语言。
///
/// Info string 常用语言名（如 `rust`），不一定是文件扩展名（如 `rs`）。
/// TS 生态（`ts`/`tsx`/`typescript`/`mts`/`cts`）优先解析为内置 TypeScript 语法；
/// 语法缺失时回退到 JavaScript，保证 TS 代码至少获得 JS 级高亮。
pub fn language_by_fence_info(info: &str) -> Option<syntect::parsing::SyntaxReference> {
    let lang = info.split_whitespace().next()?.to_ascii_lowercase();
    let ext = match lang.as_str() {
        "rust" => "rs",
        "typescript" => "ts",
        "tsx" => "tsx",
        "mts" | "cts" => "ts",
        _ => lang.as_str(),
    };
    language_by_extension(ext)
        .or_else(|| {
            if matches!(lang.as_str(), "ts" | "tsx" | "typescript" | "mts" | "cts") {
                language_by_extension("js")
            } else {
                None
            }
        })
        .or_else(|| SYNTAX_SET.find_syntax_by_name(&lang).cloned())
}

/// 一段同语言代码的有状态语法高亮会话。
///
/// 同一代码块或 diff 必须复用该会话，让 syntect 保留跨行解析状态，避免逐行重建
/// `HighlightLines` 及其正则上下文。
pub(crate) struct SyntaxHighlighter<'a> {
    highlighter: HighlightLines<'a>,
}

impl<'a> SyntaxHighlighter<'a> {
    pub(crate) fn new(syntax: &'a syntect::parsing::SyntaxReference) -> Self {
        #[cfg(test)]
        crate::tui::render::performance::record_syntax_highlighter_creation();
        Self {
            highlighter: HighlightLines::new(syntax, &THEME),
        }
    }

    pub(crate) fn highlight_line(&mut self, line: &str) -> Option<Vec<SpanPart>> {
        #[cfg(test)]
        let started = std::time::Instant::now();
        let ranges = self.highlighter.highlight_line(line, &SYNTAX_SET).ok();
        #[cfg(test)]
        crate::tui::render::performance::record_syntax_highlight(line.len(), started.elapsed());
        let ranges = ranges?;

        Some(
            ranges
                .into_iter()
                .map(|(style, text)| SpanPart {
                    text: text.to_string(),
                    color: Color::Rgb(style.foreground.r, style.foreground.g, style.foreground.b),
                })
                .collect(),
        )
    }
}

/// 对单行代码进行语法高亮，返回带颜色的文本段。
///
/// `syntax_ref` 为 None 时返回 None（调用方回退到纯色渲染）。
pub fn highlight_line(
    line: &str,
    syntax_ref: Option<&syntect::parsing::SyntaxReference>,
) -> Option<Vec<SpanPart>> {
    let syntax = syntax_ref?;
    SyntaxHighlighter::new(syntax).highlight_line(line)
}

/// 从文件路径提取扩展名（不含点）。
pub fn extension_from_path(path: &str) -> Option<&str> {
    std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
}

use ratatui::style::Color;

#[cfg(test)]
#[path = "syntax_tests.rs"]
mod tests;
