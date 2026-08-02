pub mod event;
pub(crate) mod frame_driver;
mod resize;
mod run_loop;
mod runtime;
pub mod state;

use crate::tui::app::state::{ChatState, InputState, SessionState, UiLayout};
use crate::tui::frame_diagnostics::{
    FrameDiagnosticContext, FrameDiagnosticEvent, FrameDiagnosticKind, FrameDiagnostics,
    FrameTiming,
};
use crate::tui::model::conversation::intent::*;
use crate::tui::model::root::TuiModel;
use crate::tui::model::runtime::session_intent::SessionIntent;
use crate::tui::model::runtime::status_notice::StatusNotice;
use crate::tui::model::runtime_presentation::RuntimePresentationIntent;
use crate::tui::model::workspace_provider::WorkspaceIntent;
use crate::tui::process_memory::{ProcessMemoryBaseline, ProcessMemorySnapshot};
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
#[cfg(test)]
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

const SLOW_FRAME_THRESHOLD: Duration = Duration::from_millis(50);
const SLOW_FRAME_LOG_COOLDOWN: Duration = Duration::from_secs(5);
const RSS_SAMPLE_INTERVAL: Duration = Duration::from_secs(1);

#[cfg(test)]
use event::StatusContextUpdate;
pub use event::UiEvent;

/// `refresh_output_document_from_model` 的增量装配结果 owner。
#[derive(Default)]
pub(crate) struct OutputViewState {
    pub(crate) retained: crate::tui::view_assembler::retained_output_view::RetainedOutputView,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct SkillCompletionCatalog {
    pub(crate) revision: String,
    pub(crate) entries: Vec<crate::tui::adapter::tui_runtime_event::TuiSkillView>,
    pub(crate) slash_routes: Vec<crate::tui::adapter::tui_runtime_event::TuiSkillSlashRoute>,
}

impl SkillCompletionCatalog {
    fn from_sdk(snapshot: sdk::SkillsUpdatedEvent) -> Self {
        Self {
            revision: snapshot.revision,
            entries: snapshot
                .skills
                .into_iter()
                .map(
                    |skill| crate::tui::adapter::tui_runtime_event::TuiSkillView {
                        name: skill.name,
                        aliases: skill.aliases,
                        slash_command: skill.slash_command,
                        slash_aliases: skill.slash_aliases,
                        description: skill.description,
                        argument_hint: skill.argument_hint,
                    },
                )
                .collect(),
            slash_routes: snapshot
                .slash_routes
                .into_iter()
                .map(
                    |route| crate::tui::adapter::tui_runtime_event::TuiSkillSlashRoute {
                        skill: route.skill,
                        slash_command: route.slash_command,
                        aliases: route.aliases,
                        argument_hint: route.argument_hint,
                    },
                )
                .collect(),
        }
    }

    fn resolve(&self, input: &str) -> Option<sdk::SkillRequestCommand> {
        let mut tokens = input.split_whitespace();
        let command = sdk::CommandName::new(tokens.next()?).ok()?;
        let route = self.slash_routes.iter().find(|route| {
            route.slash_command.eq_ignore_ascii_case(command.as_str())
                || route
                    .aliases
                    .iter()
                    .any(|alias| alias.eq_ignore_ascii_case(command.as_str()))
        })?;
        Some(sdk::SkillRequestCommand {
            skill: route.skill.clone(),
            command,
            arguments: sdk::ParsedArguments::new(tokens.map(str::to_string).collect()),
        })
    }
}

/// Main TUI application
pub struct App {
    // 视图组件（直接持有，不随 State 变化重建）
    pub output_area: OutputArea,
    pub input_area: InputArea,
    pub status_bar: StatusBar,
    pub(crate) output_document_renderer: OutputDocumentRenderer,
    /// 输出窗口状态：缓存轻量历史索引、当前物化窗口和渲染结果；
    /// revision 不变时复用，避免重新同步和装配历史。
    pub(crate) output_view: OutputViewState,
    frame_diagnostics: FrameDiagnostics,
    process_memory: ProcessMemoryBaseline,
    started_at: Instant,
    pending_prepare_duration: Duration,
    pending_flush_duration: Duration,
    pending_frame_started_at: Option<Instant>,
    pending_frame_context: Option<FrameDiagnosticContext>,
    /// 记录本进程实际执行的 assemble 次数，供帧诊断判断本帧是否重建 view model。
    assemble_count: usize,
    // 纯数据子状态
    pub chat: ChatState,
    pub input: InputState,
    pub session: SessionState,
    pub layout: UiLayout,
    pub model: TuiModel,
    pub view_state: AppViewState,
    // 业务数据（非 UI 状态）
    pub command_catalog: Option<Arc<dyn sdk::CommandCatalogPort>>,
    pub command_router: Option<Arc<dyn sdk::CommandRouterPort>>,
    pub(crate) skill_completion_catalog: SkillCompletionCatalog,
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

#[cfg(test)]
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
            output_view: OutputViewState::default(),
            frame_diagnostics: FrameDiagnostics::new(SLOW_FRAME_THRESHOLD, SLOW_FRAME_LOG_COOLDOWN),
            process_memory: ProcessMemoryBaseline::new(RSS_SAMPLE_INTERVAL),
            started_at: Instant::now(),
            pending_prepare_duration: Duration::ZERO,
            pending_flush_duration: Duration::ZERO,
            pending_frame_started_at: None,
            pending_frame_context: None,
            assemble_count: 0,
            chat: ChatState::default(),
            input: InputState::default(),
            session: SessionState {
                session_id,
                cwd,
                session_created_at: None,
                current_model_display: model,
                memory_config: sdk::MemoryConfigView::default(),
            },
            layout: UiLayout::default(),
            model: model_state,
            view_state: AppViewState::default(),
            command_catalog: command_wiring.as_ref().map(|wiring| wiring.catalog()),
            command_router: command_wiring.map(|wiring| wiring.router()),
            skill_completion_catalog: SkillCompletionCatalog::default(),
            config_view: sdk::ConfigView::default(),
            agent_client: None,
            user_agent: composition::update::default_user_agent(),
        }
    }

    pub(crate) fn set_skill_snapshot(&mut self, snapshot: sdk::SkillsUpdatedEvent) {
        let catalog = SkillCompletionCatalog::from_sdk(snapshot);
        self.set_tui_skill_snapshot(catalog.revision, catalog.entries, catalog.slash_routes);
    }

    pub(crate) fn set_tui_skill_snapshot(
        &mut self,
        revision: String,
        entries: Vec<crate::tui::adapter::tui_runtime_event::TuiSkillView>,
        slash_routes: Vec<crate::tui::adapter::tui_runtime_event::TuiSkillSlashRoute>,
    ) {
        self.skill_completion_catalog = SkillCompletionCatalog {
            revision,
            entries,
            slash_routes,
        };
        self.update_suggestions();
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
        let draw_duration = draw_start.elapsed();
        #[cfg(test)]
        crate::tui::render::performance::record_terminal_draw(draw_duration);
        crate::tui::log_trace!(
            "tui.draw.complete elapsed_ms={} terminal={}x{} output_rect={:?} input_rect={:?} status_rect={:?} spinner_active={} spinner_phase={:?} spinner_frame={} output_lines={}",
            draw_duration.as_millis(),
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
        self.finish_frame_diagnostics(draw_duration);
        Ok(())
    }

    fn finish_frame_diagnostics(&mut self, draw_duration: Duration) {
        let Some(frame_started_at) = self.pending_frame_started_at.take() else {
            return;
        };
        let context = self
            .pending_frame_context
            .take()
            .unwrap_or_else(|| self.frame_diagnostic_context(self.view_state.dirty.output));
        let now = Instant::now();
        let timing = FrameTiming {
            prepare: self.pending_prepare_duration,
            flush: self.pending_flush_duration,
            draw: draw_duration,
            total: now.saturating_duration_since(frame_started_at),
        };
        let elapsed = now.saturating_duration_since(self.started_at);
        let memory = self
            .process_memory
            .observe(elapsed, crate::tui::process_memory::current_rss_bytes());
        if let Some(event) = self
            .frame_diagnostics
            .classify(elapsed, timing, context, memory)
        {
            self.log_frame_diagnostic(event);
        }
    }

    fn frame_diagnostic_context(&self, output_dirty: bool) -> FrameDiagnosticContext {
        FrameDiagnosticContext {
            output_dirty,
            revision: self.model.conversation.revision(),
            timeline_items: self.model.conversation.timeline.items().len(),
            output_roots: self.output_view.retained.view_model().roots.len(),
            document_lines: self.output_area.document().total_lines(),
            assemble_calls: 0,
        }
    }

    fn assemble_count_for_diagnostics(&self) -> usize {
        self.assemble_count
    }

    fn log_frame_diagnostic(&self, event: FrameDiagnosticEvent) {
        let memory = event.memory.unwrap_or(ProcessMemorySnapshot {
            current_rss_bytes: 0,
            peak_rss_bytes: 0,
            first_rss_bytes: 0,
            growth_from_first_bytes: 0,
            growth_from_previous_bytes: 0,
        });
        let event_type = match event.kind {
            FrameDiagnosticKind::FirstFrame => "tui_first_frame",
            FrameDiagnosticKind::SlowFrame => "tui_slow_frame",
        };
        let message = format!(
            "event_type={event_type} frame_ms={} prepare_ms={} flush_ms={} draw_ms={} output_dirty={} revision={} timeline_items={} output_roots={} document_lines={} assemble_calls={} rss_bytes={} peak_rss_bytes={} first_rss_bytes={} rss_growth_first_bytes={} rss_growth_previous_bytes={}",
            event.timing.total.as_millis(),
            event.timing.prepare.as_millis(),
            event.timing.flush.as_millis(),
            event.timing.draw.as_millis(),
            event.context.output_dirty,
            event.context.revision,
            event.context.timeline_items,
            event.context.output_roots,
            event.context.document_lines,
            event.context.assemble_calls,
            memory.current_rss_bytes,
            memory.peak_rss_bytes,
            memory.first_rss_bytes,
            memory.growth_from_first_bytes,
            memory.growth_from_previous_bytes,
        );
        match event.kind {
            FrameDiagnosticKind::FirstFrame => crate::tui::log_info!("{message}"),
            FrameDiagnosticKind::SlowFrame => crate::tui::log_warn!("{message}"),
        }
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
