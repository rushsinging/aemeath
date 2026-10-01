use std::path::{Path, PathBuf};

use share::session_types::WorktreeKind;

use crate::domain::types::{GitOperationError, GitProbeError};

/// Result of probing a path before a Project identity exists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RepositoryProbe {
    Git {
        canonical_top_level: PathBuf,
        canonical_common_dir: PathBuf,
        worktree_kind: WorktreeKind,
    },
    NonGit,
}

/// Outbound port for repository probing and git worktree operations.
pub(crate) trait GitWorktreeOps: Send + Sync {
    fn probe_repository(&self, path: &Path) -> Result<RepositoryProbe, GitProbeError>;
    fn show_toplevel(&self, path: &Path) -> Result<PathBuf, GitOperationError>;
    fn is_linked_worktree(&self, path: &Path) -> Result<bool, GitOperationError>;
    fn worktree_add(
        &self,
        repo_root: &Path,
        path: &Path,
        branch: &str,
        base: &str,
    ) -> Result<(), GitOperationError>;
    fn current_branch(&self, path: &Path) -> Result<Option<String>, GitOperationError>;
}

#[cfg(test)]
#[path = "git_tests.rs"]
mod tests;
