pub mod markdown;
pub mod diff;
pub mod progress;
pub mod theme;

use aemeath_llm::stream::StreamHandler;
use crossterm::style::{Color, Print, ResetColor, SetForegroundColor};
use crossterm::ExecutableCommand;
use std::io::{self, Write};

pub use progress::ThinkingIndicator;
pub use theme::{StyledText, Theme};

use theme::StyledText as ST;

pub struct TerminalRenderer;

/// Format token count with k/m suffix
fn format_tokens(n: u64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}m", n as f64 / 1_000_000.0)
    } else if n >= 1000 {
        format!("{:.1}k", n as f64 / 1000.0)
    } else {
        n.to_string()
    }
}

impl TerminalRenderer {
    pub fn print_user_prompt() {
        let mut stdout = io::stdout();
        let _ = stdout.execute(SetForegroundColor(Theme::USER_PROMPT));
        let _ = stdout.execute(Print(ST::user_prompt("> ")));
        let _ = stdout.execute(ResetColor);
        let _ = stdout.flush();
    }

    pub fn print_tool_call(name: &str, summary: &str) {
        println!("{}", ST::tool_call(name, summary));
    }

    pub fn print_tool_result(output: &str, is_error: bool) {
        let color = if is_error { Theme::TOOL_ERROR } else { Theme::INFO };
        let mut stdout = io::stdout();
        let _ = stdout.execute(SetForegroundColor(color));
        
        let lines: Vec<&str> = output.lines().collect();
        let display_lines = if lines.len() > 5 {
            let mut v: Vec<&str> = lines[..5].to_vec();
            v.push("  ... (truncated)");
            v
        } else {
            lines
        };
        
        for line in display_lines {
            println!("  {}", ST::info(line));
        }
        
        let _ = stdout.execute(ResetColor);
    }

    /// Check if output looks like a TodoRun result
    fn is_todorun_output(output: &str) -> bool {
        output.contains("TodoRun:") && output.contains("━━━")
    }

    /// Print TodoRun results with friendly colored formatting (no truncation)
    pub fn print_todorun_result(output: &str) {
        use console::style;
        for line in output.lines() {
            let trimmed = line.trim();
            let formatted = if trimmed.is_empty() {
                String::new()
            } else if trimmed.contains('✓') {
                format!("  {}", style(trimmed).green())
            } else if trimmed.contains('✗') {
                format!("  {}", style(trimmed).red())
            } else if trimmed.contains("━━━ Summary") {
                format!("  {}", style(trimmed).yellow().bold())
            } else if trimmed.contains("━━━ [") {
                format!("  {}", ST::highlight(trimmed))
            } else if trimmed.contains("Sub ") || trimmed.contains("Sub-task") {
                format!("    {}", style(trimmed).dim())
            } else if trimmed.contains("Error:") {
                format!("  {}", style(trimmed).red())
            } else {
                format!("  {}", style(trimmed).dim())
            };
            println!("{}", formatted);
        }
    }

    pub fn print_tool_result_with_diff(tool_name: &str, output: &str, is_error: bool) {
        if is_error {
            println!("{}", ST::error(output));
            return;
        }

        if tool_name == "TodoRun" && Self::is_todorun_output(output) {
            Self::print_todorun_result(output);
            return;
        }

        if tool_name == "Edit" && output.contains("\n---DIFF---\n") {
            let parts: Vec<&str> = output.splitn(3, "\n---DIFF---\n").collect();
            if parts.len() == 3 {
                println!("  {}", ST::info(parts[0]));
                diff::print_diff(parts[1], parts[2]);
                return;
            }
        }

        Self::print_tool_result(output, is_error);
    }

    pub fn print_newline() {
        println!();
    }

    pub fn print_usage(input_tokens: u32, output_tokens: u32) {
        println!(
            "{}",
            ST::info(&format!("[tokens: {} in / {} out]", format_tokens(input_tokens as u64), format_tokens(output_tokens as u64)))
        );
    }

    pub fn print_welcome() {
        println!();
        println!("{}", ST::header("╔══════════════════════════════════╗"));
        println!("{}", ST::header("║      Aemeath - AI Agent          ║"));
        println!("{}", ST::header("╚══════════════════════════════════╝"));
        println!();
        println!("{}", ST::info("Type /help for commands"));
        println!();
    }

    pub fn print_goodbye() {
        println!("{}", ST::success("Goodbye!"));
    }

    pub fn print_session_saved(session_id: &str) {
        println!("{}", ST::success(&format!("[session saved: {}]", session_id)));
    }

    pub fn print_compaction(old_len: usize, new_len: usize) {
        println!(
            "{}",
            ST::warning(&format!("[auto-compacted: {} → {} messages]", old_len, new_len))
        );
    }

    pub fn print_interrupted() {
        println!("{}", ST::warning("[interrupted]"));
    }

    pub fn print_cancelled() {
        println!("{}", ST::error("Cancelled"));
    }

    pub fn print_resumed_session(session_id: &str, msg_count: usize) {
        println!(
            "{}",
            ST::success(&format!("[resumed session {}, {} messages]", session_id, msg_count))
        );
    }

    pub fn print_pending_images(count: usize) {
        let mut stdout = io::stdout();
        let _ = stdout.execute(SetForegroundColor(Theme::INFO));
        let _ = stdout.execute(Print(format!("[{} image(s) pending - will be sent with next message]\n", count)));
        let _ = stdout.execute(ResetColor);
    }
}

pub struct TerminalStreamHandler {
    pub verbose: bool,
    pub use_markdown: bool,
    text_buffer: String,
}

impl TerminalStreamHandler {
    pub fn new(verbose: bool, use_markdown: bool) -> Self {
        Self {
            verbose,
            use_markdown,
            text_buffer: String::new(),
        }
    }
}

impl StreamHandler for TerminalStreamHandler {
    fn on_text(&mut self, text: &str) {
        if self.use_markdown {
            self.text_buffer.push_str(text);
        } else {
            print!("{text}");
            let _ = io::stdout().flush();
        }
    }

    fn on_text_block_complete(&mut self, full_text: &str) {
        if self.use_markdown {
            self.text_buffer.clear();
            println!();
            markdown::render_markdown(full_text);
        }
    }

    fn on_tool_use_start(&mut self, name: &str) {
        if self.use_markdown && !self.text_buffer.is_empty() {
            markdown::render_markdown(&self.text_buffer);
            self.text_buffer.clear();
        }
        println!();
        let mut stdout = io::stdout();
        let _ = stdout.execute(SetForegroundColor(Color::Cyan));
        let _ = stdout.execute(Print(format!("[calling {name}...]\n")));
        let _ = stdout.execute(ResetColor);
    }

    fn on_error(&mut self, error: &str) {
        let mut stdout = io::stdout();
        let _ = stdout.execute(SetForegroundColor(Color::Red));
        let _ = stdout.execute(Print(format!("Error: {error}\n")));
        let _ = stdout.execute(ResetColor);
    }

    fn on_raw_line(&mut self, line: &str) {
        if self.verbose {
            log::debug!("[SSE] {line}");
        }
    }
}
