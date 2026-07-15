//! Gateway/OHS for tool catalog and registration.
//!
//! Migration-period exports delegate to the existing registry and registration
//! orchestration without moving execution logic.

use share::skill_ops::Skill;
use std::collections::HashMap;
use std::sync::Arc;
use storage::TaskStore;
use tokio::sync::Mutex;

pub use crate::business::bash::is_readonly_command;
pub use crate::business::mcp_manager::McpConnectionManager;
pub use crate::business::mcp_tool::McpTool;
pub use crate::core::registry::{
    register_all_tools, register_all_tools_except_agent, register_subagent_tools,
};
pub use crate::core::tool_registry::ToolRegistry;

/// Published name for the tool catalog gateway.
pub type ToolCatalog = ToolRegistry;

/// OHS gateway for constructing and populating tool catalogs.
pub trait ToolCatalogGateway: Send + Sync {
    fn new_registry(&self) -> ToolRegistry;

    fn register_all_tools(
        &self,
        registry: &ToolRegistry,
        task_store: Arc<TaskStore>,
        skills: Arc<Mutex<HashMap<String, Skill>>>,
    );

    fn register_all_tools_except_agent(
        &self,
        registry: &ToolRegistry,
        task_store: Arc<TaskStore>,
        skills: Arc<Mutex<HashMap<String, Skill>>>,
    );

    fn register_subagent_tools(
        &self,
        registry: &mut ToolRegistry,
        task_store: Arc<TaskStore>,
        skills: Arc<Mutex<HashMap<String, Skill>>>,
    );
}

/// Default tool catalog gateway backed by the existing registry functions.
#[derive(Debug, Default, Clone, Copy)]
pub struct DefaultToolCatalogGateway;

pub fn wire_tools() -> Arc<dyn ToolCatalogGateway> {
    Arc::new(DefaultToolCatalogGateway)
}

impl ToolCatalogGateway for DefaultToolCatalogGateway {
    fn new_registry(&self) -> ToolRegistry {
        ToolRegistry::new()
    }

    fn register_all_tools(
        &self,
        registry: &ToolRegistry,
        task_store: Arc<TaskStore>,
        skills: Arc<Mutex<HashMap<String, Skill>>>,
    ) {
        register_all_tools(registry, task_store, skills);
    }

    fn register_all_tools_except_agent(
        &self,
        registry: &ToolRegistry,
        task_store: Arc<TaskStore>,
        skills: Arc<Mutex<HashMap<String, Skill>>>,
    ) {
        register_all_tools_except_agent(registry, task_store, skills);
    }

    fn register_subagent_tools(
        &self,
        registry: &mut ToolRegistry,
        task_store: Arc<TaskStore>,
        skills: Arc<Mutex<HashMap<String, Skill>>>,
    ) {
        register_subagent_tools(registry, task_store, skills);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_tool_catalog_gateway_is_object_safe_and_callable() {
        let gateway: &dyn ToolCatalogGateway = &DefaultToolCatalogGateway;
        let mut registry = gateway.new_registry();
        let task_store = Arc::new(TaskStore::new());
        let skills = Arc::new(Mutex::new(HashMap::new()));

        gateway.register_subagent_tools(&mut registry, task_store, skills);

        assert!(registry.contains("Read"));
        assert!(registry.contains("Bash"));
    }
}
