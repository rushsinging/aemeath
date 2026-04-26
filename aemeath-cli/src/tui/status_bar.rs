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
      /// Tokens per second from last API call
      tps: f64,
      /// 选中状态
      is_selecting: bool,
      selection_start: Option<usize>,
      selection_end: Option<usize>,
      /// Thinking/reasoning mode
      thinking: bool,
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
            tps: 0.0,
            is_selecting: false,
            selection_start: None,
            selection_end: None,
            thinking: true,
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
        log::debug!("[STATUS] set_processing(\"{msg}\") was=\"{}\"", self.processing_msg);
        self.is_processing = true;
        self.processing_msg = msg.to_string();
        self.status_type = StatusType::Processing;
    }

    /// Clear processing status
    pub fn clear_processing(&mut self) {
        log::debug!("[STATUS] clear_processing() was=\"{}\"", self.processing_msg);
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

    /// Set tokens per second from last API call
    pub fn set_tps(&mut self, tps: f64) {
        self.tps = tps;
    }

    /// Set thinking/reasoning mode
    pub fn set_thinking(&mut self, enabled: bool) {
        self.thinking = enabled;
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

        // Thinking mode indicator
        {
            let label = if self.thinking { "ON" } else { "OFF" };
            let color = if self.thinking { Color::Green } else { Color::DarkGray };
            spans.push(Span::styled(
                format!(" Think:{} │", label),
                Style::default().fg(color),
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

        // Token usage: in/out + t/s + context window usage
        {
            let in_out = format!("In: {} / Out: {}", format_tokens(self.input_tokens), format_tokens(self.output_tokens));
            spans.push(Span::styled(
                format!(" {} ", in_out),
                Style::default().fg(Color::Gray),
            ));

            // t/s display
            if self.tps > 0.0 {
                spans.push(Span::styled(
                    format!(" {:.0} t/s │", self.tps),
                    Style::default().fg(Color::DarkGray),
                ));
            }

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
  
        // 如果有选中，替换为带高亮的 spans
        if let (Some(start), Some(end)) = (self.selection_start, self.selection_end) {
            let (start, end) = if start < end { (start, end) } else { (end, start) };
            if start < end {
                let full_text: String = line.spans.iter().map(|s| s.content.clone()).collect();
                let chars: Vec<char> = full_text.chars().collect();
                let len = chars.len();
                let sel_start = start.min(len);
                let sel_end = end.min(len);
  
                let before: String = chars[..sel_start].iter().collect();
                let selected: String = chars[sel_start..sel_end].iter().collect();
                let after: String = chars[sel_end..].iter().collect();
  
                let selection_style = Style::default().bg(Color::Blue).fg(Color::White);
                let base = Style::default().bg(Color::Black);
  
                let mut highlighted = Vec::new();
                if !before.is_empty() {
                    highlighted.push(Span::styled(before, base));
                }
                if !selected.is_empty() {
                    highlighted.push(Span::styled(selected, selection_style));
                }
                if !after.is_empty() {
                    highlighted.push(Span::styled(after, base));
                }
                let paragraph = Paragraph::new(Line::from(highlighted))
                    .style(Style::default().bg(Color::Black));
                paragraph.render(area, buf);
                return;
            }
        }
  
        let paragraph = Paragraph::new(line)
            .style(Style::default().bg(Color::Black));
  
        paragraph.render(area, buf);
    }
  
    /// 构建状态栏的完整文本（用于选中复制）
    fn build_full_text(&self) -> String {
        let mut parts = Vec::new();
        if let Some(ref model) = self.model {
            parts.push(format!(" {} ", model));
            parts.push("│".to_string());
        }
        {
            let label = if self.thinking { "ON" } else { "OFF" };
            parts.push(format!(" Think:{} │", label));
        }
        if self.is_processing {
            parts.push(format!(" {} ", self.processing_msg));
        } else {
            parts.push(format!(" {} ", self.status));
        }
        {
            let in_out = format!("In: {} / Out: {}",
                aemeath_core::cost::format_tokens(self.input_tokens),
                aemeath_core::cost::format_tokens(self.output_tokens));
            parts.push(format!(" {} ", in_out));
            if self.tps > 0.0 {
                parts.push(format!(" {:.0} t/s │", self.tps));
            }
            if self.context_size > 0 {
                let pct = if self.last_input_tokens > 0 {
                    self.last_input_tokens * 100 / self.context_size
                } else { 0 };
                parts.push(format!("Ctx: {}% │", pct));
            } else {
                parts.push("│".to_string());
            }
        }
        if let Some(ref id) = self.session_id {
            parts.push(format!(" Session: {} │ Calls: {} ", id, self.api_calls));
        }
        parts.join("")
    }
  
    /// 开始选中
    pub fn start_selection(&mut self, col: u16) {
        self.selection_start = Some(col as usize);
        self.selection_end = Some(col as usize);
        self.is_selecting = true;
    }
  
    /// 更新选中位置
    pub fn update_selection(&mut self, col: u16) {
        if self.is_selecting {
            self.selection_end = Some(col as usize);
        }
    }
  
    /// 结束选中并复制到剪贴板
    pub fn end_selection(&mut self) -> Option<String> {
        self.is_selecting = false;
        let text = self.get_selected_text();
        if let Some(ref t) = text {
            self.copy_to_clipboard(t);
        }
        self.selection_start = None;
        self.selection_end = None;
        text
    }
  
    /// 获取选中的文本
    pub fn get_selected_text(&self) -> Option<String> {
        let start = self.selection_start?;
        let end = self.selection_end?;
        let (start, end) = if start < end { (start, end) } else { (end, start) };
        if start == end {
            return None;
        }
        let full = self.build_full_text();
        let chars: Vec<char> = full.chars().collect();
        let len = chars.len();
        let selected: String = chars[start.min(len)..end.min(len)].iter().collect();
        if selected.is_empty() { None } else { Some(selected) }
    }
  
    /// 复制到剪贴板
    fn copy_to_clipboard(&self, text: &str) {
        use std::io::Write;
        if let Ok(mut child) = std::process::Command::new("pbcopy")
            .stdin(std::process::Stdio::piped()).spawn()
        {
            if let Some(mut stdin) = child.stdin.take() {
                let _ = stdin.write_all(text.as_bytes());
            }
            let _ = child.wait();
        }
    }
  
    /// 清除选中
    pub fn clear_selection(&mut self) {
        self.selection_start = None;
        self.selection_end = None;
        self.is_selecting = false;
    }
  
    /// 是否正在选中
    pub fn is_selecting(&self) -> bool {
        self.is_selecting
    }
}