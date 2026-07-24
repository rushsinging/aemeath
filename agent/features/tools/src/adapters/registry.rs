//! Built-in tool registration and named registry-scope assembly.

use crate::adapters::{
    agent_tool, ask_user, bash, brief, file_edit, file_read, file_write, glob_tool, grep,
    memory_tool, plan_mode, task_create, task_get, task_list, task_list_complete, task_list_create,
    task_stop, task_update, tool_search, web_fetch, web_search, worktree,
};
use crate::domain::memory_source::MemoryPortSource;
use crate::domain::published_language::ToolCapabilities as Caps;
use crate::domain::scope_profile::{
    is_authorized, RegistryScope, RegistryScopeBuilder, ToolProfile, ToolRegistrationSpec,
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

    match scope {
        BuiltinRegistryScope::Main => *main_parent,
        BuiltinRegistryScope::SubAgent => ToolProfile::derive_restricted(main_parent, requested)
            .expect("built-in child profiles must only restrict the main profile"),
    }
}

fn belongs_to(scope: BuiltinRegistryScope, main: bool, sub: bool) -> bool {
    match scope {
        BuiltinRegistryScope::Main => main,
        BuiltinRegistryScope::SubAgent => sub,
    }
}

pub(crate) fn register_named_scope(
    registry: &ToolRegistry,
    task_access: Arc<dyn TaskAccess>,
    memory_source: Arc<dyn MemoryPortSource>,
    workspace_control: Arc<dyn project::WorkspaceControl>,
    selected_scope: BuiltinRegistryScope,
) -> RegistryScope {
    let mut scope = RegistryScopeBuilder::new(selected_scope.name());
    let main_profile = ToolProfile::baseline(Caps::all());
    let profile = profile_for(selected_scope, &main_profile);

    // This macro is the single built-in registration specification: each row
    // declares identity, required capabilities, scope membership, and factory.
    macro_rules! builtin {
        ($name:literal, $caps:expr, [$main:literal, $sub:literal], $tool:expr) => {{
            if belongs_to(selected_scope, $main, $sub) {
                let spec = ToolRegistrationSpec::new($name, $caps);
                scope
                    .register_mut(spec.clone())
                    .expect("built-in tool registration specification must be valid");
                if is_authorized(&spec, &profile) {
                    registry.register_with_capabilities($tool, spec.required_capabilities());
                }
            }
        }};
    }

    builtin!(
        "Bash",
        Caps::ReadWorkspace | Caps::ExecuteProcess | Caps::WorkspaceControl,
        [true, true],
        bash::BashTool {
            control: workspace_control.clone()
        }
    );
    builtin!(
        "Read",
        Caps::ReadWorkspace,
        [true, true],
        file_read::FileReadTool
    );
    builtin!(
        "Write",
        Caps::ReadWorkspace | Caps::WriteWorkspace,
        [true, true],
        file_write::FileWriteTool
    );
    builtin!(
        "Edit",
        Caps::ReadWorkspace | Caps::WriteWorkspace,
        [true, true],
        file_edit::FileEditTool
    );
    builtin!(
        "Glob",
        Caps::ReadWorkspace,
        [true, true],
        glob_tool::GlobTool
    );
    builtin!("Grep", Caps::ReadWorkspace, [true, true], grep::GrepTool);
    builtin!(
        "WebFetch",
        Caps::NetworkAccess,
        [true, true],
        web_fetch::WebFetchTool
    );
    builtin!(
        "WebSearch",
        Caps::NetworkAccess,
        [true, true],
        web_search::WebSearchTool
    );
    builtin!(
        "Agent",
        Caps::AgentDispatch,
        [true, false],
        agent_tool::AgentTool
    );
    builtin!(
        "TaskCreate",
        Caps::TaskMutation,
        [true, false],
        task_create::TaskCreateTool {
            access: task_access.clone()
        }
    );
    builtin!(
        "TaskUpdate",
        Caps::TaskMutation,
        [true, false],
        task_update::TaskUpdateTool {
            access: task_access.clone()
        }
    );
    builtin!(
        "TaskList",
        Caps::TaskRead,
        [true, false],
        task_list::TaskListTool {
            access: task_access.clone()
        }
    );
    builtin!(
        "TaskListCreate",
        Caps::TaskMutation,
        [true, false],
        task_list_create::TaskListCreateTool {
            access: task_access.clone()
        }
    );
    builtin!(
        "TaskListComplete",
        Caps::TaskMutation,
        [true, false],
        task_list_complete::TaskListCompleteTool {
            access: task_access.clone()
        }
    );
    builtin!(
        "TaskGet",
        Caps::TaskRead,
        [true, false],
        task_get::TaskGetTool {
            access: task_access.clone()
        }
    );
    builtin!(
        "TaskStop",
        Caps::TaskMutation,
        [true, false],
        task_stop::TaskStopTool {
            access: task_access.clone()
        }
    );
    builtin!(
        "Memory",
        Caps::empty(),
        [true, true],
        memory_tool::MemoryTool {
            source: memory_source.clone(),
        }
    );
    builtin!(
        "AskUserQuestion",
        Caps::UserInteraction,
        [true, false],
        ask_user::AskUserQuestionTool
    );
    builtin!("Brief", Caps::empty(), [true, true], brief::BriefTool);
    builtin!(
        "ToolSearch",
        Caps::empty(),
        [true, true],
        tool_search::ToolSearchTool
    );
    builtin!(
        "EnterPlanMode",
        Caps::PlanControl,
        [true, false],
        plan_mode::EnterPlanModeTool
    );
    builtin!(
        "ExitPlanMode",
        Caps::PlanControl,
        [true, false],
        plan_mode::ExitPlanModeTool
    );
    builtin!(
        "EnterWorktree",
        Caps::ReadWorkspace | Caps::WorkspaceControl,
        [true, false],
        worktree::EnterWorktreeTool {
            control: workspace_control.clone()
        }
    );
    builtin!(
        "ExitWorktree",
        Caps::ReadWorkspace | Caps::WorkspaceControl,
        [true, false],
        worktree::ExitWorktreeTool {
            control: workspace_control.clone()
        }
    );

    let built_scope = scope.build();
    debug_assert_eq!(built_scope.name().as_str(), selected_scope.name());
    debug_assert!(registry.len() >= built_scope.len());
    debug_assert!(built_scope
        .iter()
        .all(|spec| built_scope.get(spec.name()).is_some()));
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
        let control = project::wire_production_workspace(workspace.path().to_path_buf())
            .expect("workspace wiring")
            .into_views()
            .control();
        register_named_scope(&registry, task_access, test_memory_source(), control, scope)
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
        "TaskList",
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
    ];
    const SUB_AGENT: &[&str] = &[
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
    ];
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
        for name in ["TaskGet", "TaskList"] {
            let spec = main_scope
                .get(&crate::domain::published_language::ToolName::new(name))
                .unwrap();
            assert_eq!(spec.required_capabilities(), Caps::TaskRead);
        }
        for name in [
            "TaskCreate",
            "TaskUpdate",
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
    fn sub_agent_scope_characterization_is_exact() {
        assert_eq!(names_for(BuiltinRegistryScope::SubAgent), set(SUB_AGENT));
    }
}
