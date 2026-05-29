#![deny(clippy::print_stdout, clippy::print_stderr)]

pub mod api;

mod agent_tool;
mod ask_user;
mod bash;
mod brief;
mod config_tool;
mod file_edit;
mod file_read;
mod file_write;
mod glob_tool;
mod grep;
// 该 Tool 尚未注册到任何 register_* 入口，收窄可见性后内部 API 暂无消费方，
// 保留实现以备后续接线（refs #61 D3）。
#[allow(dead_code)]
mod list_mcp_resources;
mod lsp;
// mcp / mcp_manager 内含若干面向完整性的辅助类型/函数（diff、sse、validation 等），
// 当前仅部分经 tools::api 暴露消费，其余 re-export 保留备用（refs #61 D3）。
#[allow(dead_code, unused_imports)]
mod mcp;
#[allow(dead_code, unused_imports)]
mod mcp_manager;
mod mcp_tool; // McpTool is dynamically created, not statically registered
mod memory_tool;
// path_security 保留 *_from_base 之外的便捷包装（validate_and_normalize_path 等），
// 当前仅 *_from_base 变体被各 Tool 调用，包装函数保留备用（refs #61 D3）。
#[allow(dead_code)]
mod path_security;
mod plan_mode;
// 同 list_mcp_resources：尚未注册的 MCP 资源读取 Tool，保留实现（refs #61 D3）。
#[allow(dead_code)]
mod read_mcp_resource;
mod skill_tool;
mod sleep;
mod task_create;
mod task_get;
mod task_list;
mod task_list_complete;
mod task_list_create;
mod task_output;
mod task_stop;
mod task_update;
mod tool_search;
mod web_fetch;
mod web_search;
mod worktree;

// Re-export McpTool for dynamic creation (consumed via tools::api).
pub use mcp_tool::McpTool;

use share::skill_ops::Skill;
use share::task_ops::TaskStore;
use share::tool::ToolRegistry;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

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
    registry.register(Box::new(task_output::TaskOutputTool {
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
    registry.register(Box::new(task_stop::TaskStopTool {
        store: task_store.clone(),
    }));
    registry.register(Box::new(task_output::TaskOutputTool { store: task_store }));

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
            "TaskOutput",
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
