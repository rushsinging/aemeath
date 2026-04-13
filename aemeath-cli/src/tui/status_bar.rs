use aemeath_core::cost::format_tokens;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Paragraph, Widget},
};

/// The status bar at the bottom of the screen
pub struct StatusBar {
    /// Current status message
    status: String,
    /// Status type for coloring
    status_type: StatusType,
    /// Cumulative token usage across all API calls
    input_tokens: u64,
    output_tokens: u64,
    /// Last API call's input_tokens (= current context window usage)
    last_input_tokens: u64,
    /// Session ID
    session_id: Option<String>,
    /// LLM API call count
    api_calls: u64,
    /// Is processing
    is_processing: bool,
    /// Processing message
    processing_msg: String,
    /// Current model name
    model: Option<String>,
    /// Context window size
    context_size: u64,
}

#[derive(Clone, Copy, Debug, Default)]
pub enum StatusType {
    #[default]
    Normal,
    Success,
    Warning,
    Processing,
}

impl Default for StatusBar {
    fn default() -> Self {
        Self::new()
    }
}

impl StatusBar {
    pub fn new() -> Self {
        Self {
            status: "Ready".to_string(),
            status_type: StatusType::Normal,
            input_tokens: 0,
            output_tokens: 0,
            last_input_tokens: 0,
            session_id: None,
            api_calls: 0,
            is_processing: false,
            processing_msg: String::new(),
            model: None,
            context_size: 0,
        }
    }

    /// Set success status
    pub fn set_success(&mut self, status: &str) {
        self.status = status.to_string();
        self.status_type = StatusType::Success;
    }

    /// Set warning status
    pub fn set_warning(&mut self, status: &str) {
        self.status = status.to_string();
        self.status_type = StatusType::Warning;
    }

    /// Set processing status
    pub fn set_processing(&mut self, msg: &str) {
        self.is_processing = true;
        self.processing_msg = msg.to_string();
        self.status_type = StatusType::Processing;
    }

    /// Clear processing status
    pub fn clear_processing(&mut self) {
        self.is_processing = false;
        self.processing_msg.clear();
        self.status_type = StatusType::Normal;
    }

    /// Update token usage (cumulative totals + last call's input for Ctx%)
    pub fn set_tokens(&mut self, input: u64, output: u64, last_input: u64) {
        self.input_tokens = input;
        self.output_tokens = output;
        self.last_input_tokens = last_input;
    }

    /// Set session ID
    pub fn set_session_id(&mut self, id: &str) {
        self.session_id = Some(id.to_string());
    }

    /// Set model name
    pub fn set_model(&mut self, model: &str) {
        self.model = Some(model.to_string());
    }

    /// Set context window size
    pub fn set_context_size(&mut self, size: u64) {
        self.context_size = size;
    }

    /// Set message count
    pub fn set_api_calls(&mut self, count: u64) {
        self.api_calls = count;
    }

    /// Check if processing
    pub fn is_processing(&self) -> bool {
        self.is_processing
    }

    /// Render the status bar
    pub fn render(&self, area: Rect, buf: &mut Buffer) {
        if area.height == 0 {
            return;
        }

        let mut spans = Vec::new();

        // Left side: model name
        if let Some(ref model) = self.model {
            spans.push(Span::styled(
                format!(" {} ", model),
                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
            ));
            spans.push(Span::styled(
                "│",
                Style::default().fg(Color::DarkGray),
            ));
        }

        // Processing status or regular status
        if self.is_processing {
            spans.push(Span::styled(
                format!(" {} ", self.processing_msg),
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
            ));
        } else {
            let status_style = match self.status_type {
                StatusType::Normal => Style::default().fg(Color::White),
                StatusType::Success => Style::default().fg(Color::Green),
                StatusType::Warning => Style::default().fg(Color::Yellow),
                StatusType::Processing => Style::default().fg(Color::Yellow),
            };
            spans.push(Span::styled(
                format!(" {} ", self.status),
                status_style,
            ));
        }

        // Token usage: in/out + context window usage
        {
            let in_out = format!("In: {} / Out: {}", format_tokens(self.input_tokens), format_tokens(self.output_tokens));
            spans.push(Span::styled(
                format!(" {} ", in_out),
                Style::default().fg(Color::Gray),
            ));

            if self.context_size > 0 {
                let pct = if self.last_input_tokens > 0 {
                    self.last_input_tokens * 100 / self.context_size
                } else {
                    0
                };
                let pct_color = if pct >= 80 {
                    Color::Red
                } else if pct >= 50 {
                    Color::Yellow
                } else {
                    Color::Gray
                };
                spans.push(Span::styled(
                    format!("Ctx: {}% │", pct),
                    Style::default().fg(pct_color),
                ));
            } else {
                spans.push(Span::styled("│", Style::default().fg(Color::Gray)));
            }
        }

        // Session info
        if let Some(ref id) = self.session_id {
            spans.push(Span::styled(
                format!(" Session: {} │ Calls: {} ", id, self.api_calls),
                Style::default().fg(Color::Gray),
            ));
        }

        let line = Line::from(spans);
        let paragraph = Paragraph::new(line)
            .style(Style::default().bg(Color::Black));

        paragraph.render(area, buf);
    }
}