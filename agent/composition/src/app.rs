use std::sync::Arc;

use runtime::api::client::AgentClientImpl;
use sdk::AgentClient;

pub type AgentClientHandle = Arc<dyn AgentClient>;

pub fn agent_client_from_runtime(client: AgentClientImpl) -> AgentClientHandle {
    Arc::new(client)
}
