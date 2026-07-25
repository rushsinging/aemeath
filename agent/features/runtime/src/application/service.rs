use crate::application::main_loop::request::{NoTuiChatLaunch, TuiChatLaunch};
use crate::ports::legacy::{ChatRuntimeContext, ChatRuntimePort, TuiChatOutcome};

pub struct ChatApplicationService<P> {
    runtime: P,
}

impl<P> ChatApplicationService<P>
where
    P: ChatRuntimePort,
{
    pub fn new(runtime: P) -> Self {
        Self { runtime }
    }

    pub fn validate_no_tui_launch(launch: &NoTuiChatLaunch) -> Result<(), String> {
        launch.validate()
    }

    pub fn validate_tui_launch(launch: &TuiChatLaunch) -> Result<(), String> {
        launch.validate()
    }

    pub async fn run_no_tui_chat(
        &self,
        launch: NoTuiChatLaunch,
        context: ChatRuntimeContext,
    ) -> Result<(), String> {
        Self::validate_no_tui_launch(&launch)?;
        self.runtime.run_no_tui_chat(launch, context).await
    }

    pub async fn run_tui_chat(
        &self,
        launch: TuiChatLaunch,
        context: ChatRuntimeContext,
    ) -> Result<TuiChatOutcome, String> {
        Self::validate_tui_launch(&launch)?;
        self.runtime.run_tui_chat(launch, context).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::main_loop::request::ChatLaunchOptions;
    use async_trait::async_trait;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    #[derive(Default)]
    struct RecordingRuntimePort {
        no_tui_calls: Arc<AtomicUsize>,
        tui_calls: Arc<AtomicUsize>,
    }

    #[async_trait(?Send)]
    impl ChatRuntimePort for RecordingRuntimePort {
        async fn run_no_tui_chat(
            &self,
            _launch: NoTuiChatLaunch,
            _context: ChatRuntimeContext,
        ) -> Result<(), String> {
            self.no_tui_calls.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }

        async fn run_tui_chat(
            &self,
            launch: TuiChatLaunch,
            _context: ChatRuntimeContext,
        ) -> Result<TuiChatOutcome, String> {
            self.tui_calls.fetch_add(1, Ordering::SeqCst);
            Ok(TuiChatOutcome {
                session_id: launch.session_id,
            })
        }
    }

    fn base_options() -> ChatLaunchOptions {
        ChatLaunchOptions {
            cwd: PathBuf::from("/tmp/aemeath"),
            verbose: false,
            markdown: true,
            context_size: 200_000,
            resume: None,
            max_tool_concurrency: 10,
        }
    }
    fn valid_no_tui_launch() -> NoTuiChatLaunch {
        NoTuiChatLaunch {
            options: base_options(),
        }
    }

    fn invalid_no_tui_launch() -> NoTuiChatLaunch {
        let mut options = base_options();
        options.max_tool_concurrency = 0;
        NoTuiChatLaunch { options }
    }

    fn valid_tui_launch() -> TuiChatLaunch {
        TuiChatLaunch {
            options: base_options(),
            session_id: "session-1".to_string(),
            model_display: "provider/model".to_string(),
            max_agent_concurrency: 4,
        }
    }
    fn invalid_tui_launch() -> TuiChatLaunch {
        let mut launch = valid_tui_launch();
        launch.session_id = String::new();
        launch
    }

    /// #1385 Task 7: `ChatRuntimeContext` degraded to startup params only.
    fn runtime_context() -> ChatRuntimeContext {
        ChatRuntimeContext {
            verbose: false,
            resume: None,
        }
    }

    #[test]
    fn test_validate_no_tui_launch_delegates_to_launch_validation() {
        let launch = invalid_no_tui_launch();

        let result =
            ChatApplicationService::<RecordingRuntimePort>::validate_no_tui_launch(&launch);

        assert_eq!(result, Err("max_tool_concurrency 必须大于 0".to_string()));
    }

    #[test]
    fn test_validate_tui_launch_delegates_to_launch_validation() {
        let launch = invalid_tui_launch();

        let result = ChatApplicationService::<RecordingRuntimePort>::validate_tui_launch(&launch);

        assert_eq!(result, Err("TUI 启动必须提供 session_id".to_string()));
    }

    #[tokio::test]
    async fn test_run_no_tui_chat_dispatches_valid_launch_to_runtime() {
        let runtime = RecordingRuntimePort::default();
        let no_tui_calls = Arc::clone(&runtime.no_tui_calls);
        let service = ChatApplicationService::new(runtime);

        let result = service
            .run_no_tui_chat(valid_no_tui_launch(), runtime_context())
            .await;

        assert_eq!(result, Ok(()));
        assert_eq!(no_tui_calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_run_tui_chat_rejects_invalid_launch_before_runtime_dispatch() {
        let runtime = RecordingRuntimePort::default();
        let tui_calls = Arc::clone(&runtime.tui_calls);
        let service = ChatApplicationService::new(runtime);

        let result = service
            .run_tui_chat(invalid_tui_launch(), runtime_context())
            .await;

        assert_eq!(result, Err("TUI 启动必须提供 session_id".to_string()));
        assert_eq!(tui_calls.load(Ordering::SeqCst), 0);
    }
}
