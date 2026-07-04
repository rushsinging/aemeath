use crate::tui::app::App;
use crate::tui::effect::session::resume::apply_resume_input_history;
use crate::tui::effect::session::terminal_guard::TerminalGuard;
use futures::FutureExt;
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
                        let ev = crate::tui::app::status_context_for_workspace(ws.clone());
                        if let crate::tui::app::event::UiEvent::WorkingDirectoryChanged(ctx) = ev {
                            // 工作目录上下文真相归 RuntimeModel，StatusBar 渲染时直接消费 StatusViewModel。
                            self.model.conversation.apply(
                                crate::tui::model::conversation::intent::WorkspaceSnapshotReceived {
                                    path_base: Some(ctx.path_base),
                                    workspace_root: Some(ctx.workspace_root),
                                    branch: ctx.branch,
                                    kind: ctx.kind,
                                },
                            );
                        }
                    }
                    // 恢复任务状态
                    if let Some(tasks) = &s.tasks {
                        let _ = agent_client.restore_tasks(tasks.clone()).await;
                    }
                    // 渲染已恢复的消息（走 ResumeConversation intent，不触发 spinner）
                    let msgs = s.messages;
                    self.model.conversation.apply(
                        crate::tui::model::conversation::intent::ConversationIntent::ResumeConversation(
                            crate::tui::model::conversation::intent::ResumeConversation {
                                messages: msgs.clone(),
                            },
                        ),
                    );
                    self.chat.messages = msgs.clone();
                    self.mark_output_dirty();
                    apply_resume_input_history(self, &msgs);
                    self.append_system_notice(format!(
                        "[resumed session {} ({} messages)]",
                        id, msg_count
                    ));
                    if s.trimmed > 0 {
                        self.append_system_notice(format!(
                            "[trimmed {} incomplete tool-call message(s)]",
                            s.trimmed
                        ));
                    }
                    if s.repaired > 0 {
                        self.append_system_notice(format!(
                            "[repaired {} message(s): removed orphaned tool results and fixed role ordering]",
                            s.repaired
                        ));
                    }
                }
                Err(e) => {
                    self.append_system_notice(format!(
                        "[warning: failed to resume session {}: {}, starting new]",
                        id, e
                    ));
                }
            }
        }

        // Pre-load session list for /resume autocomplete
        self.refresh_session_cache().await;
        // Pre-load model list for /model dialog + completion suggestions（消除纯路径 block_on）
        self.refresh_model_cache().await;

        // 进入 TUI：RAII guard 保证任何退出路径（正常 / ? / panic 展开）都恢复终端。
        let mut guard = TerminalGuard::enter()?;
        let interrupted = Arc::new(AtomicBool::new(false));

        // catch_unwind 包裹主循环：panic 不再 abort 进程，捕获后仍可 auto-save。
        let loop_result =
            std::panic::AssertUnwindSafe(self.run_loop(guard.terminal_mut(), interrupted))
                .catch_unwind()
                .await;

        // auto-save 已下沉到 runtime：run_loop 退出时 drop input_event_tx →
        // 常驻 loop shutdown → chat_impl spawn task 自动 save。
        // 此处 await spawn task 完成（带超时兜底），确保 auto-save 在 runtime drop 前执行。
        crate::tui::effect::session::processing::shutdown_and_save(
            self.chat.take_processing_handle(),
        )
        .await;

        // guard 离开作用域 → Drop 恢复终端；此后 panic 可正常打印到 stderr。
        drop(guard);

        match loop_result {
            Ok(inner) => inner,
            Err(panic) => {
                let msg = crate::panic_hook::payload_message(panic.as_ref());
                crate::tui::log_error!("TUI 事件循环 panic，已优雅退出: {msg}");
                Ok(())
            }
        }
    }
}
