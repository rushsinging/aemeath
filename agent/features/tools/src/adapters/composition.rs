//! Composition-only construction surface for Tool Catalog/Execution adapters.
//!
//! Concrete adapters and their shared backing stay private to Tools. The
//! composition root receives only port trait objects plus run-context lifecycle
//! operations.

use std::{collections::HashMap, sync::Arc};

use crate::adapters::{
    catalog::{CatalogAdapter, ToolBacking, ToolBackingError},
    execution::ExecutionAdapter,
    registry::{profile_for, register_named_scope, BuiltinRegistryScope},
    tool_registry::ToolRegistry,
};
use crate::domain::{
    scope_profile::RegistryScope, RegistryScopeName, ToolCapabilities, ToolCatalogPort,
    ToolExecutionPort, ToolProfile, ToolProfileName, TypedTool,
};

#[cfg(feature = "test-harness")]
pub use super::test_harness::{TestCatalogExecution, TestCatalogExecutionFactory};

/// The narrow result of composition-time Tool adapter assembly.
pub struct CatalogExecutionWiring {
    catalog: Arc<dyn ToolCatalogPort>,
    execution: Arc<dyn ToolExecutionPort>,
    backing: ToolBacking,
}

impl CatalogExecutionWiring {
    pub fn catalog(&self) -> Arc<dyn ToolCatalogPort> {
        self.catalog.clone()
    }

    pub fn execution(&self) -> Arc<dyn ToolExecutionPort> {
        self.execution.clone()
    }

    pub async fn sync_mcp_source(
        &self,
        manager: &crate::adapters::mcp_manager::McpConnectionManager,
    ) {
        let registrations = manager.registration_specs().await;
        manager.register_tools(self.backing.registry()).await;
        self.backing.sync_dynamic_membership(&registrations, &[]);
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
    let catalog: Arc<dyn ToolCatalogPort> = Arc::new(CatalogAdapter::new(backing.clone()));
    let execution: Arc<dyn ToolExecutionPort> = Arc::new(ExecutionAdapter::new(backing.clone()));
    Ok(CatalogExecutionWiring {
        catalog,
        execution,
        backing,
    })
}

pub fn wire_builtin_catalog_execution(
    task_access: Arc<dyn task::TaskAccess>,
    memory_source: Arc<dyn crate::domain::MemoryPortSource>,
    workspace_control: Arc<dyn project::WorkspaceControl>,
    skill_loader: Arc<dyn crate::domain::SkillLoadPort>,
) -> Result<CatalogExecutionWiring, ToolBackingError> {
    let registry = Arc::new(ToolRegistry::new());
    let main_profile = ToolProfile::baseline(ToolCapabilities::all());
    let mut scopes = HashMap::new();
    let mut profiles = HashMap::new();
    for scope_kind in [BuiltinRegistryScope::Main, BuiltinRegistryScope::SubAgent] {
        let scope = register_named_scope(
            &registry,
            task_access.clone(),
            memory_source.clone(),
            workspace_control.clone(),
            skill_loader.clone(),
            scope_kind,
        );
        let profile = profile_for(scope_kind, &main_profile);
        let profile_name = match scope_kind {
            BuiltinRegistryScope::Main => ToolProfileName::new("main-full"),
            BuiltinRegistryScope::SubAgent => ToolProfileName::new("sub-agent-restricted"),
        };
        scopes.insert(scope.name().clone(), scope);
        profiles.insert(profile_name, profile);
    }
    wire_catalog_execution(registry, scopes, profiles)
}

/// Production Skill Catalog / Load wiring over one stateless adapter.
pub struct SkillWiring {
    catalog: Arc<dyn crate::domain::SkillCatalogPort>,
    loader: Arc<dyn crate::domain::SkillLoadPort>,
}

impl SkillWiring {
    pub fn catalog(&self) -> Arc<dyn crate::domain::SkillCatalogPort> {
        self.catalog.clone()
    }

    pub fn loader(&self) -> Arc<dyn crate::domain::SkillLoadPort> {
        self.loader.clone()
    }
}

/// Assemble both Skill ports over the same stateless filesystem adapter.
pub fn wire_skills() -> SkillWiring {
    let adapter = Arc::new(super::skill_filesystem::FilesystemSkillAdapter::default());
    SkillWiring {
        catalog: adapter.clone(),
        loader: adapter,
    }
}

/// Production Command Catalog / Router wiring over one immutable adapter.
pub struct CommandWiring {
    catalog: Arc<dyn crate::domain::CommandCatalogPort>,
    router: Arc<dyn crate::domain::CommandRouterPort>,
}

impl CommandWiring {
    pub fn catalog(&self) -> Arc<dyn crate::domain::CommandCatalogPort> {
        self.catalog.clone()
    }

    pub fn router(&self) -> Arc<dyn crate::domain::CommandRouterPort> {
        self.router.clone()
    }
}

pub fn wire_commands(
    extensions: Vec<crate::domain::CommandDescriptor>,
) -> Result<CommandWiring, crate::domain::CommandParseError> {
    let mut descriptors = builtin_command_descriptors()?;
    descriptors.extend(extensions);
    let adapter = Arc::new(super::command::CommandAdapter::try_new(descriptors)?);
    Ok(CommandWiring {
        catalog: adapter.clone(),
        router: adapter,
    })
}

fn builtin_command_descriptors(
) -> Result<Vec<crate::domain::CommandDescriptor>, crate::domain::CommandParseError> {
    use crate::domain::{CommandArgumentSchema as A, CommandMechanism as M, CommandTarget as T};
    type CommandSpec<'a> = (&'a str, &'a [&'a str], &'a str, M, T, A);
    let specs: &[CommandSpec<'_>] = &[
        (
            "help",
            &[],
            "Show available commands",
            M::SnapshotQuery,
            T::ApplicationShell,
            A::None,
        ),
        (
            "clear",
            &[],
            "Clear the current conversation",
            M::ApplicationControl,
            T::ContextManagement,
            A::None,
        ),
        (
            "compact",
            &[],
            "Compact the current conversation",
            M::ApplicationControl,
            T::ContextManagement,
            A::None,
        ),
        (
            "usage",
            &[],
            "Show current token usage",
            M::SnapshotQuery,
            T::Audit,
            A::None,
        ),
        (
            "model",
            &[],
            "Switch model",
            M::ApplicationControl,
            T::Config,
            A::OptionalText,
        ),
        (
            "context",
            &[],
            "Show context window usage",
            M::SnapshotQuery,
            T::ContextManagement,
            A::None,
        ),
        (
            "cost",
            &[],
            "Show current token usage (legacy alias)",
            M::SnapshotQuery,
            T::Audit,
            A::None,
        ),
        (
            "status",
            &[],
            "Show current session status",
            M::SnapshotQuery,
            T::Runtime,
            A::None,
        ),
        (
            "config",
            &[],
            "Show configuration settings",
            M::SnapshotQuery,
            T::Config,
            A::None,
        ),
        (
            "stats",
            &[],
            "Show statistics",
            M::SnapshotQuery,
            T::Runtime,
            A::None,
        ),
        (
            "init",
            &[],
            "Initialize project",
            M::ApplicationControl,
            T::Project,
            A::OptionalText,
        ),
        (
            "session",
            &[],
            "Manage sessions",
            M::ApplicationControl,
            T::ContextManagement,
            A::OptionalText,
        ),
        (
            "resume",
            &[],
            "Resume a previous session",
            M::ApplicationControl,
            T::ContextManagement,
            A::RequiredText,
        ),
        (
            "memory",
            &["mem"],
            "Manage memory",
            M::ApplicationControl,
            T::Memory,
            A::OptionalText,
        ),
        (
            "version",
            &[],
            "Show version information",
            M::SnapshotQuery,
            T::ApplicationShell,
            A::None,
        ),
        (
            "doctor",
            &[],
            "Run system diagnostics",
            M::SnapshotQuery,
            T::ApplicationShell,
            A::None,
        ),
        (
            "rewind",
            &[],
            "Rewind conversation",
            M::ApplicationControl,
            T::ContextManagement,
            A::OptionalText,
        ),
        (
            "save",
            &[],
            "Save current session",
            M::ApplicationControl,
            T::ContextManagement,
            A::None,
        ),
        (
            "reflect",
            &[],
            "Show reflection history",
            M::SnapshotQuery,
            T::Memory,
            A::OptionalPositiveUsize { default: 10 },
        ),
        (
            "paste",
            &[],
            "Paste image from clipboard",
            M::ApplicationControl,
            T::ApplicationShell,
            A::None,
        ),
        (
            "images",
            &[],
            "List pending images",
            M::SnapshotQuery,
            T::ApplicationShell,
            A::None,
        ),
        (
            "clear-images",
            &[],
            "Clear pending images",
            M::ApplicationControl,
            T::ApplicationShell,
            A::None,
        ),
        (
            "update",
            &[],
            "Update aemeath",
            M::ApplicationControl,
            T::ApplicationVersionControl,
            A::None,
        ),
        (
            "exit",
            &["quit"],
            "Exit the application",
            M::ApplicationControl,
            T::ApplicationShell,
            A::None,
        ),
    ];
    specs
        .iter()
        .map(|(name, aliases, description, mechanism, target, schema)| {
            crate::domain::CommandDescriptor::new(
                name,
                aliases,
                description,
                *mechanism,
                *target,
                schema.clone(),
            )
        })
        .collect()
}
