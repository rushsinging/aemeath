//! Tool 注册编排：将各 business 层 Tool 实现注册到 `ToolRegistry`。

use crate::business::{
    agent_tool, ask_user, bash, brief, config_tool, file_edit, file_read, file_write, glob_tool,
    grep, lsp, memory_tool, plan_mode, skill_tool, sleep, task_create, task_get, task_list,
    task_list_complete, task_list_create, task_stop, task_update, tool_search, web_fetch,
    web_search, worktree,
};
use share::skill_ops::Skill;
use std::collections::HashMap;
use std::sync::Arc;
use storage::api::TaskStore;
use tokio::sync::Mutex;

use super::tool_registry::ToolRegistry;

pub fn register_all_tools(
    registry: &ToolRegistry,
    task_store: Arc<TaskStore>,
    skills: Arc<Mutex<HashMap<String, Skill>>>,
) {
    // Core tools
    registry.register(Box::new(bash::BashTool));
    registry.register(Box::new(file_read::FileReadTool));
    registry.register(Box::new(file_write::FileWriteTool));
    registry.register(Box::new(file_edit::FileEditTool));
    registry.register(Box::new(glob_tool::GlobTool));
    registry.register(Box::new(grep::GrepTool));
    registry.register(Box::new(lsp::LspTool));

    // Web tools
    registry.register(Box::new(web_fetch::WebFetchTool));
    registry.register(Box::new(web_search::WebSearchTool));

    // Agent tools
    registry.register(Box::new(agent_tool::AgentTool {
        store: task_store.clone(),
    }));

    // Task management tools
    registry.register(Box::new(task_create::TaskCreateTool {
        store: task_store.clone(),
    }));
    registry.register(Box::new(task_update::TaskUpdateTool {
        store: task_store.clone(),
    }));
    registry.register(Box::new(task_list::TaskListTool {
        store: task_store.clone(),
    }));
    registry.register(Box::new(task_list_create::TaskListCreateTool {
        store: task_store.clone(),
    }));
    registry.register(Box::new(task_list_complete::TaskListCompleteTool {
        store: task_store.clone(),
    }));
    registry.register(Box::new(task_get::TaskGetTool {
        store: task_store.clone(),
    }));
    registry.register(Box::new(task_stop::TaskStopTool {
        store: task_store.clone(),
    }));

    // Skill and memory tools (MCP tools are dynamically registered)
    registry.register(Box::new(skill_tool::SkillTool { skills }));
    registry.register(Box::new(memory_tool::MemoryTool));

    // Utility tools
    registry.register(Box::new(config_tool::ConfigTool));
    registry.register(Box::new(sleep::SleepTool));
    registry.register(Box::new(ask_user::AskUserQuestionTool));
    registry.register(Box::new(brief::BriefTool));

    // Tool discovery
    registry.register(Box::new(tool_search::ToolSearchTool));

    // Plan mode tools
    registry.register(Box::new(plan_mode::EnterPlanModeTool));
    registry.register(Box::new(plan_mode::ExitPlanModeTool));

    // Worktree tools
    registry.register(Box::new(worktree::EnterWorktreeTool));
    registry.register(Box::new(worktree::ExitWorktreeTool));
}

pub fn register_subagent_tools(
    registry: &mut ToolRegistry,
    _task_store: Arc<TaskStore>,
    skills: Arc<Mutex<HashMap<String, Skill>>>,
) {
    // Core tools
    registry.register(Box::new(bash::BashTool));
    registry.register(Box::new(file_read::FileReadTool));
    registry.register(Box::new(file_write::FileWriteTool));
    registry.register(Box::new(file_edit::FileEditTool));
    registry.register(Box::new(glob_tool::GlobTool));
    registry.register(Box::new(grep::GrepTool));
    registry.register(Box::new(lsp::LspTool));

    // Web tools
    registry.register(Box::new(web_fetch::WebFetchTool));
    registry.register(Box::new(web_search::WebSearchTool));

    // Skill and memory tools (MCP tools are dynamically registered)
    registry.register(Box::new(skill_tool::SkillTool { skills }));
    registry.register(Box::new(memory_tool::MemoryTool));

    // Utility tools that do not coordinate with the user or parent task list
    registry.register(Box::new(config_tool::ConfigTool));
    registry.register(Box::new(sleep::SleepTool));
    registry.register(Box::new(brief::BriefTool));
    registry.register(Box::new(tool_search::ToolSearchTool));
}

pub fn register_all_tools_except_agent(
    registry: &ToolRegistry,
    task_store: Arc<TaskStore>,
    skills: Arc<Mutex<HashMap<String, Skill>>>,
) {
    // Core tools
    registry.register(Box::new(bash::BashTool));
    registry.register(Box::new(file_read::FileReadTool));
    registry.register(Box::new(file_write::FileWriteTool));
    registry.register(Box::new(file_edit::FileEditTool));
    registry.register(Box::new(glob_tool::GlobTool));
    registry.register(Box::new(grep::GrepTool));
    registry.register(Box::new(lsp::LspTool));

    // Web tools
    registry.register(Box::new(web_fetch::WebFetchTool));
    registry.register(Box::new(web_search::WebSearchTool));

    // Task management tools
    registry.register(Box::new(task_create::TaskCreateTool {
        store: task_store.clone(),
    }));
    registry.register(Box::new(task_update::TaskUpdateTool {
        store: task_store.clone(),
    }));
    registry.register(Box::new(task_list::TaskListTool {
        store: task_store.clone(),
    }));
    registry.register(Box::new(task_list_create::TaskListCreateTool {
        store: task_store.clone(),
    }));
    registry.register(Box::new(task_list_complete::TaskListCompleteTool {
        store: task_store.clone(),
    }));
    registry.register(Box::new(task_get::TaskGetTool {
        store: task_store.clone(),
    }));
    registry.register(Box::new(task_stop::TaskStopTool { store: task_store }));

    // Skill and memory tools (MCP tools are dynamically registered)
    registry.register(Box::new(skill_tool::SkillTool { skills }));
    registry.register(Box::new(memory_tool::MemoryTool));

    // Utility tools
    registry.register(Box::new(config_tool::ConfigTool));
    registry.register(Box::new(sleep::SleepTool));
    registry.register(Box::new(ask_user::AskUserQuestionTool));
    registry.register(Box::new(brief::BriefTool));

    // Worktree tools
    registry.register(Box::new(worktree::EnterWorktreeTool));
    registry.register(Box::new(worktree::ExitWorktreeTool));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_register_subagent_tools_excludes_coordination_tools() {
        let mut registry = ToolRegistry::new();
        let task_store = Arc::new(TaskStore::new());
        let skills = Arc::new(Mutex::new(HashMap::new()));

        register_subagent_tools(&mut registry, task_store, skills);

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
}
