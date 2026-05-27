use crate::tui::core::App;
use crossterm::{
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

impl App {
    /// Run the TUI event loop.
    /// `agent_client` 是唯一的 runtime 注入点；`resume_id` 由 CLI 启动参数决定。
    pub async fn run(
        &mut self,
        agent_client: Arc<dyn sdk::AgentClient>,
        resume_id: Option<String>,
    ) -> io::Result<()> {
        // Resume existing session if requested
        if let Some(ref id) = resume_id {
            match agent_client.load_session(id).await {
                Ok(s) => {
                    let msg_count = s.message_count;
                    self.session.session_created_at = s.created_at.clone();
                    // 恢复 workspace 上下文
                    if let Some(ref ws) = s.workspace {
                        self.session.cwd = ws.path_base.clone();
                        let ev = crate::tui::core::status_context_for_workspace(ws.clone());
                        if let crate::tui::core::event::UiEvent::WorkingDirectoryChanged(ctx) = ev {
                            self.status_bar
                                .set_context_paths(ctx.path_base, ctx.working_root);
                            self.status_bar
                                .set_git_context(ctx.kind, ctx.branch.unwrap_or_default());
                        }
                    }
                    // 恢复任务状态
                    if let Some(tasks) = &s.tasks {
                        let _ = agent_client.restore_tasks(tasks.clone()).await;
                    }
                    // 渲染已恢复的消息（已由 runtime 完成清洗）
                    let msgs = s.messages;
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
                    if s.trimmed > 0 {
                        self.output_area.push_system(&format!(
                            "[trimmed {} incomplete tool-call message(s)]",
                            s.trimmed
                        ));
                    }
                    if s.repaired > 0 {
                        self.output_area.push_system(&format!(
                            "[repaired {} message(s): removed orphaned tool results and fixed role ordering]",
                            s.repaired
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
            if let Err(e) = agent_client
                .sync_current_messages(self.chat.messages.clone())
                .await
            {
                log::warn!("failed to sync session messages: {e}");
            }
            if let Err(e) = agent_client.save_current_session().await {
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
}
