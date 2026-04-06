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
    /// Token usage
    input_tokens: u64,
    output_tokens: u64,
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
            session_id: None,
            api_calls: 0,
            is_processing: false,
            processing_msg: String::new(),
            model: None,
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

    /// Update token usage
    pub fn set_tokens(&mut self, input: u64, output: u64) {
        self.input_tokens = input;
        self.output_tokens = output;
    }

    /// Set session ID
    pub fn set_session_id(&mut self, id: &str) {
        self.session_id = Some(id.to_string());
    }

    /// Set model name
    pub fn set_model(&mut self, model: &str) {
        self.model = Some(model.to_string());
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
            let spinner = Self::get_spinner_char();
            spans.push(Span::styled(
                format!(" {} {} ", spinner, self.processing_msg),
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
            ));
        } else {
            let status_style = match self.status_type {
                StatusType::Normal => Style::default().fg(Color::White),
                StatusType::Success => Style::default().fg(Color::Green),
                StatusType::Warning => Style::default().fg(Color::Yellow),
                // Error uses Warning style
                StatusType::Processing => Style::default().fg(Color::Yellow),
            };
            spans.push(Span::styled(
                format!(" {} ", self.status),
                status_style,
            ));
        }

        // Token usage
        let token_text = format!("Tokens: {} in / {} out", format_tokens(self.input_tokens), format_tokens(self.output_tokens));
        spans.push(Span::styled(
            format!(" {} │", token_text),
            Style::default().fg(Color::DarkGray),
        ));

        // Session info
        if let Some(ref id) = self.session_id {
            let short_id: String = id.chars().take(8).collect();
            spans.push(Span::styled(
                format!(" Session: {} │ Calls: {} ", short_id, self.api_calls),
                Style::default().fg(Color::DarkGray),
            ));
        }

        // Key hints
        spans.push(Span::styled(
            " Ctrl+C: interrupt | /help: commands ",
            Style::default().fg(Color::DarkGray),
        ));

        let line = Line::from(spans);
        let paragraph = Paragraph::new(line)
            .style(Style::default().bg(Color::Black));

        paragraph.render(area, buf);
    }

    /// Get a simple spinner character
    fn get_spinner_char() -> char {
        use std::time::{SystemTime, UNIX_EPOCH};
        let ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        match (ms / 100) % 4 {
            0 => '⠋',
            1 => '⠙',
            2 => '⠹',
            3 => '⠸',
            _ => '⠋',
        }
    }
}