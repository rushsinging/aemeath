use std::ffi::OsString;
use std::io::{self, ErrorKind};
use std::path::{Path, PathBuf};
use std::process::Command;

use share::session_types::WorktreeKind;

use crate::domain::git::{GitWorktreeOps, RepositoryProbe};
use crate::domain::types::{GitOperationError, GitProbeError};

/// Completed result of a spawned `git` invocation. Project-private value type
/// that decouples the git logic from `std::process::Output` so a runner can be
/// injected within the crate for tests.
pub(crate) struct GitCommandOutput {
    success: bool,
    exit_code: Option<i32>,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

/// Project-private SPI for spawning `git`. Not exported from the crate; the
/// production implementation shells out, while tests inject a scripted runner.
pub(crate) trait GitCommandRunner: Send + Sync {
    fn run(&self, cwd: &Path, args: &[OsString]) -> Result<GitCommandOutput, io::Error>;
}

/// Production runner: spawns the real `git` CLI with a fixed `C` locale so the
/// parsed output stays stable across environments.
struct SystemGitRunner;

impl GitCommandRunner for SystemGitRunner {
    fn run(&self, cwd: &Path, args: &[OsString]) -> Result<GitCommandOutput, io::Error> {
        let mut command = Command::new("git");
        command
            .env("LC_ALL", "C")
            .env("LANG", "C")
            .args(args)
            .current_dir(cwd);
        utils::configure_std_noninteractive(&mut command)?;
        let output = command.output()?;
        Ok(GitCommandOutput {
            success: output.status.success(),
            exit_code: output.status.code(),
            stdout: output.stdout,
            stderr: output.stderr,
        })
    }
}

/// Production git adapter. Spawns the `git` CLI (project may spawn; share may not).
pub(crate) struct GitCli;

impl GitCli {
    fn production() -> GitOps<SystemGitRunner> {
        GitOps::new(SystemGitRunner)
    }

    /// Crate-internal test seam: build a `GitCli` backed by an injected runner.
    #[cfg(test)]
    fn with_runner<R: GitCommandRunner>(runner: R) -> GitOps<R> {
        GitOps::new(runner)
    }
}

impl GitWorktreeOps for GitCli {
    fn probe_repository(&self, path: &Path) -> Result<RepositoryProbe, GitProbeError> {
        Self::production().probe_repository(path)
    }

    fn show_toplevel(&self, path: &Path) -> Result<PathBuf, GitOperationError> {
        Self::production().show_toplevel(path)
    }

    fn is_linked_worktree(&self, path: &Path) -> Result<bool, GitOperationError> {
        Self::production().is_linked_worktree(path)
    }

    fn worktree_add(
        &self,
        repo_root: &Path,
        path: &Path,
        branch: &str,
        base: &str,
    ) -> Result<(), GitOperationError> {
        Self::production().worktree_add(repo_root, path, branch, base)
    }

    fn current_branch(&self, path: &Path) -> Result<Option<String>, GitOperationError> {
        Self::production().current_branch(path)
    }
}

/// Shared git logic parameterised over a runner. `GitCli` delegates here so the
/// production and test paths execute identical parsing and error mapping.
struct GitOps<R: GitCommandRunner> {
    runner: R,
}

impl<R: GitCommandRunner> GitOps<R> {
    fn new(runner: R) -> Self {
        Self { runner }
    }
}

/// 跨 crate 公开的 git 子进程执行结果值对象（字段私有，只读访问器）。
/// 与 `GitCommandOutput` 解耦：内部 SPI 保留测试注入，公开面仅暴露消费
/// 方（context probe / runtime git 上下文）所需的最小读取能力。
#[derive(Debug)]
pub struct GitCommandOutcome {
    success: bool,
    exit_code: Option<i32>,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

impl GitCommandOutcome {
    pub fn is_success(&self) -> bool {
        self.success
    }

    pub fn exit_code(&self) -> Option<i32> {
        self.exit_code
    }

    pub fn stdout_bytes(&self) -> &[u8] {
        &self.stdout
    }

    pub fn stderr_bytes(&self) -> &[u8] {
        &self.stderr
    }
}

/// 全仓唯一 git 子进程 spawn 窄面：context/runtime 经此执行 git，统一
/// `LC_ALL=C` locale 与非交互 session 隔离。spawn 层失败映射
/// [`GitOperationError`]；命令非零退出不属于 `Err`，由
/// `outcome.is_success()` 表达，供消费方按 sentinel 自行分流。
pub fn run_git_command(
    cwd: &Path,
    args: &[&str],
) -> Result<GitCommandOutcome, crate::domain::types::GitOperationError> {
    let os_args: Vec<OsString> = args.iter().map(OsString::from).collect();
    SystemGitRunner
        .run(cwd, &os_args)
        .map(|output| GitCommandOutcome {
            success: output.success,
            exit_code: output.exit_code,
            stdout: output.stdout,
            stderr: output.stderr,
        })
        .map_err(|error| operation_spawn(error, cwd))
}

fn probe_spawn(error: io::Error, cwd: &Path) -> GitProbeError {
    match error.kind() {
        ErrorKind::NotFound => utils::describe_cwd_gone_failure(&error, cwd, "git 命令")
            .map(GitProbeError::CwdGone)
            .unwrap_or(GitProbeError::GitUnavailable),
        ErrorKind::PermissionDenied => GitProbeError::PermissionDenied,
        _ => GitProbeError::CommandFailed { exit_code: None },
    }
}

fn operation_spawn(error: io::Error, cwd: &Path) -> GitOperationError {
    match error.kind() {
        ErrorKind::NotFound => utils::describe_cwd_gone_failure(&error, cwd, "git 命令")
            .map(GitOperationError::CwdGone)
            .unwrap_or(GitOperationError::GitUnavailable),
        ErrorKind::PermissionDenied => GitOperationError::PermissionDenied,
        _ => GitOperationError::CommandFailed { exit_code: None },
    }
}

fn operation_output(output: GitCommandOutput) -> Result<String, GitOperationError> {
    if !output.success {
        return Err(GitOperationError::CommandFailed {
            exit_code: output.exit_code,
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

fn os_args<const N: usize>(items: [&str; N]) -> Vec<OsString> {
    items.into_iter().map(OsString::from).collect()
}

impl<R: GitCommandRunner> GitWorktreeOps for GitOps<R> {
    fn probe_repository(&self, path: &Path) -> Result<RepositoryProbe, GitProbeError> {
        let output = self
            .runner
            .run(
                path,
                &os_args([
                    "rev-parse",
                    "--show-toplevel",
                    "--git-common-dir",
                    "--git-dir",
                ]),
            )
            .map_err(|error| probe_spawn(error, path))?;
        if !output.success {
            let stderr = String::from_utf8_lossy(&output.stderr).to_ascii_lowercase();
            if stderr.contains("not a git repository") {
                return Ok(RepositoryProbe::NonGit);
            }
            if stderr.contains("permission denied") {
                return Err(GitProbeError::PermissionDenied);
            }
            return Err(GitProbeError::CommandFailed {
                exit_code: output.exit_code,
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
        let output = self
            .runner
            .run(path, &os_args(["rev-parse", "--show-toplevel"]))
            .map_err(|error| operation_spawn(error, path))?;
        let value = operation_output(output)?;
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
            Err(GitProbeError::CwdGone(attribution)) => {
                Err(GitOperationError::CwdGone(attribution))
            }
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
            std::fs::create_dir_all(parent).map_err(|error| operation_spawn(error, repo_root))?;
        }
        let args = vec![
            OsString::from("worktree"),
            OsString::from("add"),
            OsString::from("-b"),
            OsString::from(branch),
            OsString::from("--"),
            path.as_os_str().to_os_string(),
            OsString::from(base),
        ];
        let output = self
            .runner
            .run(repo_root, &args)
            .map_err(|error| operation_spawn(error, repo_root))?;
        if output.success {
            Ok(())
        } else {
            Err(GitOperationError::CommandFailed {
                exit_code: output.exit_code,
            })
        }
    }

    fn current_branch(&self, path: &Path) -> Result<Option<String>, GitOperationError> {
        let output = self
            .runner
            .run(path, &os_args(["rev-parse", "--abbrev-ref", "HEAD"]))
            .map_err(|error| operation_spawn(error, path))?;
        let branch = operation_output(output)?;
        if branch == "HEAD" {
            Ok(None)
        } else {
            Ok(Some(branch))
        }
    }
}

#[cfg(test)]
#[path = "git_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "git_option_tests.rs"]
mod option_tests;

#[cfg(test)]
#[path = "git_command_tests.rs"]
mod command_tests;
