use super::port::{ChatRuntimeContext, ChatRuntimePort, TuiChatOutcome};
use crate::business::chat::request::{NoTuiChatLaunch, TuiChatLaunch};

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
    use crate::business::chat::request::ChatLaunchOptions;
    use async_trait::async_trait;
    use hook::api::HookRunner;
    use provider::api::LlmClient;
    use share::config::MemoryConfig;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use storage::api::TaskStore;
    use tools::api::{AgentRunRequest, AgentRunner, ToolExecutionContext, ToolRegistry};

    #[derive(Default)]
    struct RecordingRuntimePort {
        no_tui_calls: Arc<AtomicUsize>,
        tui_calls: Arc<AtomicUsize>,
    }

    struct NoopAgentRunner;

    #[async_trait]
    impl AgentRunner for NoopAgentRunner {
        async fn run_agent(&self, _request: AgentRunRequest<'_>) -> tools::api::AgentRunTerminal {
            tools::api::AgentRunTerminal::Completed {
                result: String::new(),
            }
        }

        async fn complete(
            &self,
            _prompt: &str,
            _system: &str,
            _ctx: &ToolExecutionContext,
        ) -> String {
            String::new()
        }
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
            allow_all: false,
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
            max_agent_concurrency: 4,
            session_id: "session-1".to_string(),
            model_display: "provider/model".to_string(),
        }
    }
    fn invalid_tui_launch() -> TuiChatLaunch {
        let mut launch = valid_tui_launch();
        launch.session_id = String::new();
        launch
    }

    fn runtime_context() -> ChatRuntimeContext {
        ChatRuntimeContext {
            resources: crate::core::resources::RuntimeResources {
                client: Arc::new(LlmClient::new(String::new())),
                registry: Arc::new(ToolRegistry::new()),
                system_blocks: Vec::new(),
                system_prompt_text: String::new(),
                user_context: String::new(),
                agent_runner: Arc::new(NoopAgentRunner),
                task_store: Arc::new(TaskStore::new()),
                skills_map: HashMap::new(),
                hook_runner: HookRunner::empty(),
                memory_config: MemoryConfig::default(),
                agent_semaphore: Arc::new(tokio::sync::Semaphore::new(4)),
                allow_all: false,
                context_size: 200_000,
                language: "en".to_string(),
                reasoning_graph_config: None,
            },
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
