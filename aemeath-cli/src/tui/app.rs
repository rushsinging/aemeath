use crate::image::{ProcessedImage, is_image_file, process_image_file};
use crate::tui::completion::{SuggestionContext, generate_suggestions, apply_suggestion};
use crate::tui::dialog::Dialog;
use super::{InputArea, OutputArea, StatusBar};
use super::output_area::{LineStyle, OutputLine};
use aemeath_core::agent::Agent;
use aemeath_core::command::{cmd, CommandContext, CommandRegistry, CommandResult};
use aemeath_core::cost::format_tokens;
use aemeath_core::message::Message;
use aemeath_core::session;
use aemeath_core::state::AppState;
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
    /// Reasoning/thinking content — displayed in a dimmed style
    Thinking(String),
    TextBlockComplete(String),
    ToolCallStart(String),
    ToolCall { name: String, summary: String },
    ToolResult { tool_name: String, output: String, is_error: bool, images: Vec<ImageData> },
    Usage { input: u32, output: u32, last_input: u32 },
    Error(String),
    Cancelled,
    /// Update status bar processing message directly
    StatusUpdate(String),
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
    session_id: String,
    total_input_tokens: u64,
    total_output_tokens: u64,
    total_api_calls: u64,
    /// Last API call's input_tokens (current context size)
    last_input_tokens: u64,
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
    /// Current LLM client (replaceable via /model command)
    client: Option<Arc<aemeath_llm::client::LlmClient>>,
    /// Models configuration for /model command
    models_config: aemeath_core::config::ModelsConfig,
    /// Original created_at timestamp (preserved across resume)
    session_created_at: Option<String>,
    /// Active modal dialog (e.g. model selection)
    active_dialog: Option<Dialog>,
    /// Model options for dialog selection (provider/name keys)
    dialog_model_keys: Vec<String>,
    /// Current model display name (provider/name) for UI
    current_model_display: String,
    /// Time of last Ctrl+C in idle state (for double-press-to-exit)
    last_ctrlc: Option<std::time::Instant>,
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
            session_id,
            total_input_tokens: 0,
            total_output_tokens: 0,
            total_api_calls: 0,
            last_input_tokens: 0,
            should_exit: false,
            pending_images: Vec::new(),
            output_area_rect: Rect::default(),
            just_pasted: false,
            queued_input: None,
            last_click: None,
            system_prompt_text: String::new(),
            context_size: 200_000,
            client: None,
            models_config: aemeath_core::config::ModelsConfig::default(),
            session_created_at: None,
            active_dialog: None,
            dialog_model_keys: Vec::new(),
            current_model_display: model,
            last_ctrlc: None,
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
        resume_id: Option<String>,
        task_store: Arc<aemeath_core::task::TaskStore>,
        max_tool_concurrency: usize,
        agent_semaphore: Arc<tokio::sync::Semaphore>,
    ) -> io::Result<()> {
        // Store client and config for runtime switching
        self.client = Some(client.clone());
        self.system_prompt_text = system_prompt_text.clone();
        self.context_size = context_size;
        self.status_bar.set_context_size(context_size as u64);

        // Resume existing session if requested
        if let Some(ref id) = resume_id {
            match aemeath_core::session::load_session(id).await {
                Ok(s) => {
                    let msg_count = s.messages.len();
                    self.session_created_at = Some(s.created_at);
                    self.messages = s.messages;
                    self.output_area.push_system(&format!(
                        "[resumed session {} ({} messages)]",
                        id, msg_count
                    ));
                }
                Err(e) => {
                    self.output_area.push_system(&format!(
                        "[warning: failed to resume session {}: {}, starting new]",
                        id, e
                    ));
                }
            }
        }

        // Load models config from config files
        let config_paths = [
            dirs::home_dir().map(|h| h.join(".aemeath").join("config.json")).unwrap_or_default(),
            std::path::PathBuf::from(".aemeath/config.json"),
        ];
        for path in &config_paths {
            if path.exists() {
                if let Ok(content) = std::fs::read_to_string(path) {
                    if let Ok(config) = serde_json::from_str::<aemeath_core::config::Config>(&content) {
                        if !config.models.providers.is_empty() {
                            self.models_config = config.models;
                            break;
                        }
                    }
                }
            }
        }

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
            task_store,
            max_tool_concurrency,
            agent_semaphore,
        ).await;

        // Auto-save session on exit
        if !self.messages.is_empty() {
            use aemeath_core::session::{self, Session, now_iso};
            let s = Session {
                id: self.session_id.clone(),
                cwd: self.cwd.to_string_lossy().to_string(),
                messages: self.messages.clone(),
                created_at: self.session_created_at.clone().unwrap_or_else(now_iso),
                updated_at: now_iso(),
                metadata: Default::default(),
            };
            if let Err(e) = session::save_session(&s).await {
                log::warn!("failed to auto-save session: {e}");
            }
        }

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
        system_prompt_text: String,
        user_context: String,
        context_size: usize,
        _verbose: bool,
        _use_markdown: bool,
        agent_runner: Option<Arc<dyn aemeath_core::tool::AgentRunner>>,
        allow_all: bool,
        interrupted: Arc<AtomicBool>,
        task_store: Arc<aemeath_core::task::TaskStore>,
        max_tool_concurrency: usize,
        agent_semaphore: Arc<tokio::sync::Semaphore>,
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
            // Update task status lines for display below spinner
            {
                let tasks = task_store.list().await;
                let active: Vec<_> = tasks.iter()
                    .filter(|t| t.status != aemeath_core::task::TaskStatus::Deleted)
                    .collect();
                if active.is_empty() {
                    self.output_area.set_task_status(Vec::new());
                } else {
                    let completed = active.iter().filter(|t| t.status == aemeath_core::task::TaskStatus::Completed).count();
                    let total = active.len();
                    let mut lines = vec![format!("━━ Tasks: {}/{} ━━", completed, total)];
                    for t in &active {
                        let icon = match t.status {
                            aemeath_core::task::TaskStatus::Completed => "✓",
                            aemeath_core::task::TaskStatus::InProgress => "■",
                            aemeath_core::task::TaskStatus::Pending => "□",
                            _ => continue,
                        };
                        let owner = t.owner.as_deref().map(|o| format!(" (@{})", o)).unwrap_or_default();
                        lines.push(format!("{} #{} {}{}", icon, t.id, t.subject, owner));
                    }
                    self.output_area.set_task_status(lines);
                }
            }

            // Draw UI
            let mut output_rect = Rect::default();
            terminal.draw(|f| {
                let size = f.area();

                // Skip rendering if terminal is too small to avoid buffer panics
                if size.height < 8 || size.width < 20 {
                    return;
                }

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
                self.status_bar.set_tokens(self.total_input_tokens, self.total_output_tokens, self.last_input_tokens);
                self.status_bar.set_api_calls(self.total_api_calls);
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    self.status_bar.render(chunks[3], buf);
                }));

                // Render modal dialog on top if active
                if let Some(ref dialog) = self.active_dialog {
                    dialog.render(size, buf);
                }
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
                            UiEvent::Text(text) => {
                                self.output_area.stop_spinner();
                                self.output_area.append_assistant_text(&text);
                            }
                            UiEvent::Thinking(text) => {
                                self.output_area.stop_spinner();
                                self.output_area.append_thinking_text(&text);
                            }
                            UiEvent::TextBlockComplete(_text) => {
                                self.output_area.finish_streaming();
                                self.output_area.push_system("");
                            }
                            UiEvent::ToolCallStart(name) => {
                                self.output_area.start_spinner();
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
                            UiEvent::Usage { input, output, last_input } => {
                                self.total_input_tokens += input as u64;
                                self.total_output_tokens += output as u64;
                                self.total_api_calls += 1;
                                self.last_input_tokens = last_input as u64;
                            }
                            UiEvent::StatusUpdate(msg) => {
                                self.status_bar.set_processing(&msg);
                                // Any status update means we're still working — keep the
                                // spinner turning. Text/Thinking handlers stop it while
                                // content is actively streaming to avoid overlap.
                                self.output_area.start_spinner();
                            }
                            UiEvent::Error(msg) => {
                                self.output_area.push_error(&msg);
                                self.output_area.stop_spinner();
                                is_processing = false;
                                self.status_bar.clear_processing();
                            }
                            UiEvent::Cancelled => {
                                self.output_area.push_cancelled();
                                self.output_area.stop_spinner();
                                is_processing = false;
                                self.status_bar.clear_processing();
                            }
                            UiEvent::MessagesSync(msgs) => {
                                self.messages = msgs;
                                // Auto-save session on every sync so that /save and exit always have up-to-date data
                                if !self.messages.is_empty() {
                                    use aemeath_core::session::{self as sess, Session, now_iso};
                                    let s = Session {
                                        id: self.session_id.clone(),
                                        cwd: self.cwd.to_string_lossy().to_string(),
                                        messages: self.messages.clone(),
                                        created_at: self.session_created_at.clone().unwrap_or_else(now_iso),
                                        updated_at: now_iso(),
                                        metadata: Default::default(),
                                    };
                                    if let Err(e) = sess::save_session(&s).await {
                                        log::warn!("failed to auto-save session on sync: {e}");
                                    }
                                }
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
                                self.output_area.stop_spinner();
                                is_processing = false;
                                self.status_bar.clear_processing();
                                self.status_bar.set_success("Ready");

                                // Process queued input immediately
                                if let Some(queued) = self.queued_input.take() {
                                    // Ensure interrupted flag is clear before starting a new run
                                    interrupted.store(false, Ordering::Relaxed);

                                    self.messages.push(Message::user(&queued));
                                    self.status_bar.set_processing("Thinking...");
                                    self.output_area.start_spinner();
                                    is_processing = true; // Sync with StatusBar

                                    let tx = ui_tx.clone();
                                    let client = self.client.as_ref().unwrap_or(&client).clone();
                                    let registry = registry.clone();
                                    let system_blocks = system_blocks.clone();
                                    let system_prompt_text = system_prompt_text.clone();
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
                                    let sid = self.session_id.clone();
                                    let task_store = task_store.clone();
                                    let agent_sem = agent_semaphore.clone();
                                    tokio::spawn(async move {
                                        process_in_background(
                                            tx, client, registry, system_blocks,
                                            system_prompt_text, user_context, messages,
                                            context_size, cwd, sid, read_files,
                                            agent_runner, allow_all, interrupted, cancel,
                                            task_store,
                                            max_tool_concurrency, agent_sem,
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
                        // If a dialog is active, handle dialog keys first
                        if self.active_dialog.is_some() {
                            match key.code {
                                KeyCode::Up => {
                                    if let Some(ref mut d) = self.active_dialog { d.select_prev(); }
                                }
                                KeyCode::Down => {
                                    if let Some(ref mut d) = self.active_dialog { d.select_next(); }
                                }
                                KeyCode::Enter => {
                                    let selected = self.active_dialog.as_ref()
                                        .and_then(|d| d.get_selected());
                                    if let Some(idx) = selected {
                                        if idx < self.dialog_model_keys.len() {
                                            let model_key = self.dialog_model_keys[idx].clone();
                                            // Execute model switch
                                            let _ = self.handle_slash_command_str(&format!("/model {}", model_key)).await;
                                        }
                                    }
                                    self.active_dialog = None;
                                    self.dialog_model_keys.clear();
                                }
                                KeyCode::Esc => {
                                    self.active_dialog = None;
                                    self.dialog_model_keys.clear();
                                }
                                _ => {}
                            }
                            continue;
                        }

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
                                    let now = std::time::Instant::now();
                                    if let Some(last) = self.last_ctrlc {
                                        if now.duration_since(last).as_secs_f64() < 3.0 {
                                            self.should_exit = true;
                                        } else {
                                            self.last_ctrlc = Some(now);
                                            self.status_bar.set_warning("Press Ctrl+C again to exit");
                                        }
                                    } else {
                                        self.last_ctrlc = Some(now);
                                        self.status_bar.set_warning("Press Ctrl+C again to exit");
                                    }
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
                                        let review_prompt = self.handle_slash_command(&input).await;
                                        self.input_area.clear();

                                        // If the command returned a review prompt, send it to the LLM
                                        if let Some(prompt) = review_prompt {
                                            self.messages.push(Message::user(&prompt));

                                            interrupted.store(false, Ordering::Relaxed);
                                            self.status_bar.set_processing("Thinking...");
                                            self.output_area.start_spinner();
                                            is_processing = true;

                                            let tx = ui_tx.clone();
                                            let client = self.client.as_ref().unwrap_or(&client).clone();
                                            let registry = registry.clone();
                                            let system_blocks = system_blocks.clone();
                                            let system_prompt_text = system_prompt_text.clone();
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
                                            let sid = self.session_id.clone();
                                            let task_store = task_store.clone();
                                            let agent_sem = agent_semaphore.clone();
                                            tokio::spawn(async move {
                                                process_in_background(
                                                    tx, client, registry, system_blocks,
                                                    system_prompt_text, user_context, messages,
                                                    context_size, cwd, sid, read_files,
                                                    agent_runner, allow_all, interrupted, cancel,
                                                    task_store,
                                                    max_tool_concurrency, agent_sem,
                                                ).await;
                                            });
                                        }
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
                                        // Ensure interrupted flag is clear before starting a new run
                                        interrupted.store(false, Ordering::Relaxed);

                                        self.status_bar.set_processing("Thinking...");
                                        self.output_area.start_spinner();
                                        is_processing = true; // Sync with StatusBar

                                        let tx = ui_tx.clone();
                                        let client = self.client.as_ref().unwrap_or(&client).clone();
                                        let registry = registry.clone();
                                        let system_blocks = system_blocks.clone();
                                        let system_prompt_text = system_prompt_text.clone();
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

                                        let sid = self.session_id.clone();
                                        let task_store = task_store.clone();
                                        let agent_sem = agent_semaphore.clone();
                                        tokio::spawn(async move {
                                            process_in_background(
                                                tx, client, registry, system_blocks,
                                                system_prompt_text, user_context, messages,
                                                context_size, cwd, sid, read_files,
                                                agent_runner, allow_all, interrupted, cancel,
                                                task_store,
                                                max_tool_concurrency, agent_sem,
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
        
        let models: Vec<(String, String)> = self.models_config.list_models()
            .into_iter()
            .map(|(p, m)| (p, if m.name.is_empty() { m.id } else { m.name }))
            .collect();
        
        let ctx = SuggestionContext {
            input,
            cursor_offset,
            cwd: self.cwd.clone(),
            models,
        };
        
        let suggestions = generate_suggestions(&ctx);
        self.input_area.set_suggestions(suggestions);
    }

    /// Called from dialog Enter handler to dispatch /model switch
    async fn handle_slash_command_str(&mut self, input: &str) -> Option<String> {
        self.handle_slash_command(input).await
    }

    /// Handle slash commands. Returns Some(prompt) if a message should be sent to the LLM (e.g. /review).
    async fn handle_slash_command(&mut self, input: &str) -> Option<String> {
        let parts: Vec<&str> = input.split_whitespace().collect();
        let cmd = *parts.first().unwrap_or(&"");
        let has_args = parts.len() > 1;

        // /model with no args → open selection dialog
        if cmd == "/model" && !has_args {
            let models = self.models_config.list_models();
            if models.is_empty() {
                self.output_area.push_system("No models configured. Add models to ~/.aemeath/config.json");
                return None;
            }
            let current = self.current_model_display.clone();
            let mut options = Vec::new();
            let mut keys = Vec::new();
            for (provider_name, model) in &models {
                let display_name = if model.name.is_empty() { &model.id } else { &model.name };
                let key = format!("{}/{}", provider_name, display_name);
                let marker = if key == current { " ←" } else { "" };
                options.push(format!(
                    "{}/{} ctx:{}k{}", provider_name, display_name,
                    model.context_window / 1000, marker,
                ));
                keys.push(key);
            }
            self.active_dialog = Some(Dialog::select("Select Model", options));
            self.dialog_model_keys = keys;
            return None;
        }

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
                self.output_area.push_system("  /help  /exit  /clear  /compact  /usage  /save  /sessions");
                self.output_area.push_system("  /paste  /images  /clear-images  /context  /review");
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
            "/sessions" => {
                let sessions = session::list_sessions().await;
                if sessions.is_empty() {
                    self.output_area.push_system("No saved sessions.");
                } else {
                    self.output_area.push_system(&format!("Saved sessions ({}):", sessions.len()));
                    for (i, s) in sessions.iter().take(10).enumerate() {
                        self.output_area.push_system(&format!(
                            "  {}. {} ({} msgs, {})",
                            i + 1,
                            s.id,
                            s.messages.len(),
                            s.updated_at
                        ));
                    }
                    self.output_area.push_system("");
                    self.output_area.push_system("Resume with: aemeath --resume <session-id>");
                }
            }
            "/save" => {
                use aemeath_core::session::{Session, now_iso};
                let s = Session {
                    id: self.session_id.clone(),
                    cwd: self.cwd.to_string_lossy().to_string(),
                    messages: self.messages.clone(),
                    created_at: self.session_created_at.clone().unwrap_or_else(now_iso),
                    updated_at: now_iso(),
                    metadata: Default::default(),
                };
                match session::save_session(&s).await {
                    Ok(()) => self.output_area.push_system(&format!("[session saved: {}]", self.session_id)),
                    Err(e) => self.output_area.push_error(&format!("Failed to save session: {e}")),
                }
            }
            "/context" => {
                use aemeath_core::compact;
                let estimated = compact::estimate_messages_tokens(&self.messages)
                    + compact::estimate_tokens(&self.system_prompt_text);
                let pct = estimated * 100 / self.context_size.max(1);
                self.output_area.push_system(&format!(
                    "Context window: ~{} / {} tokens ({}%)",
                    estimated,
                    self.context_size,
                    pct
                ));
                self.output_area.push_system(&format!("Messages: {}", self.messages.len()));
                if pct > 80 {
                    self.output_area.push_system("[auto-compaction will trigger at 80%]");
                }
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
            // Try to execute via CommandRegistry
            _ => {
                let cmd_name = cmd.trim_start_matches('/');
                let args = parts.get(1..).map(|p| p.join(" ")).unwrap_or_default();

                // Try to find command in registry
                let registry = CommandRegistry::with_defaults();
                if let Some(cmd_obj) = registry.find(cmd_name) {
                    // Create minimal context for command execution
                    let state = AppState::default();
                    let config = aemeath_core::config::Config::default();
                    let mut ctx = CommandContext::new(
                        Arc::new(state),
                        config,
                        self.cwd.to_string_lossy().to_string(),
                        self.session_id.clone(),
                    );
                    ctx.models_config = self.models_config.clone();
                    ctx.current_model = self.current_model_display.clone();

                    match cmd_obj.execute(&args, &mut ctx).await {
                        CommandResult::Success(msg) => self.output_area.push_system(&msg),
                        CommandResult::Error(msg) => self.output_area.push_error(&msg),
                        CommandResult::Action(action) => {
                            match action {
                                aemeath_core::command::CommandAction::Exit => self.should_exit = true,
                                aemeath_core::command::CommandAction::Clear => {
                                    self.messages.clear();
                                    self.output_area.push_system("[cleared]");
                                }
                                aemeath_core::command::CommandAction::Compact => {
                                    use aemeath_core::compact;
                                    let (compacted, was_compacted) = compact::compact_messages(
                                        &mut self.messages,
                                        &self.system_prompt_text,
                                        self.context_size,
                                    );
                                    if was_compacted {
                                        self.messages = compacted;
                                        self.output_area.push_system("[compacted]");
                                    } else {
                                        self.output_area.push_system("[no compaction needed]");
                                    }
                                }
                                aemeath_core::command::CommandAction::SwitchModel {
                                    provider_name, model_id, model_name, base_url, api_key, api_type, max_tokens, context_window, reasoning,
                                } => {
                                    // Determine provider type from api_type and provider_name
                                    let provider = match api_type.as_str() {
                                        "anthropic" => aemeath_core::provider::Provider::Anthropic,
                                        "ollama" => aemeath_core::provider::Provider::Ollama,
                                        _ => {
                                            // Try to match known providers by name for correct URL suffix
                                            aemeath_core::provider::Provider::from_str(&provider_name)
                                                .unwrap_or(aemeath_core::provider::Provider::OpenAICompatible)
                                        }
                                    };

                                    let new_client = aemeath_llm::client::LlmClient::with_provider(
                                        provider,
                                        api_key,
                                        Some(base_url),
                                        Some(model_id.clone()),
                                        max_tokens,
                                        reasoning,
                                    );

                                    self.client = Some(Arc::new(new_client));
                                    if context_window > 0 {
                                        self.context_size = context_window;
                                        self.status_bar.set_context_size(context_window as u64);
                                    }
                                    let display_name = if model_name.is_empty() { &model_id } else { &model_name };
                                    let display = format!("{}/{}", provider_name, display_name);
                                    self.current_model_display = display.clone();
                                    self.status_bar.set_model(&display);
                                    self.output_area.push_system(&format!("[switched to {}]", display));
                                }
                                aemeath_core::command::CommandAction::Review(prompt) => {
                                    self.output_area.push_system("[reviewing code changes...]");
                                    return Some(prompt);
                                }
                                _ => self.output_area.push_system(&format!("[action: {:?}]", action)),
                            }
                        }
                        CommandResult::Confirm { message, .. } => {
                            self.output_area.push_system(&format!("[confirm: {}]", message));
                        }
                    }
                } else {
                    self.output_area.push_error(&format!("Unknown command: {cmd}"));
                }
            }
        }
        None
    }
}

/// Background task: runs the agent loop and sends UI events via channel
#[allow(unused_assignments)]
async fn process_in_background(
    tx: mpsc::Sender<UiEvent>,
    client: Arc<aemeath_llm::client::LlmClient>,
    registry: Arc<ToolRegistry>,
    system_blocks: Vec<aemeath_llm::types::SystemBlock>,
    system_prompt_text: String,
    user_context: String,
    mut messages: Vec<Message>,
    context_size: usize,
    cwd: PathBuf,
    session_id: String,
    read_files: Arc<std::sync::Mutex<std::collections::HashSet<String>>>,
    agent_runner: Option<Arc<dyn aemeath_core::tool::AgentRunner>>,
    allow_all: bool,
    interrupted: Arc<AtomicBool>,
    cancel: CancellationToken,
    _task_store: Arc<aemeath_core::task::TaskStore>,
    max_tool_concurrency: usize,
    agent_semaphore: Arc<tokio::sync::Semaphore>,
) {
    let tool_schemas = registry.schemas();
    // Pre-compute fixed token overhead from tool schemas (sent with every API call)
    let tool_schema_tokens = aemeath_core::compact::estimate_tool_schemas_tokens(&tool_schemas);

    let ctx = ToolContext {
        cwd: cwd.clone(),
        cancel: cancel.clone(),
        read_files: read_files.clone(),
        agent_runner: agent_runner.clone(),
        plan_mode: None,
        allow_all,
        max_tool_concurrency,
        max_agent_concurrency: 0,
        agent_semaphore,
    };
    let agent = Agent {
        registry: &registry,
        ctx,
    };

    const MAX_TURNS: usize = 100;

    // Remember message count at start — on cancel, truncate back to this point
    let messages_at_start = messages.len();

    #[allow(unused_assignments)]
    let mut last_api_input_tokens: u64 = 0;

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
                // Use SystemMessage for non-fatal warnings from the LLM layer
                // (streaming retries, fallbacks, idle timeouts).
                // UiEvent::Error would stop processing and reset status bar.
                let _ = self.tx.try_send(UiEvent::SystemMessage(format!("[warn] {}", error)));
            }
            fn on_text_block_complete(&mut self, text: &str) {
                let _ = self.tx.try_send(UiEvent::TextBlockComplete(text.to_string()));
            }
            fn on_thinking(&mut self, text: &str) {
                let _ = self.tx.try_send(UiEvent::Thinking(text.to_string()));
            }
        }

        // Auto-compact if approaching context limit
        // Use actual API token count when available, fall back to estimation (including tool schema overhead)
        {
            use aemeath_core::compact;
            let should_compact = if last_api_input_tokens > 0 {
                compact::needs_compaction_actual(last_api_input_tokens, 0, context_size)
            } else {
                compact::needs_compaction_full(&messages, &system_prompt_text, context_size, tool_schema_tokens)
            };
            if should_compact && messages.len() > 4 {
                let old_len = messages.len();
                compact::microcompact(&mut messages, 10);
                // Re-check after microcompact
                if compact::needs_compaction_full(&messages, &system_prompt_text, context_size, tool_schema_tokens)
                    || (last_api_input_tokens > 0 && compact::needs_compaction_actual(last_api_input_tokens, 0, context_size))
                {
                    let (compacted, was_compacted) = compact::compact_messages(&messages, &system_prompt_text, context_size);
                    if was_compacted {
                        messages = compacted;
                        last_api_input_tokens = 0;
                        let _ = tx.send(UiEvent::SystemMessage(
                            format!("[auto-compacted: {} → {} messages]", old_len, messages.len()),
                        )).await;
                    }
                }
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
                last_api_input_tokens = resp.usage.input_tokens as u64;
                let _ = tx.send(UiEvent::Usage {
                    input: resp.usage.input_tokens,
                    output: resp.usage.output_tokens,
                    last_input: resp.usage.input_tokens,
                }).await;

                messages.push(resp.assistant_message.clone());

                // Sync messages to main thread after every assistant response (real-time persistence)
                let _ = tx.send(UiEvent::MessagesSync(messages.clone())).await;

                let tool_calls = Agent::extract_tool_calls(&resp.assistant_message);
                if tool_calls.is_empty() || resp.stop_reason == StopReason::EndTurn {
                    break;
                }

                // Show tool calls before execution
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

                    // Show elapsed time and poll task status changes while tools are running
                    let tool_names: Vec<String> = approved_calls.iter().map(|c| c.name.clone()).collect();
                    let has_long_running = tool_names.iter().any(|n| n == "Agent");
                    let has_active_tasks = {
                        let tasks = _task_store.list().await;
                        tasks.iter().any(|t| t.status == aemeath_core::task::TaskStatus::Pending
                            || t.status == aemeath_core::task::TaskStatus::InProgress)
                    };
                    let should_poll = has_long_running || has_active_tasks;
                    let timer_tx = tx.clone();
                    let timer_cancel = cancel.clone();
                    let timer_store = _task_store.clone();
                    let timer_handle = if should_poll {
                        let names = tool_names.clone();
                        Some(tokio::spawn(async move {
                            use aemeath_core::task::TaskStatus;
                            let start = std::time::Instant::now();
                            let mut last_statuses: std::collections::HashMap<String, TaskStatus> =
                                std::collections::HashMap::new();
                            for t in timer_store.list().await {
                                last_statuses.insert(t.id.clone(), t.status.clone());
                            }
                            loop {
                                tokio::select! {
                                    _ = timer_cancel.cancelled() => break,
                                    _ = tokio::time::sleep(std::time::Duration::from_secs(2)) => {
                                        let elapsed = start.elapsed().as_secs();
                                        // Poll task status changes
                                        let current_tasks = timer_store.list().await;
                                        for t in &current_tasks {
                                            let prev = last_statuses.get(&t.id);
                                            let changed = match prev {
                                                Some(prev_status) => *prev_status != t.status,
                                                None => t.status == TaskStatus::InProgress || t.status == TaskStatus::Completed,
                                            };
                                            if changed {
                                                let msg = crate::tui::task_display::format_task_change(t);
                                                if !msg.is_empty() {
                                                    let _ = timer_tx.send(UiEvent::SystemMessage(msg)).await;
                                                }
                                                last_statuses.insert(t.id.clone(), t.status.clone());
                                            }
                                        }
                                        let _ = timer_tx.send(UiEvent::StatusUpdate(
                                            format!("Running {} tool(s)... {}s", names.len(), elapsed)
                                        )).await;
                                    }
                                }
                            }
                        }))
                    } else {
                        None
                    };

                    let results = agent.execute_tools(&approved_calls).await;

                    // Stop the timer
                    if let Some(handle) = timer_handle {
                        handle.abort();
                    }
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
                    // Insert task snapshot if TaskCreate or TaskUpdate(completed) was called
                    {
                        let has_task_create = tool_name_map.values().any(|n| *n == "TaskCreate");
                        let has_task_update_completed = tool_name_map.values().any(|n| *n == "TaskUpdate")
                            && results.iter().any(|(_, output, is_err, _)| !is_err && output.contains("Completed"));

                        if has_task_create || has_task_update_completed {
                            let tasks = _task_store.list().await;
                            let snapshot = crate::tui::task_display::format_task_snapshot(&tasks);
                            if !snapshot.is_empty() {
                                let _ = tx.send(UiEvent::SystemMessage(snapshot)).await;
                            }
                        }
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

                    // Persist oversized tool results to disk, replace with preview reference
                    let persisted = aemeath_core::tool_result_storage::persist_oversized_results(
                        &session_id, &mut all_results,
                    );
                    if persisted > 0 {
                        let _ = tx.send(UiEvent::SystemMessage(
                            format!("[{persisted} tool result(s) persisted to disk]"),
                        )).await;
                    }

                    messages.push(Message::tool_results_rich(all_results));

                    // Sync messages after tool results (real-time persistence)
                    let _ = tx.send(UiEvent::MessagesSync(messages.clone())).await;
                }

                // Inner-loop auto-compact: use actual API token count for accurate decision
                {
                    use aemeath_core::compact;
                    let urgency = if last_api_input_tokens > 0 {
                        let new_tokens = messages.last()
                            .map(|m| compact::estimate_messages_tokens(std::slice::from_ref(m)))
                            .unwrap_or(0) as u64;
                        compact::compaction_urgency(last_api_input_tokens + new_tokens, context_size)
                    } else if compact::needs_compaction_full(&messages, &system_prompt_text, context_size, tool_schema_tokens) {
                        2
                    } else {
                        0
                    };

                    if urgency >= 1 && messages.len() > 4 {
                        let old_len = messages.len();
                        compact::microcompact(&mut messages, 6);
                        if urgency >= 2 {
                            let (compacted, was_compacted) = compact::compact_messages(
                                &messages, &system_prompt_text, context_size,
                            );
                            if was_compacted {
                                messages = compacted;
                                last_api_input_tokens = 0;
                                let _ = tx.send(UiEvent::SystemMessage(
                                    format!("[auto-compacted: {} → {} messages]", old_len, messages.len()),
                                )).await;
                            }
                        } else {
                            let _ = tx.send(UiEvent::SystemMessage(
                                format!("[microcompacted: {} messages]", messages.len()),
                            )).await;
                        }
                    }
                }

                // Update status bar before next LLM call
                let _ = tx.send(UiEvent::StatusUpdate("Thinking...".to_string())).await;
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
