use std::path::{Path, PathBuf};

/// Outbound port for git worktree operations used by workspace transition rules.
pub trait GitWorktreeOps: Send + Sync {
    fn git_common_dir(&self, path: &Path) -> Result<PathBuf, String>;
    fn show_toplevel(&self, path: &Path) -> Result<PathBuf, String>;
    fn in_worktree(&self, path: &Path) -> bool;
    fn worktree_add(
        &self,
        repo_root: &Path,
        path: &Path,
        branch: &str,
        base: &str,
    ) -> Result<(), String>;
    /// 当前分支名。detached HEAD / 无分支时返回 `Ok(None)`。
    fn current_branch(&self, path: &Path) -> Result<Option<String>, String>;
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
        pub added: Mutex<Vec<PathBuf>>,
        /// 按路径返回的当前分支名（缺省时返回 `Ok(None)`）。
        pub branches: HashMap<PathBuf, String>,
    }

    impl GitWorktreeOps for FakeGit {
        fn git_common_dir(&self, path: &Path) -> Result<PathBuf, String> {
            self.common_dir
                .get(path)
                .cloned()
                .ok_or_else(|| "no common dir".into())
        }

        fn show_toplevel(&self, path: &Path) -> Result<PathBuf, String> {
            self.toplevel
                .get(path)
                .cloned()
                .ok_or_else(|| "not a repo".into())
        }

        fn in_worktree(&self, path: &Path) -> bool {
            self.worktrees.contains(path)
        }

        fn worktree_add(
            &self,
            _repo: &Path,
            path: &Path,
            _branch: &str,
            _base: &str,
        ) -> Result<(), String> {
            self.added.lock().unwrap().push(path.to_path_buf());
            Ok(())
        }

        fn current_branch(&self, path: &Path) -> Result<Option<String>, String> {
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
