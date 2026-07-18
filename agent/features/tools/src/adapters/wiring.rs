//! Gateway/OHS for tool catalog and registration.
//!
//! Migration-period exports delegate to the existing registry and registration
//! orchestration without moving execution logic.

pub use super::mcp::{McpServerConfig, McpToolDef, McpTransportKind};
use share::skill_ops::Skill;
use std::collections::HashMap;
use std::sync::Arc;
use task::TaskAccess;
use tokio::sync::Mutex;

pub use crate::adapters::bash::is_readonly_command;
pub use crate::adapters::mcp_manager::McpConnectionManager;
pub use crate::adapters::mcp_tool::McpTool;
pub use crate::adapters::registry::{
    register_all_tools, register_all_tools_except_agent, register_subagent_tools,
};
pub use crate::adapters::tool_registry::ToolRegistry;

/// Published name for the tool catalog gateway.
pub type ToolCatalog = ToolRegistry;

/// OHS gateway for constructing and populating tool catalogs.
pub trait ToolCatalogGateway: Send + Sync {
    fn new_registry(&self) -> ToolRegistry;

    fn register_all_tools(
        &self,
        registry: &ToolRegistry,
        task_access: Arc<dyn TaskAccess>,
        skills: Arc<Mutex<HashMap<String, Skill>>>,
        workspace_control: Arc<dyn project::WorkspaceControl>,
    );

    fn register_all_tools_except_agent(
        &self,
        registry: &ToolRegistry,
        task_access: Arc<dyn TaskAccess>,
        skills: Arc<Mutex<HashMap<String, Skill>>>,
        workspace_control: Arc<dyn project::WorkspaceControl>,
    );

    fn register_subagent_tools(
        &self,
        registry: &mut ToolRegistry,
        task_access: Arc<dyn TaskAccess>,
        skills: Arc<Mutex<HashMap<String, Skill>>>,
        workspace_control: Arc<dyn project::WorkspaceControl>,
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
        task_access: Arc<dyn TaskAccess>,
        skills: Arc<Mutex<HashMap<String, Skill>>>,
        workspace_control: Arc<dyn project::WorkspaceControl>,
    ) {
        register_all_tools(registry, task_access, skills, workspace_control);
    }

    fn register_all_tools_except_agent(
        &self,
        registry: &ToolRegistry,
        task_access: Arc<dyn TaskAccess>,
        skills: Arc<Mutex<HashMap<String, Skill>>>,
        workspace_control: Arc<dyn project::WorkspaceControl>,
    ) {
        register_all_tools_except_agent(registry, task_access, skills, workspace_control);
    }

    fn register_subagent_tools(
        &self,
        registry: &mut ToolRegistry,
        task_access: Arc<dyn TaskAccess>,
        skills: Arc<Mutex<HashMap<String, Skill>>>,
        workspace_control: Arc<dyn project::WorkspaceControl>,
    ) {
        register_subagent_tools(registry, task_access, skills, workspace_control);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use task::TaskStore;

    #[test]
    fn default_tool_catalog_gateway_is_object_safe_and_callable() {
        let gateway: &dyn ToolCatalogGateway = &DefaultToolCatalogGateway;
        let mut registry = gateway.new_registry();
        let task_store = Arc::new(TaskStore::new());
        let task_access: Arc<dyn TaskAccess> = task_store.clone();
        let skills = Arc::new(Mutex::new(HashMap::new()));
        let workspace = tempfile::tempdir().expect("workspace");
        let control = project::wire_production_workspace(workspace.path().to_path_buf())
            .expect("workspace wiring")
            .into_views()
            .control();

        gateway.register_subagent_tools(&mut registry, task_access, skills, control);

        assert!(registry.contains("Read"));
        assert!(registry.contains("Bash"));
    }
}
