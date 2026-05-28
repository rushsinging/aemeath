//! TUI 模式下的语法高亮封装。
//!
//! 基于 syntect，将代码行高亮为 `Vec<SpanPart>` 供 ratatui 渲染。
#![allow(dead_code)]

use once_cell::sync::Lazy;
use syntect::easy::HighlightLines;
use syntect::highlighting::{Theme as SyntectTheme, ThemeSet};
use syntect::parsing::SyntaxSet;

use crate::tui::output_area::SpanPart;

/// 全局语法集（懒加载，只加载一次）
static SYNTAX_SET: Lazy<SyntaxSet> = Lazy::new(SyntaxSet::load_defaults_newlines);

/// 全局主题集，使用 `base16-ocean.dark` 主题，提供丰富的语法着色。
static THEME: Lazy<SyntectTheme> = Lazy::new(|| {
    let ts = ThemeSet::load_defaults();
    ts.themes
        .get("base16-ocean.dark")
        .expect("default ThemeSet must contain base16-ocean.dark")
        .clone()
});

/// 从文件扩展名推断 syntect 语言，失败返回 None。
pub fn language_by_extension(ext: &str) -> Option<syntect::parsing::SyntaxReference> {
    SYNTAX_SET.find_syntax_by_extension(ext).cloned()
}

/// 对单行代码进行语法高亮，返回带颜色的文本段。
///
/// `syntax_ref` 为 None 时返回 None（调用方回退到纯色渲染）。
pub fn highlight_line(
    line: &str,
    syntax_ref: Option<&syntect::parsing::SyntaxReference>,
) -> Option<Vec<SpanPart>> {
    let syntax = syntax_ref?;
    let mut highlighter = HighlightLines::new(syntax, &THEME);
    let ranges = highlighter.highlight_line(line, &SYNTAX_SET).ok()?;

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

/// 从文件路径提取扩展名（不含点）。
pub fn extension_from_path(path: &str) -> Option<&str> {
    std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
}

use ratatui::style::Color;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extension_from_path() {
        assert_eq!(extension_from_path("src/lib.rs"), Some("rs"));
        assert_eq!(extension_from_path("foo.tsx"), Some("tsx"));
        assert_eq!(extension_from_path("Makefile"), None);
        assert_eq!(extension_from_path("dir/file"), None);
    }

    #[test]
    fn test_language_by_extension() {
        let syntax = language_by_extension("rs");
        assert!(syntax.is_some(), "Rust syntax should be found");
    }

    #[test]
    fn test_highlight_line_with_rust() {
        let syntax = language_by_extension("rs").unwrap();
        let result = highlight_line("fn main() {", Some(&syntax));
        assert!(result.is_some());
        let spans = result.unwrap();
        assert!(!spans.is_empty());
        // "fn" 应该被高亮为关键字
        let fn_text: String = spans.iter().map(|s| s.text.as_str()).collect();
        assert!(fn_text.contains("fn"));
    }

    #[test]
    fn test_highlight_line_none_syntax() {
        let result = highlight_line("hello", None);
        assert!(result.is_none());
    }
}
