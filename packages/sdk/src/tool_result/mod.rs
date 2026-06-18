//! Typed tool result structs for the 31 built-in tools.
//!
//! Each tool defines its own R struct here so that:
//! - tools business layer (`agent/features/tools`) can return typed results via
//!   `Tool::Result` associated type.
//! - runtime (`agent/features/runtime`) can adapt `serde_json::Value` back to
//!   the typed R struct at the tool call boundary.
//! - TUI (`apps/cli`) and future server mode (`agent/features/server`) can
//!   deserialize the typed payload without depending on the business layer.
//!
//! See `docs/superpowers/plans/2026-06-18-tool-display-structured-data.md`
//! Phase 0a for the full design.

pub mod read;
pub mod write;
pub mod edit;
pub mod glob;
pub mod grep;
pub mod web_fetch;
pub mod web_search;
pub mod bash;
pub mod sleep;
pub mod agent;
pub mod ask_user;
pub mod enter_worktree;
pub mod exit_worktree;
pub mod brief;
pub mod config_tool;
pub mod lsp;
pub mod plan_mode;
pub mod memory;
pub mod skill;
pub mod task_create;
pub mod task_get;
pub mod task_list;
pub mod task_stop;
pub mod task_update;
pub mod task_list_create;
pub mod task_list_complete;
pub mod tool_search;
pub mod mcp_tool;
pub mod mcp_manager;
pub mod list_mcp_resources;
pub mod read_mcp_resource;
