#![deny(clippy::print_stdout, clippy::print_stderr)]

pub mod bash;
pub mod file_read;
pub mod file_write;
pub mod file_edit;
pub mod glob_tool;
pub mod grep;
pub mod lsp;
pub mod web_fetch;
pub mod web_search;
pub mod agent_tool;
pub mod task_create;
pub mod task_update;
pub mod task_list;
pub mod task_get;
pub mod task_stop;
pub mod task_output;
pub mod mcp_tool;  // McpTool is dynamically created, not statically registered
pub mod skill_tool;
pub mod memory_tool;
pub mod config_tool;
pub mod sleep;
pub mod ask_user;
pub mod tool_search;
pub mod list_mcp_resources;
pub mod read_mcp_resource;
pub mod plan_mode;
pub mod brief;
pub mod path_security;

// Re-export McpTool for dynamic creation
pub use mcp_tool::McpTool;

use aemeath_core::skill::Skill;
use aemeath_core::task::TaskStore;
use aemeath_core::tool::ToolRegistry;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

pub fn register_all_tools(
    registry: &mut ToolRegistry,
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
    registry.register(Box::new(agent_tool::AgentTool { store: task_store.clone() }));
    
    // Task management tools
    registry.register(Box::new(task_create::TaskCreateTool { store: task_store.clone() }));
    registry.register(Box::new(task_update::TaskUpdateTool { store: task_store.clone() }));
    registry.register(Box::new(task_list::TaskListTool { store: task_store.clone() }));
    registry.register(Box::new(task_get::TaskGetTool { store: task_store.clone() }));
    registry.register(Box::new(task_stop::TaskStopTool { store: task_store.clone() }));
    registry.register(Box::new(task_output::TaskOutputTool { store: task_store.clone() }));
    
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
}

pub fn register_all_tools_except_agent(
    registry: &mut ToolRegistry,
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
    registry.register(Box::new(task_create::TaskCreateTool { store: task_store.clone() }));
    registry.register(Box::new(task_update::TaskUpdateTool { store: task_store.clone() }));
    registry.register(Box::new(task_list::TaskListTool { store: task_store.clone() }));
    registry.register(Box::new(task_get::TaskGetTool { store: task_store.clone() }));
    registry.register(Box::new(task_stop::TaskStopTool { store: task_store.clone() }));
    registry.register(Box::new(task_output::TaskOutputTool { store: task_store }));
    
    // Skill and memory tools (MCP tools are dynamically registered)
    registry.register(Box::new(skill_tool::SkillTool { skills }));
    registry.register(Box::new(memory_tool::MemoryTool));
      
    // Utility tools
    registry.register(Box::new(config_tool::ConfigTool));
    registry.register(Box::new(sleep::SleepTool));
    registry.register(Box::new(ask_user::AskUserQuestionTool));
    registry.register(Box::new(brief::BriefTool));
}
