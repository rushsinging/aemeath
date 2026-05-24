use aemeath_core::config::MemoryConfig;
use aemeath_core::hook::HookRunner;
use aemeath_core::logging::JsonLogger;
use aemeath_core::skill::Skill;
use aemeath_core::task::TaskStore;
use aemeath_core::tool::{AgentRunner, ToolRegistry};
use aemeath_llm::client::LlmClient;
use aemeath_llm::types::SystemBlock;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ChatLaunchMode {
    NoTui,
    Tui,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ChatLaunchRequest {
    pub mode: ChatLaunchMode,
    pub session_id: Option<String>,
    pub cwd: PathBuf,
    pub model_display: Option<String>,
    pub verbose: bool,
    pub markdown: bool,
    pub context_size: usize,
    pub resume: Option<String>,
    pub allow_all: bool,
    pub max_tool_concurrency: usize,
    pub max_agent_concurrency: usize,
}

pub(crate) struct ChatApplicationService;

impl ChatApplicationService {
    pub(crate) fn validate_request(request: &ChatLaunchRequest) -> Result<(), String> {
        if request.max_tool_concurrency == 0 {
            return Err("max_tool_concurrency 必须大于 0".to_string());
        }
        if request.max_agent_concurrency == 0 {
            return Err("max_agent_concurrency 必须大于 0".to_string());
        }
        match request.mode {
            ChatLaunchMode::NoTui => Ok(()),
            ChatLaunchMode::Tui => {
                if request.session_id.as_deref().unwrap_or_default().is_empty() {
                    return Err("TUI 启动必须提供 session_id".to_string());
                }
                if request
                    .model_display
                    .as_deref()
                    .unwrap_or_default()
                    .is_empty()
                {
                    return Err("TUI 启动必须提供 model_display".to_string());
                }
                Ok(())
            }
        }
    }
}

pub(crate) struct NoTuiChatDependencies {
    pub client: Arc<LlmClient>,
    pub registry: Arc<ToolRegistry>,
    pub system_blocks: Vec<SystemBlock>,
    pub system_prompt_text: String,
    pub user_context: String,
    pub agent_runner: Arc<dyn AgentRunner>,
    pub task_store: Arc<TaskStore>,
    pub agent_semaphore: Arc<tokio::sync::Semaphore>,
    pub skills_map: HashMap<String, Skill>,
    pub hook_runner: HookRunner,
    pub memory_config: MemoryConfig,
    pub json_logger: Option<Arc<Mutex<JsonLogger>>>,
}

impl ChatApplicationService {
    pub(crate) async fn run_no_tui_chat(
        request: ChatLaunchRequest,
        dependencies: NoTuiChatDependencies,
    ) -> Result<(), String> {
        Self::validate_request(&request)?;
        crate::repl::run_repl(
            dependencies.client,
            dependencies.registry,
            dependencies.system_blocks,
            dependencies.system_prompt_text,
            dependencies.user_context,
            request.cwd,
            request.verbose,
            request.markdown,
            request.context_size,
            request.resume,
            Some(dependencies.agent_runner),
            request.allow_all,
            dependencies.task_store,
            request.max_tool_concurrency,
            dependencies.agent_semaphore,
            dependencies.skills_map,
            dependencies.hook_runner,
            dependencies.memory_config,
            dependencies.json_logger,
        )
        .await;
        Ok(())
    }
}

pub(crate) struct TuiChatDependencies {
    pub client: Arc<LlmClient>,
    pub registry: Arc<ToolRegistry>,
    pub system_blocks: Vec<SystemBlock>,
    pub system_prompt_text: String,
    pub user_context: String,
    pub agent_runner: Arc<dyn AgentRunner>,
    pub task_store: Arc<TaskStore>,
    pub skills_map: HashMap<String, Skill>,
    pub hook_runner: HookRunner,
    pub memory_config: MemoryConfig,
    pub json_logger: Option<Arc<Mutex<JsonLogger>>>,
    pub max_agent_concurrency: usize,
    pub agent_semaphore: Arc<tokio::sync::Semaphore>,
}

impl ChatApplicationService {
    pub(crate) async fn run_tui_chat(
        request: ChatLaunchRequest,
        dependencies: TuiChatDependencies,
    ) -> Result<String, String> {
        Self::validate_request(&request)?;
        let session_id = request
            .session_id
            .clone()
            .ok_or_else(|| "TUI 启动必须提供 session_id".to_string())?;
        let model_display = request
            .model_display
            .clone()
            .ok_or_else(|| "TUI 启动必须提供 model_display".to_string())?;
        let mut app = crate::tui::App::new(session_id.clone(), request.cwd, model_display);
        app.memory_config = dependencies.memory_config;
        app.set_skills(dependencies.skills_map);
        app.hook_runner = dependencies.hook_runner;
        app.json_logger = dependencies.json_logger;
        app.run(
            dependencies.client,
            dependencies.registry,
            dependencies.system_blocks,
            dependencies.system_prompt_text,
            dependencies.user_context,
            request.context_size,
            request.verbose,
            request.markdown,
            Some(dependencies.agent_runner),
            request.allow_all,
            request.resume,
            dependencies.task_store,
            request.max_tool_concurrency,
            dependencies.max_agent_concurrency,
            dependencies.agent_semaphore,
        )
        .await
        .map_err(|e| e.to_string())?;
        Ok(session_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn test_validate_request_accepts_no_tui_without_tui_fields() {
        let request = base_request(ChatLaunchMode::NoTui);

        let result = ChatApplicationService::validate_request(&request);

        assert_eq!(result, Ok(()));
    }

    #[test]
    fn test_validate_request_accepts_tui_with_required_fields() {
        let mut request = base_request(ChatLaunchMode::Tui);
        request.session_id = Some("session-1".to_string());
        request.model_display = Some("provider/model".to_string());

        let result = ChatApplicationService::validate_request(&request);

        assert_eq!(result, Ok(()));
    }

    #[test]
    fn test_validate_request_rejects_tui_missing_session_id() {
        let mut request = base_request(ChatLaunchMode::Tui);
        request.model_display = Some("provider/model".to_string());

        let result = ChatApplicationService::validate_request(&request);

        assert_eq!(result, Err("TUI 启动必须提供 session_id".to_string()));
    }

    #[test]
    fn test_validate_request_rejects_zero_tool_concurrency() {
        let mut request = base_request(ChatLaunchMode::NoTui);
        request.max_tool_concurrency = 0;

        let result = ChatApplicationService::validate_request(&request);

        assert_eq!(result, Err("max_tool_concurrency 必须大于 0".to_string()));
    }

    #[test]
    fn test_validate_request_rejects_no_tui_zero_agent_concurrency() {
        let mut request = base_request(ChatLaunchMode::NoTui);
        request.max_agent_concurrency = 0;

        let result = ChatApplicationService::validate_request(&request);

        assert_eq!(result, Err("max_agent_concurrency 必须大于 0".to_string()));
    }
}
