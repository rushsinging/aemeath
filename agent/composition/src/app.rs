use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use sdk::{AgentClient, MemoryConfigView, SdkError, SkillView};

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

pub async fn build_agent_client(args: AgentArgs) -> Result<AgentClientHandle, SdkError> {
    let runtime_client = crate::runtime::from_args(args).await?;
    Ok(agent_client_from_runtime(runtime_client))
}

pub async fn build_agent_bootstrap(args: AgentArgs) -> Result<AgentClientBootstrap, SdkError> {
    let runtime_client = crate::runtime::from_args(args).await?;
    let launch = runtime_client.tui_launch_context();
    let thinking = launch.client.is_reasoning();
    let client = agent_client_from_runtime(runtime_client);

    Ok(AgentClientBootstrap {
        client,
        session_id: launch.session_id,
        cwd: launch.cwd,
        model_display: launch.model_display,
        allow_all: launch.allow_all,
        context_size: launch.context_size,
        thinking,
        memory_config: launch.memory_config,
        skills_map: launch.skills_map,
    })
}
