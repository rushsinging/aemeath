//! Typed tool result structs for the 31 built-in tools.
//!
//! # Module strategy (方案 D)
//!
//! The authoritative definitions live in `tools::types::*`. This module
//! is a **thin re-export layer** so that:
//!
//! - existing imports of the form `use sdk::tool_result::{Tool}Result` keep
//!   working unchanged,
//! - `apps/cli` (TUI) and any future server consumer can deserialize typed
//!   payloads without depending on the business layer (`agent/features/tools`),
//! - the workspace dependency graph stays clean (`sdk → share` is the only
//!   `sdk → business` style edge, and the architecture guard whitelists it).
//!
//! When the `agent/features/server` crate lands, it can layer a
//! `WireSchema` derive on top of these re-exports to expose OpenAPI /
//! Protobuf schemas without touching the SDK surface.
//!
//! See `docs/superpowers/plans/2026-06-18-tool-display-structured-data.md`
//! Phase 0a (方案 D) for the full design.
pub use tools::types::agent::AgentResult;
pub use tools::types::ask_user::AskUserQuestionResult;
pub use tools::types::bash::BashResult;
pub use tools::types::brief::BriefResult;
pub use tools::types::edit::EditResult;
pub use tools::types::enter_worktree::EnterWorktreeResult;
pub use tools::types::exit_worktree::ExitWorktreeResult;
pub use tools::types::glob::GlobResult;
pub use tools::types::grep::GrepResult;
pub use tools::types::list_mcp_resources::ListMcpResourcesResult;
pub use tools::types::lsp::LspResult;
pub use tools::types::mcp_manager::McpManagerResult;
pub use tools::types::mcp_tool::McpToolResult;
pub use tools::types::memory::MemoryResult;
pub use tools::types::plan_mode::PlanModeResult;
pub use tools::types::read::ReadResult;
pub use tools::types::read_mcp_resource::ReadMcpResourceResult;
pub use tools::types::skill::SkillResult;
pub use tools::types::task_create::TaskCreateResult;
pub use tools::types::task_get::TaskGetResult;
pub use tools::types::task_list::TaskListResult;
pub use tools::types::task_list_complete::TaskListCompleteResult;
pub use tools::types::task_list_create::TaskListCreateResult;
pub use tools::types::task_stop::TaskStopResult;
pub use tools::types::task_update::TaskUpdateResult;
pub use tools::types::tool_search::ToolSearchResult;
pub use tools::types::web_fetch::WebFetchResult;
pub use tools::types::web_search::WebSearchResult;
pub use tools::types::write::WriteResult;

// Re-export the module sub-paths (e.g. `sdk::tool_result::read`) so that
// any code that imports the module form keeps compiling. Each submodule
// is itself a thin `pub use tools::types::...;` re-export.
pub mod agent;
pub mod ask_user;
pub mod bash;
pub mod brief;
pub mod edit;
pub mod enter_worktree;
pub mod exit_worktree;
pub mod glob;
pub mod grep;
pub mod list_mcp_resources;
pub mod lsp;
pub mod mcp_manager;
pub mod mcp_tool;
pub mod memory;
pub mod plan_mode;
pub mod read;
pub mod read_mcp_resource;
pub mod skill;
pub mod task_create;
pub mod task_get;
pub mod task_list;
pub mod task_list_complete;
pub mod task_list_create;
pub mod task_stop;
pub mod task_update;
pub mod tool_search;
pub mod web_fetch;
pub mod web_search;
pub mod write;
