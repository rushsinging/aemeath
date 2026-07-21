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

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct WorktreeAddCall {
        pub repo_root: PathBuf,
        pub path: PathBuf,
        pub branch: String,
        pub base: String,
    }

    /// In-memory fake for unit testing transition rules without real git.
    #[derive(Default)]
    pub struct FakeGit {
        pub common_dir: HashMap<PathBuf, PathBuf>,
        pub toplevel: HashMap<PathBuf, PathBuf>,
        pub worktrees: HashSet<PathBuf>,
        pub worktree_probe_error: Option<String>,
        pub added: Mutex<Vec<WorktreeAddCall>>,
        pub branches: HashMap<PathBuf, String>,
        pub non_git: HashSet<PathBuf>,
    }

    impl FakeGit {
        fn added_worktree_for(&self, path: &Path) -> Option<WorktreeAddCall> {
            self.added
                .lock()
                .unwrap()
                .iter()
                .rev()
                .find(|call| path == call.path || path.starts_with(&call.path))
                .cloned()
        }
    }

    impl GitWorktreeOps for FakeGit {
        fn probe_repository(&self, path: &Path) -> Result<RepositoryProbe, GitProbeError> {
            if self.non_git.contains(path) {
                return Ok(RepositoryProbe::NonGit);
            }
            if let Some(call) = self.added_worktree_for(path) {
                let canonical_common_dir = self
                    .common_dir
                    .get(&call.repo_root)
                    .cloned()
                    .unwrap_or_else(|| call.repo_root.join(".git"));
                return Ok(RepositoryProbe::Git {
                    canonical_top_level: call.path,
                    canonical_common_dir,
                    worktree_kind: WorktreeKind::Linked,
                });
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
                .or_else(|| self.added_worktree_for(path).map(|call| call.path))
                .ok_or(GitOperationError::InvalidOutput)
        }

        fn is_linked_worktree(&self, path: &Path) -> Result<bool, GitOperationError> {
            match &self.worktree_probe_error {
                Some(_) => Err(GitOperationError::CommandFailed { exit_code: None }),
                None => {
                    Ok(self.worktrees.contains(path) || self.added_worktree_for(path).is_some())
                }
            }
        }

        fn worktree_add(
            &self,
            repo_root: &Path,
            path: &Path,
            branch: &str,
            base: &str,
        ) -> Result<(), GitOperationError> {
            std::fs::create_dir_all(path).map_err(|error| match error.kind() {
                std::io::ErrorKind::PermissionDenied => GitOperationError::PermissionDenied,
                _ => GitOperationError::CommandFailed { exit_code: None },
            })?;
            self.added.lock().unwrap().push(WorktreeAddCall {
                repo_root: repo_root.to_path_buf(),
                path: path.to_path_buf(),
                branch: branch.to_string(),
                base: base.to_string(),
            });
            Ok(())
        }

        fn current_branch(&self, path: &Path) -> Result<Option<String>, GitOperationError> {
            Ok(self.branches.get(path).cloned())
        }
    }

    #[test]
    fn fake_git_records_worktree_add() {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "aemeath_project_fake_git_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join(".worktrees/x");
        let git = FakeGit::default();
        git.worktree_add(&root, &path, "x", "main").unwrap();
        assert_eq!(
            git.added.lock().unwrap().as_slice(),
            &[WorktreeAddCall {
                repo_root: root.clone(),
                path,
                branch: "x".into(),
                base: "main".into(),
            }]
        );
        let _ = std::fs::remove_dir_all(root);
    }
}
