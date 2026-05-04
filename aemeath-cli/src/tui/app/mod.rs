use crate::tui::{InputArea, OutputArea, StatusBar};
use aemeath_core::message::Message;
use aemeath_core::skill::Skill;
use aemeath_core::tool::{AgentProgressEvent, ImageData, ToolRegistry};
use crossterm::{
    event::{Event, EventStream},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use futures::StreamExt;
use msg::{Cmd, Msg};
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
#[derive(Debug)]
pub enum UiEvent {
    Text(String),
    Thinking(String),
    TextBlockComplete(String),
    ToolCallStart(String),
    ToolCall {
        id: String,
        name: String,
        summary: String,
    },
    ToolResult {
        id: String,
        tool_name: String,
        output: String,
        is_error: bool,
        images: Vec<ImageData>,
    },
    Usage {
        input: u32,
        output: u32,
        last_input: u32,
        elapsed_secs: f64,
    },
    Error(String),
    Cancelled,
    MessagesSync(Vec<Message>),
    Done,
    DoneWithDuration(std::time::Duration),
    LiveTps(f64),
    ClipboardImage(crate::image::ProcessedImage),
    SystemMessage(String),
    /// AskUserQuestion tool call: pause and wait for user input
    AskUser {
        id: String,
        question: String,
        options: Vec<String>,
        #[allow(dead_code)]
        allow_free_input: bool,
        multi_select: bool,
        default: Option<String>,
        reply_tx: tokio::sync::oneshot::Sender<String>,
    },
    /// Sub-agent progress update (streams per-turn output to TUI)
    AgentProgress {
        tool_id: String,
        event: AgentProgressEvent,
    },
    /// Results from StopFailure hook (API error导致 agent 循环结束)
    StopFailureHook {
        system_message: Option<String>,
        additional_context: Option<String>,
    },
    /// Background agent loop requests queued user input before next LLM call.
    DrainQueuedInput {
        reply_tx: tokio::sync::oneshot::Sender<Vec<String>>,
    },
    /// Lifecycle hook execution started.
    HookStart {
        event: String,
        command: String,
    },
    /// Lifecycle hook execution finished.
    HookEnd {
        event: String,
        blocked: bool,
        error: Option<String>,
    },
}
/// Main TUI application
pub struct App {
    pub output_area: OutputArea,
    pub input_area: InputArea,
    pub status_bar: StatusBar,
    pub messages: Vec<Message>,
    pub cwd: PathBuf,
    pub session_id: String,
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
    pub total_api_calls: u64,
    pub last_input_tokens: u64,
    pub should_exit: bool,
    pub pending_images: Vec<crate::image::ProcessedImage>,
    pub output_area_rect: Rect,
    pub input_area_rect: Rect,
    pub status_bar_rect: Rect,
    pub just_pasted: bool,
    pub input_queue: std::collections::VecDeque<String>,
    pub last_click: Option<(std::time::Instant, u16, u16)>,
    pub system_prompt_text: String,
    pub context_size: usize,
    pub client: Option<Arc<aemeath_llm::client::LlmClient>>,
    pub models_config: aemeath_core::config::ModelsConfig,
    pub session_created_at: Option<String>,
    pub active_dialog: Option<crate::tui::dialog::Dialog>,
    pub dialog_model_keys: Vec<String>,
    pub current_model_display: String,
    pub last_ctrlc: Option<std::time::Instant>,
    pub skills: std::collections::HashMap<String, Skill>,
    /// Cached session list for /resume autocomplete (id, summary)
    pub cached_sessions: Vec<(String, String)>,
    /// Whether a tool call is currently active (suppresses thinking output)
    pub tool_call_active: bool,
    /// Tool call IDs that have started and have not emitted a result yet.
    pub active_tool_call_ids: std::collections::HashSet<String>,
    /// Hook runner for lifecycle events
    pub hook_runner: aemeath_core::hook::HookRunner,
    /// Pending oneshot sender for AskUserQuestion reply
    pub ask_user_reply_tx: Option<tokio::sync::oneshot::Sender<String>>,
    /// Interactive ask-user selection state
    pub ask_user_state: Option<AskUserState>,
    /// Session-local reminders for MemoryTool recap.
    pub session_reminders: Arc<std::sync::Mutex<aemeath_core::memory::SessionReminders>>,
    pub memory_config: aemeath_core::config::MemoryConfig,
    /// Task store (shared with tools), cleared on /clear
    pub task_store: Option<Arc<aemeath_core::task::TaskStore>>,
}

/// State for interactive AskUserQuestion option selection
pub struct AskUserState {
    pub reply_tx: tokio::sync::oneshot::Sender<String>,
    pub options: Vec<String>,
    pub cursor: usize,
    pub multi_select: bool,
    pub selected: Vec<bool>,
    /// Index in output_area.lines where option rows start
    pub option_line_start: usize,
    /// Whether free-text input is allowed
    #[allow(dead_code)]
    pub allow_free_input: bool,
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
            input_area_rect: Rect::default(),
            status_bar_rect: Rect::default(),
            just_pasted: false,
            input_queue: std::collections::VecDeque::new(),
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
            skills: std::collections::HashMap::new(),
            cached_sessions: Vec::new(),
            tool_call_active: false,
            active_tool_call_ids: std::collections::HashSet::new(),
            hook_runner: aemeath_core::hook::HookRunner::empty(
                std::env::current_dir()
                    .map(|p| p.display().to_string())
                    .unwrap_or_default(),
            ),
            ask_user_reply_tx: None,
            ask_user_state: None,
            session_reminders: Arc::new(std::sync::Mutex::new(
                aemeath_core::memory::SessionReminders::new(),
            )),
            memory_config: aemeath_core::config::MemoryConfig::default(),
            task_store: None,
        }
    }

    /// Reset per-conversation runtime state while preserving model/provider/session environment.
    pub(crate) async fn reset_runtime_state(&mut self) {
        self.total_input_tokens = 0;
        self.total_output_tokens = 0;
        self.total_api_calls = 0;
        self.last_input_tokens = 0;
        self.tool_call_active = false;
        self.active_tool_call_ids.clear();
        self.input_queue.clear();
        self.output_area.reset_runtime_state();
        self.status_bar.reset_runtime_state();
        self.ask_user_reply_tx = None;
        self.ask_user_state = None;
        if let Ok(mut reminders) = self.session_reminders.lock() {
            reminders.clear();
        }
        // Clear task store so stale tasks don't leak into new conversations
        if let Some(ref ts) = self.task_store {
            ts.clear().await;
        }
    }

    /// Set loaded skills for slash command alias lookup
    pub fn set_skills(&mut self, skills: std::collections::HashMap<String, Skill>) {
        self.skills = skills;
    }

    /// Find a skill by its name or alias
    fn find_skill_by_alias(&self, alias: &str) -> Option<&Skill> {
        self.skills
            .values()
            .find(|s| s.name == alias || s.aliases.iter().any(|a| a == alias))
    }

    /// Run the TUI event loop
    pub async fn run(
        &mut self,
        client: Arc<aemeath_llm::client::LlmClient>,
        registry: ToolRegistry,
        system_blocks: Vec<aemeath_llm::types::SystemBlock>,
        system_prompt_text: String,
        mut user_context: String,
        context_size: usize,
        verbose: bool,
        use_markdown: bool,
        agent_runner: Option<Arc<dyn aemeath_core::tool::AgentRunner>>,
        allow_all: bool,
        resume_id: Option<String>,
        task_store: Arc<aemeath_core::task::TaskStore>,
        max_tool_concurrency: usize,
        max_agent_concurrency: usize,
        agent_semaphore: Arc<tokio::sync::Semaphore>,
    ) -> io::Result<()> {
        self.client = Some(client.clone());
        self.system_prompt_text = system_prompt_text.clone();
        self.context_size = context_size;
        self.status_bar.set_context_size(context_size as u64);
        self.status_bar.set_thinking(client.is_reasoning());
        self.task_store = Some(task_store.clone());

        // Resume existing session if requested
        if let Some(ref id) = resume_id {
            match aemeath_core::session::load_session(id).await {
                Ok(s) => {
                    let msg_count = s.messages.len();
                    self.session_created_at = Some(s.created_at.clone());
                    // Restore task snapshot if present
                    if let (Some(ts), Some(snapshot)) = (&self.task_store, s.tasks) {
                        ts.restore(snapshot).await;
                    }
                    let mut msgs = s.messages;
                    aemeath_core::message::sanitize_messages(&mut msgs);
                    let trimmed = msg_count - msgs.len();
                    // Check for deeper integrity issues (orphaned tool results
                    // in the middle, role order violations, etc.)
                    let integrity = aemeath_core::message::check_message_integrity(&msgs);
                    let auto_repaired = if integrity.has_issues() {
                        let n = aemeath_core::message::deep_clean_messages(&mut msgs);
                        n
                    } else {
                        0
                    };
                    for i in 0..msgs.len() {
                        let subsequent = if i + 1 < msgs.len() {
                            Some(&msgs[i + 1])
                        } else {
                            None
                        };
                        self.render_history_message(&msgs[i], subsequent);
                    }
                    self.messages = msgs;
                    self.output_area.push_system(&format!(
                        "[resumed session {} ({} messages)]",
                        id, msg_count
                    ));
                    if trimmed > 0 {
                        self.output_area.push_system(&format!(
                            "[trimmed {} incomplete tool-call message(s)]",
                            trimmed
                        ));
                    }
                    if auto_repaired > 0 {
                        self.output_area.push_system(&format!(
                                "[repaired {} message(s): removed orphaned tool results and fixed role ordering]",
                                auto_repaired
                            ));
                    }
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
            dirs::home_dir()
                .map(|h| h.join(".aemeath").join("config.json"))
                .unwrap_or_default(),
            std::path::PathBuf::from(".aemeath/config.json"),
        ];
        for path in &config_paths {
            if path.exists() {
                if let Ok(content) = std::fs::read_to_string(path) {
                    if let Ok(config) =
                        serde_json::from_str::<aemeath_core::config::Config>(&content)
                    {
                        if !config.models.providers.is_empty() {
                            self.models_config = config.models;
                            break;
                        }
                    }
                }
            }
        }

        // Pre-load session list for /resume autocomplete
        self.refresh_session_cache().await;

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

        // Run SessionStart hooks: inject additional_context into user_context,
        // and display system_message in the output area.
        {
            use aemeath_core::config::hooks::HookEvent;
            use aemeath_core::hook::{HookData, SessionHookData};
            let hook_results = self
                .hook_runner
                .run_hooks_with_json(
                    HookEvent::SessionStart,
                    None,
                    HookData::Session(SessionHookData {}),
                )
                .await;
            for (_, result, json_output) in &hook_results {
                if let Some(json) = json_output {
                    if let Some(ref ctx) = json.additional_context {
                        user_context = if user_context.is_empty() {
                            ctx.clone()
                        } else {
                            format!("{}\n\n{}", ctx, user_context)
                        };
                    }
                    if let Some(ref msg) = json.system_message {
                        self.output_area.push_system(msg);
                    }
                }
                if result.blocked {
                    self.output_area
                        .push_system("[SessionStart hook blocked session start]");
                }
            }
        }

        let result = self
            .run_loop(
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
                max_agent_concurrency,
                agent_semaphore,
            )
            .await;

        // Auto-save session on exit
        if !self.messages.is_empty() {
            let s = self.build_session(self.messages.clone()).await;
            if let Err(e) = aemeath_core::session::save_session(&s).await {
                log::warn!("failed to auto-save session: {e}");
            }
        }

        // Run SessionEnd hooks: display system_message in the output area
        {
            let hook_results = self.hook_runner.on_session_end().await;
            for (_, result, json_output) in &hook_results {
                if let Some(json) = json_output {
                    if let Some(ref msg) = json.system_message {
                        self.output_area.push_system(msg);
                    }
                }
                if result.error.is_some() {
                    log::warn!("SessionEnd hook error: {:?}", result.error);
                }
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
        max_agent_concurrency: usize,
        agent_semaphore: Arc<tokio::sync::Semaphore>,
    ) -> io::Result<()> {
        let read_files = Arc::new(std::sync::Mutex::new(std::collections::HashSet::new()));
        let session_reminders = self.session_reminders.clone();
        let (ui_tx, mut ui_rx) = mpsc::channel::<UiEvent>(256);
        let mut is_processing = false;
        let active_cancel: Arc<std::sync::Mutex<Option<CancellationToken>>> =
            Arc::new(std::sync::Mutex::new(None));

        let mut event_stream = EventStream::new();

        loop {
            // Update task status lines
            self.update_task_status(&task_store, is_processing).await;

            // Draw UI
            self.draw(terminal)?;

            // Build spawn context refs for update()
            let hook_runner_clone = self.hook_runner.clone();
            let memory_config_clone = self.memory_config.clone();
            let spawn_refs = processing::SpawnContextRefs {
                client: &client,
                registry: &registry,
                system_blocks: &system_blocks,
                system_prompt_text: &system_prompt_text,
                user_context: &user_context,
                context_size,
                read_files: &read_files,
                session_reminders: &session_reminders,
                agent_runner: &agent_runner,
                allow_all,
                interrupted: &interrupted,
                task_store: &task_store,
                max_tool_concurrency,
                max_agent_concurrency,
                agent_semaphore: &agent_semaphore,
                hook_runner: &hook_runner_clone,
                memory_config: &memory_config_clone,
            };

            // --- TEA event collection: produce a Msg ---
            let msg: Option<Msg> = tokio::select! {
                biased;
                // UI events have highest priority
                ev = ui_rx.recv() => {
                    ev.map(Msg::Ui)
                }
                // Terminal events
                ev = event_stream.next() => {
                    match ev {
                        Some(Ok(event)) => match event {
                            Event::Paste(text) => Some(Msg::Paste(text)),
                            Event::Mouse(mouse) => Some(Msg::Mouse(mouse)),
                            Event::Key(key) => Some(Msg::Key(key)),
                            Event::Resize(w, h) => Some(Msg::Resize(w, h)),
                            _ => None,
                        },
                        _ => None,
                    }
                }
                // Tick timeout for spinner etc.
                _ = tokio::time::sleep(std::time::Duration::from_millis(50)) => {
                    None
                }
            };

            let Some(msg) = msg else {
                self.just_pasted = false;
                continue;
            };

            // --- TEA update: state transition ---
            let result = self.update(msg, &mut is_processing, &ui_tx, &active_cancel, &spawn_refs);

            // --- Handle pending slash commands (async) ---
            if let Some(input) = result.pending_slash {
                let review_prompt = self.handle_slash_command(&input).await;
                if let Some(prompt) = review_prompt {
                    self.output_area.push_user_message(&input);
                    self.messages.push(Message::user(&prompt));
                    interrupted.store(false, Ordering::Relaxed);
                    self.output_area.start_spinner();
                    self.output_area.set_spinner_phase("Thinking...");
                    is_processing = true;
                    let cancel = CancellationToken::new();
                    if let Ok(mut guard) = active_cancel.lock() {
                        *guard = Some(cancel.clone());
                    }
                    processing::spawn_processing(processing::SpawnContext {
                        tx: ui_tx.clone(),
                        queue_request_tx: ui_tx.clone(),
                        client: client.clone(),
                        registry: registry.clone(),
                        system_blocks: system_blocks.clone(),
                        system_prompt_text: system_prompt_text.clone(),
                        user_context: user_context.clone(),
                        messages: self.messages.clone(),
                        context_size,
                        cwd: self.cwd.clone(),
                        session_id: self.session_id.clone(),
                        read_files: read_files.clone(),
                        session_reminders: self.session_reminders.clone(),
                        agent_runner: agent_runner.clone(),
                        allow_all,
                        interrupted: interrupted.clone(),
                        cancel,
                        task_store: task_store.clone(),
                        max_tool_concurrency,
                        max_agent_concurrency,
                        agent_semaphore: agent_semaphore.clone(),
                        hook_runner: self.hook_runner.clone(),
                        memory_config: self.memory_config.clone(),
                    });
                }
            }

            // --- TEA command execution ---
            match result.cmd {
                Cmd::None => {}
                Cmd::Quit => {
                    self.should_exit = true;
                }
                Cmd::SpawnProcessing(ctx) => {
                    if let Ok(mut guard) = active_cancel.lock() {
                        *guard = Some(ctx.cancel.clone());
                    }
                    processing::spawn_processing(ctx);
                }
                Cmd::SendEvents(events) => {
                    for ev in events {
                        let _ = ui_tx.send(ev).await;
                    }
                }
                Cmd::QueueInput(_) => {
                    // Handled via pending_slash above
                }
                Cmd::SaveSession(msgs) => {
                    if !msgs.is_empty() {
                        let s = self.build_session(msgs).await;
                        if let Err(e) = aemeath_core::session::save_session(&s).await {
                            log::warn!("failed to auto-save session on sync: {e}");
                        }
                    }
                }
            }

            self.just_pasted = false;
            if self.should_exit {
                break;
            }
        }
        Ok(())
    }

    /// Update task status display in output area. Also runs lifecycle checks.
    async fn update_task_status(
        &mut self,
        task_store: &Arc<aemeath_core::task::TaskStore>,
        _is_processing: bool,
    ) {
        let tasks = task_store.list_current_batch().await;
        let active: Vec<_> = tasks
            .iter()
            .filter(|t| t.status != aemeath_core::task::TaskStatus::Deleted)
            .cloned()
            .collect();

        if active.is_empty() {
            // Check lifecycle: if previous batch was completed and auto-cleared
            self.output_area.set_task_status(Vec::new());
        } else {
            let task_list_config = aemeath_core::config::TaskListConfig::default();
            let lines = task_window::build_task_window(
                &active,
                task_list_config.max_lines,
                task_list_config.show_last_completed,
            );
            self.output_area.set_task_status(lines);
        }
    }

    /// Build a Session from current state, including task snapshot.
    async fn build_session(
        &self,
        messages: Vec<aemeath_core::message::Message>,
    ) -> aemeath_core::session::Session {
        use aemeath_core::session::{now_iso, Session};
        let task_snapshot = match &self.task_store {
            Some(ts) => {
                let snap = ts.snapshot().await;
                if snap.tasks.is_empty() {
                    None
                } else {
                    Some(snap)
                }
            }
            None => None,
        };
        Session {
            id: self.session_id.clone(),
            cwd: self.cwd.to_string_lossy().to_string(),
            messages,
            created_at: self.session_created_at.clone().unwrap_or_else(now_iso),
            updated_at: now_iso(),
            metadata: Default::default(),
            tasks: task_snapshot,
        }
    }

    /// Refresh the cached session list for /resume autocomplete
    pub async fn refresh_session_cache(&mut self) {
        let sessions = aemeath_core::session::list_sessions().await;
        self.cached_sessions = sessions
            .iter()
            .take(20)
            .map(|s| {
                let summary = build_session_summary(s);
                (s.id.clone(), summary)
            })
            .collect();
    }

    /// Draw the TUI frame.
    fn draw(&mut self, terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> io::Result<()> {
        let mut output_rect = Rect::default();
        let mut input_rect = Rect::default();
        let mut status_rect = Rect::default();
        terminal.draw(|f| {
            let size = f.area();
            if size.height < 8 || size.width < 20 {
                return;
            }

            let suggestions_height = if self.input_area.is_showing_suggestions() {
                let count = self.input_area.get_suggestions().len().min(5) as u16;
                if count > 0 {
                    count + 1
                } else {
                    0
                }
            } else {
                0
            };

            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Min(10),
                    Constraint::Length(5),
                    Constraint::Length(suggestions_height),
                    Constraint::Length(1),
                ])
                .split(size);

            output_rect = chunks[0];
            input_rect = chunks[1];
            status_rect = chunks[3];
            if chunks.iter().any(|c| c.height == 0 && c.width == 0) {
                return;
            }

            let buf = f.buffer_mut();
            if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                self.output_area.render(chunks[0], buf);
            }))
            .is_err()
            {
                self.status_bar.set_warning("Render error, try resizing");
            }
            self.input_area
                .set_pending_images(self.pending_images.len());
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                self.input_area.render(chunks[1], buf);
            }));
            if suggestions_height > 0 {
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    self.input_area.render_suggestions_in_area(chunks[2], buf);
                }));
            }
            self.status_bar.set_tokens(
                self.total_input_tokens,
                self.total_output_tokens,
                self.last_input_tokens,
            );
            self.status_bar.set_api_calls(self.total_api_calls);
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                self.status_bar.render(chunks[3], buf);
            }));
            if let Some(ref dialog) = self.active_dialog {
                dialog.render(size, buf);
            }
        })?;
        self.output_area_rect = output_rect;
        self.input_area_rect = input_rect;
        self.status_bar_rect = status_rect;
        Ok(())
    }
}

pub mod event_handler;
/// Build a one-line summary for a session, shown in /resume autocomplete
fn build_session_summary(session: &aemeath_core::session::Session) -> String {
    format!("{} [{}msg]", session.summary(), session.messages.len())
}

pub mod input_handler;
pub mod mouse_handler;
pub mod msg;
pub mod paste_handler;
pub mod processing;
pub mod render;
pub mod slash;
pub mod stream;
pub mod task_window;
pub mod update;
pub mod util;
