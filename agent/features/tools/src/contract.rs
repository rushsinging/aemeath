//! Published language for the tools feature.
//!
//! This module exposes tool-domain DTOs and shared-kernel tool types without
//! exposing tool execution internals.

pub mod agent_port;
pub mod context;
pub mod tool;

pub use agent_port::{AgentRunRequest, AgentRunner};
pub use context::ToolContext;
pub use share::tool::{AgentToolCallProgress, ImageData, ToolResult};
pub use tool::Tool;

use project::api::{self as project_api, WorktreeWorkingContext};
use share::session_types::WorkspaceContext;

/// Projection helpers from tools runtime context into project worktree context.
pub trait WorktreeContextExt {
    fn worktree_working_context(&self) -> WorktreeWorkingContext;

    fn workspace_context(&self) -> WorkspaceContext {
        let wc = self.worktree_working_context();
        project_api::workspace_context_from_worktree_context(&wc)
    }
}

impl WorktreeContextExt for ToolContext {
    fn worktree_working_context(&self) -> WorktreeWorkingContext {
        WorktreeWorkingContext {
            working_root: self.working_root.clone(),
            path_base: self.path_base.clone(),
            context_stack: self.context_stack.clone(),
        }
    }
}

pub use crate::business::mcp::{McpServerConfig, McpToolDef, McpTransportKind};
