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
}

/// Production git adapter. Spawns the `git` CLI (project may spawn; share may not).
pub struct GitCli;

impl GitWorktreeOps for GitCli {
    fn git_common_dir(&self, path: &Path) -> Result<PathBuf, String> {
        let output = std::process::Command::new("git")
            .args(["rev-parse", "--git-common-dir"])
            .current_dir(path)
            .output()
            .map_err(|e| format!("git rev-parse --git-common-dir 执行失败: {}", e))?;
        if !output.status.success() {
            return Err("无法获取 git common dir".to_string());
        }
        let s = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let p = PathBuf::from(&s);
        if p.is_absolute() {
            Ok(p.canonicalize().unwrap_or(p))
        } else {
            Ok(path
                .join(&s)
                .canonicalize()
                .unwrap_or_else(|_| path.join(&s)))
        }
    }

    fn show_toplevel(&self, path: &Path) -> Result<PathBuf, String> {
        let output = std::process::Command::new("git")
            .args(["rev-parse", "--show-toplevel"])
            .current_dir(path)
            .output()
            .map_err(|e| format!("git rev-parse 执行失败: {}", e))?;
        if !output.status.success() {
            return Err(format!("路径 {} 不是 git 仓库或 worktree", path.display()));
        }
        Ok(PathBuf::from(
            String::from_utf8_lossy(&output.stdout).trim().to_string(),
        ))
    }

    fn in_worktree(&self, path: &Path) -> bool {
        std::process::Command::new("git")
            .args(["rev-parse", "--git-dir"])
            .current_dir(path)
            .output()
            .ok()
            .and_then(|o| {
                o.status.success().then(|| {
                    String::from_utf8_lossy(&o.stdout)
                        .trim()
                        .contains("/.git/worktrees/")
                })
            })
            .unwrap_or(false)
    }

    fn worktree_add(
        &self,
        repo_root: &Path,
        path: &Path,
        branch: &str,
        base: &str,
    ) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("创建 worktree 父目录失败 {}: {}", parent.display(), e))?;
        }
        let output = std::process::Command::new("git")
            .args(["worktree", "add"])
            .arg(path)
            .args(["-b", branch, base])
            .current_dir(repo_root)
            .output()
            .map_err(|e| format!("git worktree add 执行失败: {}", e))?;
        if !output.status.success() {
            return Err(format!(
                "创建 worktree 失败：git worktree add {} -b {} {}\nstdout: {}\nstderr: {}",
                path.display(),
                branch,
                base,
                String::from_utf8_lossy(&output.stdout).trim(),
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
pub mod tests {
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
            _b: &str,
            _base: &str,
        ) -> Result<(), String> {
            self.added.lock().unwrap().push(path.to_path_buf());
            Ok(())
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
