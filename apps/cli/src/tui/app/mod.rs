mod event;
mod resize;
mod resume;
mod run_loop;
mod runtime;
mod session_lifecycle;
#[cfg(test)]
#[path = "status_path_tests.rs"]
mod status_path_tests;

use crate::tui::{InputArea, OutputArea, StatusBar};
use kernel::message::Message;
use kernel::skill::Skill;
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    Terminal,
};
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

pub use event::{StatusContextUpdate, UiEvent};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct TerminalSize {
    pub width: u16,
    pub height: u16,
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
    pub last_terminal_size: Option<TerminalSize>,
    pub just_pasted: bool,
    pub input_queue: std::collections::VecDeque<String>,
    pub last_click: Option<(std::time::Instant, u16, u16)>,
    pub system_prompt_text: String,
    pub context_size: usize,
    pub client: Option<Arc<provider::client::LlmClient>>,
    pub models_config: kernel::config::ModelsConfig,
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
    /// Number of completed LLM conversation turns since last /clear.
    pub turn_count: usize,
    /// Hook runner for lifecycle events
    pub hook_runner: kernel::hook::HookRunner,
    /// Pending oneshot sender for AskUserQuestion reply
    pub ask_user_reply_tx: Option<tokio::sync::oneshot::Sender<String>>,
    /// Interactive ask-user selection state
    pub ask_user_state: Option<AskUserState>,
    /// Session-local reminders for MemoryTool recap.
    pub session_reminders: Arc<std::sync::Mutex<kernel::memory::SessionReminders>>,
    pub memory_config: kernel::config::MemoryConfig,
    /// Pending LLM reflection output waiting for `/reflect apply`.
    pub pending_reflection: Option<kernel::reflection::ReflectionOutput>,
    /// Task store (shared with tools), cleared on /clear
    pub task_store: Option<Arc<kernel::task::TaskStore>>,
    /// Whether background processing is active (LLM streaming / tool calls)
    pub is_processing: bool,
    /// Current persisted tool/worktree workspace context.
    pub workspace_context: Option<kernel::session::WorkspaceContext>,
    /// 分化日志写入器（input.log / output.log / tool.log）
    pub json_logger: Option<Arc<std::sync::Mutex<kernel::logging::JsonLogger>>>,
}

/// State for interactive AskUserQuestion option selection
pub struct AskUserState {
    pub reply_tx: tokio::sync::oneshot::Sender<String>,
    pub options: Vec<String>,
    pub cursor: usize,
    pub multi_select: bool,
    pub selected: Vec<bool>,
    /// Ranges in output_area.lines for each rendered option row.
    pub option_line_ranges: Vec<std::ops::Range<usize>>,
    /// Whether free-text input is allowed
    #[allow(dead_code)]
    pub allow_free_input: bool,
}

#[cfg(test)]
pub(crate) fn display_working_dir(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .map_or_else(|| path.display().to_string(), |name| name.to_string())
}

pub(crate) fn display_status_path(path: &Path) -> String {
    let raw = path.display().to_string();
    let Some(home) = dirs::home_dir() else {
        return raw;
    };
    let home = home.display().to_string();
    if raw == home {
        "~".to_string()
    } else if let Some(rest) = raw.strip_prefix(&(home + "/")) {
        format!("~/{rest}")
    } else {
        raw
    }
}

pub(crate) fn git_branch_for(path: &Path) -> Option<String> {
    let output = Command::new("git")
        .args(["branch", "--show-current"])
        .current_dir(path)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let branch = String::from_utf8(output.stdout).ok()?;
    let branch = branch.trim();
    if branch.is_empty() {
        None
    } else {
        Some(branch.to_string())
    }
}

pub(crate) fn worktree_kind_for(path: &Path) -> crate::tui::status_bar::WorktreeKind {
    let is_worktree = Command::new("git")
        .args(["rev-parse", "--git-dir", "--git-common-dir"])
        .current_dir(path)
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|stdout| {
            let mut lines = stdout.lines().map(str::trim);
            match (lines.next(), lines.next()) {
                (Some(git_dir), Some(common_dir)) => git_dir != common_dir,
                _ => false,
            }
        })
        .unwrap_or(false);

    if is_worktree {
        crate::tui::status_bar::WorktreeKind::Worktree
    } else {
        crate::tui::status_bar::WorktreeKind::Main
    }
}

#[cfg(test)]
pub(crate) fn status_context_for_paths(path_base: &Path, working_root: &Path) -> UiEvent {
    status_context_for_workspace(kernel::session::WorkspaceContext {
        path_base: path_base.display().to_string(),
        working_root: working_root.display().to_string(),
        context_stack: Vec::new(),
    })
}

pub(crate) fn status_context_for_workspace(
    workspace: kernel::session::WorkspaceContext,
) -> UiEvent {
    let path_base = PathBuf::from(&workspace.path_base);
    let working_root = PathBuf::from(&workspace.working_root);
    UiEvent::WorkingDirectoryChanged(StatusContextUpdate {
        path_base: display_status_path(&path_base),
        working_root: display_status_path(&working_root),
        branch: git_branch_for(&working_root),
        kind: worktree_kind_for(&working_root),
        raw_path_base: path_base,
        raw_working_root: working_root,
        workspace,
    })
}

impl App {
    pub fn new(session_id: String, cwd: PathBuf, model: String) -> Self {
        let mut status_bar = StatusBar::new();
        status_bar.set_session_id(&session_id);
        status_bar.set_model(&model);
        let cwd_display = display_status_path(&cwd);
        status_bar.set_context_paths(cwd_display.clone(), cwd_display);
        if let Some(branch) = git_branch_for(&cwd) {
            status_bar.set_git_context(worktree_kind_for(&cwd), branch);
        }
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
            last_terminal_size: None,
            just_pasted: false,
            input_queue: std::collections::VecDeque::new(),
            last_click: None,
            system_prompt_text: String::new(),
            context_size: 200_000,
            client: None,
            models_config: kernel::config::ModelsConfig::default(),
            session_created_at: None,
            active_dialog: None,
            dialog_model_keys: Vec::new(),
            current_model_display: model,
            last_ctrlc: None,
            skills: std::collections::HashMap::new(),
            cached_sessions: Vec::new(),
            tool_call_active: false,
            active_tool_call_ids: std::collections::HashSet::new(),
            turn_count: 0,
            hook_runner: kernel::hook::HookRunner::empty(
                std::env::current_dir()
                    .map(|p| p.display().to_string())
                    .unwrap_or_default(),
            ),
            ask_user_reply_tx: None,
            ask_user_state: None,
            session_reminders: Arc::new(std::sync::Mutex::new(
                kernel::memory::SessionReminders::new(),
            )),
            memory_config: kernel::config::MemoryConfig::default(),
            pending_reflection: None,
            task_store: None,
            is_processing: false,
            workspace_context: None,
            json_logger: None,
        }
    }

    /// Check if Ctrl+C timeout has expired and restore status line.
    fn check_ctrlc_timeout(&mut self) {
        if let Some(last) = self.last_ctrlc {
            if std::time::Instant::now().duration_since(last).as_secs_f64()
                >= update::CTRL_C_TIMEOUT_SECS
            {
                self.last_ctrlc = None;
                self.status_bar.set_success("Ready");
            }
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
                    Constraint::Length(2),
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
#[cfg(test)]
mod slash_tests;
pub mod stream;
pub mod task_window;
pub mod update;
pub mod util;
