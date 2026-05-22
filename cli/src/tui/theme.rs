//! TUI 默认主题色。
//!
//! 采用 Catppuccin Macchiato 风格的暗色主题语义色，避免各组件直接散落硬编码颜色。

use ratatui::style::Color;

/// 主文本色。
pub const TEXT: Color = Color::Rgb(202, 211, 245);
/// 次级文本色。
pub const TEXT_MUTED: Color = Color::Rgb(165, 173, 203);
/// 弱化文本色。
pub const TEXT_DIM: Color = Color::Rgb(110, 115, 141);
/// 面板边框色。
pub const BORDER: Color = Color::Rgb(73, 77, 100);
/// 聚焦边框与品牌强调色。
pub const ACCENT: Color = Color::Rgb(138, 173, 244);
/// 强调色高亮。
pub const ACCENT_BRIGHT: Color = Color::Rgb(198, 160, 246);
/// 深色背景。
pub const SURFACE: Color = Color::Rgb(36, 39, 58);
/// 状态栏背景。
pub const STATUS_BG: Color = SURFACE;
/// 浮层背景。
pub const SURFACE_ELEVATED: Color = Color::Rgb(54, 58, 79);
/// 选中背景。
pub const SELECTION_BG: Color = Color::Rgb(73, 77, 100);
/// 选中前景。
pub const SELECTION_FG: Color = TEXT;

/// 用户消息色。
pub const USER: Color = Color::Rgb(138, 173, 244);
/// 助手消息色。
pub const ASSISTANT: Color = TEXT;
/// 工具运行中色。
pub const TOOL_RUNNING: Color = Color::Rgb(245, 169, 127);
/// 成功色。
pub const SUCCESS: Color = Color::Rgb(166, 218, 149);
/// 警告色。
pub const WARNING: Color = Color::Rgb(238, 212, 159);
/// 错误色。
pub const ERROR: Color = Color::Rgb(237, 135, 150);
/// Thinking 文本色。
pub const THINKING: Color = Color::Rgb(128, 135, 162);
/// Markdown 链接色。
pub const LINK: Color = Color::Rgb(138, 173, 244);
/// 行内代码与代码块强调色。
pub const CODE: Color = Color::Rgb(184, 192, 224);
/// Spinner 基础色。
pub const SPINNER_BASE: Color = Color::Rgb(139, 213, 202);
/// Spinner 高亮色。
pub const SPINNER_HIGHLIGHT: Color = Color::Rgb(166, 218, 149);
/// Spinner 弱化色。
pub const SPINNER_DIM: Color = Color::Rgb(91, 96, 120);
