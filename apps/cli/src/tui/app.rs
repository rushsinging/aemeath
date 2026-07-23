pub mod event;
pub(crate) mod frame_driver;
mod resize;
mod run_loop;
mod runtime;
pub mod state;

use crate::tui::app::state::{ChatState, InputState, SessionState, UiLayout};
use crate::tui::model::conversation::intent::*;
use crate::tui::model::root::TuiModel;
use crate::tui::model::runtime::session_intent::SessionIntent;
use crate::tui::model::runtime::status_notice::StatusNotice;
use crate::tui::model::runtime_presentation::RuntimePresentationIntent;
use crate::tui::model::workspace_provider::WorkspaceIntent;
use crate::tui::render::input::input_area::suggestions::SuggestionViewState;
use crate::tui::render::input::input_area::InputArea;
use crate::tui::render::output::document_renderer::OutputDocumentRenderer;
use crate::tui::render::output_area::OutputArea;
use crate::tui::render::status::StatusBar;
use crate::tui::view_state::AppViewState;
use ratatui::{
    backend::Backend,
    layout::{Constraint, Direction, Layout, Rect},
    Terminal,
};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

#[cfg(test)]
use event::StatusContextUpdate;
pub use event::{UiEvent, UiTurnContext};

/// `refresh_output_document_from_model` 的 assemble 产物 memo。
pub(crate) struct OutputViewCache {
    pub(crate) revision: u64,
    /// memo key 第二维：workspace_root 变化时强制重 assemble，使工具标题路径立即刷新。
    pub(crate) workspace_root: Option<String>,
    pub(crate) view_model: crate::tui::view_model::OutputViewModel,
}

/// Main TUI application
pub struct App {
    // 视图组件（直接持有，不随 State 变化重建）
    pub output_area: OutputArea,
    pub input_area: InputArea,
    pub status_bar: StatusBar,
    pub(crate) output_document_renderer: OutputDocumentRenderer,
    /// memo：缓存上次 assemble 的 (revision, view_model)。revision 不变即复用，
    /// 跳过 `assemble_from_conversation` 的全量遍历+clone（大会话伪卡死根治）。
    pub(crate) output_view_cache: Option<OutputViewCache>,
    /// 测试探针：assemble 实际执行次数，验证 memo 是否跳过重建。
    #[cfg(test)]
    pub(crate) assemble_count: usize,
    // 纯数据子状态
    pub chat: ChatState,
    pub input: InputState,
    pub session: SessionState,
    pub layout: UiLayout,
    pub model: TuiModel,
    pub view_state: AppViewState,
    // 业务数据（非 UI 状态）
    pub skills: std::collections::HashMap<String, sdk::SkillView>,
    pub command_catalog: Option<Arc<dyn sdk::CommandCatalogPort>>,
    pub command_router: Option<Arc<dyn sdk::CommandRouterPort>>,
    pub agent_client: Option<Arc<dyn sdk::AgentClient>>,
    /// Session 初始化时固定的 HTTP User-Agent。
    pub user_agent: String,
    /// 缓存的配置视图（由 runtime 推送，TUI 只读）
    pub config_view: sdk::ConfigView,
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

#[cfg(test)]
pub(crate) fn status_context_for_paths(path_base: &Path, workspace_root: &Path) -> UiEvent {
    status_context_for_workspace(sdk::WorkspaceContextView {
        path_base: path_base.to_path_buf(),
        workspace_root: workspace_root.to_path_buf(),
        context_stack: Vec::new(),
    })
}

#[cfg(test)]
pub(crate) fn status_context_for_workspace(workspace: sdk::WorkspaceContextView) -> UiEvent {
    let path_base = workspace.path_base.clone();
    let workspace_root = workspace.workspace_root.clone();
    UiEvent::WorkingDirectoryChanged(StatusContextUpdate {
        path_base: display_status_path(&path_base),
        workspace_root: display_status_path(&workspace_root),
        raw_path_base: path_base,
        raw_workspace_root: workspace_root,
        workspace,
    })
}

impl App {
    pub fn new(session_id: String, cwd: PathBuf, model: String) -> Self {
        let status_bar = StatusBar::new();
        let output_area = OutputArea::new();

        let mut model_state = TuiModel::default();
        // 经聚合根 apply(intent) 初始化，不直接写内部字段（保持单一变更入口）。
        crate::tui::update::root_reducer::reduce_intent(
            &mut model_state,
            crate::tui::update::intent::AgentIntent::Session(SessionIntent::SetCurrentSession {
                id: session_id.clone(),
            }),
        );
        crate::tui::update::root_reducer::reduce_intent(
            &mut model_state,
            crate::tui::update::intent::AgentIntent::RuntimePresentation(
                RuntimePresentationIntent::ProviderModel {
                    provider: None,
                    model_id: Some(model.clone()),
                },
            ),
        );
        crate::tui::update::root_reducer::reduce_intent(
            &mut model_state,
            crate::tui::update::intent::AgentIntent::Workspace(WorkspaceIntent::SetCurrent {
                cwd: cwd.display().to_string(),
                worktree: None,
            }),
        );
        // 启动横幅纳入单一真相源 ConversationModel，经 document 渲染。
        model_state.conversation.seed_banner();

        let command_wiring = composition::tools::wire_commands().ok();
        Self {
            output_area,
            input_area: InputArea::new(),
            status_bar,
            output_document_renderer: OutputDocumentRenderer::default(),
            output_view_cache: None,
            #[cfg(test)]
            assemble_count: 0,
            chat: ChatState::default(),
            input: InputState::default(),
            session: SessionState {
                session_id,
                cwd,
                session_created_at: None,
                current_model_display: model,
                memory_config: sdk::MemoryConfigView::default(),
                pending_resume_id: None,
            },
            layout: UiLayout::default(),
            model: model_state,
            view_state: AppViewState::default(),
            skills: std::collections::HashMap::new(),
            command_catalog: command_wiring.as_ref().map(|wiring| wiring.catalog()),
            command_router: command_wiring.map(|wiring| wiring.router()),
            config_view: sdk::ConfigView::default(),
            agent_client: None,
            user_agent: composition::update::default_user_agent(),
        }
    }

    /// Check if Ctrl+C timeout has expired and restore status line.
    fn check_ctrlc_timeout(&mut self) {
        if let Some(last) = self.layout.last_ctrlc {
            if std::time::Instant::now().duration_since(last).as_secs_f64()
                >= update::CTRL_C_TIMEOUT_SECS
            {
                self.layout.clear_ctrlc();
                self.apply_agent_intent(crate::tui::update::intent::AgentIntent::Conversation(
                    ConversationIntent::SetStatusNotice(SetStatusNotice(StatusNotice::success(
                        "Ready",
                    ))),
                ));
            }
        }
    }

    /// Draw the TUI frame.
    pub(crate) fn draw<B: Backend>(&mut self, terminal: &mut Terminal<B>) -> Result<(), B::Error> {
        let draw_start = Instant::now();
        let mut output_rect = Rect::default();
        let mut input_rect = Rect::default();
        let mut status_rect = Rect::default();
        terminal.draw(|f| {
            let size = f.area();
            if size.height < 8 || size.width < 20 {
                return;
            }

            let suggestions_height = self
                .input_area
                .suggestions_height(&self.model.input.completion);
            let input_vm =
                crate::tui::view_assembler::input::InputViewAssembler::assemble_from_model(
                    &self.model.input,
                    0,    // queued_count
                    true, // focused
                );
            let input_height = InputArea::desired_height(size.width, &input_vm);
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Min(10),
                    Constraint::Length(input_height),
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

            let live_status = self.live_status_view_model();
            let mut status_view = self.status_view_model();
            let buf = f.buffer_mut();
            if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                self.output_area
                    .render(chunks[0], buf, &self.view_state.output, &live_status);
            }))
            .is_err()
            {
                self.apply_agent_intent(crate::tui::update::intent::AgentIntent::Conversation(
                    ConversationIntent::SetStatusNotice(SetStatusNotice(StatusNotice::warning(
                        "Render error, try resizing",
                    ))),
                ));
                status_view = self.status_view_model();
            }
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let suggestions_view =
                    SuggestionViewState::from_completion(&self.model.input.completion);
                self.input_area.draw(
                    chunks[1],
                    chunks[2],
                    buf,
                    &input_vm,
                    &self.view_state.input_sel,
                    &suggestions_view,
                );
            }));
            self.status_bar
                .draw(chunks[3], buf, &self.view_state.status_sel, &status_view);
            if let Some(dialog_vm) = self.dialog_view_model() {
                crate::tui::render::dialog::render_dialog_vm(&dialog_vm, size, buf);
            } else if let Some(dialog) = self.layout.active_dialog() {
                dialog.render(size, buf);
            }
        })?;
        self.layout
            .update_areas(output_rect, input_rect, status_rect);
        crate::tui::log_trace!(
            "tui.draw.complete elapsed_ms={} terminal={}x{} output_rect={:?} input_rect={:?} status_rect={:?} spinner_active={} spinner_phase={:?} spinner_frame={} output_lines={}",
            draw_start.elapsed().as_millis(),
            self.layout
                .last_terminal_size
                .map(|size| size.width)
                .unwrap_or_default(),
            self.layout
                .last_terminal_size
                .map(|size| size.height)
                .unwrap_or_default(),
            output_rect,
            input_rect,
            status_rect,
            self.model.conversation.runtime.spinner.chat_active,
            self.model.conversation.runtime.spinner.phase,
            self.view_state.animation.spinner_frame,
            self.output_area.document().total_lines()
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::App;
    use crate::tui::render::output_area::SCROLLBAR_RESERVE_COLS;
    use ratatui::layout::Rect;

    #[test]
    fn test_output_document_width_reserves_scrollbar_and_two_padding_columns() {
        let mut app = App::new(
            "session".to_string(),
            std::env::current_dir().unwrap(),
            "model".to_string(),
        );
        app.layout.output_area_rect = Rect::new(0, 0, 80, 20);

        assert_eq!(
            app.output_document_width(),
            80 - SCROLLBAR_RESERVE_COLS,
            "文档预换行宽度 = 终端宽度 - 滚动条预留列数"
        );
    }

    #[test]
    fn test_output_document_width_never_underflows() {
        let mut app = App::new(
            "session".to_string(),
            std::env::current_dir().unwrap(),
            "model".to_string(),
        );
        app.layout.output_area_rect = Rect::new(0, 0, 3, 20);

        assert_eq!(app.output_document_width(), 1);
    }
}

#[cfg(test)]
mod scenario_tests;
pub mod slash;
#[cfg(test)]
mod slash_effect_tests;
#[cfg(test)]
mod slash_tests;
#[cfg(test)]
mod testing;
pub mod update;
pub mod util;
