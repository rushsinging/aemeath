//! Built-in tool registration and named registry-scope assembly.

use crate::adapters::{
    agent_tool, ask_user, bash, brief, file_edit, file_read, file_write, glob_tool, grep, lsp,
    memory_tool, plan_mode, skill_tool, task_create, task_get, task_list, task_list_complete,
    task_list_create, task_stop, task_update, tool_search, web_fetch, web_search, worktree,
};
use crate::domain::memory_source::MemoryPortSource;
use crate::domain::published_language::ToolCapabilities as Caps;
use crate::domain::scope_profile::{
    is_authorized, RegistryScope, RegistryScopeBuilder, ToolProfile, ToolRegistrationSpec,
};
use share::skill_ops::Skill;
use std::collections::HashMap;
use std::sync::Arc;
use task::TaskAccess;
use tokio::sync::Mutex;

use super::tool_registry::ToolRegistry;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BuiltinRegistryScope {
    Main,
    SubAgent,
    /// Exact compatibility scope retained until #914.
    LegacyNoAgent,
}

impl BuiltinRegistryScope {
    fn name(self) -> &'static str {
        match self {
            Self::Main => "main",
            Self::SubAgent => "sub-agent",
            Self::LegacyNoAgent => "legacy-no-agent",
        }
    }
}

fn profile_for(scope: BuiltinRegistryScope, main_parent: &ToolProfile) -> ToolProfile {
    let requested = match scope {
        BuiltinRegistryScope::Main | BuiltinRegistryScope::LegacyNoAgent => Caps::all(),
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
        BuiltinRegistryScope::SubAgent | BuiltinRegistryScope::LegacyNoAgent => {
            ToolProfile::derive_restricted(main_parent, requested)
                .expect("built-in child profiles must only restrict the main profile")
        }
    }
}

fn belongs_to(scope: BuiltinRegistryScope, main: bool, sub: bool, no_agent: bool) -> bool {
    match scope {
        BuiltinRegistryScope::Main => main,
        BuiltinRegistryScope::SubAgent => sub,
        BuiltinRegistryScope::LegacyNoAgent => no_agent,
    }
}

fn register_named_scope(
    registry: &ToolRegistry,
    task_access: Arc<dyn TaskAccess>,
    skills: Arc<Mutex<HashMap<String, Skill>>>,
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
        ($name:literal, $caps:expr, [$main:literal, $sub:literal, $no_agent:literal], $tool:expr) => {{
            if belongs_to(selected_scope, $main, $sub, $no_agent) {
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
        [true, true, true],
        bash::BashTool {
            control: workspace_control.clone()
        }
    );
    builtin!(
        "Read",
        Caps::ReadWorkspace,
        [true, true, true],
        file_read::FileReadTool
    );
    builtin!(
        "Write",
        Caps::ReadWorkspace | Caps::WriteWorkspace,
        [true, true, true],
        file_write::FileWriteTool
    );
    builtin!(
        "Edit",
        Caps::ReadWorkspace | Caps::WriteWorkspace,
        [true, true, true],
        file_edit::FileEditTool
    );
    builtin!(
        "Glob",
        Caps::ReadWorkspace,
        [true, true, true],
        glob_tool::GlobTool
    );
    builtin!(
        "Grep",
        Caps::ReadWorkspace,
        [true, true, true],
        grep::GrepTool
    );
    builtin!(
        "LSP",
        Caps::ReadWorkspace | Caps::ExecuteProcess,
        [true, true, true],
        lsp::LspTool
    );
    builtin!(
        "WebFetch",
        Caps::NetworkAccess,
        [true, true, true],
        web_fetch::WebFetchTool
    );
    builtin!(
        "WebSearch",
        Caps::NetworkAccess,
        [true, true, true],
        web_search::WebSearchTool
    );
    builtin!(
        "Agent",
        Caps::AgentDispatch,
        [true, false, false],
        agent_tool::AgentTool
    );
    builtin!(
        "TaskCreate",
        Caps::TaskMutation,
        [true, false, true],
        task_create::TaskCreateTool {
            access: task_access.clone()
        }
    );
    builtin!(
        "TaskUpdate",
        Caps::TaskMutation,
        [true, false, true],
        task_update::TaskUpdateTool {
            access: task_access.clone()
        }
    );
    builtin!(
        "TaskList",
        Caps::TaskRead,
        [true, false, true],
        task_list::TaskListTool {
            access: task_access.clone()
        }
    );
    builtin!(
        "TaskListCreate",
        Caps::TaskMutation,
        [true, false, true],
        task_list_create::TaskListCreateTool {
            access: task_access.clone()
        }
    );
    builtin!(
        "TaskListComplete",
        Caps::TaskMutation,
        [true, false, true],
        task_list_complete::TaskListCompleteTool {
            access: task_access.clone()
        }
    );
    builtin!(
        "TaskGet",
        Caps::TaskRead,
        [true, false, true],
        task_get::TaskGetTool {
            access: task_access.clone()
        }
    );
    builtin!(
        "TaskStop",
        Caps::TaskMutation,
        [true, false, true],
        task_stop::TaskStopTool {
            access: task_access.clone()
        }
    );
    builtin!(
        "Skill",
        Caps::empty(),
        [true, true, true],
        skill_tool::SkillTool {
            skills: skills.clone()
        }
    );
    builtin!(
        "Memory",
        Caps::empty(),
        [true, true, true],
        memory_tool::MemoryTool {
            source: memory_source.clone(),
        }
    );
    builtin!(
        "AskUserQuestion",
        Caps::UserInteraction,
        [true, false, true],
        ask_user::AskUserQuestionTool
    );
    builtin!("Brief", Caps::empty(), [true, true, true], brief::BriefTool);
    builtin!(
        "ToolSearch",
        Caps::empty(),
        [true, true, false],
        tool_search::ToolSearchTool
    );
    builtin!(
        "EnterPlanMode",
        Caps::PlanControl,
        [true, false, false],
        plan_mode::EnterPlanModeTool
    );
    builtin!(
        "ExitPlanMode",
        Caps::PlanControl,
        [true, false, false],
        plan_mode::ExitPlanModeTool
    );
    builtin!(
        "EnterWorktree",
        Caps::ReadWorkspace | Caps::WorkspaceControl,
        [true, false, true],
        worktree::EnterWorktreeTool {
            control: workspace_control.clone()
        }
    );
    builtin!(
        "ExitWorktree",
        Caps::ReadWorkspace | Caps::WorkspaceControl,
        [true, false, true],
        worktree::ExitWorktreeTool {
            control: workspace_control.clone()
        }
    );

    let built_scope = scope.build();
    debug_assert_eq!(built_scope.name().as_str(), selected_scope.name());
    debug_assert_eq!(built_scope.len(), registry.len());
    debug_assert!(built_scope
        .iter()
        .all(|spec| built_scope.get(spec.name()).is_some()));
    built_scope
}

pub fn register_all_tools(
    registry: &ToolRegistry,
    task_access: Arc<dyn TaskAccess>,
    skills: Arc<Mutex<HashMap<String, Skill>>>,
    memory_source: Arc<dyn MemoryPortSource>,
    workspace_control: Arc<dyn project::WorkspaceControl>,
) {
    register_named_scope(
        registry,
        task_access,
        skills,
        memory_source,
        workspace_control,
        BuiltinRegistryScope::Main,
    );
}

pub fn register_subagent_tools(
    registry: &mut ToolRegistry,
    task_access: Arc<dyn TaskAccess>,
    skills: Arc<Mutex<HashMap<String, Skill>>>,
    memory_source: Arc<dyn MemoryPortSource>,
    workspace_control: Arc<dyn project::WorkspaceControl>,
) {
    register_named_scope(
        registry,
        task_access,
        skills,
        memory_source,
        workspace_control,
        BuiltinRegistryScope::SubAgent,
    );
}

/// Compatibility façade preserving the historical set exactly until #914.
pub fn register_all_tools_except_agent(
    registry: &ToolRegistry,
    task_access: Arc<dyn TaskAccess>,
    skills: Arc<Mutex<HashMap<String, Skill>>>,
    memory_source: Arc<dyn MemoryPortSource>,
    workspace_control: Arc<dyn project::WorkspaceControl>,
) {
    register_named_scope(
        registry,
        task_access,
        skills,
        memory_source,
        workspace_control,
        BuiltinRegistryScope::LegacyNoAgent,
    );
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
        register_named_scope(
            &registry,
            task_access,
            Arc::new(Mutex::new(HashMap::new())),
            test_memory_source(),
            control,
            scope,
        )
    }

    fn names_for(scope: BuiltinRegistryScope) -> BTreeSet<String> {
        assembled_scope(scope)
            .iter()
            .map(|spec| spec.name().as_str().to_owned())
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
        "LSP",
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
        "Skill",
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
        "LSP",
        "WebFetch",
        "WebSearch",
        "Skill",
        "Memory",
        "Brief",
        "ToolSearch",
    ];
    const NO_AGENT: &[&str] = &[
        "Bash",
        "Read",
        "Write",
        "Edit",
        "Glob",
        "Grep",
        "LSP",
        "WebFetch",
        "WebSearch",
        "TaskCreate",
        "TaskUpdate",
        "TaskList",
        "TaskListCreate",
        "TaskListComplete",
        "TaskGet",
        "TaskStop",
        "Skill",
        "Memory",
        "AskUserQuestion",
        "Brief",
        "EnterWorktree",
        "ExitWorktree",
    ];

    #[test]
    fn production_profiles_are_main_baseline_or_restricted_children() {
        let main = ToolProfile::baseline(Caps::all());
        let main_profile = profile_for(BuiltinRegistryScope::Main, &main);
        assert_eq!(main_profile.allowed_capabilities(), Caps::all());

        for child_scope in [
            BuiltinRegistryScope::SubAgent,
            BuiltinRegistryScope::LegacyNoAgent,
        ] {
            let child = profile_for(child_scope, &main);
            assert!(child
                .allowed_capabilities()
                .is_subset_of(main.allowed_capabilities()));
        }
        assert_ne!(
            profile_for(BuiltinRegistryScope::SubAgent, &main).allowed_capabilities(),
            main.allowed_capabilities()
        );
    }

    #[test]
    fn side_effect_capability_characterization_matches_builtin_behavior() {
        let main_scope = assembled_scope(BuiltinRegistryScope::Main);
        let lsp = main_scope
            .get(&crate::domain::published_language::ToolName::new("LSP"))
            .unwrap();
        assert_eq!(
            lsp.required_capabilities(),
            Caps::ReadWorkspace | Caps::ExecuteProcess,
            "LSP invokes cargo/npx/python/go/grep and may write compiler caches"
        );

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
    fn full_scope_characterization_is_exact() {
        assert_eq!(names_for(BuiltinRegistryScope::Main), set(FULL));
    }

    #[test]
    fn sub_agent_scope_characterization_is_exact() {
        assert_eq!(names_for(BuiltinRegistryScope::SubAgent), set(SUB_AGENT));
    }

    #[test]
    fn legacy_no_agent_scope_characterization_is_exact() {
        assert_eq!(
            names_for(BuiltinRegistryScope::LegacyNoAgent),
            set(NO_AGENT)
        );
    }
}
