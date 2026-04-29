/// 最大保留行数
pub const MAX_LINES: usize = 10000;

/// 默认终端宽度
pub const DEFAULT_WIDTH: usize = 120;

/// 工具调用详情行的缩进
pub const INDENT: &str = "  ";

/// 带样式信息的输出行
#[derive(Clone, Debug, Default)]
pub struct OutputLine {
    pub content: String,
    pub style: LineStyle,
    /// 关联到特定 tool_use 块的标识符
    pub tool_id: Option<String>,
}

#[derive(Clone, Copy, Debug, Default)]
pub enum LineStyle {
    #[default]
    Normal,
    User,
    Assistant,
    ToolCallRunning,
    ToolCallSuccess,
    ToolCallError,
    Error,
    System,
    Thinking,
    DiffAdd,
    DiffRemove,
}

impl LineStyle {
    pub fn to_style(&self) -> ratatui::style::Style {
        use ratatui::style::{Color, Modifier, Style};
        match self {
            LineStyle::Normal => Style::default(),
            LineStyle::User => Style::default().fg(Color::Cyan),
            LineStyle::Assistant => Style::default().fg(Color::Green),
            LineStyle::ToolCallRunning => Style::default().fg(Color::Green),
            LineStyle::ToolCallSuccess => Style::default().fg(Color::Green),
            LineStyle::ToolCallError => Style::default().fg(Color::Red),
            LineStyle::Error => Style::default().fg(Color::Red),
            LineStyle::System => Style::default().fg(Color::DarkGray),
            LineStyle::Thinking => Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC),
            LineStyle::DiffAdd => {
                let (r, g, b) = crate::render::theme::Theme::DIFF_ADD_BG_RGB;
                let (fr, fg, fb) = crate::render::theme::Theme::DIFF_ADD_FG_RGB;
                Style::default().bg(Color::Rgb(r, g, b)).fg(Color::Rgb(fr, fg, fb))
            }
            LineStyle::DiffRemove => {
                let (r, g, b) = crate::render::theme::Theme::DIFF_REMOVE_BG_RGB;
                let (fr, fg, fb) = crate::render::theme::Theme::DIFF_REMOVE_FG_RGB;
                Style::default().bg(Color::Rgb(r, g, b)).fg(Color::Rgb(fr, fg, fb))
            }
        }
    }
}

pub struct SpinnerState {
    /// 动画帧计数器
    pub frame: u64,
    /// 当前动词文本
    pub verb: String,
    /// spinner 启动时间
    pub start: std::time::Instant,
}
