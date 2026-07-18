//! Composition-only construction surface for Tool Catalog/Execution adapters.
//!
//! Concrete adapters and their shared backing stay private to Tools. The
//! composition root receives only port trait objects plus run-context lifecycle
//! operations.

use std::{collections::HashMap, sync::Arc};

use crate::adapters::{
    catalog::{CatalogAdapter, ToolBacking, ToolBackingError},
    execution::{BoundExecutionContexts, ExecutionAdapter},
    registry::{profile_for, register_named_scope, BuiltinRegistryScope},
    tool_registry::ToolRegistry,
};
use crate::domain::{
    scope_profile::RegistryScope, RegistryScopeName, ToolCapabilities, ToolCatalogPort,
    ToolExecutionContext, ToolExecutionPort, ToolProfile, ToolProfileName, TypedTool,
};

#[cfg(feature = "test-harness")]
pub use super::test_harness::{
    CountingToolCatalogGateway, TestCatalogExecution, TestCatalogExecutionFactory,
};

/// The narrow result of composition-time Tool adapter assembly.
pub struct CatalogExecutionWiring {
    catalog: Arc<dyn ToolCatalogPort>,
    execution: Arc<dyn ToolExecutionPort>,
    backing: ToolBacking,
    contexts: Arc<BoundExecutionContexts>,
    binding: Arc<dyn crate::domain::ToolExecutionContextBindingPort>,
}

impl CatalogExecutionWiring {
    pub fn catalog(&self) -> Arc<dyn ToolCatalogPort> {
        self.catalog.clone()
    }

    pub fn execution(&self) -> Arc<dyn ToolExecutionPort> {
        self.execution.clone()
    }

    pub fn binding(&self) -> Arc<dyn crate::domain::ToolExecutionContextBindingPort> {
        self.binding.clone()
    }

    pub fn bind(&self, context: ToolExecutionContext) -> Result<(), String> {
        self.contexts.bind(context)
    }

    pub fn unbind(&self, run_id: &str) {
        self.contexts.unbind(run_id);
    }

    pub async fn sync_mcp_source(
        &self,
        manager: &crate::adapters::mcp_manager::McpConnectionManager,
    ) {
        manager.register_tools(self.backing.registry()).await;
    }

    /// Conservative MCP/dynamic seam: callable registration alone never grants
    /// scope membership or profile authorization.
    pub fn register_conservative_dynamic<T: TypedTool + 'static>(&self, tool: T) {
        self.backing.register_conservative_dynamic(tool);
    }
}

/// Assemble the two public ports over one private backing.
///
/// This function is intentionally reachable only as `tools::composition::*`;
/// Runtime business code must receive the resulting ports from Composition.
pub(crate) fn wire_catalog_execution(
    registry: Arc<ToolRegistry>,
    scopes: HashMap<RegistryScopeName, RegistryScope>,
    profiles: HashMap<ToolProfileName, ToolProfile>,
) -> Result<CatalogExecutionWiring, ToolBackingError> {
    let backing = ToolBacking::try_new(registry, scopes, profiles)?;
    let contexts = Arc::new(BoundExecutionContexts::new());
    let catalog: Arc<dyn ToolCatalogPort> = Arc::new(CatalogAdapter::new(backing.clone()));
    let execution: Arc<dyn ToolExecutionPort> =
        Arc::new(ExecutionAdapter::new(backing.clone(), contexts.clone()));
    let binding: Arc<dyn crate::domain::ToolExecutionContextBindingPort> = contexts.clone();
    Ok(CatalogExecutionWiring {
        catalog,
        execution,
        backing,
        contexts,
        binding,
    })
}

pub fn wire_builtin_catalog_execution(
    task_access: Arc<dyn task::TaskAccess>,
    skills: Arc<tokio::sync::Mutex<HashMap<String, share::skill_ops::Skill>>>,
    workspace_control: Arc<dyn project::WorkspaceControl>,
) -> Result<CatalogExecutionWiring, ToolBackingError> {
    let registry = Arc::new(ToolRegistry::new());
    let main_profile = ToolProfile::baseline(ToolCapabilities::all());
    let mut scopes = HashMap::new();
    let mut profiles = HashMap::new();
    for scope_kind in [BuiltinRegistryScope::Main, BuiltinRegistryScope::SubAgent] {
        let scope = register_named_scope(
            &registry,
            task_access.clone(),
            skills.clone(),
            workspace_control.clone(),
            scope_kind,
        );
        let profile = profile_for(scope_kind, &main_profile);
        let profile_name = match scope_kind {
            BuiltinRegistryScope::Main => ToolProfileName::new("main-full"),
            BuiltinRegistryScope::SubAgent => ToolProfileName::new("sub-agent-restricted"),
            BuiltinRegistryScope::LegacyNoAgent => unreachable!(),
        };
        scopes.insert(scope.name().clone(), scope);
        profiles.insert(profile_name, profile);
    }
    wire_catalog_execution(registry, scopes, profiles)
}
