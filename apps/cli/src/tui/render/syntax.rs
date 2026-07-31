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
use syntect::parsing::SyntaxDefinition;
use syntect::parsing::SyntaxSet;

use crate::tui::render::{output_area::SpanPart, theme};

/// 全局语法集（懒加载，只加载一次）。
///
/// 在 syntect 默认语法集基础上合并内置的 TypeScript / TSX 语法（默认集不含 TS，
/// 资产由 microsoft/TypeScript-TmLanguage 转换而来，见 `assets/syntaxes/`）。
static SYNTAX_SET: Lazy<SyntaxSet> = Lazy::new(|| {
    let mut builder = SyntaxSet::load_defaults_newlines().into_builder();
    for (asset_name, source) in [
        (
            "TypeScript.sublime-syntax",
            include_str!("../../../assets/syntaxes/TypeScript.sublime-syntax"),
        ),
        (
            "TSX.sublime-syntax",
            include_str!("../../../assets/syntaxes/TSX.sublime-syntax"),
        ),
    ] {
        let definition = SyntaxDefinition::load_from_str(source, true, None)
            .unwrap_or_else(|error| panic!("内置语法资产 {asset_name} 加载失败: {error}"));
        builder.add(definition);
    }
    builder.build()
});

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
