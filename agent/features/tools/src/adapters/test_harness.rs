//! Test-only composition factory for consumers that need executable fake tool ports.
//!
//! This module is available only through the opt-in `test-harness` feature. It
//! keeps the concrete registry private while allowing cross-crate tests to
//! preserve real `TypedTool` behavior behind the catalog/execution ports.

use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
};

use parking_lot::Mutex;

use crate::{
    adapters::{composition::wire_catalog_execution, tool_registry::ToolRegistry},
    domain::{
        scope_profile::{RegistryScopeBuilder, ToolRegistrationSpec},
        RegistryScopeName, ToolCapabilities, ToolCatalogPort, ToolCatalogSnapshot,
        ToolExecutionContext, ToolExecutionPort, ToolProfile, ToolProfileName, TypedTool,
    },
};

const TEST_SCOPE: &str = "main";
const TEST_PROFILE: &str = "main-full";

/// Cross-crate test fixture that assembles real typed test tools behind ports.
pub struct TestCatalogExecutionFactory {
    registry: Arc<ToolRegistry>,
    registrations: Mutex<Vec<ToolRegistrationSpec>>,
}

impl Default for TestCatalogExecutionFactory {
    fn default() -> Self {
        Self::new()
    }
}

impl TestCatalogExecutionFactory {
    pub fn new() -> Self {
        Self {
            registry: Arc::new(ToolRegistry::new()),
            registrations: Mutex::new(Vec::new()),
        }
    }

    pub fn empty() -> TestCatalogExecution {
        let factory = Self::new();
        factory.build_for_scope(
            RegistryScopeName::new(TEST_SCOPE),
            ToolProfileName::new(TEST_PROFILE),
        )
    }

    pub fn register<T: TypedTool + 'static>(&self, tool: T) {
        let spec = ToolRegistrationSpec::new(tool.name(), ToolCapabilities::empty());
        self.registry.register(tool);
        self.registrations.lock().push(spec);
    }

    fn build_for_scope(
        &self,
        scope_name: RegistryScopeName,
        profile_name: ToolProfileName,
    ) -> TestCatalogExecution {
        let registrations = self.registrations.lock().clone();
        let mut scopes = HashMap::new();
        let mut profiles = HashMap::new();
        for (candidate_scope, candidate_profile) in [
            (
                RegistryScopeName::new(TEST_SCOPE),
                ToolProfileName::new(TEST_PROFILE),
            ),
            (
                RegistryScopeName::new("sub-agent"),
                ToolProfileName::new("sub-agent-restricted"),
            ),
            (scope_name.clone(), profile_name.clone()),
        ] {
            let mut scope = RegistryScopeBuilder::new(candidate_scope.clone());
            for spec in registrations.iter().cloned() {
                scope
                    .register_mut(spec)
                    .expect("test tools must have unique normalized names");
            }
            scopes.insert(candidate_scope, scope.build());
            profiles.insert(
                candidate_profile,
                ToolProfile::baseline(ToolCapabilities::all()),
            );
        }
        let wiring = wire_catalog_execution(self.registry.clone(), scopes, profiles)
            .expect("test catalog/execution wiring");
        let catalog = wiring
            .catalog()
            .snapshot(&scope_name, &profile_name)
            .expect("test catalog snapshot");
        TestCatalogExecution {
            catalog,
            catalog_port: wiring.catalog(),
            execution: wiring.execution(),
            binding: wiring.binding(),
        }
    }

    pub fn build(&self, context: ToolExecutionContext) -> TestCatalogExecution {
        let result = self.build_for_scope(
            context.scope().registry_scope().clone(),
            context.scope().profile().clone(),
        );
        result.binding.bind(context).expect("bind context");
        result
    }
}

/// Built pair of ports and its catalog projection. Keeps wiring alive for the
/// bound execution context.
pub struct TestCatalogExecution {
    catalog: ToolCatalogSnapshot,
    catalog_port: Arc<dyn ToolCatalogPort>,
    execution: Arc<dyn ToolExecutionPort>,
    binding: Arc<dyn crate::domain::ToolExecutionContextBindingPort>,
}

impl TestCatalogExecution {
    pub fn catalog(&self) -> ToolCatalogSnapshot {
        self.catalog.clone()
    }

    pub fn catalog_port(&self) -> Arc<dyn ToolCatalogPort> {
        self.catalog_port.clone()
    }

    pub fn execution(&self) -> Arc<dyn ToolExecutionPort> {
        self.execution.clone()
    }

    pub fn binding(&self) -> Arc<dyn crate::domain::ToolExecutionContextBindingPort> {
        self.binding.clone()
    }
}

/// Gateway fake used by composition tests without exposing a concrete registry.
#[derive(Default)]
pub struct CountingToolCatalogGateway {
    new_registry_calls: AtomicUsize,
    register_all_tools_calls: AtomicUsize,
}

impl CountingToolCatalogGateway {
    pub fn new_registry_calls(&self) -> usize {
        self.new_registry_calls.load(Ordering::SeqCst)
    }

    pub fn register_all_tools_calls(&self) -> usize {
        self.register_all_tools_calls.load(Ordering::SeqCst)
    }
}

impl crate::adapters::wiring::ToolCatalogGateway for CountingToolCatalogGateway {
    fn new_registry(&self) -> ToolRegistry {
        self.new_registry_calls.fetch_add(1, Ordering::SeqCst);
        ToolRegistry::new()
    }

    fn register_all_tools(
        &self,
        registry: &ToolRegistry,
        task_access: Arc<dyn task::TaskAccess>,
        skills: Arc<tokio::sync::Mutex<HashMap<String, share::skill_ops::Skill>>>,
        memory_source: Arc<dyn crate::domain::MemoryPortSource>,
        workspace_control: Arc<dyn project::WorkspaceControl>,
    ) {
        self.register_all_tools_calls.fetch_add(1, Ordering::SeqCst);
        crate::adapters::registry::register_all_tools(
            registry,
            task_access,
            skills,
            memory_source,
            workspace_control,
        );
    }

    fn register_all_tools_except_agent(
        &self,
        registry: &ToolRegistry,
        task_access: Arc<dyn task::TaskAccess>,
        skills: Arc<tokio::sync::Mutex<HashMap<String, share::skill_ops::Skill>>>,
        memory_source: Arc<dyn crate::domain::MemoryPortSource>,
        workspace_control: Arc<dyn project::WorkspaceControl>,
    ) {
        crate::adapters::registry::register_all_tools_except_agent(
            registry,
            task_access,
            skills,
            memory_source,
            workspace_control,
        );
    }

    fn register_subagent_tools(
        &self,
        registry: &mut ToolRegistry,
        task_access: Arc<dyn task::TaskAccess>,
        skills: Arc<tokio::sync::Mutex<HashMap<String, share::skill_ops::Skill>>>,
        memory_source: Arc<dyn crate::domain::MemoryPortSource>,
        workspace_control: Arc<dyn project::WorkspaceControl>,
    ) {
        crate::adapters::registry::register_subagent_tools(
            registry,
            task_access,
            skills,
            memory_source,
            workspace_control,
        );
    }
}
