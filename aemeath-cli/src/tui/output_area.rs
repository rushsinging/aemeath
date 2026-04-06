use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, StatefulWidget, Widget},
};
use std::collections::VecDeque;
use unicode_width::UnicodeWidthChar;

/// Maximum lines to keep in the scrollback buffer
const MAX_LINES: usize = 10000;

/// Default terminal width for pre-wrapping
const DEFAULT_WIDTH: usize = 120;

/// A line in the output with styling information
#[derive(Clone, Debug)]
pub struct OutputLine {
    pub content: String,
    pub style: LineStyle,
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
    ToolResult,
    Error,
    System,
    Thinking,
    DiffAdd,
    DiffRemove,
}

impl LineStyle {
    pub fn to_style(&self) -> Style {
        match self {
            LineStyle::Normal => Style::default(),
            LineStyle::User => Style::default().fg(Color::Cyan),
            LineStyle::Assistant => Style::default().fg(Color::Green),
            LineStyle::ToolCallRunning => Style::default().fg(Color::Yellow),
            LineStyle::ToolCallSuccess => Style::default().fg(Color::Green),
            LineStyle::ToolCallError => Style::default().fg(Color::Red),
            LineStyle::ToolResult => Style::default().fg(Color::Blue),
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

/// Format a tool call for human-friendly display.
/// Returns (header_line, detail_lines) where header is "● ToolName(target)" and details are indented.
fn format_tool_call(name: &str, raw_json: &str) -> (String, Vec<String>) {
    // Try parsing the JSON input
    let parsed: Result<serde_json::Value, _> = serde_json::from_str(raw_json);

    match name {
        "Bash" => {
            if let Ok(v) = &parsed {
                let cmd = v.get("command").and_then(|c| c.as_str()).unwrap_or("?");
                let timeout = v.get("timeout").and_then(|t| t.as_u64());
                let mut detail = format!("$ {cmd}");
                if let Some(t) = timeout {
                    if t != 120_000 {
                        detail.push_str(&format!("  (timeout: {}s)", t / 1000));
                    }
                }
                return (format!("● Bash"), vec![detail]);
            }
        }
        "Read" => {
            if let Ok(v) = &parsed {
                let path = v.get("file_path").and_then(|p| p.as_str()).unwrap_or("?");
                let offset = v.get("offset").and_then(|o| o.as_u64());
                let limit = v.get("limit").and_then(|l| l.as_u64());
                let mut detail = format!("Read {path}");
                if let Some(o) = offset {
                    detail.push_str(&format!(" (offset: {o}"));
                    if let Some(l) = limit {
                        detail.push_str(&format!(", limit: {l}"));
                    }
                    detail.push(')');
                }
                return (format!("● Read({path})"), vec![detail]);
            }
        }
        "Write" => {
            if let Ok(v) = &parsed {
                let path = v.get("file_path").and_then(|p| p.as_str()).unwrap_or("?");
                let content = v.get("content").and_then(|c| c.as_str()).unwrap_or("");
                return (format!("● Write({path})"), vec![format!("{} bytes", content.len())]);
            }
        }
        "Edit" => {
            if let Ok(v) = &parsed {
                let path = v.get("file_path").and_then(|p| p.as_str()).unwrap_or("?");
                let old = v.get("old_string").and_then(|s| s.as_str()).unwrap_or("");
                let new = v.get("new_string").and_then(|s| s.as_str()).unwrap_or("");
                let old_lines = old.lines().count();
                let new_lines = new.lines().count();
                let detail = if old_lines == new_lines {
                    format!("Changed {} -> {} chars", old.len(), new.len())
                } else if new_lines > old_lines {
                    format!("Added {} line(s), {} -> {} chars", new_lines - old_lines, old.len(), new.len())
                } else {
                    format!("Removed {} line(s), {} -> {} chars", old_lines - new_lines, old.len(), new.len())
                };
                return (
                    format!("● Edit({path})"),
                    vec![detail],
                );
            }
        }
        "Glob" => {
            if let Ok(v) = &parsed {
                let pattern = v.get("pattern").and_then(|p| p.as_str()).unwrap_or("?");
                return (format!("● Glob({pattern})"), vec![]);
            }
        }
        "Grep" => {
            if let Ok(v) = &parsed {
                let pattern = v.get("pattern").and_then(|p| p.as_str()).unwrap_or("?");
                let path = v.get("path").and_then(|p| p.as_str()).unwrap_or(".");
                return (format!("● Grep /{pattern}/"), vec![format!("in {path}")]);
            }
        }
        "Agent" => {
            if let Ok(v) = &parsed {
                let desc = v.get("description").and_then(|d| d.as_str()).unwrap_or("sub-task");
                let prompt = v.get("prompt").and_then(|p| p.as_str()).unwrap_or("");
                let preview = if prompt.len() > 80 {
                    format!("{}...", &prompt[..prompt.char_indices().take_while(|(i, _)| *i < 80).last().map(|(i, c)| i + c.len_utf8()).unwrap_or(80)])
                } else {
                    prompt.to_string()
                };
                return (format!("● Agent({desc})"), vec![preview]);
            }
        }
        "WebFetch" => {
            if let Ok(v) = &parsed {
                let url = v.get("url").and_then(|u| u.as_str()).unwrap_or("?");
                return (format!("● WebFetch({url})"), vec![]);
            }
        }
        "TaskCreate" => {
            if let Ok(v) = &parsed {
                let subject = v.get("subject").and_then(|s| s.as_str()).unwrap_or("?");
                return (format!("● TaskCreate({subject})"), vec![]);
            }
        }
        "TaskUpdate" => {
            if let Ok(v) = &parsed {
                let id = v.get("taskId").and_then(|s| s.as_str()).unwrap_or("?");
                let status = v.get("status").and_then(|s| s.as_str()).unwrap_or("");
                return (format!("● TaskUpdate({id})"), vec![format!("-> {status}")]);
            }
        }
        "TaskList" => {
            return (format!("● TaskList"), vec![]);
        }
        "Skill" => {
            if let Ok(v) = &parsed {
                let skill = v.get("skill").and_then(|s| s.as_str()).unwrap_or("?");
                return (format!("● Skill({skill})"), vec![]);
            }
        }
        "LSP" => {
            if let Ok(v) = &parsed {
                let op = v.get("operation").and_then(|o| o.as_str()).unwrap_or("?");
                let path = v.get("filePath").and_then(|p| p.as_str()).unwrap_or("?");
                return (format!("● LSP::{op}({path})"), vec![]);
            }
        }
        "TodoWrite" => {
            if let Ok(v) = &parsed {
                if let Some(todos) = v.get("todos").and_then(|t| t.as_array()) {
                    let count = todos.len();
                    let mut details: Vec<String> = Vec::new();
                    for todo in todos.iter().take(3) {
                        let subject = todo.get("subject").and_then(|s| s.as_str()).unwrap_or("?");
                        let status = todo.get("status").and_then(|s| s.as_str()).unwrap_or("pending");
                        let icon = match status {
                            "completed" => "✓",
                            "in_progress" => "◐",
                            _ => "○",
                        };
                        details.push(format!("{icon} {subject}"));
                    }
                    if count > 3 {
                        details.push(format!("... +{} more", count - 3));
                    }
                    return (format!("● TodoWrite ({count} items)"), details);
                }
            }
        }
        _ => {}
    }

    // Fallback: show tool name + truncated raw input
    let truncated = if raw_json.len() > 100 {
        format!("{}...", &raw_json[..raw_json.char_indices().take_while(|(i, _)| *i < 100).last().map(|(i, c)| i + c.len_utf8()).unwrap_or(100)])
    } else {
        raw_json.to_string()
    };
    (format!("● {name}"), vec![truncated])
}

/// Sanitize a string for TUI display: expand tabs, strip ANSI escapes and control characters.
fn sanitize_for_display(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();

    while let Some(ch) = chars.next() {
        match ch {
            '\t' => result.push_str("    "), // Expand tab to 4 spaces
            '\x1b' => {
                // Strip ANSI escape sequence: ESC [ ... (letter)
                if chars.peek() == Some(&'[') {
                    chars.next(); // consume '['
                    // Consume until we hit a letter (the final byte of the sequence)
                    while let Some(&c) = chars.peek() {
                        chars.next();
                        if c.is_ascii_alphabetic() {
                            break;
                        }
                    }
                }
                // Also handle ESC ( and ESC ) sequences
            }
            '\r' => {} // Strip carriage return
            c if c.is_control() => {} // Strip other control characters
            c => result.push(c),
        }
    }
    result
}

/// Split a string into lines that fit within `max_width` display columns,
/// respecting Unicode character widths (CJK = 2 columns).
fn wrap_line(text: &str, max_width: usize) -> Vec<String> {
    let text = sanitize_for_display(text);

    if max_width == 0 {
        return vec![text];
    }

    let mut result = Vec::new();
    let mut current = String::new();
    let mut current_width = 0usize;

    for ch in text.chars() {
        let ch_width = ch.width().unwrap_or(1); // Default to 1 for safety

        if current_width + ch_width > max_width {
            result.push(std::mem::take(&mut current));
            current_width = 0;
        }

        current.push(ch);
        current_width += ch_width;
    }

    if !current.is_empty() || result.is_empty() {
        result.push(current);
    }

    result
}

/// The scrollable output area that displays conversation history
pub struct OutputArea {
    lines: VecDeque<OutputLine>,
    scroll_offset: usize,
    auto_scroll: bool,
    last_line_count: usize,
    term_width: usize,
    /// Full text of the current streaming assistant block
    streaming_buffer: String,
    /// Index in `lines` where the current streaming block starts
    streaming_start: Option<usize>,
    /// Whether mouse is currently dragging (selecting)
    is_selecting: bool,
    /// Selection start in absolute line coordinates
    selection_start: Option<(usize, usize)>,
    /// Selection end in absolute line coordinates
    selection_end: Option<(usize, usize)>,
}

impl Default for OutputArea {
    fn default() -> Self {
        Self::new()
    }
}

impl OutputArea {
    pub fn new() -> Self {
        // Get terminal width, fallback to default
        let term_width = crossterm::terminal::size()
            .map(|(w, _)| w as usize)
            .unwrap_or(DEFAULT_WIDTH)
            .saturating_sub(2); // Leave room for scrollbar + margin

        Self {
            lines: VecDeque::with_capacity(MAX_LINES),
            scroll_offset: 0,
            auto_scroll: true,
            last_line_count: 0,
            term_width,
            streaming_buffer: String::new(),
            streaming_start: None,
            is_selecting: false,
            selection_start: None,
            selection_end: None,
        }
    }

    /// Add a line, pre-wrapping if it's wider than terminal
    pub fn push_line(&mut self, line: OutputLine) {
        let wrapped = wrap_line(&line.content, self.term_width);
        for chunk in wrapped {
            if self.lines.len() >= MAX_LINES {
                self.lines.pop_front();
                // When popping old lines, adjust scroll_offset to stay in place
                if self.scroll_offset > 0 {
                    self.scroll_offset = self.scroll_offset.saturating_sub(1);
                }
            }
            self.lines.push_back(OutputLine {
                content: chunk,
                style: line.style,
            });
            // When user has scrolled up, increase offset to keep view stable
            if !self.auto_scroll {
                self.scroll_offset += 1;
            }
        }
    }

    /// Add multiple lines from a string
    pub fn push_text(&mut self, text: &str, style: LineStyle) {
        for line in text.lines() {
            self.push_line(OutputLine {
                content: line.to_string(),
                style,
            });
        }
    }

    /// Add a user message
    pub fn push_user_message(&mut self, text: &str) {
        // Handle multi-line input by splitting and adding each line
        for (i, line) in text.lines().enumerate() {
            let prefix = if i == 0 { "> " } else { "  " };
            self.push_line(OutputLine {
                content: format!("{}{}", prefix, line),
                style: LineStyle::User,
            });
        }
        // If text ends with newline or is empty, add an empty line for spacing
        if text.is_empty() || text.ends_with('\n') {
            self.push_line(OutputLine {
                content: String::new(),
                style: LineStyle::User,
            });
        }
    }

    /// Add an assistant message
    #[allow(dead_code)]
    pub fn push_assistant_message(&mut self, text: &str) {
        self.push_text(text, LineStyle::Assistant);
    }

    /// Append text to the streaming assistant block.
    /// Accumulates in a buffer and re-renders all lines from the block start.
    /// Supports `<think>...</think>` tags — thinking content is displayed dimmed/italic.
    pub fn append_assistant_text(&mut self, text: &str) {
        self.streaming_buffer.push_str(text);

        // Record where the streaming block starts (first call)
        if self.streaming_start.is_none() {
            self.streaming_start = Some(self.lines.len());
        }

        let start_idx = self.streaming_start.unwrap_or(0);

        // Track line count before re-render to adjust scroll offset
        let old_line_count = self.lines.len();

        // Remove all lines from the streaming block
        while self.lines.len() > start_idx {
            self.lines.pop_back();
        }

        // Parse buffer into segments: thinking vs normal content
        let buf = &self.streaming_buffer;
        let mut pos = 0;
        let mut segments: Vec<(&str, bool)> = Vec::new(); // (text, is_thinking)

        while pos < buf.len() {
            if let Some(think_start) = buf[pos..].find("<think>") {
                let abs_start = pos + think_start;
                // Content before <think>
                if abs_start > pos {
                    segments.push((&buf[pos..abs_start], false));
                }
                let content_start = abs_start + 7; // len("<think>")
                if let Some(think_end) = buf[content_start..].find("</think>") {
                    let abs_end = content_start + think_end;
                    segments.push((&buf[content_start..abs_end], true));
                    pos = abs_end + 8; // len("</think>")
                } else {
                    // Unclosed <think> — everything after is thinking (still streaming)
                    segments.push((&buf[content_start..], true));
                    pos = buf.len();
                }
            } else {
                segments.push((&buf[pos..], false));
                pos = buf.len();
            }
        }

        // Render segments with appropriate styles
        for (segment, is_thinking) in &segments {
            let style = if *is_thinking { LineStyle::Thinking } else { LineStyle::Assistant };
            let prefix = if *is_thinking { "💭 " } else { "" };

            for text_line in segment.lines() {
                let display_line = if *is_thinking && !text_line.is_empty() {
                    format!("{prefix}{text_line}")
                } else {
                    text_line.to_string()
                };
                let wrapped = wrap_line(&display_line, self.term_width);
                for chunk in wrapped {
                    self.lines.push_back(OutputLine {
                        content: chunk,
                        style,
                    });
                }
            }
        }

        // If buffer ends with newline, add empty line for next append
        if self.streaming_buffer.ends_with('\n') {
            self.lines.push_back(OutputLine {
                content: String::new(),
                style: LineStyle::Assistant,
            });
        }

        // Adjust scroll offset to keep view stable when scrolled up.
        // Must be AFTER trailing newline line is added so we count all lines.
        if !self.auto_scroll {
            let new_line_count = self.lines.len();
            if new_line_count > old_line_count {
                self.scroll_offset += new_line_count - old_line_count;
            } else if new_line_count < old_line_count {
                self.scroll_offset = self.scroll_offset.saturating_sub(old_line_count - new_line_count);
            }
        }
    }

    /// Finish the current streaming block — reset buffer for next use
    pub fn finish_streaming(&mut self) {
        self.streaming_buffer.clear();
        self.streaming_start = None;
    }

    /// Add a tool call with human-friendly formatting.
    /// Shows header with status dot and indented details.
    pub fn push_tool_call(&mut self, name: &str, summary: &str) {
        let (header, details) = format_tool_call(name, summary);
        
        // Push header line with ToolCallRunning style (yellow dot)
        self.push_line(OutputLine {
            content: header,
            style: LineStyle::ToolCallRunning,
        });
        
        // Push detail lines with tree connectors
        for (i, detail) in details.iter().enumerate() {
            let connector = if i < details.len() - 1 { "├─" } else { "├─" };
            self.push_line(OutputLine {
                content: format!("  {connector} {detail}"),
                style: LineStyle::System,
            });
        }
    }

    /// Add a tool result with diff support.
    /// For Edit tool results containing diff information, displays with red/green backgrounds.
    pub fn push_tool_result_with_diff(&mut self, tool_name: &str, result: &str, is_error: bool) {
        if is_error {
            self.push_line(OutputLine {
                content: format!("  └─ ✗ {result}"),
                style: LineStyle::ToolCallError,
            });
            return;
        }

        // Special handling for Edit tool with diff output
        if tool_name == "Edit" && result.contains("---DIFF---\n") {
            let parts: Vec<&str> = result.splitn(3, "---DIFF---\n").collect();
            if parts.len() == 3 {
                let summary = parts[0].trim();
                let old_content = parts[1];
                let new_content = parts[2];
                
                // Show diff with line-by-line coloring
                self.push_diff_lines(old_content, new_content);

                // Show summary last with tree end
                self.push_line(OutputLine {
                    content: format!("  └─ ✓ {summary}"),
                    style: LineStyle::ToolCallSuccess,
                });
                return;
            }
        }

        // For task-related tools, skip verbose output (already shown in header)
        let is_task_tool = matches!(tool_name, "TodoWrite" | "TaskCreate" | "TaskUpdate" | "TaskList");

        if !is_task_tool && !result.trim().is_empty() {
            // Show result content with tree connectors (truncated to 5 lines)
            let total = result.lines().count();
            let display_lines: Vec<&str> = result.lines().take(5).collect();
            let has_more = total > 5;

            for line in &display_lines {
                self.push_line(OutputLine {
                    content: format!("  │  {line}"),
                    style: LineStyle::System,
                });
            }
            if has_more {
                self.push_line(OutputLine {
                    content: format!("  │  ... ({} lines omitted)", total - 5),
                    style: LineStyle::System,
                });
            }
        }

        // Show success indicator last with tree end connector
        self.push_line(OutputLine {
            content: format!("  └─ ✓ {tool_name} completed"),
            style: LineStyle::ToolCallSuccess,
        });
    }

    /// Render diff between old and new content with colored lines.
    fn push_diff_lines(&mut self, old_content: &str, new_content: &str) {
        use similar::{ChangeTag, TextDiff};
        let diff = TextDiff::from_lines(old_content, new_content);
        
        for change in diff.iter_all_changes() {
            match change.tag() {
                ChangeTag::Delete => {
                    self.push_line(OutputLine {
                        content: format!("  - {}", change),
                        style: LineStyle::DiffRemove,
                    });
                }
                ChangeTag::Insert => {
                    self.push_line(OutputLine {
                        content: format!("  + {}", change),
                        style: LineStyle::DiffAdd,
                    });
                }
                ChangeTag::Equal => {
                    self.push_line(OutputLine {
                        content: format!("    {}", change),
                        style: LineStyle::System,
                    });
                }
            }
        }
    }

    /// Add a tool result with smart truncation.
    /// Shows first and last lines when output is too long, with a collapse indicator.
    /// Also truncates very long lines to prevent horizontal overflow.
    #[allow(dead_code)]
    pub fn push_tool_result(&mut self, result: &str) {
        const MAX_DISPLAY_LINES: usize = 5;
        const KEEP_HEAD: usize = 5;
        const KEEP_TAIL: usize = 0;
        const MAX_LINE_WIDTH: usize = 500; // Truncate lines longer than this

        // First, truncate any extremely long lines
        let truncated_lines: Vec<String> = result
            .lines()
            .map(|line| {
                if line.len() > MAX_LINE_WIDTH {
                    // Find a safe truncation point (don't cut in middle of UTF-8 char)
                    let end = line
                        .char_indices()
                        .take_while(|(i, _)| *i < MAX_LINE_WIDTH)
                        .last()
                        .map(|(i, c)| i + c.len_utf8())
                        .unwrap_or(MAX_LINE_WIDTH.min(line.len()));
                    format!("{}... ({} bytes truncated)", &line[..end], line.len() - end)
                } else {
                    line.to_string()
                }
            })
            .collect();

        let total = truncated_lines.len();

        if total <= MAX_DISPLAY_LINES {
            for line in &truncated_lines {
                self.push_line(OutputLine {
                    content: line.clone(),
                    style: LineStyle::ToolResult,
                });
            }
        } else {
            // Show head
            for line in &truncated_lines[..KEEP_HEAD] {
                self.push_line(OutputLine {
                    content: line.clone(),
                    style: LineStyle::ToolResult,
                });
            }
            // Collapse indicator with visual separator
            let hidden = total - KEEP_HEAD - KEEP_TAIL;
            self.push_line(OutputLine {
                content: format!("  ┌─ {} lines hidden ({} total) ─┐", hidden, total),
                style: LineStyle::System,
            });
            self.push_line(OutputLine {
                content: "  │     Press Shift+End to scroll to bottom     │".to_string(),
                style: LineStyle::System,
            });
            self.push_line(OutputLine {
                content: "  └─────────────────────────────────────────────┘".to_string(),
                style: LineStyle::System,
            });
            // Show tail
            for line in &truncated_lines[total - KEEP_TAIL..] {
                self.push_line(OutputLine {
                    content: line.clone(),
                    style: LineStyle::ToolResult,
                });
            }
        }
    }

    /// Add an error message
    pub fn push_error(&mut self, error: &str) {
        self.push_line(OutputLine {
            content: format!("Error: {}", error),
            style: LineStyle::Error,
        });
    }

    /// Add a cancelled message
    pub fn push_cancelled(&mut self) {
        // Clear any pending streaming state
        self.finish_streaming();
        self.push_line(OutputLine {
            content: "Cancelled".to_string(),
            style: LineStyle::Error,
        });
    }

    /// Add a system message
    pub fn push_system(&mut self, msg: &str) {
        self.push_line(OutputLine {
            content: msg.to_string(),
            style: LineStyle::System,
        });
    }

    /// Scroll up by the given number of lines
    pub fn scroll_up(&mut self, amount: usize) {
        self.auto_scroll = false;
        self.scroll_offset = self.scroll_offset.saturating_add(amount);
    }

    /// Scroll down by the given number of lines
    pub fn scroll_down(&mut self, amount: usize) {
        self.scroll_offset = self.scroll_offset.saturating_sub(amount);
        if self.scroll_offset == 0 {
            self.auto_scroll = true;
        }
    }

    /// Scroll to the bottom
    pub fn scroll_to_bottom(&mut self) {
        self.scroll_offset = 0;
        self.auto_scroll = true;
    }

    /// Get the number of lines
    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    /// Start text selection at given screen position (column, row relative to visible area)
    /// visible_height is the height of the output area in rows
    pub fn start_selection_at(&mut self, col: usize, visible_row: usize, visible_height: usize) {
        // Convert visible row to absolute line index
        let (start_line_idx, _) = self.get_visible_range(visible_height);
        let absolute_line = start_line_idx + visible_row;
        
        // Clamp column to line length
        let line_len = self.lines.get(absolute_line)
            .map(|l| l.content.chars().count())
            .unwrap_or(0);
        let clamped_col = col.min(line_len);
        
        self.is_selecting = true;
        self.selection_start = Some((absolute_line, clamped_col));
        self.selection_end = Some((absolute_line, clamped_col));
    }

    /// Update selection end position during drag
    pub fn update_selection_at(&mut self, col: usize, visible_row: usize, visible_height: usize) {
        if self.is_selecting {
            // Convert visible row to absolute line index
            let (start_line_idx, _) = self.get_visible_range(visible_height);
            let absolute_line = start_line_idx + visible_row;
            
            // Clamp column to line length
            let line_len = self.lines.get(absolute_line)
                .map(|l| l.content.chars().count())
                .unwrap_or(0);
            let clamped_col = col.min(line_len);
            
            self.selection_end = Some((absolute_line, clamped_col));
        }
    }

    /// End selection and return selected text
    pub fn end_selection(&mut self) -> Option<String> {
        self.is_selecting = false;
        self.get_selected_text()
    }

    /// Select the word at the given position (for double-click)
    pub fn select_word_at(&mut self, col: usize, visible_row: usize, visible_height: usize) {
        let (start_line_idx, _) = self.get_visible_range(visible_height);
        let absolute_line = start_line_idx + visible_row;

        let content = match self.lines.get(absolute_line) {
            Some(l) => &l.content,
            None => return,
        };

        let chars: Vec<char> = content.chars().collect();
        if col >= chars.len() {
            return;
        }

        // Find word boundaries: a "word" is a sequence of alphanumeric/underscore chars
        let is_word_char = |c: char| c.is_alphanumeric() || c == '_' || c == '-';

        if !is_word_char(chars[col]) {
            // Clicked on a non-word char, select just that char
            self.selection_start = Some((absolute_line, col));
            self.selection_end = Some((absolute_line, col + 1));
            return;
        }

        // Scan left for word start
        let mut start = col;
        while start > 0 && is_word_char(chars[start - 1]) {
            start -= 1;
        }

        // Scan right for word end
        let mut end = col;
        while end < chars.len() && is_word_char(chars[end]) {
            end += 1;
        }

        self.selection_start = Some((absolute_line, start));
        self.selection_end = Some((absolute_line, end));
    }

    /// Clear the current selection
    #[allow(dead_code)]
    pub fn clear_selection(&mut self) {
        self.selection_start = None;
        self.selection_end = None;
        self.is_selecting = false;
    }

    /// Get selected text as a string
    pub fn get_selected_text(&self) -> Option<String> {
        let (start_line, start_col) = self.selection_start?;
        let (end_line, end_col) = self.selection_end?;

        if start_line == end_line && start_col == end_col {
            return None;
        }

        // Normalize selection direction (ensure start <= end)
        let (start_line, start_col, end_line, end_col) = if start_line < end_line
            || (start_line == end_line && start_col < end_col)
        {
            (start_line, start_col, end_line, end_col)
        } else {
            (end_line, end_col, start_line, start_col)
        };

        let mut result = String::new();

        for (idx, line) in self.lines.iter().enumerate() {
            if idx < start_line || idx > end_line {
                continue;
            }

            let line_text = &line.content;

            if start_line == end_line {
                // Single line selection
                let text = line_text
                    .chars()
                    .skip(start_col)
                    .take(end_col - start_col)
                    .collect::<String>();
                result.push_str(&text);
            } else if idx == start_line {
                // First line of multi-line selection
                let text = line_text.chars().skip(start_col).collect::<String>();
                result.push_str(&text);
                result.push('\n');
            } else if idx == end_line {
                // Last line of multi-line selection
                let text = line_text.chars().take(end_col).collect::<String>();
                result.push_str(&text);
            } else {
                // Middle line
                result.push_str(line_text);
                result.push('\n');
            }
        }

        if result.is_empty() {
            None
        } else {
            Some(result)
        }
    }

    /// Check if a position is within current selection
    #[allow(dead_code)]
    fn is_in_selection(&self, line_idx: usize, col: usize) -> bool {
        let Some((start_line, start_col)) = self.selection_start else {
            return false;
        };
        let Some((end_line, end_col)) = self.selection_end else {
            return false;
        };

        // Normalize selection direction
        let (start_line, start_col, end_line, end_col) = if start_line < end_line
            || (start_line == end_line && start_col < end_col)
        {
            (start_line, start_col, end_line, end_col)
        } else {
            (end_line, end_col, start_line, start_col)
        };

        if line_idx < start_line || line_idx > end_line {
            return false;
        }

        if line_idx == start_line && col < start_col {
            return false;
        }

        if line_idx == end_line && col >= end_col {
            return false;
        }

        true
    }

    /// Get the current visible line range (start, end) for mouse event handling
    pub fn get_visible_range(&self, visible_height: usize) -> (usize, usize) {
        let total_lines = self.lines.len();
        if self.auto_scroll {
            let start = total_lines.saturating_sub(visible_height);
            (start, total_lines)
        } else {
            let max_start = total_lines.saturating_sub(visible_height);
            let start = max_start.saturating_sub(self.scroll_offset);
            let start = start.min(max_start);
            (start, (start + visible_height).min(total_lines))
        }
    }

    /// Render the output area
    pub fn render(&mut self, area: Rect, buf: &mut Buffer) {
        if area.height == 0 {
            return;
        }

        // Update width on render (handles resize)
        self.term_width = (area.width as usize).saturating_sub(2);

        let visible_lines = area.height as usize;
        let total_lines = self.lines.len();

        // Calculate which lines to show
        let (start, end) = if self.auto_scroll {
            let start = total_lines.saturating_sub(visible_lines);
            (start, total_lines)
        } else {
            let max_start = total_lines.saturating_sub(visible_lines);
            let start = max_start.saturating_sub(self.scroll_offset);
            let start = start.min(max_start);
            (start, (start + visible_lines).min(total_lines))
        };

        // Clear area first to avoid stale text artifacts
        for y in area.top()..area.bottom() {
            for x in area.left()..area.right() {
                buf[(x, y)].reset();
            }
        }

        // Build lines with selection highlighting
        let lines: Vec<Line> = self.lines
            .iter()
            .enumerate()
            .skip(start)
            .take(end - start)
            .map(|(idx, output_line)| {
                // Check if this line has any selection
                if self.selection_start.is_some() && self.selection_end.is_some() {
                    let line_spans = self.render_line_with_selection(idx, &output_line.content, output_line.style.to_style());
                    Line::from(line_spans)
                } else {
                    Line::styled(&output_line.content, output_line.style.to_style())
                }
            })
            .collect();

        // Truncate lines to area height to prevent buffer overflow
        let lines: Vec<Line> = lines.into_iter().take(area.height as usize).collect();
        // Catch any buffer overflow panics from ratatui (e.g. CJK width edge cases)
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let paragraph = Paragraph::new(lines);
            paragraph.render(area, buf);
        }));

        // Render scrollbar if needed
        if total_lines > visible_lines {
            let scrollbar_area = Rect {
                x: area.right().saturating_sub(1),
                y: area.top(),
                width: 1,
                height: area.height,
            };

            let max_scroll = total_lines.saturating_sub(visible_lines);
            let current_position = if self.auto_scroll {
                max_scroll
            } else {
                max_scroll.saturating_sub(self.scroll_offset)
            };

            let mut scrollbar_state = ScrollbarState::new(max_scroll)
                .position(current_position);

            let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight);
            StatefulWidget::render(scrollbar, scrollbar_area, buf, &mut scrollbar_state);
        }

        self.last_line_count = total_lines;
    }

    /// Render a single line with selection highlighting
    fn render_line_with_selection(&self, line_idx: usize, content: &str, base_style: Style) -> Vec<Span<'static>> {
        let Some((start_line, start_col)) = self.selection_start else {
            return vec![Span::styled(content.to_string(), base_style)];
        };
        let Some((end_line, end_col)) = self.selection_end else {
            return vec![Span::styled(content.to_string(), base_style)];
        };

        // Normalize selection direction
        let (start_line, start_col, end_line, end_col) = if start_line < end_line
            || (start_line == end_line && start_col < end_col)
        {
            (start_line, start_col, end_line, end_col)
        } else {
            (end_line, end_col, start_line, start_col)
        };

        let selection_style = Style::default()
            .bg(Color::Blue)
            .fg(Color::White);

        let chars: Vec<char> = content.chars().collect();
        let mut spans = Vec::new();

        // Determine selection range for this line
        let line_start = if line_idx == start_line { start_col } else { 0 };
        let line_end = if line_idx == end_line { end_col } else { chars.len() };

        // Check if this line is in selection at all
        let in_selection = line_idx >= start_line && line_idx <= end_line;

        if !in_selection || (line_idx == start_line && line_idx == end_line && start_col == end_col) {
            return vec![Span::styled(content.to_string(), base_style)];
        }

        // Build spans for this line
        let mut current_text = String::new();
        let mut current_is_selected = false;

        for (i, &ch) in chars.iter().enumerate() {
            let is_selected = i >= line_start && i < line_end;

            if is_selected != current_is_selected && !current_text.is_empty() {
                // Style changed, push current span
                let style = if current_is_selected { selection_style } else { base_style };
                spans.push(Span::styled(std::mem::take(&mut current_text), style));
            }

            current_text.push(ch);
            current_is_selected = is_selected;
        }

        // Push remaining text
        if !current_text.is_empty() {
            let style = if current_is_selected { selection_style } else { base_style };
            spans.push(Span::styled(current_text, style));
        }

        if spans.is_empty() {
            spans.push(Span::styled(content.to_string(), base_style));
        }

        spans
    }
}
