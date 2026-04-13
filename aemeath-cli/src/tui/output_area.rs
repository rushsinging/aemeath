use rand::prelude::IndexedRandom;
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

/// Spinner glyph frames — forward then reverse for a breathing effect
const SPINNER_FRAMES: &[char] = &[
    '·', '✢', '✳', '✶', '✻', '✽',
    '✻', '✶', '✳', '✢', '·',
];

/// Fun verbs shown while the LLM is thinking
const SPINNER_VERBS: &[&str] = &[
    "Thinking",    "Pondering",    "Crafting",     "Computing",
    "Brewing",     "Weaving",      "Conjuring",    "Forging",
    "Hatching",    "Cooking",      "Channeling",   "Ruminating",
    "Composing",   "Imagining",    "Processing",   "Puzzling",
    "Mulling",     "Noodling",     "Tinkering",    "Crystallizing",
    "Synthesizing","Architecting", "Orchestrating","Incubating",
    "Fermenting",  "Simmering",    "Percolating",  "Cogitating",
    "Meandering",  "Harmonizing",
];

/// Spinner colors (warm orange theme, like Claude Code)
const SPINNER_BASE: Color = Color::Rgb(204, 152, 87);
const SPINNER_HIGHLIGHT: Color = Color::Rgb(255, 210, 140);
const SPINNER_DIM: Color = Color::Rgb(140, 105, 60);

/// Linear interpolation between two RGB colors
fn lerp_color(a: Color, b: Color, t: f32) -> Color {
    if let (Color::Rgb(r1, g1, b1), Color::Rgb(r2, g2, b2)) = (a, b) {
        let r = (r1 as f32 + (r2 as f32 - r1 as f32) * t) as u8;
        let g = (g1 as f32 + (g2 as f32 - g1 as f32) * t) as u8;
        let b = (b1 as f32 + (b2 as f32 - b1 as f32) * t) as u8;
        Color::Rgb(r, g, b)
    } else {
        a
    }
}

/// Animated spinner state for the output area
struct SpinnerState {
    /// Animation frame counter
    frame: u64,
    /// Current verb text
    verb: String,
    /// When this spinner started
    start: std::time::Instant,
}

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
fn debug_log(msg: &str) {
    use std::io::Write;
    let path = dirs::home_dir()
        .unwrap_or_default()
        .join(".aemeath")
        .join("debug.log");
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let _ = writeln!(f, "[{ts}] {msg}");
    }
}

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
                let preview = if prompt.len() > 300 {
                    format!("{}...", &prompt[..prompt.char_indices().take_while(|(i, _)| *i < 300).last().map(|(i, c)| i + c.len_utf8()).unwrap_or(300)])
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
            debug_log(&format!("TodoWrite raw_json: {raw_json}"));
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
                    debug_log(&format!("TodoWrite header: ● TodoWrite ({count} items)"));
                    for d in &details {
                        debug_log(&format!("TodoWrite detail: {d}"));
                    }
                    return (format!("● TodoWrite ({count} items)"), details);
                } else {
                    debug_log(&format!("TodoWrite: parsed Value has no 'todos' array. Value: {v}"));
                }
            } else {
                debug_log(&format!("TodoWrite: parse failed: {:?}", parsed.as_ref().err()));
            }
        }
        "TodoRun" => {
            if let Ok(v) = &parsed {
                // Show pending todo list if injected by TUI
                if let Some(pending) = v.get("_pending").and_then(|p| p.as_array()) {
                    let mut details = Vec::new();
                    for item in pending {
                        if let Some(s) = item.as_str() {
                            details.push(format!("○ {}", s));
                        }
                    }
                    if !details.is_empty() {
                        return (format!("● TodoRun ({} todo{})", details.len(), if details.len() > 1 { "s" } else { "" }), details);
                    }
                }
                return ("● TodoRun".to_string(), vec!["execute all pending todos".to_string()]);
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

/// Convert a screen column position (display column) to a char index within the string.
/// CJK characters occupy 2 display columns but only 1 char index.
/// Returns the char index that best maps to the given screen column.
/// If `screen_col` lands in the middle of a wide char, returns that char's index.
fn screen_col_to_char_idx(text: &str, screen_col: usize) -> usize {
    let mut display_width = 0usize;
    for (i, ch) in text.char_indices() {
        let ch_w = ch.width().unwrap_or(1) as usize;
        if display_width + ch_w > screen_col {
            // screen_col falls within this character
            return i;
        }
        display_width += ch_w;
    }
    // Past the end — return char count (one past last index)
    text.chars().count()
}

/// Get the display width (screen columns) of a string, accounting for CJK wide chars.
#[allow(dead_code)]
fn display_width(text: &str) -> usize {
    text.chars().map(|c| c.width().unwrap_or(1) as usize).sum()
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
    /// True if the currently-open `<think>` block was synthetically injected by
    /// `append_thinking_text` (from a `reasoning_content` field). Only synthetic
    /// blocks get auto-closed when normal content arrives — real `<think>` tags
    /// embedded in the `content` field (e.g. MiniMax-M2) must not be force-closed.
    synthetic_think_open: bool,
    /// Number of queued user message lines (added while streaming)
    queued_line_count: usize,
    /// Whether mouse is currently dragging (selecting)
    is_selecting: bool,
    /// Selection start in absolute line coordinates
    selection_start: Option<(usize, usize)>,
    /// Selection end in absolute line coordinates
    selection_end: Option<(usize, usize)>,
    /// Active spinner animation (shown as last line when Some)
    spinner: Option<SpinnerState>,
    /// Cached visible height from last render
    last_visible_height: usize,
    /// Cache of todo id -> subject, so updates that only carry id+status can still show the subject
    todo_subject_cache: std::collections::HashMap<String, String>,
    /// Task status lines shown below the spinner (updated externally)
    task_status_lines: Vec<String>,
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
            synthetic_think_open: false,
            queued_line_count: 0,
            is_selecting: false,
            selection_start: None,
            selection_end: None,
            spinner: None,
            last_visible_height: 0,
            todo_subject_cache: std::collections::HashMap::new(),
            task_status_lines: Vec::new(),
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
            // Track lines added while streaming (for queue display)
            if self.streaming_start.is_some() {
                self.queued_line_count += 1;
            }
        }
        // If text ends with newline or is empty, add an empty line for spacing
        if text.is_empty() || text.ends_with('\n') {
            self.push_line(OutputLine {
                content: String::new(),
                style: LineStyle::User,
            });
            if self.streaming_start.is_some() {
                self.queued_line_count += 1;
            }
        }
    }

    /// Add an assistant message
    #[allow(dead_code)]
    pub fn push_assistant_message(&mut self, text: &str) {
        self.push_text(text, LineStyle::Assistant);
    }

    /// Track whether we're currently inside a thinking block (open <think> without close)
    fn has_unclosed_think(&self) -> bool {
        let opens = self.streaming_buffer.matches("<think>").count();
        let closes = self.streaming_buffer.matches("</think>").count();
        opens > closes
    }

    /// Append reasoning/thinking text to the streaming block.
    /// Wraps in `<think>` tags so the existing parser renders it dimmed/italic.
    /// However, if the text already contains `<think>` tags (e.g. from MiniMax),
    /// we append it directly without additional wrapping.
    pub fn append_thinking_text(&mut self, text: &str) {
        // If the incoming text already contains thinking tags, just append it directly
        // (avoid double-wrapping which breaks parsing)
        if text.contains("<think>") || text.contains("</think>") {
            self.streaming_buffer.push_str(text);
        } else {
            if !self.has_unclosed_think() {
                // Inject opening tag — append_assistant_text will auto-close it
                // when the next normal-content chunk arrives.
                self.streaming_buffer.push_str("<think>");
                self.synthetic_think_open = true;
            }
            self.streaming_buffer.push_str(text);
        }
        self.do_rerender();
    }

    /// Append text to the streaming assistant block.
    /// Accumulates in a buffer and re-renders all lines from the block start.
    /// Supports `<think>...</think>` tags — thinking content is displayed dimmed/italic.
    pub fn append_assistant_text(&mut self, text: &str) {
        // Only auto-close thinking blocks that WE synthetically injected via
        // `append_thinking_text`. Real `<think>` tags arriving in the content
        // stream (e.g. MiniMax-M2) will be closed by their own matching
        // `</think>` from the same stream — don't force-close them here, or
        // the trailing real `</think>` will leak as literal text.
        if self.synthetic_think_open && self.has_unclosed_think() {
            self.streaming_buffer.push_str("</think>\n");
            self.synthetic_think_open = false;
        }
        self.streaming_buffer.push_str(text);
        self.do_rerender();
    }

    /// Core re-render logic for the streaming block
    fn do_rerender(&mut self) {

        // Record where the streaming block starts (first call)
        if self.streaming_start.is_none() {
            self.streaming_start = Some(self.lines.len());
        }

        let start_idx = self.streaming_start.unwrap_or(0);

            // Track line count before re-render to adjust scroll offset
            let old_line_count = self.lines.len();

            // Save queued message lines (added while streaming)
            let queued_lines: Vec<OutputLine> = (0..self.queued_line_count)
                .filter_map(|_| self.lines.pop_back())
                .collect();
            self.queued_line_count = 0;

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

        // Restore queued message lines (they were saved before clearing the block)
        for line in queued_lines.into_iter().rev() {
            self.lines.push_back(line);
            self.queued_line_count += 1;
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
        self.synthetic_think_open = false;
        self.queued_line_count = 0;
    }

    /// Start the animated spinner in the output area
    pub fn start_spinner(&mut self) {
        if self.spinner.is_some() {
            return; // already running
        }
        let mut rng = rand::rng();
        self.spinner = Some(SpinnerState {
            frame: 0,
            verb: SPINNER_VERBS.choose(&mut rng).unwrap_or(&"Thinking").to_string(),
            start: std::time::Instant::now(),
        });
    }

    /// Stop the animated spinner
    pub fn stop_spinner(&mut self) {
        self.spinner = None;
    }

    /// Update the task status lines shown below the spinner
    pub fn set_task_status(&mut self, lines: Vec<String>) {
        self.task_status_lines = lines;
    }

    /// Build the animated spinner line (called during render)
    fn build_spinner_line(&self) -> Option<Line<'static>> {
        let s = self.spinner.as_ref()?;

        let mut spans = Vec::new();

        // 1. Rotating glyph — changes every ~3 frames (150ms at 50ms tick)
        let glyph = SPINNER_FRAMES[(s.frame / 3) as usize % SPINNER_FRAMES.len()];
        spans.push(Span::styled(
            format!(" {} ", glyph),
            Style::default().fg(SPINNER_BASE).add_modifier(Modifier::BOLD),
        ));

        // 2. Shimmer text — a highlight band sweeps across the verb
        let text = format!("{}…", s.verb);
        let text_len = text.chars().count() as i32;
        let cycle_len = text_len + 16;
        let glimmer_pos = ((s.frame / 2) as i32) % cycle_len - 8;

        for (i, ch) in text.chars().enumerate() {
            let dist = (i as i32 - glimmer_pos).abs();
            let color = if dist == 0 {
                SPINNER_HIGHLIGHT
            } else if dist <= 2 {
                lerp_color(SPINNER_HIGHLIGHT, SPINNER_BASE, dist as f32 / 3.0)
            } else if dist <= 4 {
                SPINNER_BASE
            } else {
                SPINNER_DIM
            };
            spans.push(Span::styled(ch.to_string(), Style::default().fg(color)));
        }

        // 3. Elapsed time
        let elapsed = s.start.elapsed().as_secs();
        if elapsed >= 1 {
            spans.push(Span::styled(
                format!("  {}s", elapsed),
                Style::default().fg(Color::DarkGray),
            ));
        }

        Some(Line::from(spans))
    }

    /// Add a tool call with human-friendly formatting.
    /// Shows header with status dot and indented details.
    pub fn push_tool_call(&mut self, name: &str, summary: &str) {
        // For TodoWrite, use the cache to resolve subjects on status-only updates
        let (header, details) = if name == "TodoWrite" {
            self.format_todowrite(summary)
        } else {
            format_tool_call(name, summary)
        };

        // Push header line with ToolCallRunning style (yellow dot)
        self.push_line(OutputLine {
            content: header,
            style: LineStyle::ToolCallRunning,
        });

        // Push detail lines indented under the header
        for detail in details.iter() {
            self.push_line(OutputLine {
                content: format!("    {detail}"),
                style: LineStyle::System,
            });
        }

        // Blank line to visually separate consecutive tool calls
        self.push_line(OutputLine {
            content: String::new(),
            style: LineStyle::System,
        });
    }

    /// Format TodoWrite tool call, maintaining a subject cache so that
    /// status-only updates (id + status, no subject) still display correctly.
    fn format_todowrite(&mut self, raw_json: &str) -> (String, Vec<String>) {
        let parsed: Result<serde_json::Value, _> = serde_json::from_str(raw_json);
        debug_log(&format!("TodoWrite raw_json: {raw_json}"));

        if let Ok(v) = parsed {
            if let Some(todos) = v.get("todos").and_then(|t| t.as_array()) {
                let count = todos.len();
                let mut details: Vec<String> = Vec::new();

                // Update cache with any subjects present in this call
                for todo in todos.iter() {
                    if let (Some(id), Some(subject)) = (
                        todo.get("id").and_then(|s| s.as_str()),
                        todo.get("subject").and_then(|s| s.as_str()),
                    ) {
                        self.todo_subject_cache.insert(id.to_string(), subject.to_string());
                    }
                }

                for todo in todos.iter().take(3) {
                    // Try direct subject first, then fall back to cache via id
                    let subject = todo.get("subject").and_then(|s| s.as_str())
                        .map(|s| s.to_string())
                        .or_else(|| {
                            todo.get("id").and_then(|s| s.as_str())
                                .and_then(|id| self.todo_subject_cache.get(id).cloned())
                        })
                        .unwrap_or_else(|| "?".to_string());

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

        // Fallback
        format_tool_call("TodoWrite", raw_json)
    }

    /// Add a tool result with diff support.
    /// For Edit tool results containing diff information, displays with red/green backgrounds.
    pub fn push_tool_result_with_diff(&mut self, tool_name: &str, result: &str, is_error: bool) {
        // Update the most recent ToolCallRunning header line to completed/error state
        // This stops the spinner animation for this tool
        let done_icon = if is_error { "✗" } else { "✓" };
        let done_style = if is_error { LineStyle::ToolCallError } else { LineStyle::ToolCallSuccess };
        for line in self.lines.iter_mut().rev() {
            if matches!(line.style, LineStyle::ToolCallRunning) {
                line.content = line.content.replacen('●', done_icon, 1);
                line.style = done_style;
                break;
            }
        }

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

        if !result.trim().is_empty() {
            // Task tools get more lines to show the task list summary
            let max_lines = if matches!(tool_name, "TaskList") {
                20
            } else {
                3
            };

            let total = result.lines().count();
            let display_lines: Vec<&str> = result.lines().take(max_lines).collect();
            let has_more = total > max_lines;

            for line in &display_lines {
                self.push_line(OutputLine {
                    content: format!("  │  {line}"),
                    style: LineStyle::System,
                });
            }
            if has_more {
                self.push_line(OutputLine {
                    content: format!("  │  ... ({} lines omitted)", total - max_lines),
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
        for line in msg.lines() {
            self.push_line(OutputLine {
                content: line.to_string(),
                style: LineStyle::System,
            });
        }
    }

    /// Scroll up by the given number of lines
    pub fn scroll_up(&mut self, amount: usize) {
        self.auto_scroll = false;
        let visible_height = self.last_visible_height;
        let max_offset = self.lines.len().saturating_sub(visible_height);
        self.scroll_offset = (self.scroll_offset.saturating_add(amount)).min(max_offset);
        if max_offset == 0 {
            // Not enough content to scroll; stay at bottom
            self.scroll_offset = 0;
            self.auto_scroll = true;
        }
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
    /// visible_height is the height of the output area in rows.
    /// `col` is a screen/display column (accounts for CJK wide chars).
    pub fn start_selection_at(&mut self, col: usize, visible_row: usize, visible_height: usize) {
        // Convert visible row to absolute line index
        let (start_line_idx, _) = self.get_visible_range(visible_height);
        let absolute_line = start_line_idx + visible_row;

        // Convert screen column to char index, respecting wide characters
        let char_idx = self.lines.get(absolute_line)
            .map(|l| screen_col_to_char_idx(&l.content, col))
            .unwrap_or(0);

        self.is_selecting = true;
        self.selection_start = Some((absolute_line, char_idx));
        self.selection_end = Some((absolute_line, char_idx));
    }

    /// Update selection end position during drag.
    /// `col` is a screen/display column (accounts for CJK wide chars).
    pub fn update_selection_at(&mut self, col: usize, visible_row: usize, visible_height: usize) {
        if self.is_selecting {
            // Convert visible row to absolute line index
            let (start_line_idx, _) = self.get_visible_range(visible_height);
            let absolute_line = start_line_idx + visible_row;

            // Convert screen column to char index, respecting wide characters
            let char_idx = self.lines.get(absolute_line)
                .map(|l| screen_col_to_char_idx(&l.content, col))
                .unwrap_or(0);

            self.selection_end = Some((absolute_line, char_idx));
        }
    }

    /// End selection and return selected text
    pub fn end_selection(&mut self) -> Option<String> {
        self.is_selecting = false;
        self.get_selected_text()
    }

    /// Select the word at the given position (for double-click).
    /// `col` is a screen/display column (accounts for CJK wide chars).
    pub fn select_word_at(&mut self, col: usize, visible_row: usize, visible_height: usize) {
        let (start_line_idx, _) = self.get_visible_range(visible_height);
        let absolute_line = start_line_idx + visible_row;

        let content = match self.lines.get(absolute_line) {
            Some(l) => &l.content,
            None => return,
        };

        // Convert screen column to char index
        let char_idx = screen_col_to_char_idx(content, col);
        let chars: Vec<char> = content.chars().collect();
        if char_idx >= chars.len() {
            return;
        }

        // Find word boundaries: a "word" is a sequence of alphanumeric/underscore chars
        let is_word_char = |c: char| c.is_alphanumeric() || c == '_' || c == '-';

        if !is_word_char(chars[char_idx]) {
            // Clicked on a non-word char, select just that char
            self.selection_start = Some((absolute_line, char_idx));
            self.selection_end = Some((absolute_line, char_idx + 1));
            return;
        }

        // Scan left for word start
        let mut start = char_idx;
        while start > 0 && is_word_char(chars[start - 1]) {
            start -= 1;
        }

        // Scan right for word end
        let mut end = char_idx;
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

        // Advance spinner frame
        if let Some(ref mut s) = self.spinner {
            s.frame = s.frame.wrapping_add(1);
        }

        // Update width on render (handles resize)
        self.term_width = (area.width as usize).saturating_sub(2);

        // Build spinner line (if active) and task status lines — reserve rows at bottom
        let spinner_line = self.build_spinner_line();
        let task_line_count = if self.spinner.is_some() { self.task_status_lines.len() } else { 0 };
        let reserved = if spinner_line.is_some() { 1 + task_line_count } else { 0 };

        let visible_lines = (area.height as usize).saturating_sub(reserved);
        self.last_visible_height = visible_lines;
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

        // Build lines with selection highlighting and animated dots
        let spinner_frame_idx = self.spinner.as_ref().map(|s| s.frame).unwrap_or(0);

        let mut lines: Vec<Line> = self.lines
            .iter()
            .enumerate()
            .skip(start)
            .take(end - start)
            .map(|(idx, output_line)| {
                // For ToolCallRunning lines, show a blinking white dot ●
                if matches!(output_line.style, LineStyle::ToolCallRunning) && output_line.content.starts_with('●') {
                    let rest = &output_line.content[3..]; // ● is 3 bytes in UTF-8
                    // Blink: alternate between bright white and dim gray
                    let blink_on = (spinner_frame_idx / 10) % 2 == 0; // ~500ms per phase at 50ms tick
                    let dot_color = if blink_on {
                        Color::White
                    } else {
                        Color::DarkGray
                    };
                    let dot_span = Span::styled(
                        "●".to_string(),
                        Style::default().fg(dot_color),
                    );
                    let text_span = Span::styled(
                        rest.to_string(),
                        output_line.style.to_style(),
                    );
                    return Line::from(vec![dot_span, text_span]);
                }

                // For completed ToolCall lines (✓), show green dot ●
                if matches!(output_line.style, LineStyle::ToolCallSuccess) && output_line.content.starts_with('✓') {
                    let rest = &output_line.content[3..]; // ✓ is 3 bytes in UTF-8
                    let dot_span = Span::styled(
                        "●".to_string(),
                        Style::default().fg(Color::Green),
                    );
                    let text_span = Span::styled(
                        rest.to_string(),
                        output_line.style.to_style(),
                    );
                    return Line::from(vec![dot_span, text_span]);
                }

                // For failed ToolCall lines (✗), show red dot ●
                if matches!(output_line.style, LineStyle::ToolCallError) && output_line.content.starts_with('✗') {
                    let rest = &output_line.content[3..]; // ✗ is 3 bytes in UTF-8
                    let dot_span = Span::styled(
                        "●".to_string(),
                        Style::default().fg(Color::Red),
                    );
                    let text_span = Span::styled(
                        rest.to_string(),
                        output_line.style.to_style(),
                    );
                    return Line::from(vec![dot_span, text_span]);
                }

                // Check if this line has any selection
                if self.selection_start.is_some() && self.selection_end.is_some() {
                    let line_spans = self.render_line_with_selection(idx, &output_line.content, output_line.style.to_style());
                    Line::from(line_spans)
                } else {
                    Line::styled(&output_line.content, output_line.style.to_style())
                }
            })
            .collect();

        // Append spinner line at the bottom, then task status lines below it
        if let Some(sl) = spinner_line {
            lines.push(sl);
            // Render task status lines below spinner
            for task_line in &self.task_status_lines {
                lines.push(Line::styled(
                    format!("  {task_line}"),
                    Style::default().fg(Color::DarkGray),
                ));
            }
        }

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn todowrite_real_input_from_session() {
        let raw = r#"{"todos":[{"activeForm":"Reviewing aemeath-core","description":"Read","id":"1","status":"in_progress","subject":"Review aemeath-core (核心逻辑)"},{"activeForm":"Reviewing aemeath-llm","description":"Read","id":"2","status":"pending","subject":"Review aemeath-llm (LLM 抽象层)"},{"activeForm":"Reviewing aemeath-tools","description":"Read","id":"3","status":"pending","subject":"Review aemeath-tools (工具实现)"}]}"#;
        let (header, details) = format_tool_call("TodoWrite", raw);
        println!("HEADER: {header}");
        for d in &details {
            println!("DETAIL: {d}");
        }
        assert!(header.contains("3 items"), "header was: {header}");
        assert!(details[0].contains("核心"), "detail[0]: {}", details[0]);
        assert!(details[0].starts_with("◐"), "detail[0] icon: {}", details[0]);
        assert!(details[1].starts_with("○"), "detail[1] icon: {}", details[1]);
    }

    #[test]
    fn todowrite_via_value_to_string_roundtrip() {
        // Simulate the full path: Value -> to_string -> format_tool_call
        let v: serde_json::Value = serde_json::from_str(r#"{"todos":[{"subject":"Review aemeath-core (核心逻辑)","status":"in_progress"},{"subject":"T2","status":"pending"}]}"#).unwrap();
        let s = v.to_string();
        println!("ROUNDTRIP STRING: {s}");
        let (header, details) = format_tool_call("TodoWrite", &s);
        println!("HEADER: {header}");
        for d in &details {
            println!("DETAIL: {d}");
        }
        assert!(details[0].contains("核心"), "detail[0]: {}", details[0]);
        assert!(details[0].starts_with("◐"));
    }

    #[test]
    fn todorun_with_max_turns() {
        let raw = r#"{"max_turns_per_todo": 100}"#;
        let (header, details) = format_tool_call("TodoRun", raw);
        assert_eq!(header, "● TodoRun");
        assert_eq!(details.len(), 1);
        assert_eq!(details[0], "max turns per todo: 100");
    }

    #[test]
    fn todorun_without_max_turns() {
        let raw = "{}";
        let (header, details) = format_tool_call("TodoRun", raw);
        assert_eq!(header, "● TodoRun");
        assert_eq!(details.len(), 1);
        assert_eq!(details[0], "execute all pending todos");
    }
}
