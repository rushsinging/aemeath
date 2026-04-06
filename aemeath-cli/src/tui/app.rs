use crate::image::{ProcessedImage, is_image_file, process_image_file};
use crate::tui::completion::{SuggestionContext, generate_suggestions, apply_suggestion};
use super::{InputArea, OutputArea, StatusBar};
use super::output_area::{LineStyle, OutputLine};
use aemeath_core::agent::Agent;
use aemeath_core::command::cmd;
use aemeath_core::cost::format_tokens;
use aemeath_core::message::Message;
use aemeath_core::tool::{ImageData, ToolContext, ToolRegistry};
use aemeath_llm::stream::StreamHandler;
use aemeath_llm::types::StopReason;
use crossterm::{
    event::{Event, EventStream, KeyCode, KeyModifiers, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use futures::StreamExt;
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    Terminal,
};
use std::io;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

/// Events sent from background task to UI
enum UiEvent {
    Text(String),
    TextBlockComplete(String),
    ToolCallStart(String),
    ToolCall { name: String, summary: String },
    ToolResult { tool_name: String, output: String, is_error: bool, images: Vec<ImageData> },
    Usage { input: u32, output: u32 },
    Error(String),
    Cancelled,
    /// Sync messages back from background task to main thread
    MessagesSync(Vec<Message>),
    Done,
    /// Clipboard image loaded from background task
    ClipboardImage(ProcessedImage),
    /// System message (non-error)
    SystemMessage(String),
}

/// Main TUI application
pub struct App {
    output_area: OutputArea,
    input_area: InputArea,
    status_bar: StatusBar,
    messages: Vec<Message>,
    cwd: PathBuf,
    total_input_tokens: u64,
    total_output_tokens: u64,
    total_api_calls: u64,
    should_exit: bool,
    pending_images: Vec<ProcessedImage>,
    /// Stores the output area rect for mouse coordinate conversion
    output_area_rect: Rect,
    /// Flag to track if we just processed a paste event (to avoid duplicate handling)
    just_pasted: bool,
    /// Queued user input to send after current processing finishes
    queued_input: Option<String>,
    /// Last click time and position for double-click detection
    last_click: Option<(std::time::Instant, u16, u16)>,
    /// System prompt text for compaction
    system_prompt_text: String,
    /// Context size for compaction threshold
    context_size: usize,
}

impl App {
    pub fn new(session_id: String, cwd: PathBuf, model: String) -> Self {
        let mut status_bar = StatusBar::new();
        status_bar.set_session_id(&session_id);
        status_bar.set_model(&model);

        let mut output_area = OutputArea::new();
        output_area.push_system("Aemeath - AI Agent");
        output_area.push_system("");
        output_area.push_system("Type /help for available commands");
        output_area.push_system("");

        Self {
            output_area,
            input_area: InputArea::new(),
            status_bar,
            messages: Vec::new(),
            cwd,
            total_input_tokens: 0,
            total_output_tokens: 0,
            total_api_calls: 0,
            should_exit: false,
            pending_images: Vec::new(),
            output_area_rect: Rect::default(),
            just_pasted: false,
            queued_input: None,
            last_click: None,
            system_prompt_text: String::new(),
            context_size: 200_000,
        }
    }

    /// Run the TUI event loop
    pub async fn run(
        &mut self,
        client: Arc<aemeath_llm::client::LlmClient>,
        registry: ToolRegistry,
        system_blocks: Vec<aemeath_llm::types::SystemBlock>,
        system_prompt_text: String,
        user_context: String,
        context_size: usize,
        verbose: bool,
        use_markdown: bool,
        agent_runner: Option<Arc<dyn aemeath_core::tool::AgentRunner>>,
        allow_all: bool,
    ) -> io::Result<()> {
        // Store for compaction
        self.system_prompt_text = system_prompt_text.clone();
        self.context_size = context_size;

        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(
            stdout,
            EnterAlternateScreen,
            crossterm::event::EnableBracketedPaste,
            crossterm::event::EnableMouseCapture,
        )?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;

        let interrupted = Arc::new(AtomicBool::new(false));
        let registry = Arc::new(registry);

        let result = self.run_loop(
            &mut terminal,
            client,
            registry,
            system_blocks,
            system_prompt_text,
            user_context,
            context_size,
            verbose,
            use_markdown,
            agent_runner,
            allow_all,
            interrupted,
        ).await;

        disable_raw_mode()?;
        execute!(
            terminal.backend_mut(),
            crossterm::event::DisableMouseCapture,
            crossterm::event::DisableBracketedPaste,
            LeaveAlternateScreen,
        )?;
        terminal.show_cursor()?;

        result
    }

    async fn run_loop(
        &mut self,
        terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
        client: Arc<aemeath_llm::client::LlmClient>,
        registry: Arc<ToolRegistry>,
        system_blocks: Vec<aemeath_llm::types::SystemBlock>,
        _system_prompt_text: String,
        user_context: String,
        _context_size: usize,
        _verbose: bool,
        _use_markdown: bool,
        agent_runner: Option<Arc<dyn aemeath_core::tool::AgentRunner>>,
        allow_all: bool,
        interrupted: Arc<AtomicBool>,
    ) -> io::Result<()> {
        let read_files = Arc::new(std::sync::Mutex::new(std::collections::HashSet::new()));
        let (ui_tx, mut ui_rx) = mpsc::channel::<UiEvent>(256);
        // Use StatusBar's is_processing method for consistency
        let mut is_processing = self.status_bar.is_processing();
        // Shared cancel token — recreated for each processing run
        let active_cancel: Arc<std::sync::Mutex<Option<CancellationToken>>> =
            Arc::new(std::sync::Mutex::new(None));

        let mut event_stream = EventStream::new();

        loop {
            // Draw UI
            let mut output_rect = Rect::default();
            terminal.draw(|f| {
                let size = f.area();
                
                // Calculate suggestions height
                let suggestions_height = if self.input_area.is_showing_suggestions() {
                    let count = self.input_area.get_suggestions().len().min(5) as u16;
                    if count > 0 { count + 1 } else { 0 } // +1 for border/padding
                } else {
                    0
                };

                let chunks = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([
                        Constraint::Min(10),        // Output area
                        Constraint::Length(5),      // Input area
                        Constraint::Length(suggestions_height), // Suggestions (dynamic)
                        Constraint::Length(1),       // Status bar
                    ])
                    .split(size);

                output_rect = chunks[0]; // Save for mouse handling

                // Guard: skip rendering if any chunk has zero height
                if chunks.iter().any(|c| c.height == 0 && c.width == 0) {
                    return;
                }

                // Wrap each render in catch_unwind to prevent ratatui buffer panics
                let buf = f.buffer_mut();
                if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    self.output_area.render(chunks[0], buf);
                })).is_err() {
                    self.status_bar.set_warning("Render error, try resizing");
                }
                self.input_area.set_pending_images(self.pending_images.len());
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    self.input_area.render(chunks[1], buf);
                }));
                if suggestions_height > 0 {
                    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        self.input_area.render_suggestions_in_area(chunks[2], buf);
                    }));
                }
                self.status_bar.set_tokens(self.total_input_tokens, self.total_output_tokens);
                self.status_bar.set_api_calls(self.total_api_calls);
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    self.status_bar.render(chunks[3], buf);
                }));
            })?;
            self.output_area_rect = output_rect;

            // Handle events via async select (non-blocking, no block_in_place needed)
            let maybe_event = tokio::select! {
                biased;
                // Background events first — tool results, streaming text, etc.
                ev = ui_rx.recv() => {
                    // Process background events inline with event loop
                    if let Some(ev) = ev {
                        match ev {
                            UiEvent::Text(text) => self.output_area.append_assistant_text(&text),
                            UiEvent::TextBlockComplete(_text) => {
                                self.output_area.finish_streaming();
                                self.output_area.push_system("");
                            }
                            UiEvent::ToolCallStart(name) => {
                                self.status_bar.set_processing(&format!("Calling {}...", name));
                            }
                            UiEvent::ToolCall { name, summary } => {
                                self.output_area.push_tool_call(&name, &summary);
                            }
                            UiEvent::ToolResult { tool_name, output, is_error, images } => {
                                  let image_note = if images.is_empty() {
                                      String::new()
                                  } else {
                                      format!("  │  [{} image(s) attached]\n", images.len())
                                  };
                                  self.output_area.push_tool_result_with_diff(&tool_name, &output, is_error);
                                  if !image_note.is_empty() {
                                      self.output_area.push_line(OutputLine {
                                          content: image_note.trim().to_string(),
                                          style: LineStyle::System,
                                      });
                                  }
                              }
                            UiEvent::Usage { input, output } => {
                                self.total_input_tokens += input as u64;
                                self.total_output_tokens += output as u64;
                                self.total_api_calls += 1;
                            }
                            UiEvent::Error(msg) => {
                                self.output_area.push_error(&msg);
                                is_processing = false;
                                self.status_bar.clear_processing();
                            }
                            UiEvent::Cancelled => {
                                self.output_area.push_cancelled();
                                is_processing = false;
                                self.status_bar.clear_processing();
                            }
                            UiEvent::MessagesSync(msgs) => {
                                self.messages = msgs;
                            }
                            UiEvent::ClipboardImage(img) => {
                                self.pending_images.push(img);
                                self.input_area.set_pending_images(self.pending_images.len());
                            }
                            UiEvent::SystemMessage(msg) => {
                                self.output_area.push_system(&msg);
                            }
                            UiEvent::Done => {
                                self.output_area.finish_streaming();
                                is_processing = false;
                                self.status_bar.clear_processing();
                                self.status_bar.set_success("Ready");

                                // Process queued input immediately
                                if let Some(queued) = self.queued_input.take() {
                                    self.messages.push(Message::user(&queued));
                                    self.status_bar.set_processing("Thinking...");
                                    is_processing = true; // Sync with StatusBar

                                    let tx = ui_tx.clone();
                                    let client = client.clone();
                                    let registry = registry.clone();
                                    let system_blocks = system_blocks.clone();
                                    let user_context = user_context.clone();
                                    let messages = self.messages.clone();
                                    let cwd = self.cwd.clone();
                                    let read_files = read_files.clone();
                                    let agent_runner = agent_runner.clone();
                                    let interrupted = interrupted.clone();
                                    let cancel = CancellationToken::new();
                                    if let Ok(mut guard) = active_cancel.lock() {
                                        *guard = Some(cancel.clone());
                                    }
                                    tokio::spawn(async move {
                                        process_in_background(
                                            tx, client, registry, system_blocks,
                                            user_context, messages, cwd, read_files,
                                            agent_runner, allow_all, interrupted, cancel,
                                        ).await;
                                    });
                                }
                            }
                        }
                    }
                    continue; // Go back to draw + select
                }
                // Terminal input events (keyboard, mouse, paste)
                ev = event_stream.next() => ev,
                _ = tokio::time::sleep(std::time::Duration::from_millis(50)) => {
                    continue; // Redraw periodically even without events
                }
            };

            if let Some(Ok(event)) = maybe_event {
                match event {
                    Event::Paste(text) if !is_processing => {
                        // Set flag to skip duplicate keyboard event handling
                        self.just_pasted = true;
                        
                        if text.trim().is_empty() {
                            // Empty paste — likely an image in clipboard
                            // Spawn background task to avoid blocking the event loop
                            let output_tx = ui_tx.clone();
                            tokio::spawn(async move {
                                match crate::image::read_clipboard_image().await {
                                    Ok(img) => {
                                        let size = img.final_size;
                                        let _ = output_tx.send(UiEvent::ClipboardImage(img)).await;
                                        let _ = output_tx.send(UiEvent::SystemMessage(
                                            format!("[clipboard image added ({} bytes). Type message to send.]", size)
                                        )).await;
                                    }
                                    Err(e) => {
                                        let _ = output_tx.send(UiEvent::Error(
                                            format!("No image in clipboard: {e}")
                                        )).await;
                                    }
                                }
                            });
                            self.output_area.push_system("[reading clipboard image...]");
                        } else if is_image_file(text.trim()) {
                            // Pasted content is an image file path — load it as an image
                            self.output_area.push_system(&format!("[loading image: {}...]", text.trim()));
                            match process_image_file(text.trim()).await {
                                Ok(img) => {
                                    let size = img.final_size;
                                    self.pending_images.push(img);
                                    self.input_area.set_pending_images(self.pending_images.len());
                                    self.output_area.push_system(&format!(
                                        "[image loaded ({} bytes). Type message to send.]",
                                        size
                                    ));
                                }
                                Err(e) => {
                                    self.output_area.push_error(&format!("Failed to load image: {e}"));
                                }
                            }
                        } else {
                            // Text paste
                            for ch in text.chars() {
                                if ch == '\n' || ch == '\r' {
                                    self.input_area.enter(true);
                                } else {
                                    self.input_area.input(ch);
                                }
                            }
                            self.update_suggestions();
                        }
                    }
                    Event::Mouse(mouse) => {
                        match mouse.kind {
                            crossterm::event::MouseEventKind::ScrollUp => {
                                self.output_area.scroll_up(3);
                            }
                            crossterm::event::MouseEventKind::ScrollDown => {
                                self.output_area.scroll_down(3);
                            }
                            crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left) => {
                                let rect = self.output_area_rect;
                                if mouse.row >= rect.top() && mouse.row < rect.bottom()
                                    && mouse.column >= rect.left() && mouse.column < rect.right()
                                {
                                    let col = (mouse.column - rect.left()) as usize;
                                    let row = (mouse.row - rect.top()) as usize;
                                    let now = std::time::Instant::now();

                                    // Double-click detection: same position within 400ms
                                    let is_double_click = self.last_click
                                        .map(|(t, c, r)| {
                                            now.duration_since(t).as_millis() < 400
                                                && c == mouse.column && r == mouse.row
                                        })
                                        .unwrap_or(false);

                                    if is_double_click {
                                        // Select word at position (highlight only, no clipboard copy)
                                        self.output_area.select_word_at(col, row, rect.height as usize);
                                        self.last_click = None;
                                    } else {
                                        // Single click: start selection
                                        self.output_area.start_selection_at(col, row, rect.height as usize);
                                        self.last_click = Some((now, mouse.column, mouse.row));
                                    }
                                }
                            }
                            crossterm::event::MouseEventKind::Drag(crossterm::event::MouseButton::Left) => {
                                // Update selection while dragging
                                let rect = self.output_area_rect;
                                if mouse.row >= rect.top() && mouse.row < rect.bottom()
                                    && mouse.column >= rect.left() && mouse.column < rect.right()
                                {
                                    let col = (mouse.column - rect.left()) as usize;
                                    let row = (mouse.row - rect.top()) as usize;
                                    self.output_area.update_selection_at(col, row, rect.height as usize);
                                }
                            }
                            crossterm::event::MouseEventKind::Up(crossterm::event::MouseButton::Left) => {
                                // Finish selection and copy to clipboard
                                if let Some(text) = self.output_area.end_selection() {
                                    let _ = self.copy_to_clipboard(&text);
                                    self.status_bar.set_success(&format!("Copied {} chars", text.len()));
                                }
                            }
                            _ => {}
                        }
                    }
                    Event::Key(key) if key.kind == KeyEventKind::Press => {
                        // Shift+Enter / Alt+Enter: insert newline.
                        // Handle before main match because some terminals report
                        // Shift+Enter as Enter+SHIFT, Char('\n')+SHIFT, or even just Char('\n').
                        if (key.code == KeyCode::Enter || key.code == KeyCode::Char('\n'))
                            && key.modifiers.intersects(KeyModifiers::SHIFT | KeyModifiers::ALT)
                        {
                            self.input_area.enter(true);
                        } else {
                        match (key.modifiers, key.code) {
                            (KeyModifiers::CONTROL, KeyCode::Char('c')) => {
                                if is_processing {
                                    interrupted.store(true, Ordering::Relaxed);
                                    // Cancel the active token to immediately abort API/tool calls
                                    if let Ok(guard) = active_cancel.lock() {
                                        if let Some(token) = guard.as_ref() {
                                            token.cancel();
                                        }
                                    }
                                    self.status_bar.set_warning("Interrupted");
                                } else if self.input_area.is_showing_suggestions() {
                                    // Dismiss suggestions on Ctrl+C
                                    self.input_area.clear_suggestions();
                                } else {
                                    self.should_exit = true;
                                }
                            }
                            // Tab to accept suggestion or trigger autocomplete
                            (KeyModifiers::NONE, KeyCode::Tab) if !is_processing => {
                                if self.input_area.is_showing_suggestions() {
                                    self.apply_current_suggestion();
                                } else {
                                    self.update_suggestions();
                                }
                            }
                            // Escape to dismiss suggestions
                            (KeyModifiers::NONE, KeyCode::Esc) if !is_processing => {
                                if self.input_area.is_showing_suggestions() {
                                    self.input_area.clear_suggestions();
                                }
                            }
                            (_, KeyCode::Enter) if is_processing => {
                                // Queue input while processing
                                if !self.input_area.is_empty() {
                                    let input = self.input_area.get_text();
                                    self.output_area.push_user_message(&input);
                                    self.input_area.add_history(&input);
                                    self.input_area.clear();
                                    self.queued_input = Some(input);
                                    self.status_bar.set_warning("Message queued");
                                }
                            }
                            (_, KeyCode::Enter) if !is_processing => {
                                // If suggestions are showing, accept them first
                                if self.input_area.is_showing_suggestions() {
                                    self.apply_current_suggestion();
                                } else if !self.input_area.is_empty() {
                                    let input = self.input_area.get_text();

                                    if input.starts_with('/') {
                                        self.handle_slash_command(&input);
                                        self.input_area.clear();
                                    } else {
                                        self.output_area.push_user_message(&input);
                                        // Add to history before clearing
                                        self.input_area.add_history(&input);
                                        self.input_area.clear();

                                        // Build message
                                        let images: Vec<(String, String)> = self.pending_images
                                            .drain(..)
                                            .map(|img| (img.base64, img.media_type))
                                            .collect();
                                        if images.is_empty() {
                                            self.messages.push(Message::user(&input));
                                        } else {
                                            self.messages.push(Message::user_with_images(&input, images));
                                        }

                                        // Spawn background task
                                        self.status_bar.set_processing("Thinking...");
                                        is_processing = true; // Sync with StatusBar

                                        let tx = ui_tx.clone();
                                        let client = client.clone();
                                        let registry = registry.clone();
                                        let system_blocks = system_blocks.clone();
                                        let user_context = user_context.clone();
                                        let messages = self.messages.clone();
                                        let cwd = self.cwd.clone();
                                        let read_files = read_files.clone();
                                        let agent_runner = agent_runner.clone();
                                        let interrupted = interrupted.clone();

                                        // Create a new cancel token for this run
                                        let cancel = CancellationToken::new();
                                        if let Ok(mut guard) = active_cancel.lock() {
                                            *guard = Some(cancel.clone());
                                        }

                                        tokio::spawn(async move {
                                            process_in_background(
                                                tx, client, registry, system_blocks,
                                                user_context, messages, cwd, read_files,
                                                agent_runner, allow_all, interrupted, cancel,
                                            ).await;
                                        });
                                    }
                                }
                            }
                            (KeyModifiers::NONE, KeyCode::PageUp) => {
                                self.output_area.scroll_up(10);
                            }
                            (KeyModifiers::NONE, KeyCode::PageDown) => {
                                self.output_area.scroll_down(10);
                            }
                            // Shift+Up/Down to scroll output by 1 line
                            (KeyModifiers::SHIFT, KeyCode::Up) => {
                                self.output_area.scroll_up(1);
                            }
                            (KeyModifiers::SHIFT, KeyCode::Down) => {
                                self.output_area.scroll_down(1);
                            }
                            // Home/End for scroll to top/bottom
                            (KeyModifiers::SHIFT, KeyCode::Home) => {
                                let total = self.output_area.line_count();
                                self.output_area.scroll_up(total);
                            }
                            (KeyModifiers::SHIFT, KeyCode::End) => {
                                self.output_area.scroll_to_bottom();
                            }
                            (KeyModifiers::NONE | KeyModifiers::SHIFT, KeyCode::Char(c)) => {
                                                              let ch = if key.modifiers.contains(KeyModifiers::SHIFT) {
                                                                  c.to_ascii_uppercase()
                                                              } else {
                                                                  c
                                                              };
                                                              self.input_area.input(ch);
                                                              if !is_processing {
                                                                  self.update_suggestions();
                                                              }
                                                          }
                            (KeyModifiers::NONE, KeyCode::Backspace) => {
                                self.input_area.backspace();
                                if !is_processing {
                                    self.update_suggestions();
                                }
                            }
                            (KeyModifiers::NONE, KeyCode::Left) => {
                                self.input_area.move_left();
                                self.input_area.clear_suggestions();
                            }
                            (KeyModifiers::NONE, KeyCode::Right) => {
                                self.input_area.move_right();
                                self.input_area.clear_suggestions();
                            }
                            (KeyModifiers::NONE, KeyCode::Up) => self.input_area.move_up(),
                            (KeyModifiers::NONE, KeyCode::Down) => self.input_area.move_down(),
                            (KeyModifiers::CONTROL, KeyCode::Char('a')) => self.input_area.move_home(),
                            (KeyModifiers::CONTROL, KeyCode::Char('e')) => self.input_area.move_end(),
                            (KeyModifiers::CONTROL, KeyCode::Char('w')) => self.input_area.delete_word(),
                            // Ctrl+V / Cmd+V: try paste image from clipboard (skip if we just processed a paste event)
                            (KeyModifiers::CONTROL | KeyModifiers::SUPER, KeyCode::Char('v')) if !is_processing && !self.just_pasted => {
                                  self.just_pasted = true;
                                  let tx = ui_tx.clone();
                                  // Read clipboard image in background to avoid blocking UI
                                  tokio::spawn(async move {
                                      tx.send(UiEvent::SystemMessage("[reading clipboard image...]".to_string())).await.ok();
                                      match crate::image::read_clipboard_image().await {
                                          Ok(img) => {
                                              let size = img.final_size;
                                              tx.send(UiEvent::ClipboardImage(img)).await.ok();
                                              tx.send(UiEvent::SystemMessage(format!(
                                                  "[clipboard image added ({} bytes). Type message to send.]",
                                                  size
                                              ))).await.ok();
                                          }
                                          Err(e) => {
                                              tx.send(UiEvent::SystemMessage(format!("No image in clipboard: {e}"))).await.ok();
                                          }
                                      }
                                  });
                              }
                            (KeyModifiers::NONE, KeyCode::End) => self.input_area.move_end(),
                            _ => {}
                        }
                        } // else block for Shift+Enter check
                    }
                    _ => {}
                }
            }

            // Reset paste flag after processing all events
            self.just_pasted = false;

            if self.should_exit {
                break;
            }
        }

        Ok(())
    }

    /// Copy text to system clipboard
    fn copy_to_clipboard(&self, text: &str) -> Result<(), Box<dyn std::error::Error>> {
        use std::process::Command;
        
        // Try different clipboard tools based on platform
        #[cfg(target_os = "macos")]
        {
            let mut child = Command::new("pbcopy")
                .stdin(std::process::Stdio::piped())
                .spawn()?;
            if let Some(mut stdin) = child.stdin.take() {
                use std::io::Write;
                stdin.write_all(text.as_bytes())?;
            }
            child.wait()?;
        }
        
        #[cfg(target_os = "linux")]
        {
            // Try xclip first, then xsel
            let result = Command::new("xclip")
                .args(["-selection", "clipboard"])
                .stdin(std::process::Stdio::piped())
                .spawn();
            
            if let Ok(mut child) = result {
                if let Some(mut stdin) = child.stdin.take() {
                    use std::io::Write;
                    stdin.write_all(text.as_bytes())?;
                }
                child.wait()?;
            } else {
                let mut child = Command::new("xsel")
                    .args(["--clipboard", "--input"])
                    .stdin(std::process::Stdio::piped())
                    .spawn()?;
                if let Some(mut stdin) = child.stdin.take() {
                    use std::io::Write;
                    stdin.write_all(text.as_bytes())?;
                }
                child.wait()?;
            }
        }
        
        #[cfg(target_os = "windows")]
        {
            let mut child = Command::new("clip")
                .stdin(std::process::Stdio::piped())
                .spawn()?;
            if let Some(mut stdin) = child.stdin.take() {
                use std::io::Write;
                stdin.write_all(text.as_bytes())?;
            }
            child.wait()?;
        }
        
        Ok(())
    }

    /// Accept current suggestion and apply it to input text
    fn apply_current_suggestion(&mut self) {
        if let Some(suggestion) = self.input_area.accept_suggestion() {
            let input = self.input_area.get_text();
            // Convert column (char count) to byte offset for string slicing
            let (_row, col) = self.input_area.cursor_position();
            let cursor_byte_offset = input
                .char_indices()
                .nth(col)
                .map(|(i, _)| i)
                .unwrap_or(input.len());
            let (new_input, _new_cursor) = apply_suggestion(&input, cursor_byte_offset, &suggestion);
            self.input_area.set_text(&new_input);
        }
    }

    /// Update suggestions based on current input
    fn update_suggestions(&mut self) {
        let input = self.input_area.get_text();
        let (_row, col) = self.input_area.cursor_position();
        // Convert column (char count) to byte offset
        let cursor_offset = input
            .char_indices()
            .nth(col)
            .map(|(i, _)| i)
            .unwrap_or(input.len());
        
        let ctx = SuggestionContext {
            input,
            cursor_offset,
            cwd: self.cwd.clone(),
        };
        
        let suggestions = generate_suggestions(&ctx);
        self.input_area.set_suggestions(suggestions);
    }

    fn handle_slash_command(&mut self, input: &str) {
        let parts: Vec<&str> = input.split_whitespace().collect();
        let cmd = *parts.first().unwrap_or(&"");

        match cmd {
            cmd if cmd == format!("/{}", cmd::EXIT) || cmd == format!("/{}", cmd::QUIT) => self.should_exit = true,
            cmd if cmd == format!("/{}", cmd::CLEAR) => {
                self.messages.clear();
                self.pending_images.clear();
                self.output_area.push_system("[conversation cleared]");
            }
            cmd if cmd == format!("/{}", cmd::COMPACT) => {
                use aemeath_core::compact;
                let (compacted, was_compacted) = compact::compact_messages(
                    &mut self.messages,
                    &self.system_prompt_text,
                    self.context_size,
                );
                if was_compacted {
                    let old_len = self.messages.len();
                    self.messages = compacted;
                    self.output_area.push_system(&format!(
                        "[compacted: {} → {} messages]",
                        old_len,
                        self.messages.len()
                    ));
                } else {
                    self.output_area.push_system("[no compaction needed]");
                }
            }
            cmd if cmd == format!("/{}", cmd::HELP) => {
                self.output_area.push_system("Commands:");
                self.output_area.push_system("  /help  /exit  /clear  /compact  /usage  /paste  /images  /clear-images");
                self.output_area.push_system("");
                self.output_area.push_system("Scrolling:");
                self.output_area.push_system("  Mouse wheel     - scroll 3 lines");
                self.output_area.push_system("  PageUp/PageDown - scroll 10 lines");
                self.output_area.push_system("  Shift+Up/Down   - scroll 1 line");
                self.output_area.push_system("  Shift+Home      - scroll to top");
                self.output_area.push_system("  Shift+End       - scroll to bottom");
                self.output_area.push_system("");
                self.output_area.push_system("Input:");
                self.output_area.push_system("  Enter           - send message");
                self.output_area.push_system("  Alt+Enter       - new line");
                self.output_area.push_system("  Tab             - accept suggestion");
                self.output_area.push_system("  Ctrl+C          - interrupt / exit");
                self.output_area.push_system("  Ctrl+V          - paste image from clipboard");
            }
            cmd if cmd == format!("/{}", cmd::USAGE) => {
                let total = self.total_input_tokens + self.total_output_tokens;
                self.output_area.push_system(&format!(
                    "API calls: {} | Tokens: {} in / {} out / {} total",
                    self.total_api_calls,
                    format_tokens(self.total_input_tokens),
                    format_tokens(self.total_output_tokens),
                    format_tokens(total)
                ));
            }
            "/paste" => {
                // block_in_place allows async call from non-async context in tokio runtime
                let result = tokio::task::block_in_place(|| {
                    tokio::runtime::Handle::current().block_on(crate::image::read_clipboard_image())
                });
                match result {
                    Ok(img) => {
                        let size = img.final_size;
                        self.pending_images.push(img);
                        self.input_area.set_pending_images(self.pending_images.len());
                        self.output_area.push_system(&format!(
                            "[clipboard image added ({} bytes)]",
                            size
                        ));
                    }
                    Err(e) => {
                        self.output_area.push_error(&format!("Failed to read clipboard: {e}"));
                    }
                }
            }
            "/images" => {
                if self.pending_images.is_empty() {
                    self.output_area.push_system("No pending images.");
                } else {
                    self.output_area.push_system(&format!("Pending images: {}", self.pending_images.len()));
                    for (i, img) in self.pending_images.iter().enumerate() {
                        self.output_area.push_system(&format!("  {}. [image {}] ({} bytes)", i + 1, i + 1, img.final_size));
                    }
                }
            }
            "/clear-images" => {
                self.pending_images.clear();
                self.input_area.set_pending_images(0);
                self.output_area.push_system("[pending images cleared]");
            }
            _ => {
                self.output_area.push_error(&format!("Unknown command: {cmd}"));
            }
        }
    }
}

/// Background task: runs the agent loop and sends UI events via channel
async fn process_in_background(
    tx: mpsc::Sender<UiEvent>,
    client: Arc<aemeath_llm::client::LlmClient>,
    registry: Arc<ToolRegistry>,
    system_blocks: Vec<aemeath_llm::types::SystemBlock>,
    user_context: String,
    mut messages: Vec<Message>,
    cwd: PathBuf,
    read_files: Arc<std::sync::Mutex<std::collections::HashSet<String>>>,
    agent_runner: Option<Arc<dyn aemeath_core::tool::AgentRunner>>,
    allow_all: bool,
    interrupted: Arc<AtomicBool>,
    cancel: CancellationToken,
) {
    let tool_schemas = registry.schemas();

    let ctx = ToolContext {
        cwd,
        cancel: cancel.clone(),
        read_files,
        agent_runner,
        plan_mode: None,
        allow_all,
    };
    let agent = Agent {
        registry: &registry,
        ctx,
    };

    const MAX_TURNS: usize = 100;

    // Remember message count at start — on cancel, truncate back to this point
    let messages_at_start = messages.len();

    for _ in 0..MAX_TURNS {
        if interrupted.load(Ordering::Relaxed) {
            interrupted.store(false, Ordering::Relaxed);
            messages.truncate(messages_at_start);
            let _ = tx.send(UiEvent::MessagesSync(messages)).await;
            let _ = tx.send(UiEvent::Cancelled).await;
            let _ = tx.send(UiEvent::Done).await;
            return;
        }

        // Stream handler that sends events to UI
        struct TuiStreamHandler {
            tx: mpsc::Sender<UiEvent>,
        }
        impl StreamHandler for TuiStreamHandler {
            fn on_text(&mut self, text: &str) {
                let _ = self.tx.try_send(UiEvent::Text(text.to_string()));
            }
            fn on_tool_use_start(&mut self, name: &str) {
                let _ = self.tx.try_send(UiEvent::ToolCallStart(name.to_string()));
            }
            fn on_error(&mut self, error: &str) {
                let _ = self.tx.try_send(UiEvent::Error(error.to_string()));
            }
            fn on_text_block_complete(&mut self, text: &str) {
                let _ = self.tx.try_send(UiEvent::TextBlockComplete(text.to_string()));
            }
        }

        // Prepend CLAUDE.md user context for the API call
        let messages_for_api: Vec<Message> = {
            let mut api_msgs = Vec::new();
            if !user_context.is_empty() {
                api_msgs.push(Message::user(format!(
                    "<system-reminder>\nAs you answer the user's questions, you can use the following context:\n# claudeMd\n{user_context}\n\nIMPORTANT: this context may or may not be relevant to your tasks. You should not respond to this context unless it is highly relevant to your task.\n</system-reminder>"
                )));
            }
            api_msgs.extend(messages.iter().cloned());
            api_msgs
        };

        let mut handler = TuiStreamHandler { tx: tx.clone() };
        let response = client
            .stream_message(&system_blocks, &messages_for_api, &tool_schemas, &mut handler, &cancel)
            .await;

        match response {
            Ok(resp) => {
                let _ = tx.send(UiEvent::Usage {
                    input: resp.usage.input_tokens,
                    output: resp.usage.output_tokens,
                }).await;

                messages.push(resp.assistant_message.clone());

                let tool_calls = Agent::extract_tool_calls(&resp.assistant_message);
                if tool_calls.is_empty() || resp.stop_reason == StopReason::EndTurn {
                    break;
                }

                // Execute tools
                for call in &tool_calls {
                    let _ = tx.send(UiEvent::ToolCall {
                        name: call.name.clone(),
                        summary: call.input.to_string(),
                    }).await;
                }

                {
                    // Filter out non-safe tools if allow_all is not set
                    let (approved, denied): (Vec<_>, Vec<_>) = if allow_all {
                        (tool_calls.iter().collect(), vec![])
                    } else {
                        tool_calls.iter().partition(|call| {
                            if call.name == "Bash" {
                                call.input.get("command")
                                    .and_then(|v| v.as_str())
                                    .map(aemeath_tools::bash::is_readonly_command)
                                    .unwrap_or(false)
                            } else {
                                registry.get(&call.name)
                                    .map(|t| t.is_read_only())
                                    .unwrap_or(false)
                            }
                        })
                    };

                    // Report denied tools
                    for call in &denied {
                        let _ = tx.send(UiEvent::ToolResult {
                            tool_name: call.name.clone(),
                            output: format!("Tool {} denied: use --allow-all to permit write operations", call.name),
                            is_error: true,
                            images: Vec::new(),
                        }).await;
                    }

                    let approved_calls: Vec<aemeath_core::agent::ToolCall> = approved.into_iter().map(|c| {
                        aemeath_core::agent::ToolCall { id: c.id.clone(), name: c.name.clone(), input: c.input.clone() }
                    }).collect();
                    let results = agent.execute_tools(&approved_calls).await;
                    // Build a map from tool_call id to name for correct indexing
                    let tool_name_map: std::collections::HashMap<&str, &str> = tool_calls
                        .iter()
                        .map(|c| (c.id.as_str(), c.name.as_str()))
                        .collect();
                    for (_id, output, is_error, images) in results.iter() {
                        let tool_name = tool_name_map.get(_id.as_str()).unwrap_or(&"Unknown");
                        let _ = tx.send(UiEvent::ToolResult {
                            tool_name: tool_name.to_string(),
                            output: output.clone(),
                            is_error: *is_error,
                            images: images.clone(),
                        }).await;
                    }
                    // Build combined results (approved + denied), preserving images for approved tools
                    let mut all_results: Vec<(String, String, bool, Vec<ImageData>)> = results
                        .into_iter()
                        .map(|(id, output, is_error, images)| (id, output, is_error, images))
                        .collect();
                    for call in &denied {
                        all_results.push((
                            call.id.clone(),
                            format!("Tool {} denied: requires --allow-all", call.name),
                            true,
                            Vec::new(),
                        ));
                    }
                    messages.push(Message::tool_results_rich(all_results));
                }
            }
            Err(e) => {
                let msg = format!("{e}");
                if msg.contains("interrupted by user") {
                    // Truncate back to the state before this turn started.
                    // This removes the user message, any partial assistant reply,
                    // and any tool results — maintaining clean message alternation.
                    messages.truncate(messages_at_start);
                    let _ = tx.send(UiEvent::MessagesSync(messages)).await;
                    let _ = tx.send(UiEvent::Cancelled).await;
                    let _ = tx.send(UiEvent::Done).await;
                    return; // Early return, skip the final MessagesSync below
                } else {
                    let _ = tx.send(UiEvent::Error(format!("API error: {e}"))).await;
                }
                break;
            }
        }
    }

    // Sync messages back to main thread before signaling done
    let _ = tx.send(UiEvent::MessagesSync(messages)).await;
    let _ = tx.send(UiEvent::Done).await;
}
