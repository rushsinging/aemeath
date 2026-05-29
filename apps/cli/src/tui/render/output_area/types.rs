/// 最大保留行数
pub const MAX_LINES: usize = 10000;

/// 默认终端宽度
pub const DEFAULT_WIDTH: usize = 120;

/// 工具调用详情行的缩进
pub const INDENT: &str = "  ";

use crate::tui::render::output::markdown;
use crate::tui::render::output::rendered::RenderedLine;
use crate::tui::render::theme;
use ratatui::{style::Color, text::Span};

/// 带颜色的一段文本，用于行内分段着色（如 diff 语法高亮）
#[derive(Clone, Debug)]
pub struct SpanPart {
    pub text: String,
    pub color: Color,
}

impl SpanPart {
    #[allow(dead_code)]
    pub fn plain(text: impl Into<String>, color: Color) -> Self {
        Self {
            text: text.into(),
            color,
        }
    }
}

/// 带样式信息的输出行
#[derive(Clone, Debug, Default)]
pub struct OutputLine {
    pub content: String,
    pub style: LineStyle,
    /// 关联到特定 tool_use 块的标识符
    pub tool_id: Option<String>,
    /// 行内分段着色：当 Some 时渲染层使用此字段替代 content + style
    pub spans: Option<Vec<SpanPart>>,
}

impl OutputLine {
    pub fn as_rendered_line(&self, width: usize) -> RenderedLine {
        if let Some(parts) = &self.spans {
            let spans = parts
                .iter()
                .map(|part| Span::styled(part.text.clone(), self.style.to_style().fg(part.color)))
                .collect();
            return RenderedLine::new(spans);
        }

        match self.style {
            LineStyle::Assistant => {
                markdown::inline_markdown_lines(&self.content, self.style.to_style(), width.max(1))
                    .into_iter()
                    .next()
                    .map(|line| {
                        let visible = line
                            .spans
                            .iter()
                            .map(|span| span.content.as_ref())
                            .collect::<String>();
                        RenderedLine::with_plain(
                            line.spans,
                            markdown::strip_inline_formatting(&visible),
                        )
                    })
                    .unwrap_or_default()
            }
            _ => RenderedLine::new(vec![Span::styled(
                self.content.clone(),
                self.style.to_style(),
            )]),
        }
    }
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
    /// AskUserQuestion 醒目样式：亮黄 + 粗体
    AskUser,
}

impl LineStyle {
    pub fn to_style(self) -> ratatui::style::Style {
        use ratatui::style::{Modifier, Style};

        match self {
            LineStyle::Normal => Style::default().fg(theme::TEXT),
            LineStyle::User => Style::default().fg(theme::USER),
            LineStyle::Assistant => Style::default().fg(theme::ASSISTANT),
            LineStyle::ToolCallRunning => Style::default().fg(theme::TOOL_RUNNING),
            LineStyle::ToolCallSuccess => Style::default().fg(theme::SUCCESS),
            LineStyle::ToolCallError => Style::default().fg(theme::ERROR),
            LineStyle::Error => Style::default().fg(theme::ERROR),
            LineStyle::System => Style::default().fg(theme::TEXT_DIM),
            LineStyle::Thinking => Style::default()
                .fg(theme::THINKING)
                .add_modifier(Modifier::ITALIC),
            LineStyle::DiffAdd => Style::default()
                .bg(theme::DIFF_ADD_BG)
                .fg(theme::DIFF_ADD_FG),
            LineStyle::DiffRemove => Style::default()
                .bg(theme::DIFF_REMOVE_BG)
                .fg(theme::DIFF_REMOVE_FG),
            LineStyle::AskUser => Style::default()
                .fg(theme::WARNING)
                .add_modifier(Modifier::BOLD),
        }
    }
}

pub struct SpinnerState {
    /// 动画帧计数器，只能由固定 ticker 推进
    pub frame: u64,
    /// 当前动词文本
    pub verb: String,
    /// spinner 启动时间
    pub start: std::time::Instant,
    /// 当前细分阶段，显示在 spinner 行括号中
    pub phase: Option<String>,
}
