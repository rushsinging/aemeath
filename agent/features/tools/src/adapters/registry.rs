//! Built-in tool registration and named registry-scope assembly.

use crate::adapters::{
    agent_tool, ask_user, bash, brief, file_edit, file_read, file_write, glob_tool, grep,
    memory_tool, plan_mode, skill_tool, task_block_by, task_create, task_get, task_list,
    task_list_complete, task_list_create, task_lists, task_stop, task_update, tool_search,
    web_fetch, web_search, worktree,
};
use crate::domain::memory_source::MemoryPortSource;
use crate::domain::published_language::ToolCapabilities as Caps;
use crate::domain::scope_profile::{
    RegistryScope, RegistryScopeBuilder, ToolProfile, ToolRegistrationSpec,
};
use std::sync::Arc;
use task::TaskAccess;

use super::tool_registry::ToolRegistry;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BuiltinRegistryScope {
    Main,
    SubAgent,
}

impl BuiltinRegistryScope {
    fn name(self) -> &'static str {
        match self {
            Self::Main => "main",
            Self::SubAgent => "sub-agent",
        }
    }
}

/// Legacy sub-agent toolset, carried explicitly by the sub-agent-restricted
/// profile since the static `[main, sub]` registration booleans retired.
/// Equivalence guarantee: a sub run without a role policy sees exactly these
/// tools, byte-for-byte with the pre-role behavior.
pub(crate) const SUB_AGENT_TOOLSET: &[&str] = &[
    "Bash",
    "Read",
    "Write",
    "Edit",
    "Glob",
    "Grep",
    "WebFetch",
    "WebSearch",
    "Memory",
    "Brief",
    "ToolSearch",
    "Skill",
];

pub(crate) fn profile_for(scope: BuiltinRegistryScope, main_parent: &ToolProfile) -> ToolProfile {
    let requested = match scope {
        BuiltinRegistryScope::Main => Caps::all(),
        BuiltinRegistryScope::SubAgent => {
            Caps::ReadWorkspace
                | Caps::WriteWorkspace
                | Caps::ExecuteProcess
                | Caps::NetworkAccess
                | Caps::WorkspaceControl
        }
    };
    let requested_names = match scope {
        BuiltinRegistryScope::Main => None,
        BuiltinRegistryScope::SubAgent => Some(
            SUB_AGENT_TOOLSET
                .iter()
                .map(|tool| crate::domain::published_language::ToolName::new(*tool))
                .collect(),
        ),
    };

    match scope {
        BuiltinRegistryScope::Main => main_parent.clone(),
        BuiltinRegistryScope::SubAgent => {
            ToolProfile::derive_restricted(main_parent, requested, requested_names)
                .expect("built-in child profiles must only restrict the main profile")
        }
    }
}

pub(crate) fn register_named_scope(
    registry: &ToolRegistry,
    task_access: Arc<dyn TaskAccess>,
    memory_source: Arc<dyn MemoryPortSource>,
    workspace_control: Arc<dyn project::WorkspaceControl>,
    skill_loader: Arc<dyn crate::domain::SkillLoadPort>,
    selected_scope: BuiltinRegistryScope,
) -> RegistryScope {
    let mut scope = RegistryScopeBuilder::new(selected_scope.name());

    // This macro is the single built-in registration specification: each row
    // declares identity, required capabilities, and factory. Both scopes
    // register the full pool unconditionally; per-run narrowing (visibility
    // and executability) is carried solely by ToolProfile at snapshot time
    // and execution time respectively.
    macro_rules! builtin {
        ($name:literal, $caps:expr, $tool:expr) => {{
            let spec = ToolRegistrationSpec::new($name, $caps);
            scope
                .register_mut(spec.clone())
                .expect("built-in tool registration specification must be valid");
            registry.register_with_capabilities($tool, spec.required_capabilities());
        }};
    }

    builtin!(
        "Bash",
        Caps::ReadWorkspace | Caps::ExecuteProcess | Caps::WorkspaceControl,
        bash::BashTool {
            control: workspace_control.clone()
        }
    );
    builtin!("Read", Caps::ReadWorkspace, file_read::FileReadTool);
    builtin!(
        "Write",
        Caps::ReadWorkspace | Caps::WriteWorkspace,
        file_write::FileWriteTool
    );
    builtin!(
        "Edit",
        Caps::ReadWorkspace | Caps::WriteWorkspace,
        file_edit::FileEditTool
    );
    builtin!("Glob", Caps::ReadWorkspace, glob_tool::GlobTool);
    builtin!("Grep", Caps::ReadWorkspace, grep::GrepTool);
    builtin!("WebFetch", Caps::NetworkAccess, web_fetch::WebFetchTool);
    builtin!("WebSearch", Caps::NetworkAccess, web_search::WebSearchTool);
    builtin!("Agent", Caps::AgentDispatch, agent_tool::AgentTool);
    builtin!(
        "TaskCreate",
        Caps::TaskMutation,
        task_create::TaskCreateTool {
            access: task_access.clone()
        }
    );
    builtin!(
        "TaskUpdate",
        Caps::TaskMutation,
        task_update::TaskUpdateTool {
            access: task_access.clone()
        }
    );
    builtin!(
        "TaskBlockBy",
        Caps::TaskMutation,
        task_block_by::TaskBlockByTool {
            access: task_access.clone()
        }
    );
    builtin!(
        "TaskListGet",
        Caps::TaskRead,
        task_list::TaskListTool {
            access: task_access.clone()
        }
    );
    builtin!(
        "TaskLists",
        Caps::TaskRead,
        task_lists::TaskListsTool {
            access: task_access.clone()
        }
    );
    builtin!(
        "TaskListCreate",
        Caps::TaskMutation,
        task_list_create::TaskListCreateTool {
            access: task_access.clone()
        }
    );
    builtin!(
        "TaskListComplete",
        Caps::TaskMutation,
        task_list_complete::TaskListCompleteTool {
            access: task_access.clone()
        }
    );
    builtin!(
        "TaskGet",
        Caps::TaskRead,
        task_get::TaskGetTool {
            access: task_access.clone()
        }
    );
    builtin!(
        "TaskStop",
        Caps::TaskMutation,
        task_stop::TaskStopTool {
            access: task_access.clone()
        }
    );
    builtin!(
        "Memory",
        Caps::empty(),
        memory_tool::MemoryTool {
            source: memory_source.clone(),
        }
    );
    builtin!(
        "Skill",
        Caps::ReadWorkspace,
        skill_tool::SkillTool::new(skill_loader)
    );
    builtin!(
        "AskUserQuestion",
        Caps::UserInteraction,
        ask_user::AskUserQuestionTool
    );
    builtin!("Brief", Caps::empty(), brief::BriefTool);
    builtin!("ToolSearch", Caps::empty(), tool_search::ToolSearchTool);
    builtin!(
        "EnterPlanMode",
        Caps::PlanControl,
        plan_mode::EnterPlanModeTool
    );
    builtin!(
        "ExitPlanMode",
        Caps::PlanControl,
        plan_mode::ExitPlanModeTool
    );
    builtin!(
        "EnterWorktree",
        Caps::ReadWorkspace | Caps::WorkspaceControl,
        worktree::EnterWorktreeTool {
            control: workspace_control.clone()
        }
    );
    builtin!(
        "ExitWorktree",
        Caps::ReadWorkspace | Caps::WorkspaceControl,
        worktree::ExitWorktreeTool {
            control: workspace_control.clone()
        }
    );

    let built_scope = scope.build();
    debug_assert_eq!(built_scope.name().as_str(), selected_scope.name());
    debug_assert!(registry.len() >= built_scope.len());
    built_scope
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::memory_source::MemoryPortSource;
    use std::collections::BTreeSet;
    use std::sync::Arc;
    use task::TaskStore;

    /// Test-only source that returns a fresh empty in-memory port.
    fn test_memory_source() -> Arc<dyn MemoryPortSource> {
        struct TestSource;
        impl MemoryPortSource for TestSource {
            fn current(&self) -> Arc<dyn memory::MemoryPort> {
                Arc::new(
                    memory::InMemoryMemory::new(memory::MemoryPolicy::default())
                        .expect("valid default policy"),
                )
            }
        }
        Arc::new(TestSource)
    }

    fn assembled_scope(scope: BuiltinRegistryScope) -> RegistryScope {
        let registry = ToolRegistry::new();
        let task_access: Arc<dyn TaskAccess> = Arc::new(TaskStore::new());
        let workspace = tempfile::tempdir().expect("workspace");
        let control = project::wire_production_workspace(workspace.path().to_path_buf(), None)
            .expect("workspace wiring")
            .into_views()
            .control();
        register_named_scope(
            &registry,
            task_access,
            test_memory_source(),
            control,
            crate::composition::wire_skills().loader(),
            scope,
        )
    }

    fn names_for(scope: BuiltinRegistryScope) -> BTreeSet<String> {
        assembled_scope(scope)
            .iter()
            .map(|spec| spec.name().normalized().to_owned())
            .collect()
    }

    fn set(names: &[&str]) -> BTreeSet<String> {
        names.iter().map(|name| name.to_ascii_lowercase()).collect()
    }

    const FULL: &[&str] = &[
        "Bash",
        "Read",
        "Write",
        "Edit",
        "Glob",
        "Grep",
        "WebFetch",
        "WebSearch",
        "Agent",
        "TaskCreate",
        "TaskUpdate",
        "TaskBlockBy",
        "TaskListGet",
        "TaskLists",
        "TaskListCreate",
        "TaskListComplete",
        "TaskGet",
        "TaskStop",
        "Memory",
        "AskUserQuestion",
        "Brief",
        "ToolSearch",
        "EnterPlanMode",
        "ExitPlanMode",
        "EnterWorktree",
        "ExitWorktree",
        "Skill",
    ];
    const SUB_AGENT: &[&str] = SUB_AGENT_TOOLSET;

    #[test]
    fn production_profiles_are_main_baseline_or_restricted_children() {
        let main = ToolProfile::baseline(Caps::all());
        let main_profile = profile_for(BuiltinRegistryScope::Main, &main);
        assert_eq!(main_profile.allowed_capabilities(), Caps::all());

        let child = profile_for(BuiltinRegistryScope::SubAgent, &main);
        assert!(child
            .allowed_capabilities()
            .is_subset_of(main.allowed_capabilities()));
        assert_ne!(child.allowed_capabilities(), main.allowed_capabilities());
    }

    #[test]
    fn side_effect_capability_characterization_matches_builtin_behavior() {
        let main_scope = assembled_scope(BuiltinRegistryScope::Main);
        for name in ["TaskGet", "TaskListGet", "TaskLists"] {
            let spec = main_scope
                .get(&crate::domain::published_language::ToolName::new(name))
                .unwrap();
            assert_eq!(spec.required_capabilities(), Caps::TaskRead);
        }
        for name in [
            "TaskCreate",
            "TaskUpdate",
            "TaskBlockBy",
            "TaskListCreate",
            "TaskListComplete",
            "TaskStop",
        ] {
            let spec = main_scope
                .get(&crate::domain::published_language::ToolName::new(name))
                .unwrap();
            assert_eq!(spec.required_capabilities(), Caps::TaskMutation);
        }
    }

    #[test]
    fn retired_lsp_is_absent_from_all_builtin_scopes() {
        for scope in [BuiltinRegistryScope::Main, BuiltinRegistryScope::SubAgent] {
            assert!(
                !names_for(scope).contains("lsp"),
                "retired LSP tool leaked into {scope:?} scope"
            );
        }
    }

    #[test]
    fn full_scope_characterization_is_exact() {
        assert_eq!(names_for(BuiltinRegistryScope::Main), set(FULL));
    }

    #[test]
    fn sub_agent_registration_pool_equals_main_pool() {
        // 注册池统一：静态 [main, sub] 布尔退役，两个 scope 注册同一全量名单；
        // 等价迁移由 sub-agent-restricted profile 的显式名单承载。
        assert_eq!(
            names_for(BuiltinRegistryScope::SubAgent),
            names_for(BuiltinRegistryScope::Main),
        );
    }

    #[test]
    fn sub_agent_restricted_profile_carries_legacy_toolset_explicitly() {
        let main = ToolProfile::baseline(Caps::all());
        let restricted = profile_for(BuiltinRegistryScope::SubAgent, &main);
        let names = restricted
            .allowed_tool_names()
            .expect("restricted profile carries an explicit allowlist");
        for tool in SUB_AGENT {
            assert!(
                names.contains(&crate::domain::published_language::ToolName::new(*tool)),
                "{tool} missing from restricted profile"
            );
        }
        for absent in [
            "Agent",
            "AskUserQuestion",
            "TaskCreate",
            "TaskUpdate",
            "TaskBlockBy",
            "TaskListCreate",
            "TaskListComplete",
            "TaskStop",
            "EnterPlanMode",
            "ExitPlanMode",
            "EnterWorktree",
            "ExitWorktree",
        ] {
            assert!(
                !names.contains(&crate::domain::published_language::ToolName::new(absent)),
                "{absent} must stay out of the restricted profile"
            );
        }
    }

    #[test]
    fn sub_agent_scope_characterization_is_exact() {
        let main_names = names_for(BuiltinRegistryScope::Main);
        let sub_agent_names = names_for(BuiltinRegistryScope::SubAgent);

        assert_eq!(sub_agent_names, main_names);
        assert!(main_names.contains("agent"));
        assert!(sub_agent_names.contains("agent"));
        for ordinary_tool in ["read", "grep", "bash", "skill"] {
            assert!(sub_agent_names.contains(ordinary_tool));
        }
    }
}
