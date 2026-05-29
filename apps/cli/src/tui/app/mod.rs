pub mod event;
mod resize;
mod run_loop;
mod runtime;
pub mod state;

use crate::tui::app::state::{ChatState, InputState, SessionState, UiLayout};
use crate::tui::model::root::TuiModel;
use crate::tui::view_state::AppViewState;
use crate::tui::{InputArea, OutputArea, StatusBar};
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

/// Main TUI application
pub struct App {
    // 视图组件（直接持有，不随 State 变化重建）
    pub output_area: OutputArea,
    pub input_area: InputArea,
    pub status_bar: StatusBar,
    // 纯数据子状态
    pub chat: ChatState,
    pub input: InputState,
    pub session: SessionState,
    pub layout: UiLayout,
    pub model: TuiModel,
    pub view_state: AppViewState,
    // 业务数据（非 UI 状态）
    pub skills: std::collections::HashMap<String, sdk::SkillView>,
    pub agent_client: Option<Arc<dyn sdk::AgentClient>>,
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

pub(crate) fn worktree_kind_for(path: &Path) -> crate::tui::render::status::WorktreeKind {
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
        crate::tui::render::status::WorktreeKind::Worktree
    } else {
        crate::tui::render::status::WorktreeKind::Main
    }
}

#[cfg(test)]
pub(crate) fn status_context_for_paths(path_base: &Path, working_root: &Path) -> UiEvent {
    status_context_for_workspace(sdk::WorkspaceContextView {
        path_base: path_base.to_path_buf(),
        working_root: working_root.to_path_buf(),
        context_stack: Vec::new(),
    })
}

pub(crate) fn status_context_for_workspace(workspace: sdk::WorkspaceContextView) -> UiEvent {
    let path_base = workspace.path_base.clone();
    let working_root = workspace.working_root.clone();
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
        status_bar.init(&session_id, &model, &cwd);

        let output_area = OutputArea::new();

        let mut model_state = TuiModel::default();
        model_state.session.current_session_id = Some(session_id.clone());
        model_state.runtime.model_id = Some(model.clone());
        model_state.runtime.workspace.cwd = Some(cwd.display().to_string());
        // 启动横幅纳入单一真相源 ConversationModel，经 document 渲染。
        model_state.conversation.seed_banner();

        Self {
            output_area,
            input_area: InputArea::new(),
            status_bar,
            chat: ChatState::default(),
            input: InputState::default(),
            session: SessionState {
                session_id,
                cwd,
                session_created_at: None,
                cached_sessions: Vec::new(),
                cached_models: Vec::new(),
                current_model_display: model,
                memory_config: sdk::MemoryConfigView::default(),
            },
            layout: UiLayout::default(),
            model: model_state,
            view_state: AppViewState::default(),
            skills: std::collections::HashMap::new(),
            agent_client: None,
        }
    }

    /// Check if Ctrl+C timeout has expired and restore status line.
    fn check_ctrlc_timeout(&mut self) {
        if let Some(last) = self.layout.last_ctrlc {
            if std::time::Instant::now().duration_since(last).as_secs_f64()
                >= update::CTRL_C_TIMEOUT_SECS
            {
                self.layout.clear_ctrlc();
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

            let suggestions_height = self.input_area.suggestions_height();
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
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                self.input_area
                    .draw(chunks[1], chunks[2], buf, self.chat.pending_image_count());
            }));
            let usage = self.chat.usage_snapshot();
            self.status_bar.draw(
                chunks[3],
                buf,
                usage.total_input_tokens,
                usage.total_output_tokens,
                usage.last_input_tokens,
                usage.total_api_calls,
            );
            if let Some(dialog) = self.layout.active_dialog() {
                dialog.render(size, buf);
            }
        })?;
        self.layout
            .update_areas(output_rect, input_rect, status_rect);
        Ok(())
    }
}
pub mod slash;
#[cfg(test)]
mod slash_effect_tests;
#[cfg(test)]
mod slash_tests;
pub mod update;
pub mod util;
