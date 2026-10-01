//! Project domain 常量（#1146 双轨归位）。

pub(crate) const DEFAULT_WORKTREE_BASE: &str = "main";
pub(crate) const UNNAMED_WORKSPACE_SEGMENT: &str = "workspace";

// ─── state.rs ───
#[cfg(test)]
/// 测试构造器默认：repo 根下 `.worktrees`（生产链路由 wiring 注入配置值）。
pub(crate) const DEFAULT_WORKTREE_DIR: &str = ".worktrees";
