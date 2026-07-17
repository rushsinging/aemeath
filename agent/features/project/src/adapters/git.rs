use std::path::{Path, PathBuf};

use crate::domain::git::GitWorktreeOps;

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

    fn current_branch(&self, path: &Path) -> Result<Option<String>, String> {
        let output = std::process::Command::new("git")
            .args(["rev-parse", "--abbrev-ref", "HEAD"])
            .current_dir(path)
            .output()
            .map_err(|e| format!("git rev-parse --abbrev-ref HEAD 执行失败: {}", e))?;
        if !output.status.success() {
            return Ok(None);
        }
        let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if branch.is_empty() || branch == "HEAD" {
            Ok(None)
        } else {
            Ok(Some(branch))
        }
    }
}
