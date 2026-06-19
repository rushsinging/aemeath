//! Typed tool result structs for the 31 built-in tools.
//!
//! # Module strategy (方案 D)
//!
//! The authoritative definitions live in `share::tool::types::*`. This module
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
pub use share::tool::types::agent::AgentResult;
pub use share::tool::types::ask_user::AskUserQuestionResult;
pub use share::tool::types::bash::BashResult;
pub use share::tool::types::brief::BriefResult;
pub use share::tool::types::edit::EditResult;
pub use share::tool::types::enter_worktree::EnterWorktreeResult;
pub use share::tool::types::exit_worktree::ExitWorktreeResult;
pub use share::tool::types::glob::GlobResult;
pub use share::tool::types::grep::GrepResult;
pub use share::tool::types::list_mcp_resources::ListMcpResourcesResult;
pub use share::tool::types::lsp::LspResult;
pub use share::tool::types::mcp_manager::McpManagerResult;
pub use share::tool::types::mcp_tool::McpToolResult;
pub use share::tool::types::memory::MemoryResult;
pub use share::tool::types::plan_mode::PlanModeResult;
pub use share::tool::types::read::ReadResult;
pub use share::tool::types::read_mcp_resource::ReadMcpResourceResult;
pub use share::tool::types::skill::SkillResult;
pub use share::tool::types::sleep::SleepResult;
pub use share::tool::types::task_create::TaskCreateResult;
pub use share::tool::types::task_get::TaskGetResult;
pub use share::tool::types::task_list::TaskListResult;
pub use share::tool::types::task_list_complete::TaskListCompleteResult;
pub use share::tool::types::task_list_create::TaskListCreateResult;
pub use share::tool::types::task_stop::TaskStopResult;
pub use share::tool::types::task_update::TaskUpdateResult;
pub use share::tool::types::tool_search::ToolSearchResult;
pub use share::tool::types::web_fetch::WebFetchResult;
pub use share::tool::types::web_search::WebSearchResult;
pub use share::tool::types::write::WriteResult;

// Re-export the module sub-paths (e.g. `sdk::tool_result::read`) so that
// any code that imports the module form keeps compiling. Each submodule
// is itself a thin `pub use share::tool::types::...;` re-export.
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
pub mod sleep;
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
