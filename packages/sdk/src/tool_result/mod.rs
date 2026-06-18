//! Typed tool result structs for the 31 built-in tools.
//!
//! Each tool defines its own R struct here so that:
//! - runtime (`agent/features/runtime`) can adapt `serde_json::Value` back to
//!   the typed R struct at the tool call boundary via `ToolResultAdapter::from_raw`.
//! - TUI (`apps/cli`) and future server mode (`agent/features/server`) can
//!   deserialize the typed payload without depending on the business layer.
//!
//! See `docs/superpowers/plans/2026-06-18-tool-display-structured-data.md`
//! Phase 0a for the full design.
pub mod agent;
pub use agent::AgentResult;
pub mod ask_user;
pub use ask_user::AskUserResult;
pub mod bash;
pub use bash::BashResult;
pub mod brief;
pub use brief::BriefResult;
pub mod config_tool;
pub use config_tool::ConfigToolResult;
pub mod edit;
pub use edit::EditResult;
pub mod enter_worktree;
pub use enter_worktree::EnterWorktreeResult;
pub mod exit_worktree;
pub use exit_worktree::ExitWorktreeResult;
pub mod glob;
pub use glob::GlobResult;
pub mod grep;
pub use grep::GrepResult;
pub mod list_mcp_resources;
pub use list_mcp_resources::ListMcpResourcesResult;
pub mod lsp;
pub use lsp::LspResult;
pub mod mcp_manager;
pub use mcp_manager::McpManagerResult;
pub mod mcp_tool;
pub use mcp_tool::McpToolResult;
pub mod memory;
pub use memory::MemoryResult;
pub mod plan_mode;
pub use plan_mode::PlanModeResult;
pub mod read;
pub use read::ReadResult;
pub mod read_mcp_resource;
pub use read_mcp_resource::ReadMcpResourceResult;
pub mod skill;
pub use skill::SkillResult;
pub mod sleep;
pub use sleep::SleepResult;
pub mod task_create;
pub use task_create::TaskCreateResult;
pub mod task_get;
pub use task_get::TaskGetResult;
pub mod task_list;
pub use task_list::TaskListResult;
pub mod task_list_complete;
pub use task_list_complete::TaskListCompleteResult;
pub mod task_list_create;
pub use task_list_create::TaskListCreateResult;
pub mod task_stop;
pub use task_stop::TaskStopResult;
pub mod task_update;
pub use task_update::TaskUpdateResult;
pub mod tool_search;
pub use tool_search::ToolSearchResult;
pub mod web_fetch;
pub use web_fetch::WebFetchResult;
pub mod web_search;
pub use web_search::WebSearchResult;
pub mod write;
pub use write::WriteResult;
