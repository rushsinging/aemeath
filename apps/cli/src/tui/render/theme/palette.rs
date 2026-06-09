//! TUI 默认主题色。
//!
//! 采用 Catppuccin Macchiato 风格的暗色主题语义色，避免各组件直接散落硬编码颜色。

use ratatui::style::Color;

/// Catppuccin Macchiato rosewater。
pub const ROSEWATER: Color = Color::Rgb(244, 219, 214);
/// Catppuccin Macchiato flamingo。
pub const FLAMINGO: Color = Color::Rgb(240, 198, 198);
/// Catppuccin Macchiato pink。
pub const PINK: Color = Color::Rgb(245, 189, 230);
/// Catppuccin Macchiato mauve。
pub const MAUVE: Color = Color::Rgb(198, 160, 246);
/// Catppuccin Macchiato red。
pub const RED: Color = Color::Rgb(237, 135, 150);
/// Catppuccin Macchiato maroon。
pub const MAROON: Color = Color::Rgb(238, 153, 160);
/// Catppuccin Macchiato peach。
pub const PEACH: Color = Color::Rgb(245, 169, 127);
/// Catppuccin Macchiato yellow。
pub const YELLOW: Color = Color::Rgb(238, 212, 159);
/// Catppuccin Macchiato green。
pub const GREEN: Color = Color::Rgb(166, 218, 149);
/// Catppuccin Macchiato teal。
pub const TEAL: Color = Color::Rgb(139, 213, 202);
/// Catppuccin Macchiato sky。
pub const SKY: Color = Color::Rgb(145, 215, 227);
/// Catppuccin Macchiato sapphire。
pub const SAPPHIRE: Color = Color::Rgb(125, 196, 228);
/// Catppuccin Macchiato blue。
pub const BLUE: Color = Color::Rgb(138, 173, 244);
/// Catppuccin Macchiato lavender。
pub const LAVENDER: Color = Color::Rgb(183, 189, 248);
/// Catppuccin Macchiato text。
pub const MACCHIATO_TEXT: Color = Color::Rgb(202, 211, 245);
/// Catppuccin Macchiato subtext1。
pub const SUBTEXT1: Color = Color::Rgb(184, 192, 224);
/// Catppuccin Macchiato subtext0。
pub const SUBTEXT0: Color = Color::Rgb(165, 173, 203);
/// Catppuccin Macchiato overlay2。
pub const OVERLAY2: Color = Color::Rgb(147, 154, 183);
/// Catppuccin Macchiato overlay1。
pub const OVERLAY1: Color = Color::Rgb(128, 135, 162);
/// Catppuccin Macchiato overlay0。
pub const OVERLAY0: Color = Color::Rgb(110, 115, 141);
/// Catppuccin Macchiato surface2。
pub const SURFACE2: Color = Color::Rgb(91, 96, 120);
/// Catppuccin Macchiato surface1。
pub const SURFACE1: Color = Color::Rgb(73, 77, 100);
/// Catppuccin Macchiato surface0。
pub const SURFACE0: Color = Color::Rgb(54, 58, 79);
/// Catppuccin Macchiato base。
pub const BASE: Color = Color::Rgb(36, 39, 58);
/// Catppuccin Macchiato mantle。
pub const MANTLE: Color = Color::Rgb(30, 32, 48);
/// Catppuccin Macchiato crust。
pub const CRUST: Color = Color::Rgb(24, 25, 38);

/// 主文本色。
pub const TEXT: Color = MACCHIATO_TEXT;
/// 次级文本色。
pub const TEXT_MUTED: Color = SUBTEXT0;
/// 弱化文本色。
pub const TEXT_DIM: Color = OVERLAY0;
/// 面板边框色。
pub const BORDER: Color = SURFACE1;
/// 聚焦边框与品牌强调色。
pub const ACCENT: Color = BLUE;
/// 强调色高亮。
pub const ACCENT_BRIGHT: Color = MAUVE;
/// 深色背景。
pub const SURFACE: Color = BASE;
/// 状态栏背景。
pub const STATUS_BG: Color = SURFACE;
/// 浮层背景。
pub const SURFACE_ELEVATED: Color = SURFACE0;
/// 选中背景。
pub const SELECTION_BG: Color = SURFACE1;
/// 选中前景。
pub const SELECTION_FG: Color = TEXT;

/// 用户消息色。
pub const USER: Color = Color::Rgb(22, 50, 79);
/// 用户消息背景色。
pub const USER_BG: Color = Color::Rgb(183, 216, 255);
/// 助手消息色。
pub const ASSISTANT: Color = TEXT;
/// 工具运行中色。
pub const TOOL_RUNNING: Color = PEACH;
/// 成功色。
pub const SUCCESS: Color = GREEN;
/// 警告色。
pub const WARNING: Color = YELLOW;
/// 错误色。
pub const ERROR: Color = RED;
/// Thinking 文本色。
pub const THINKING: Color = OVERLAY1;
/// Markdown 链接色。
pub const LINK: Color = BLUE;
/// 行内代码与代码块强调色。
pub const CODE: Color = TOOL_RUNNING;
/// Spinner 基础色。
pub const SPINNER_BASE: Color = TEAL;
/// Spinner 高亮色。
pub const SPINNER_HIGHLIGHT: Color = GREEN;
/// Spinner 弱化色。
pub const SPINNER_DIM: Color = SURFACE2;

/// Diff 新增行背景色。
pub const DIFF_ADD_BG: Color = Color::Rgb(0, 40, 10);
/// Diff 新增行前景色。
pub const DIFF_ADD_FG: Color = Color::Rgb(56, 166, 96);
/// Diff 删除行背景色。
pub const DIFF_REMOVE_BG: Color = Color::Rgb(60, 20, 30);
/// Diff 删除行前景色。
pub const DIFF_REMOVE_FG: Color = Color::Rgb(220, 100, 110);
