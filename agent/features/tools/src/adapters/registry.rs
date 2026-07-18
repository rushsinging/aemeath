//! Tool 注册编排：将各 business 层 Tool 实现注册到 `ToolRegistry`。

use crate::adapters::{
    agent_tool, ask_user, bash, brief, file_edit, file_read, file_write, glob_tool, grep, lsp,
    memory_tool, plan_mode, skill_tool, task_create, task_get, task_list, task_list_complete,
    task_list_create, task_stop, task_update, tool_search, web_fetch, web_search, worktree,
};
use share::skill_ops::Skill;
use std::collections::HashMap;
use std::sync::Arc;
use task::TaskAccess;
use tokio::sync::Mutex;

use super::tool_registry::ToolRegistry;

/// 工具集 profile：决定哪些工具被注册。取代历史的 3 个近重复 `register_*` 函数，
/// 工具清单只定义一次（见 [`register_tools`]）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolProfile {
    /// 主 agent：全部工具。
    Full,
    /// 子 agent：排除协调类工具（Agent / AskUserQuestion / Task* / Worktree / PlanMode）——
    /// 子 agent 不与用户交互、不操作父任务列表。
    SubAgent,
    /// 排除 Agent 自身（避免子 agent 递归派发），同时不含 ToolSearch / PlanMode。
    NoAgent,
}

impl ToolProfile {
    /// 该 profile 是否排除指定 registry 工具名。
    fn excludes(self, name: &str) -> bool {
        match self {
            ToolProfile::Full => false,
            ToolProfile::SubAgent => matches!(
                name,
                "Agent"
                    | "AskUserQuestion"
                    | "TaskCreate"
                    | "TaskUpdate"
                    | "TaskList"
                    | "TaskListCreate"
                    | "TaskListComplete"
                    | "TaskGet"
                    | "TaskStop"
                    | "EnterWorktree"
                    | "ExitWorktree"
                    | "EnterPlanMode"
                    | "ExitPlanMode"
            ),
            ToolProfile::NoAgent => {
                matches!(
                    name,
                    "Agent" | "ToolSearch" | "EnterPlanMode" | "ExitPlanMode"
                )
            }
        }
    }
}

/// 单一注册入口：按 `profile` 把内置工具注册到 `registry`。
///
/// 工具清单在此**定义一次**（DRY）；各 profile 通过 [`ToolProfile::excludes`] 过滤。
/// MCP 工具由 connector 动态注册，不在此列。
pub fn register_tools(
    registry: &ToolRegistry,
    task_access: Arc<dyn TaskAccess>,
    skills: Arc<Mutex<HashMap<String, Skill>>>,
    profile: ToolProfile,
) {
    macro_rules! reg {
        ($name:literal, $tool:expr) => {
            if !profile.excludes($name) {
                registry.register($tool);
            }
        };
    }

    // Core tools
    reg!("Bash", bash::BashTool);
    reg!("Read", file_read::FileReadTool);
    reg!("Write", file_write::FileWriteTool);
    reg!("Edit", file_edit::FileEditTool);
    reg!("Glob", glob_tool::GlobTool);
    reg!("Grep", grep::GrepTool);
    reg!("LSP", lsp::LspTool);

    // Web tools
    reg!("WebFetch", web_fetch::WebFetchTool);
    reg!("WebSearch", web_search::WebSearchTool);

    // Agent dispatch
    reg!("Agent", agent_tool::AgentTool);

    // Task management tools
    reg!(
        "TaskCreate",
        task_create::TaskCreateTool {
            access: task_access.clone(),
        }
    );
    reg!(
        "TaskUpdate",
        task_update::TaskUpdateTool {
            access: task_access.clone(),
        }
    );
    reg!(
        "TaskList",
        task_list::TaskListTool {
            access: task_access.clone(),
        }
    );
    reg!(
        "TaskListCreate",
        task_list_create::TaskListCreateTool {
            access: task_access.clone(),
        }
    );
    reg!(
        "TaskListComplete",
        task_list_complete::TaskListCompleteTool {
            access: task_access.clone(),
        }
    );
    reg!(
        "TaskGet",
        task_get::TaskGetTool {
            access: task_access.clone(),
        }
    );
    reg!(
        "TaskStop",
        task_stop::TaskStopTool {
            access: task_access.clone(),
        }
    );

    // Skill and memory tools (MCP tools are dynamically registered)
    reg!(
        "Skill",
        skill_tool::SkillTool {
            skills: skills.clone(),
        }
    );
    reg!("Memory", memory_tool::MemoryTool);

    // Utility tools
    reg!("AskUserQuestion", ask_user::AskUserQuestionTool);
    reg!("Brief", brief::BriefTool);

    // Tool discovery
    reg!("ToolSearch", tool_search::ToolSearchTool);

    // Plan mode tools
    reg!("EnterPlanMode", plan_mode::EnterPlanModeTool);
    reg!("ExitPlanMode", plan_mode::ExitPlanModeTool);

    // Worktree tools
    reg!("EnterWorktree", worktree::EnterWorktreeTool);
    reg!("ExitWorktree", worktree::ExitWorktreeTool);
}

/// 主 agent 全量工具（[`register_tools`] 的 `Full` profile 便捷封装）。
pub fn register_all_tools(
    registry: &ToolRegistry,
    task_access: Arc<dyn TaskAccess>,
    skills: Arc<Mutex<HashMap<String, Skill>>>,
) {
    register_tools(registry, task_access, skills, ToolProfile::Full);
}

/// 子 agent 工具集（排除协调类工具；[`ToolProfile::SubAgent`] 封装）。
pub fn register_subagent_tools(
    registry: &mut ToolRegistry,
    task_access: Arc<dyn TaskAccess>,
    skills: Arc<Mutex<HashMap<String, Skill>>>,
) {
    register_tools(registry, task_access, skills, ToolProfile::SubAgent);
}

/// 排除 Agent 的工具集（[`ToolProfile::NoAgent`] 封装）。
pub fn register_all_tools_except_agent(
    registry: &ToolRegistry,
    task_access: Arc<dyn TaskAccess>,
    skills: Arc<Mutex<HashMap<String, Skill>>>,
) {
    register_tools(registry, task_access, skills, ToolProfile::NoAgent);
}

#[cfg(test)]
mod tests {
    use super::*;
    use task::TaskStore;

    #[test]
    fn test_register_subagent_tools_excludes_coordination_tools() {
        let mut registry = ToolRegistry::new();
        let task_store = Arc::new(TaskStore::new());
        let task_access: Arc<dyn TaskAccess> = task_store.clone();
        let skills = Arc::new(Mutex::new(HashMap::new()));

        register_subagent_tools(&mut registry, task_access, skills);

        for forbidden in [
            "Agent",
            "AskUserQuestion",
            "TaskCreate",
            "TaskUpdate",
            "TaskList",
            "TaskListCreate",
            "TaskListComplete",
            "TaskGet",
            "TaskStop",
            "EnterWorktree",
            "ExitWorktree",
        ] {
            assert!(
                !registry.contains(forbidden),
                "{forbidden} should be unavailable to sub-agents"
            );
        }
        assert!(registry.contains("Read"));
        assert!(registry.contains("Grep"));
        assert!(registry.contains("Bash"));
        assert!(registry.contains("Skill"));
    }

    #[test]
    fn test_full_profile_registers_all_27_tools() {
        let registry = ToolRegistry::new();
        let task_store = Arc::new(TaskStore::new());
        let task_access: Arc<dyn TaskAccess> = task_store.clone();
        register_tools(
            &registry,
            task_access,
            Arc::new(Mutex::new(HashMap::new())),
            ToolProfile::Full,
        );
        for name in [
            "Bash",
            "Read",
            "Agent",
            "TaskCreate",
            "AskUserQuestion",
            "ToolSearch",
            "EnterPlanMode",
            "ExitPlanMode",
            "EnterWorktree",
            "ExitWorktree",
        ] {
            assert!(registry.contains(name), "Full 应包含 {name}");
        }
    }

    #[test]
    fn test_no_agent_profile_excludes_agent_toolsearch_planmode_only() {
        // NoAgent 的 quirk：排除 Agent / ToolSearch / PlanMode，但保留 Task / AskUser / Worktree。
        let registry = ToolRegistry::new();
        let task_store = Arc::new(TaskStore::new());
        let task_access: Arc<dyn TaskAccess> = task_store.clone();
        register_tools(
            &registry,
            task_access,
            Arc::new(Mutex::new(HashMap::new())),
            ToolProfile::NoAgent,
        );
        for excluded in ["Agent", "ToolSearch", "EnterPlanMode", "ExitPlanMode"] {
            assert!(!registry.contains(excluded), "NoAgent 应排除 {excluded}");
        }
        for included in [
            "Bash",
            "TaskCreate",
            "TaskStop",
            "AskUserQuestion",
            "EnterWorktree",
            "ExitWorktree",
        ] {
            assert!(registry.contains(included), "NoAgent 应保留 {included}");
        }
    }
}
