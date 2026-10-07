//! Built-in tool registration and named registry-scope assembly.

use crate::adapters::{
    agent_tool, ask_user, background_tasks, bash, brief, file_edit, file_read, file_write,
    glob_tool, grep, memory_tool, skill_tool, task_block_by, task_create, task_get, task_list,
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

pub(crate) fn profile_for(scope: BuiltinRegistryScope, main_parent: &ToolProfile) -> ToolProfile {
    // sub 默认集以 capability 位为唯一载体：Read|Write|Execute|NetworkAccess。
    // Memory/Brief/ToolSearch 归 All 位（main 专属），默认对受限 profile 隐藏
    // （用户决策：empty-caps 不无脑放行）；skill 走 Read 位。
    let requested = match scope {
        BuiltinRegistryScope::Main => Caps::all(),
        BuiltinRegistryScope::SubAgent => {
            Caps::Read | Caps::Write | Caps::Execute | Caps::NetworkAccess
        }
    };

    match scope {
        BuiltinRegistryScope::Main => *main_parent,
        BuiltinRegistryScope::SubAgent => ToolProfile::derive_restricted(main_parent, requested)
            .expect("built-in child profiles must only restrict the main profile"),
    }
}

pub(crate) fn register_named_scope(
    registry: &ToolRegistry,
    task_access: Arc<dyn TaskAccess>,
    memory_source: Arc<dyn MemoryPortSource>,
    workspace_control: Arc<dyn project::WorkspaceControl>,
    skill_loader: Arc<dyn crate::domain::SkillLoadPort>,
    background_source: Arc<dyn crate::domain::background_task_port::BackgroundTaskAccessSource>,
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
        Caps::Read | Caps::Execute,
        bash::BashTool {
            control: workspace_control.clone()
        }
    );
    builtin!("Read", Caps::Read, file_read::FileReadTool);
    builtin!("Write", Caps::Read | Caps::Write, file_write::FileWriteTool);
    builtin!("Edit", Caps::Read | Caps::Write, file_edit::FileEditTool);
    builtin!("Glob", Caps::Read, glob_tool::GlobTool);
    builtin!("Grep", Caps::Read, grep::GrepTool);
    builtin!("WebFetch", Caps::NetworkAccess, web_fetch::WebFetchTool);
    builtin!(
        "BackgroundTasks",
        Caps::Read,
        background_tasks::BackgroundTasksTool {
            source: background_source.clone()
        }
    );
    builtin!("WebSearch", Caps::NetworkAccess, web_search::WebSearchTool);
    builtin!("Agent", Caps::Dispatch, agent_tool::AgentTool);
    builtin!(
        "TaskCreate",
        Caps::TaskWrite,
        task_create::TaskCreateTool {
            access: task_access.clone()
        }
    );
    builtin!(
        "TaskUpdate",
        Caps::TaskWrite,
        task_update::TaskUpdateTool {
            access: task_access.clone()
        }
    );
    builtin!(
        "TaskBlockBy",
        Caps::TaskWrite,
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
        Caps::TaskWrite,
        task_list_create::TaskListCreateTool {
            access: task_access.clone()
        }
    );
    builtin!(
        "TaskListComplete",
        Caps::TaskWrite,
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
        Caps::TaskWrite,
        task_stop::TaskStopTool {
            access: task_access.clone()
        }
    );
    builtin!(
        "MemoryAdd",
        Caps::All,
        memory_tool::MemoryAddTool {
            source: memory_source.clone()
        }
    );
    builtin!(
        "MemorySearch",
        Caps::All,
        memory_tool::MemorySearchTool {
            source: memory_source.clone()
        }
    );
    builtin!(
        "MemoryList",
        Caps::All,
        memory_tool::MemoryListTool {
            source: memory_source.clone()
        }
    );
    builtin!(
        "MemoryUpdate",
        Caps::All,
        memory_tool::MemoryUpdateTool {
            source: memory_source.clone()
        }
    );
    builtin!(
        "MemoryDelete",
        Caps::All,
        memory_tool::MemoryDeleteTool {
            source: memory_source.clone()
        }
    );
    builtin!(
        "Skill",
        Caps::Read,
        skill_tool::SkillTool::new(skill_loader)
    );
    builtin!(
        "AskUserQuestion",
        Caps::Interact,
        ask_user::AskUserQuestionTool
    );
    builtin!("Brief", Caps::All, brief::BriefTool);
    builtin!("ToolSearch", Caps::All, tool_search::ToolSearchTool);
    builtin!(
        "EnterWorktree",
        Caps::Read | Caps::WorkspaceControl,
        worktree::EnterWorktreeTool {
            control: workspace_control.clone()
        }
    );
    builtin!(
        "ExitWorktree",
        Caps::Read | Caps::WorkspaceControl,
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
#[path = "registry_tests.rs"]
mod tests;
