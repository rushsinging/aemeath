//! TUI 默认主题色。
//!
//! 采用低饱和、现代 IDE 风格的暗色主题语义色，避免各组件直接散落硬编码颜色。

use ratatui::style::Color;

/// 主文本色。
pub const TEXT: Color = Color::Rgb(219, 222, 229);
/// 次级文本色。
pub const TEXT_MUTED: Color = Color::Rgb(142, 150, 163);
/// 弱化文本色。
pub const TEXT_DIM: Color = Color::Rgb(95, 103, 117);
/// 面板边框色。
pub const BORDER: Color = Color::Rgb(78, 86, 101);
/// 聚焦边框与品牌强调色。
pub const ACCENT: Color = Color::Rgb(125, 180, 255);
/// 强调色高亮。
pub const ACCENT_BRIGHT: Color = Color::Rgb(169, 203, 255);
/// 深色背景。
pub const SURFACE: Color = Color::Rgb(20, 23, 31);
/// 状态栏背景。
pub const STATUS_BG: Color = Color::Rgb(35, 43, 58);
/// 浮层背景。
pub const SURFACE_ELEVATED: Color = Color::Rgb(30, 35, 46);
/// 选中背景。
pub const SELECTION_BG: Color = Color::Rgb(46, 91, 143);
/// 选中前景。
pub const SELECTION_FG: Color = Color::Rgb(238, 244, 255);

/// 用户消息色。
pub const USER: Color = Color::Rgb(125, 180, 255);
/// 助手消息色。
pub const ASSISTANT: Color = Color::Rgb(126, 211, 169);
/// 工具运行中色。
pub const TOOL_RUNNING: Color = Color::Rgb(214, 171, 107);
/// 成功色。
pub const SUCCESS: Color = Color::Rgb(126, 211, 169);
/// 警告色。
pub const WARNING: Color = Color::Rgb(226, 185, 104);
/// 错误色。
pub const ERROR: Color = Color::Rgb(238, 121, 133);
/// Thinking 文本色。
pub const THINKING: Color = Color::Rgb(130, 139, 153);
/// Markdown 链接色。
pub const LINK: Color = Color::Rgb(125, 180, 255);
/// 行内代码与代码块背景。
pub const CODE_BG: Color = Color::Rgb(32, 36, 48);
/// 行内代码与代码块前景。
pub const CODE_FG: Color = Color::Rgb(220, 214, 202);
/// Spinner 基础色。
pub const SPINNER_BASE: Color = Color::Rgb(126, 211, 169);
/// Spinner 高亮色。
pub const SPINNER_HIGHLIGHT: Color = Color::Rgb(183, 233, 206);
/// Spinner 弱化色。
pub const SPINNER_DIM: Color = Color::Rgb(67, 118, 101);
