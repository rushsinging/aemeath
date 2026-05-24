use super::port::{ChatRuntimePort, NoTuiChatDependencies, TuiChatDependencies, TuiChatOutcome};
use super::request::ChatLaunchRequest;

pub(crate) struct ChatApplicationService<P> {
    runtime: P,
}

impl<P> ChatApplicationService<P>
where
    P: ChatRuntimePort,
{
    pub(crate) fn new(runtime: P) -> Self {
        Self { runtime }
    }

    pub(crate) fn validate_request(request: &ChatLaunchRequest) -> Result<(), String> {
        request.validate()
    }

    pub(crate) async fn run_no_tui_chat(
        &self,
        request: ChatLaunchRequest,
        dependencies: NoTuiChatDependencies,
    ) -> Result<(), String> {
        Self::validate_request(&request)?;
        self.runtime.run_no_tui_chat(request, dependencies).await
    }

    pub(crate) async fn run_tui_chat(
        &self,
        request: ChatLaunchRequest,
        dependencies: TuiChatDependencies,
    ) -> Result<TuiChatOutcome, String> {
        Self::validate_request(&request)?;
        self.runtime.run_tui_chat(request, dependencies).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::chat::request::ChatLaunchMode;
    use async_trait::async_trait;
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};

    #[derive(Default)]
    struct RecordingRuntimePort {
        no_tui_calls: Arc<Mutex<usize>>,
        tui_calls: Arc<Mutex<usize>>,
    }

    #[async_trait(?Send)]
    impl ChatRuntimePort for RecordingRuntimePort {
        async fn run_no_tui_chat(
            &self,
            _request: ChatLaunchRequest,
            _dependencies: NoTuiChatDependencies,
        ) -> Result<(), String> {
            *self.no_tui_calls.lock().unwrap() += 1;
            Ok(())
        }

        async fn run_tui_chat(
            &self,
            request: ChatLaunchRequest,
            _dependencies: TuiChatDependencies,
        ) -> Result<TuiChatOutcome, String> {
            *self.tui_calls.lock().unwrap() += 1;
            Ok(TuiChatOutcome {
                session_id: request.session_id.unwrap_or_default(),
            })
        }
    }

    fn base_request(mode: ChatLaunchMode) -> ChatLaunchRequest {
        ChatLaunchRequest {
            mode,
            session_id: None,
            cwd: PathBuf::from("/tmp/aemeath"),
            model_display: None,
            verbose: false,
            markdown: true,
            context_size: 200_000,
            resume: None,
            allow_all: false,
            max_tool_concurrency: 10,
            max_agent_concurrency: 4,
        }
    }

    #[test]
    fn test_validate_request_delegates_to_request_validation() {
        let mut request = base_request(ChatLaunchMode::NoTui);
        request.max_tool_concurrency = 0;

        let result = ChatApplicationService::<RecordingRuntimePort>::validate_request(&request);

        assert_eq!(result, Err("max_tool_concurrency 必须大于 0".to_string()));
    }
}
