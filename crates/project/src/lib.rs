pub mod api;
pub mod worktree;
pub mod worktree_tools;

use aemeath_core::tool::ToolRegistry;

/// Register worktree tools into the given registry.
pub fn register_worktree_tools(registry: &ToolRegistry) {
    registry.register(Box::new(worktree_tools::EnterWorktreeTool));
    registry.register(Box::new(worktree_tools::ExitWorktreeTool));
}
