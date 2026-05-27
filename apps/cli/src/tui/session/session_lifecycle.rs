use crate::tui::core::App;
use ::runtime::api::core::tool::ToolRegistry;
use crossterm::{
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

fn status_context_for_runtime_workspace(
    workspace: ::runtime::api::session::WorkspaceContext,
) -> crate::tui::core::UiEvent {
    let workspace = sdk::WorkspaceContextView {
        path_base: std::path::PathBuf::from(&workspace.path_base),
        working_root: std::path::PathBuf::from(&workspace.working_root),
        context_stack: workspace
            .context_stack
            .into_iter()
            .map(|entry| sdk::WorkspaceStackEntryView {
                path_base: std::path::PathBuf::from(entry.path_base),
                working_root: std::path::PathBuf::from(entry.working_root),
            })
            .collect(),
    };
    crate::tui::core::status_context_for_workspace(workspace)
}

fn message_to_sdk(message: ::runtime::api::core::message::Message) -> sdk::ChatMessage {
    sdk::ChatMessage {
        role: match message.role {
            ::runtime::api::core::message::Role::User => "user".to_string(),
            ::runtime::api::core::message::Role::Assistant => "assistant".to_string(),
        },
        content: serde_json::to_value(&message.content).unwrap_or(serde_json::Value::Null),
    }
}

impl App {
    /// Run the TUI event loop
    pub async fn run(
        &mut self,
        client: Arc<::runtime::api::provider::client::LlmClient>,
        _registry: Arc<ToolRegistry>,
        _system_blocks: Vec<::runtime::api::provider::types::SystemBlock>,
        _system_prompt_text: String,
        _user_context: String,
        context_size: usize,
        _verbose: bool,
        _agent_runner: Option<Arc<dyn ::runtime::api::core::tool::AgentRunner>>,
        allow_all: bool,
        resume_id: Option<String>,
        task_store: Arc<::runtime::api::core::task::TaskStore>,
        _max_tool_concurrency: usize,
        _max_agent_concurrency: usize,
        _agent_semaphore: Arc<tokio::sync::Semaphore>,
    ) -> io::Result<()> {
        self.status_bar
            .set_permission_mode(if allow_all { "AllowAll" } else { "AskMe" });
        self.chat.context_size = context_size;
        self.status_bar.set_context_size(context_size as u64);
        self.status_bar.set_thinking(client.is_reasoning());

        // Resume existing session if requested
        if let Some(ref id) = resume_id {
            match ::runtime::api::session::load_session(id).await {
                Ok(s) => {
                    let msg_count = s.messages.len();
                    self.session.session_created_at = Some(s.created_at.clone());
                    if let Some(workspace) = &s.workspace {
                        let path_base = std::path::PathBuf::from(&workspace.path_base);
                        let _working_root = std::path::PathBuf::from(&workspace.working_root);
                        self.session.cwd = path_base.clone();
                        if let crate::tui::core::event::UiEvent::WorkingDirectoryChanged(ctx) =
                            status_context_for_runtime_workspace(workspace.clone())
                        {
                            self.status_bar
                                .set_context_paths(ctx.path_base, ctx.working_root);
                            self.status_bar
                                .set_git_context(ctx.kind, ctx.branch.unwrap_or_default());
                        }
                    }
                    // Restore task snapshot if present
                    if let Some(snapshot) = s.tasks {
                        task_store.restore(snapshot).await;
                    }
                    let mut msgs = s.messages;
                    ::runtime::api::core::message::sanitize_messages(&mut msgs);
                    let trimmed = msg_count - msgs.len();
                    // Check for deeper integrity issues (orphaned tool results
                    // in the middle, role order violations, etc.)
                    let integrity = ::runtime::api::core::message::check_message_integrity(&msgs);
                    let auto_repaired = if integrity.has_issues() {
                        ::runtime::api::core::message::deep_clean_messages(&mut msgs)
                    } else {
                        0
                    };
                    let msgs: Vec<_> = msgs.into_iter().map(message_to_sdk).collect();
                    for i in 0..msgs.len() {
                        let subsequent = if i + 1 < msgs.len() {
                            Some(&msgs[i + 1])
                        } else {
                            None
                        };
                        self.render_history_message(&msgs[i], subsequent);
                    }
                    self.chat.messages = msgs;
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

        let result = self.run_loop(&mut terminal, interrupted).await;

        // Auto-save session on exit
        if !self.chat.messages.is_empty() {
            if let Some(agent_client) = &self.agent_client {
                if let Err(e) = agent_client
                    .sync_current_messages(self.chat.messages.clone())
                    .await
                {
                    log::warn!("failed to sync session messages: {e}");
                }
                if let Err(e) = agent_client.save_current_session().await {
                    log::warn!("failed to auto-save session: {e}");
                }
            } else {
                log::warn!("failed to auto-save session: SDK agent client is unavailable");
            }
        }

        // Run SessionEnd hooks: display system_message in the output area
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
}
