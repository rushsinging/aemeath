mod event;
mod resume;
mod run_loop;
mod runtime;
mod session_lifecycle;

use crate::tui::{InputArea, OutputArea, StatusBar};
use aemeath_core::message::Message;
use aemeath_core::skill::Skill;
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    Terminal,
};
use std::io;
use std::path::PathBuf;
use std::sync::Arc;

pub use event::UiEvent;

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
    /// Pending LLM reflection output waiting for `/reflect apply`.
    pub pending_reflection: Option<aemeath_core::reflection::ReflectionOutput>,
    /// Task store (shared with tools), cleared on /clear
    pub task_store: Option<Arc<aemeath_core::task::TaskStore>>,
    /// Whether background processing is active (LLM streaming / tool calls)
    pub is_processing: bool,
    /// 分化日志写入器（input.log / output.log / tool.log）
    pub json_logger: Option<Arc<std::sync::Mutex<aemeath_core::logging::JsonLogger>>>,
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
            pending_reflection: None,
            task_store: None,
            is_processing: false,
            json_logger: None,
        }
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
