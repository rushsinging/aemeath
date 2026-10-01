//! 纯值常量（#1146 placement 归位）。

/// Diff 删除行前景色。
/// Diff 删除行前景色。
use ratatui::style::Color;

pub const DIFF_REMOVE_FG: Color = Color::Rgb(220, 100, 110);

/// Diff 新增行前景色。
/// Diff 新增行前景色。
pub const DIFF_ADD_FG: Color = Color::Rgb(56, 166, 96);

/// Spinner 弱化色。
/// Spinner 弱化色。
pub const SPINNER_DIM: Color = SURFACE2;

/// Spinner 高亮色。
/// Spinner 高亮色。
pub const SPINNER_HIGHLIGHT: Color = GREEN;

/// Spinner 基础色。
/// Spinner 基础色。
pub const SPINNER_BASE: Color = TEAL;

/// 行内代码与代码块强调色。
/// 行内代码与代码块强调色。
pub const CODE: Color = TOOL_RUNNING;

/// Markdown 链接色。
/// Markdown 链接色。
pub const LINK: Color = BLUE;

/// Thinking 文本色。
/// Thinking 文本色。
pub const THINKING: Color = OVERLAY1;

/// 错误色。
/// 错误色。
pub const ERROR: Color = RED;

/// 警告色。
/// 警告色。
pub const WARNING: Color = YELLOW;

/// 成功色。
/// 成功色。
pub const SUCCESS: Color = GREEN;

/// 工具运行中色。
/// 工具运行中色。
pub const TOOL_RUNNING: Color = PEACH;

/// 助手消息色。使用 SUBTEXT1 替代 TEXT（近白），降低大面积正文亮度以减少暗光环境下的眼部疲劳。
/// 助手消息色。使用 SUBTEXT1 替代 TEXT（近白），降低大面积正文亮度以减少暗光环境下的眼部疲劳。
pub const ASSISTANT: Color = SUBTEXT1;

/// 用户消息背景色。
/// 用户消息背景色。
pub const USER_BG: Color = Color::Rgb(63, 95, 143);

/// 用户消息色。
/// 用户消息色。
pub const USER: Color = Color::Rgb(220, 232, 255);

/// 选中前景。
/// 选中前景。
pub const SELECTION_FG: Color = TEXT;

/// 选中背景。
/// 选中背景。
pub const SELECTION_BG: Color = SURFACE1;

/// 浮层背景。
/// 浮层背景。
pub const SURFACE_ELEVATED: Color = SURFACE0;

/// 状态栏背景。
/// 状态栏背景。
pub const STATUS_BG: Color = SURFACE;

/// 深色背景。
/// 深色背景。
pub const SURFACE: Color = BASE;

/// 强调色高亮。
/// 强调色高亮。
pub const ACCENT_BRIGHT: Color = MAUVE;

/// 聚焦边框与品牌强调色。
/// 聚焦边框与品牌强调色。
pub const ACCENT: Color = BLUE;

/// 面板边框色。
/// 面板边框色。
pub const BORDER: Color = SURFACE1;

/// 弱化文本色。
/// 弱化文本色。
pub const TEXT_DIM: Color = OVERLAY0;

/// 次级文本色。
/// 次级文本色。
pub const TEXT_MUTED: Color = SUBTEXT0;

/// 主文本色。
/// 主文本色。
pub const TEXT: Color = MACCHIATO_TEXT;

/// Catppuccin Macchiato base。
/// Catppuccin Macchiato base。
pub const BASE: Color = Color::Rgb(36, 39, 58);

/// Catppuccin Macchiato surface0。
/// Catppuccin Macchiato surface0。
pub const SURFACE0: Color = Color::Rgb(54, 58, 79);

/// Catppuccin Macchiato surface1。
/// Catppuccin Macchiato surface1。
pub const SURFACE1: Color = Color::Rgb(73, 77, 100);

/// Catppuccin Macchiato surface2。
/// Catppuccin Macchiato surface2。
pub const SURFACE2: Color = Color::Rgb(91, 96, 120);

/// Catppuccin Macchiato overlay0。
/// Catppuccin Macchiato overlay0。
pub const OVERLAY0: Color = Color::Rgb(110, 115, 141);

/// Catppuccin Macchiato overlay1。
/// Catppuccin Macchiato overlay1。
pub const OVERLAY1: Color = Color::Rgb(128, 135, 162);

/// Catppuccin Macchiato overlay2。
/// Catppuccin Macchiato overlay2。
pub const OVERLAY2: Color = Color::Rgb(147, 154, 183);

/// Catppuccin Macchiato subtext0。
/// Catppuccin Macchiato subtext0。
pub const SUBTEXT0: Color = Color::Rgb(165, 173, 203);

/// Catppuccin Macchiato subtext1。
/// Catppuccin Macchiato subtext1。
pub const SUBTEXT1: Color = Color::Rgb(184, 192, 224);

/// Catppuccin Macchiato text。
/// Catppuccin Macchiato text。
pub const MACCHIATO_TEXT: Color = Color::Rgb(202, 211, 245);

/// Catppuccin Macchiato lavender。
/// Catppuccin Macchiato lavender。
pub const LAVENDER: Color = Color::Rgb(183, 189, 248);

/// Catppuccin Macchiato blue。
/// Catppuccin Macchiato blue。
pub const BLUE: Color = Color::Rgb(138, 173, 244);

/// Catppuccin Macchiato sapphire。
/// Catppuccin Macchiato sapphire。
pub const SAPPHIRE: Color = Color::Rgb(125, 196, 228);

/// Catppuccin Macchiato teal。
/// Catppuccin Macchiato teal。
pub const TEAL: Color = Color::Rgb(139, 213, 202);

/// Catppuccin Macchiato green。
/// Catppuccin Macchiato green。
pub const GREEN: Color = Color::Rgb(166, 218, 149);

/// Catppuccin Macchiato yellow。
/// Catppuccin Macchiato yellow。
pub const YELLOW: Color = Color::Rgb(238, 212, 159);

/// Catppuccin Macchiato peach。
/// Catppuccin Macchiato peach。
pub const PEACH: Color = Color::Rgb(245, 169, 127);

/// Catppuccin Macchiato maroon。
/// Catppuccin Macchiato maroon。
pub const MAROON: Color = Color::Rgb(238, 153, 160);

/// Catppuccin Macchiato red。
/// Catppuccin Macchiato red。
pub const RED: Color = Color::Rgb(237, 135, 150);

/// Catppuccin Macchiato mauve。
/// Catppuccin Macchiato mauve。
pub const MAUVE: Color = Color::Rgb(198, 160, 246);

/// Catppuccin Macchiato pink。
/// Catppuccin Macchiato pink。
pub const PINK: Color = Color::Rgb(245, 189, 230);

/// Catppuccin Macchiato rosewater。
/// Catppuccin Macchiato rosewater。
pub const ROSEWATER: Color = Color::Rgb(244, 219, 214);

use crate::tui::render::theme;
use once_cell::sync::Lazy;
use std::str::FromStr;
use syntect::highlighting::{
    Color as SyntectColor, FontStyle, StyleModifier, Theme as SyntectTheme, ThemeItem,
    ThemeSettings,
};

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

pub(crate) static THEME: Lazy<SyntectTheme> = Lazy::new(catppuccin_macchiato_theme);

pub(crate) fn catppuccin_macchiato_theme() -> SyntectTheme {
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
