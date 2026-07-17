use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use share::session_types::WorktreeKind;

use crate::domain::git::{GitWorktreeOps, RepositoryProbe};
use crate::domain::types::{GitOperationError, GitProbeError};

/// Production git adapter. Spawns the `git` CLI (project may spawn; share may not).
pub(crate) struct GitCli;

fn probe_spawn(error: std::io::Error) -> GitProbeError {
    match error.kind() {
        ErrorKind::NotFound => GitProbeError::GitUnavailable,
        ErrorKind::PermissionDenied => GitProbeError::PermissionDenied,
        _ => GitProbeError::CommandFailed { exit_code: None },
    }
}

fn operation_spawn(error: std::io::Error) -> GitOperationError {
    match error.kind() {
        ErrorKind::NotFound => GitOperationError::GitUnavailable,
        ErrorKind::PermissionDenied => GitOperationError::PermissionDenied,
        _ => GitOperationError::CommandFailed { exit_code: None },
    }
}

fn operation_output(output: Output) -> Result<String, GitOperationError> {
    if !output.status.success() {
        return Err(GitOperationError::CommandFailed {
            exit_code: output.status.code(),
        });
    }
    let value = std::str::from_utf8(&output.stdout)
        .map_err(|_| GitOperationError::InvalidOutput)?
        .trim();
    if value.is_empty() {
        Err(GitOperationError::InvalidOutput)
    } else {
        Ok(value.to_owned())
    }
}

fn resolve_git_path(base: &Path, value: &str) -> Result<PathBuf, GitProbeError> {
    let path = PathBuf::from(value);
    let absolute = if path.is_absolute() {
        path
    } else {
        base.join(path)
    };
    absolute
        .canonicalize()
        .map_err(|_| GitProbeError::InvalidOutput)
}

fn git_command() -> Command {
    let mut command = Command::new("git");
    command.env("LC_ALL", "C").env("LANG", "C");
    command
}

impl GitWorktreeOps for GitCli {
    fn probe_repository(&self, path: &Path) -> Result<RepositoryProbe, GitProbeError> {
        let output = git_command()
            .args([
                "rev-parse",
                "--show-toplevel",
                "--git-common-dir",
                "--git-dir",
            ])
            .current_dir(path)
            .output()
            .map_err(probe_spawn)?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).to_ascii_lowercase();
            if stderr.contains("not a git repository") {
                return Ok(RepositoryProbe::NonGit);
            }
            if stderr.contains("permission denied") {
                return Err(GitProbeError::PermissionDenied);
            }
            return Err(GitProbeError::CommandFailed {
                exit_code: output.status.code(),
            });
        }
        let stdout =
            std::str::from_utf8(&output.stdout).map_err(|_| GitProbeError::InvalidOutput)?;
        let mut lines = stdout.lines().map(str::trim);
        let top = lines
            .next()
            .filter(|s| !s.is_empty())
            .ok_or(GitProbeError::InvalidOutput)?;
        let common = lines
            .next()
            .filter(|s| !s.is_empty())
            .ok_or(GitProbeError::InvalidOutput)?;
        let git_dir = lines
            .next()
            .filter(|s| !s.is_empty())
            .ok_or(GitProbeError::InvalidOutput)?;
        if lines.next().is_some() {
            return Err(GitProbeError::InvalidOutput);
        }
        let canonical_top_level = PathBuf::from(top)
            .canonicalize()
            .map_err(|_| GitProbeError::InvalidOutput)?;
        let canonical_common_dir = resolve_git_path(path, common)?;
        let canonical_git_dir = resolve_git_path(path, git_dir)?;
        let worktree_kind = if canonical_git_dir == canonical_common_dir {
            WorktreeKind::Primary
        } else {
            WorktreeKind::Linked
        };
        Ok(RepositoryProbe::Git {
            canonical_top_level,
            canonical_common_dir,
            worktree_kind,
        })
    }

    fn show_toplevel(&self, path: &Path) -> Result<PathBuf, GitOperationError> {
        let value = operation_output(
            git_command()
                .args(["rev-parse", "--show-toplevel"])
                .current_dir(path)
                .output()
                .map_err(operation_spawn)?,
        )?;
        PathBuf::from(value)
            .canonicalize()
            .map_err(|_| GitOperationError::InvalidOutput)
    }

    fn is_linked_worktree(&self, path: &Path) -> Result<bool, GitOperationError> {
        match self.probe_repository(path) {
            Ok(RepositoryProbe::Git { worktree_kind, .. }) => {
                Ok(worktree_kind == WorktreeKind::Linked)
            }
            Ok(RepositoryProbe::NonGit) => Ok(false),
            Err(GitProbeError::GitUnavailable) => Err(GitOperationError::GitUnavailable),
            Err(GitProbeError::PermissionDenied) => Err(GitOperationError::PermissionDenied),
            Err(GitProbeError::CommandFailed { exit_code }) => {
                Err(GitOperationError::CommandFailed { exit_code })
            }
            Err(GitProbeError::InvalidOutput) => Err(GitOperationError::InvalidOutput),
        }
    }

    fn worktree_add(
        &self,
        repo_root: &Path,
        path: &Path,
        branch: &str,
        base: &str,
    ) -> Result<(), GitOperationError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(operation_spawn)?;
        }
        let output = git_command()
            .args(["worktree", "add"])
            .arg(path)
            .args(["-b", branch, base])
            .current_dir(repo_root)
            .output()
            .map_err(operation_spawn)?;
        if output.status.success() {
            Ok(())
        } else {
            Err(GitOperationError::CommandFailed {
                exit_code: output.status.code(),
            })
        }
    }

    fn current_branch(&self, path: &Path) -> Result<Option<String>, GitOperationError> {
        let output = git_command()
            .args(["rev-parse", "--abbrev-ref", "HEAD"])
            .current_dir(path)
            .output()
            .map_err(operation_spawn)?;
        let branch = operation_output(output)?;
        if branch == "HEAD" {
            Ok(None)
        } else {
            Ok(Some(branch))
        }
    }
}
