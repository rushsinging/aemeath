use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use provider::api::LlmProviderGateway;
use sdk::{AgentClient, MemoryConfigView, SdkError, SkillView};
use tools::api::ToolCatalogGateway;

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
    let runtime_client = crate::runtime::from_args_with_gateways(args, gateways).await?;
    Ok(agent_client_from_runtime(runtime_client))
}

pub async fn build_agent_bootstrap(args: AgentArgs) -> Result<AgentClientBootstrap, SdkError> {
    let gateways = FeatureGateways::wire_default();
    let runtime_client = crate::runtime::from_args_with_gateways(args, gateways).await?;
    let launch = runtime_client.tui_launch_context();
    let thinking = launch.client.is_reasoning();
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
