//! TUI 模式下的语法高亮封装。
//!
//! 基于 syntect，将代码行高亮为 `Vec<SpanPart>` 供 ratatui 渲染。

use std::str::FromStr;

use once_cell::sync::Lazy;
use syntect::easy::HighlightLines;
use syntect::highlighting::{
    Color as SyntectColor, FontStyle, StyleModifier, Theme as SyntectTheme, ThemeItem,
    ThemeSettings,
};
use syntect::parsing::SyntaxSet;

use crate::tui::render::{output_area::SpanPart, theme};

/// 全局语法集（懒加载，只加载一次）
static SYNTAX_SET: Lazy<SyntaxSet> = Lazy::new(SyntaxSet::load_defaults_newlines);

/// 全局主题集，使用 Catppuccin Macchiato，与 TUI palette 保持一致。
static THEME: Lazy<SyntectTheme> = Lazy::new(catppuccin_macchiato_theme);

fn catppuccin_macchiato_theme() -> SyntectTheme {
    SyntectTheme {
        name: Some("Catppuccin Macchiato".to_string()),
        author: Some("Catppuccin Org".to_string()),
        settings: ThemeSettings {
            foreground: Some(to_syntect_color(theme::TEXT)),
            background: Some(to_syntect_color(theme::SURFACE)),
            caret: Some(to_syntect_color(theme::SUBTEXT1)),
            line_highlight: Some(to_syntect_color(theme::SURFACE0)),
            selection: Some(to_syntect_color(theme::SURFACE1)),
            selection_foreground: Some(to_syntect_color(theme::TEXT)),
            gutter_foreground: Some(to_syntect_color(theme::OVERLAY2)),
            accent: Some(to_syntect_color(theme::ACCENT)),
            ..ThemeSettings::default()
        },
        scopes: catppuccin_macchiato_scopes(),
    }
}

fn catppuccin_macchiato_scopes() -> Vec<ThemeItem> {
    vec![
        scope("comment", theme::OVERLAY2, Some(FontStyle::ITALIC)),
        scope(
            "comment.line.shebang.shell, constant.language.shebang",
            theme::PINK,
            Some(FontStyle::ITALIC),
        ),
        scope("string", theme::GREEN, None),
        scope("string.regexp", theme::PINK, None),
        scope("constant.numeric", theme::PEACH, None),
        scope(
            "constant.language.boolean",
            theme::PEACH,
            Some(FontStyle::BOLD | FontStyle::ITALIC),
        ),
        scope("constant.language", theme::PEACH, Some(FontStyle::ITALIC)),
        scope(
            "support.function.builtin",
            theme::PEACH,
            Some(FontStyle::ITALIC),
        ),
        scope(
            "variable.other.constant, entity.name.constant",
            theme::PEACH,
            None,
        ),
        scope("constant.other.symbol", theme::RED, None),
        scope("keyword", theme::MAUVE, Some(FontStyle::ITALIC)),
        scope(
            "keyword.control.loop, keyword.control.conditional",
            theme::MAUVE,
            Some(FontStyle::BOLD),
        ),
        scope(
            "keyword.control.return, keyword.control.flow.return",
            theme::MAUVE,
            Some(FontStyle::BOLD),
        ),
        scope("keyword.declaration", theme::MAUVE, Some(FontStyle::ITALIC)),
        scope("keyword.operator.word", theme::MAUVE, None),
        scope("punctuation.accessor, keyword.operator", theme::TEAL, None),
        scope(
            "punctuation.separator, punctuation.terminator, punctuation.section",
            theme::OVERLAY2,
            None,
        ),
        scope(
            "keyword.control.import, keyword.control.import.include",
            theme::MAUVE,
            Some(FontStyle::ITALIC),
        ),
        scope("keyword", theme::MAUVE, Some(FontStyle::ITALIC)),
        scope("storage.type", theme::YELLOW, Some(FontStyle::ITALIC)),
        scope("storage.modifier", theme::MAUVE, None),
        scope("entity.name.namespace", theme::YELLOW, Some(FontStyle::ITALIC)),
        scope("storage.type.class", theme::ROSEWATER, Some(FontStyle::ITALIC)),
        scope("entity.name.label", theme::BLUE, None),
        scope(
            "entity.name.class, meta.toc-list.full-identifier",
            theme::YELLOW,
            None,
        ),
        scope(
            "entity.name.function, variable.function, support.function",
            theme::BLUE,
            Some(FontStyle::ITALIC),
        ),
        scope("entity.name.function.preprocessor", theme::RED, None),
        scope("support.constant", theme::BLUE, None),
        scope(
            "support.type, support.class, entity.name.type, entity.name.struct, entity.name.impl, entity.name.trait, entity.name.union, meta.enum, entity.other.inherited-class",
            theme::YELLOW,
            Some(FontStyle::ITALIC),
        ),
        scope(
            "storage.type.primitive, support.type.primitive, support.type.builtin, storage.type.c, storage.type.cs, support.type.python",
            theme::MAUVE,
            None,
        ),
        scope("variable.parameter, variable.parameter.function", theme::MAROON, Some(FontStyle::ITALIC)),
        scope("variable.other.member", theme::TEXT, None),
        scope("variable.language", theme::RED, None),
        scope(
            "variable.annotation, punctuation.definition.annotation",
            theme::PEACH,
            None,
        ),
        scope(
            "variable.annotation.rust, variable.annotation.cs, punctuation.definition.annotation.rust",
            theme::YELLOW,
            None,
        ),
        scope("entity.name.tag", theme::BLUE, None),
        scope(
            "entity.other.attribute-name",
            theme::YELLOW,
            Some(FontStyle::ITALIC),
        ),
        scope(
            "punctuation.definition.tag, punctuation.separator.key-value",
            theme::TEAL,
            None,
        ),
        scope(
            "markup.underline.link",
            theme::BLUE,
            Some(FontStyle::ITALIC | FontStyle::UNDERLINE),
        ),
        scope("markup.raw.code-fence", theme::TEXT, None),
        scope("markup.raw.inline", theme::GREEN, None),
        scope("markup.heading.1", theme::RED, None),
        scope("markup.heading.2", theme::PEACH, None),
        scope("markup.heading.3", theme::YELLOW, None),
        scope("markup.heading.4", theme::GREEN, None),
        scope("markup.heading.5", theme::SAPPHIRE, None),
        scope("markup.heading.6", theme::LAVENDER, None),
        scope("markup.italic", theme::MAROON, Some(FontStyle::ITALIC)),
        scope("markup.bold", theme::MAROON, Some(FontStyle::BOLD)),
        scope("constant.character.escape", theme::PINK, None),
        scope("support.macro.rust", theme::BLUE, None),
        scope(
            "meta.macro.rust meta.macro.matchers.rust variable.parameter.rust",
            theme::PINK,
            None,
        ),
        scope("punctuation.definition.generic", theme::TEAL, None),
        scope("invalid", theme::RED, None),
        scope("meta.diff, meta.diff.header", theme::OVERLAY1, None),
        scope("markup.deleted", theme::RED, None),
        scope("markup.inserted", theme::GREEN, None),
        scope("markup.changed", theme::YELLOW, None),
        scope("message.error", theme::RED, None),
        scope("source.json meta.mapping.key string", theme::BLUE, None),
        scope(
            "source.json meta.mapping.key punctuation.definition.string.begin, source.json meta.mapping.key punctuation.definition.string.end",
            theme::OVERLAY2,
            None,
        ),
        scope("source.yaml meta.mapping.key string.unquoted", theme::BLUE, None),
        scope(
            "variable.other.alias, entity.name.other.anchor",
            theme::YELLOW,
            None,
        ),
        scope("constant.other.datetime.toml", theme::PINK, None),
        scope("entity.name.table.toml", theme::YELLOW, None),
    ]
}

fn scope(selector: &str, color: Color, font_style: Option<FontStyle>) -> ThemeItem {
    ThemeItem {
        scope: syntect::highlighting::ScopeSelectors::from_str(selector)
            .expect("hard-coded Catppuccin scope selector must be valid"),
        style: StyleModifier {
            foreground: Some(to_syntect_color(color)),
            background: None,
            font_style,
        },
    }
}

fn to_syntect_color(color: Color) -> SyntectColor {
    match color {
        Color::Rgb(r, g, b) => SyntectColor { r, g, b, a: 0xff },
        _ => SyntectColor {
            r: 202,
            g: 211,
            b: 245,
            a: 0xff,
        },
    }
}

/// 从文件扩展名推断 syntect 语言，失败返回 None。
pub fn language_by_extension(ext: &str) -> Option<syntect::parsing::SyntaxReference> {
    SYNTAX_SET.find_syntax_by_extension(ext).cloned()
}

/// 从 Markdown fenced code info string 推断 syntect 语言。
///
/// Info string 常用语言名（如 `rust`），不一定是文件扩展名（如 `rs`）。
pub fn language_by_fence_info(info: &str) -> Option<syntect::parsing::SyntaxReference> {
    let lang = info.split_whitespace().next()?.to_ascii_lowercase();
    let ext = match lang.as_str() {
        "rust" => "rs",
        _ => lang.as_str(),
    };
    language_by_extension(ext).or_else(|| SYNTAX_SET.find_syntax_by_name(&lang).cloned())
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
    fn test_language_by_fence_info_maps_rust_name() {
        let by_name = language_by_fence_info("rust").expect("rust fence should resolve");
        let by_ext = language_by_extension("rs").expect("rs extension should resolve");

        assert_eq!(by_name.name, by_ext.name);
    }

    #[test]
    fn test_language_by_fence_info_keeps_extension_path() {
        let by_info = language_by_fence_info("rs").expect("rs fence should resolve");
        let by_ext = language_by_extension("rs").expect("rs extension should resolve");

        assert_eq!(by_info.name, by_ext.name);
    }

    #[test]
    fn test_highlight_line_uses_catppuccin_macchiato_keyword_color() {
        let syntax = language_by_extension("rs").unwrap();
        let spans = highlight_line("if true {", Some(&syntax)).unwrap();
        let keyword = spans.iter().find(|span| span.text == "if").unwrap();

        assert_eq!(keyword.color, crate::tui::render::theme::ACCENT_BRIGHT);
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
