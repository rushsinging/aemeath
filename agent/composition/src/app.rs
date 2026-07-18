use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use provider::LlmProviderGateway;
use sdk::{AgentClient, MemoryConfigView, SdkError, SkillView};
use tools::ToolCatalogGateway;

use crate::runtime::{AgentArgs, AgentClientImpl};

pub type AgentClientHandle = Arc<dyn AgentClient>;

pub struct AgentClientBootstrap {
    pub client: AgentClientHandle,
    pub session_id: String,
    pub cwd: PathBuf,
    pub model_display: String,
    pub allow_all: bool,
    pub context_size: usize,
    pub thinking: bool,
    pub memory_config: MemoryConfigView,
    pub skills_map: HashMap<String, SkillView>,
}

pub fn agent_client_from_runtime(client: AgentClientImpl) -> AgentClientHandle {
    Arc::new(client)
}

pub struct FeatureGateways {
    pub tools: Arc<dyn ToolCatalogGateway>,
    pub provider: Arc<dyn LlmProviderGateway>,
}

impl FeatureGateways {
    pub fn new(tools: Arc<dyn ToolCatalogGateway>, provider: Arc<dyn LlmProviderGateway>) -> Self {
        Self { tools, provider }
    }

    pub fn wire_default() -> Self {
        Self::new(crate::tools::wire_tools(), crate::provider::wire_provider())
    }
}

pub async fn build_agent_client(args: AgentArgs) -> Result<AgentClientHandle, SdkError> {
    let gateways = FeatureGateways::wire_default();
    build_agent_client_with_gateways(args, gateways).await
}

async fn build_agent_client_with_gateways(
    args: AgentArgs,
    gateways: FeatureGateways,
) -> Result<AgentClientHandle, SdkError> {
    let cwd = args
        .cwd
        .clone()
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."));
    let workspace = project::wire_production_workspace(cwd.clone())
        .map_err(|error| SdkError::Init(error.to_string()))?
        .into_views();
    let config = config::wire_project_config(&cwd)
        .await
        .map_err(|error| SdkError::Init(format!("配置初始化失败：{error:?}")))?;
    let runtime_client =
        crate::runtime::from_args_with_gateways(args, gateways, workspace, config).await?;
    Ok(agent_client_from_runtime(runtime_client))
}

pub async fn build_agent_bootstrap(args: AgentArgs) -> Result<AgentClientBootstrap, SdkError> {
    let gateways = FeatureGateways::wire_default();
    let cwd = args
        .cwd
        .clone()
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."));
    let workspace = project::wire_production_workspace(cwd.clone())
        .map_err(|error| SdkError::Init(error.to_string()))?
        .into_views();
    let config = config::wire_project_config(&cwd)
        .await
        .map_err(|error| SdkError::Init(format!("配置初始化失败：{error:?}")))?;
    let runtime_client =
        crate::runtime::from_args_with_gateways(args, gateways, workspace, config).await?;
    let launch = runtime_client.tui_launch_context();
    let thinking = !matches!(
        launch.client.default_scope().effective_reasoning(),
        provider::ReasoningLevel::Off
    );
    let client = agent_client_from_runtime(runtime_client);
    let cwd = launch.workspace_root.clone();

    Ok(AgentClientBootstrap {
        client,
        session_id: launch.session_id,
        cwd,
        model_display: launch.model_display,
        allow_all: launch.allow_all,
        context_size: launch.context_size,
        thinking,
        memory_config: launch.memory_config,
        skills_map: launch.skills_map,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use provider::{
        InvocationScope, InvocationStream, LlmClient, LlmClientPool, LlmConfigOptions, LlmError,
        LlmProvider, LlmProviderGateway, ProviderError, SystemBlock,
    };
    use share::config::ModelsConfig;
    use share::message::Message;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use task::TaskAccess;
    use tokio::sync::Mutex;
    use tokio_util::sync::CancellationToken;
    use tools::{DefaultToolCatalogGateway, ToolCatalogGateway, ToolRegistry};

    #[derive(Default)]
    struct CountingProviderGateway {
        client_from_config_calls: AtomicUsize,
    }

    #[async_trait]
    impl LlmProviderGateway for CountingProviderGateway {
        fn client_from_provider(&self, provider: Arc<dyn LlmProvider>) -> LlmClient {
            provider::wire_provider().client_from_provider(provider)
        }

        fn client_from_config(&self, options: LlmConfigOptions) -> Result<LlmClient, LlmError> {
            self.client_from_config_calls.fetch_add(1, Ordering::SeqCst);
            provider::wire_provider().client_from_config(options)
        }

        fn client_pool(
            &self,
            default_client: Arc<LlmClient>,
            models_config: Arc<ModelsConfig>,
            timeout_secs: u64,
        ) -> LlmClientPool {
            provider::wire_provider().client_pool(default_client, models_config, timeout_secs)
        }

        async fn invocation_stream(
            &self,
            client: &LlmClient,
            scope: &InvocationScope,
            system: &[SystemBlock],
            messages: &[Message],
            tool_schemas: &[serde_json::Value],
            cancel: &CancellationToken,
        ) -> Result<InvocationStream, ProviderError> {
            provider::wire_provider()
                .invocation_stream(client, scope, system, messages, tool_schemas, cancel)
                .await
        }
    }

    #[derive(Default)]
    struct CountingToolGateway {
        new_registry_calls: AtomicUsize,
        register_all_tools_calls: AtomicUsize,
    }

    impl ToolCatalogGateway for CountingToolGateway {
        fn new_registry(&self) -> ToolRegistry {
            self.new_registry_calls.fetch_add(1, Ordering::SeqCst);
            DefaultToolCatalogGateway.new_registry()
        }

        fn register_all_tools(
            &self,
            registry: &ToolRegistry,
            task_access: Arc<dyn TaskAccess>,
            skills: Arc<Mutex<HashMap<String, share::skill_ops::Skill>>>,
        ) {
            self.register_all_tools_calls.fetch_add(1, Ordering::SeqCst);
            DefaultToolCatalogGateway.register_all_tools(registry, task_access, skills);
        }

        fn register_all_tools_except_agent(
            &self,
            registry: &ToolRegistry,
            task_access: Arc<dyn TaskAccess>,
            skills: Arc<Mutex<HashMap<String, share::skill_ops::Skill>>>,
        ) {
            DefaultToolCatalogGateway.register_all_tools_except_agent(
                registry,
                task_access,
                skills,
            );
        }

        fn register_subagent_tools(
            &self,
            registry: &mut ToolRegistry,
            task_access: Arc<dyn TaskAccess>,
            skills: Arc<Mutex<HashMap<String, share::skill_ops::Skill>>>,
        ) {
            DefaultToolCatalogGateway.register_subagent_tools(registry, task_access, skills);
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn build_agent_client_with_gateways_consumes_injected_provider_and_tools() {
        let temp = tempfile::tempdir().expect("create temp root");
        let root = temp.path().join("root");
        let agents_dir = temp.path().join("agents");
        std::fs::create_dir_all(&root).expect("create project root");
        std::fs::create_dir_all(&agents_dir).expect("create agents dir");
        std::fs::write(
            agents_dir.join("aemeath.json"),
            serde_json::json!({
                "models": {
                    "default": "local/test-model",
                    "providers": {
                        "local": {
                            "baseUrl": "http://127.0.0.1:1/v1",
                            "apiKey": "test-api-key",
                            "driver": "openai",
                            "models": [{
                                "id": "test-model",
                                "name": "Test Model",
                                "input": ["text"],
                                "contextWindow": 8192,
                                "max_tokens": 1024
                            }]
                        }
                    }
                }
            })
            .to_string(),
        )
        .expect("write config");
        std::fs::write(agents_dir.join("mcp.json"), r#"{"mcpServers":{}}"#)
            .expect("write MCP config");

        let previous_agents_dir = std::env::var_os("AEMEATH_AGENTS_DIR");
        unsafe { std::env::set_var("AEMEATH_AGENTS_DIR", &agents_dir) };

        let provider = Arc::new(CountingProviderGateway::default());
        let tools = Arc::new(CountingToolGateway::default());
        let gateways = FeatureGateways::new(tools.clone(), provider.clone());
        let args = AgentArgs {
            cwd: Some(root),
            api_key: Some("test-api-key".to_string()),
            base_url: Some("http://127.0.0.1:1/v1".to_string()),
            model: Some("local/test-model".to_string()),
            context_size: 8192,
            ..Default::default()
        };

        let result = build_agent_client_with_gateways(args, gateways).await;

        unsafe {
            match previous_agents_dir {
                Some(value) => std::env::set_var("AEMEATH_AGENTS_DIR", value),
                None => std::env::remove_var("AEMEATH_AGENTS_DIR"),
            }
        }
        result.expect("build client with injected gateways");
        assert_eq!(provider.client_from_config_calls.load(Ordering::SeqCst), 1);
        assert_eq!(tools.new_registry_calls.load(Ordering::SeqCst), 1);
        assert_eq!(tools.register_all_tools_calls.load(Ordering::SeqCst), 1);
    }
}
