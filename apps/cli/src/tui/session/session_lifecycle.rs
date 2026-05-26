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

impl App {
    /// Run the TUI event loop
    pub async fn run(
        &mut self,
        client: Arc<::runtime::api::provider::client::LlmClient>,
        registry: Arc<ToolRegistry>,
        system_blocks: Vec<::runtime::api::provider::types::SystemBlock>,
        system_prompt_text: String,
        mut user_context: String,
        context_size: usize,
        verbose: bool,
        agent_runner: Option<Arc<dyn ::runtime::api::core::tool::AgentRunner>>,
        allow_all: bool,
        resume_id: Option<String>,
        task_store: Arc<::runtime::api::core::task::TaskStore>,
        max_tool_concurrency: usize,
        max_agent_concurrency: usize,
        agent_semaphore: Arc<tokio::sync::Semaphore>,
    ) -> io::Result<()> {
        self.status_bar
            .set_permission_mode(if allow_all { "AllowAll" } else { "AskMe" });
        self.cmd_exec.client = Some(client.clone());
        self.chat.system_prompt_text = system_prompt_text.clone();
        self.chat.context_size = context_size;
        self.status_bar.set_context_size(context_size as u64);
        self.status_bar.set_thinking(client.is_reasoning());
        self.cmd_exec.task_store = Some(task_store.clone());

        // Resume existing session if requested
        if let Some(ref id) = resume_id {
            match ::runtime::api::session::load_session(id).await {
                Ok(s) => {
                    let msg_count = s.messages.len();
                    self.session.session_created_at = Some(s.created_at.clone());
                    if let Some(workspace) = &s.workspace {
                        self.cmd_exec.workspace_context = Some(workspace.clone());
                        let path_base = std::path::PathBuf::from(&workspace.path_base);
                        let working_root = std::path::PathBuf::from(&workspace.working_root);
                        self.session.cwd = path_base.clone();
                        if let crate::tui::core::event::UiEvent::WorkingDirectoryChanged(ctx) =
                            crate::tui::core::status_context_for_workspace(workspace.clone())
                        {
                            self.status_bar
                                .set_context_paths(ctx.path_base, ctx.working_root);
                            self.status_bar
                                .set_git_context(ctx.kind, ctx.branch.unwrap_or_default());
                        }
                        self.cmd_exec
                            .hook_runner
                            .set_project_dir(working_root.display().to_string());
                    }
                    // Restore task snapshot if present
                    if let (Some(ts), Some(snapshot)) = (&self.cmd_exec.task_store, s.tasks) {
                        ts.restore(snapshot).await;
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
                        serde_json::from_str::<::runtime::api::core::config::Config>(&content)
                    {
                        if !config.models.providers.is_empty() {
                            self.cmd_exec.models_config = config.models;
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
        // registry is already Arc<ToolRegistry>, shared with MCP background connector

        // Run SessionStart hooks: inject additional_context into user_context,
        // and display system_message in the output area.
        {
            use ::runtime::api::core::config::hooks::HookEvent;
            use ::runtime::api::hook::hook::{HookData, SessionHookData};
            let hook_results = self
                .cmd_exec
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
        if !self.chat.messages.is_empty() {
            if let Some(agent_client) = &self.agent_client {
                if let Err(e) = agent_client
                    .sync_current_messages(crate::tui::messages_to_sdk(&self.chat.messages))
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
        {
            let hook_results = self.cmd_exec.hook_runner.on_session_end().await;
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
}
