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
        // #567：resume 走事件流。启动时存储 resume_id，
        // start_chat 后发 ResumeSession 事件，runtime 通过 SessionResumed 回传。
        self.session.pending_resume_id = resume_id;

        // #567：list_sessions / list_models 走事件流（ManageSession / ListModels）。
        // 不再启动时同步拉取。

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
