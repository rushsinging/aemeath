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
pub(crate) mod tests {
    use super::*;
    use std::collections::{HashMap, HashSet};
    use std::sync::Mutex;

    /// In-memory fake for unit testing transition rules without real git.
    #[derive(Default)]
    pub struct FakeGit {
        pub common_dir: HashMap<PathBuf, PathBuf>,
        pub toplevel: HashMap<PathBuf, PathBuf>,
        pub worktrees: HashSet<PathBuf>,
        pub worktree_probe_error: Option<String>,
        pub added: Mutex<Vec<PathBuf>>,
        pub branches: HashMap<PathBuf, String>,
        pub non_git: HashSet<PathBuf>,
    }

    impl GitWorktreeOps for FakeGit {
        fn probe_repository(&self, path: &Path) -> Result<RepositoryProbe, GitProbeError> {
            if self.non_git.contains(path) {
                return Ok(RepositoryProbe::NonGit);
            }
            let canonical_top_level = self
                .toplevel
                .get(path)
                .cloned()
                .or_else(|| {
                    self.toplevel
                        .iter()
                        .filter(|(candidate, _)| path.starts_with(candidate))
                        .max_by_key(|(candidate, _)| candidate.components().count())
                        .map(|(_, top)| top.clone())
                })
                .unwrap_or_else(|| path.to_path_buf());
            let canonical_common_dir = self
                .common_dir
                .get(path)
                .or_else(|| self.common_dir.get(&canonical_top_level))
                .or_else(|| {
                    self.common_dir
                        .iter()
                        .filter(|(candidate, _)| path.starts_with(candidate))
                        .max_by_key(|(candidate, _)| candidate.components().count())
                        .map(|(_, common)| common)
                })
                .cloned()
                .ok_or(GitProbeError::InvalidOutput)?;
            Ok(RepositoryProbe::Git {
                canonical_top_level,
                canonical_common_dir,
                worktree_kind: if self.worktrees.contains(path) {
                    WorktreeKind::Linked
                } else {
                    WorktreeKind::Primary
                },
            })
        }

        fn show_toplevel(&self, path: &Path) -> Result<PathBuf, GitOperationError> {
            self.toplevel
                .get(path)
                .cloned()
                .ok_or(GitOperationError::InvalidOutput)
        }

        fn is_linked_worktree(&self, path: &Path) -> Result<bool, GitOperationError> {
            match &self.worktree_probe_error {
                Some(_) => Err(GitOperationError::CommandFailed { exit_code: None }),
                None => Ok(self.worktrees.contains(path)),
            }
        }

        fn worktree_add(
            &self,
            _repo: &Path,
            path: &Path,
            _branch: &str,
            _base: &str,
        ) -> Result<(), GitOperationError> {
            self.added.lock().unwrap().push(path.to_path_buf());
            Ok(())
        }

        fn current_branch(&self, path: &Path) -> Result<Option<String>, GitOperationError> {
            Ok(self.branches.get(path).cloned())
        }
    }

    #[test]
    fn fake_git_records_worktree_add() {
        let git = FakeGit::default();
        git.worktree_add(
            Path::new("/repo"),
            Path::new("/repo/.worktrees/x"),
            "x",
            "main",
        )
        .unwrap();
        assert_eq!(git.added.lock().unwrap().len(), 1);
    }
}
