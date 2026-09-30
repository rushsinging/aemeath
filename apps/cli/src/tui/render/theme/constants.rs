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
