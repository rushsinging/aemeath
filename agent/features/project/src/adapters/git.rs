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
        let output = Command::new("git")
            .env("LC_ALL", "C")
            .env("LANG", "C")
            .args(args)
            .current_dir(cwd)
            .output()?;
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

fn probe_spawn(error: io::Error) -> GitProbeError {
    match error.kind() {
        ErrorKind::NotFound => GitProbeError::GitUnavailable,
        ErrorKind::PermissionDenied => GitProbeError::PermissionDenied,
        _ => GitProbeError::CommandFailed { exit_code: None },
    }
}

fn operation_spawn(error: io::Error) -> GitOperationError {
    match error.kind() {
        ErrorKind::NotFound => GitOperationError::GitUnavailable,
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
            .map_err(probe_spawn)?;
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
            .map_err(operation_spawn)?;
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
        let args = vec![
            OsString::from("worktree"),
            OsString::from("add"),
            path.as_os_str().to_os_string(),
            OsString::from("-b"),
            OsString::from(branch),
            OsString::from(base),
        ];
        let output = self.runner.run(repo_root, &args).map_err(operation_spawn)?;
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
            .map_err(operation_spawn)?;
        let branch = operation_output(output)?;
        if branch == "HEAD" {
            Ok(None)
        } else {
            Ok(Some(branch))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    use std::ffi::OsStr;
    use std::io;
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Mutex;
    use std::time::{SystemTime, UNIX_EPOCH};

    static NEXT_TEMP_DIR: AtomicU64 = AtomicU64::new(0);

    /// Minimal `tempfile::TempDir` equivalent kept here because this crate does
    /// not depend on `tempfile`. Each contract test owns and removes its own
    /// unique directory.
    struct TestTempDir {
        path: PathBuf,
    }

    impl TestTempDir {
        fn new() -> Self {
            let base = std::env::temp_dir();
            for _ in 0..100 {
                let nonce = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .expect("system clock must be after the Unix epoch")
                    .as_nanos();
                let sequence = NEXT_TEMP_DIR.fetch_add(1, Ordering::Relaxed);
                let path = base.join(format!(
                    "aemeath-project-git-test-{}-{nonce}-{sequence}",
                    std::process::id()
                ));
                match std::fs::create_dir(&path) {
                    Ok(()) => return Self { path },
                    Err(error) if error.kind() == ErrorKind::AlreadyExists => continue,
                    Err(error) => panic!("failed to create {}: {error}", path.display()),
                }
            }
            panic!("failed to allocate a unique temporary directory");
        }

        fn path(&self) -> &Path {
            &self.path
        }
    }

    impl Drop for TestTempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    struct TestRepository {
        _temp: TestTempDir,
        root: PathBuf,
        git_environment: TestGitEnvironment,
    }

    #[derive(Clone)]
    struct TestGitEnvironment {
        unavailable_global_config: PathBuf,
        hooks_dir: PathBuf,
    }

    impl TestGitEnvironment {
        fn new(parent: &Path) -> Self {
            let hooks_dir = parent.join("empty-hooks");
            std::fs::create_dir(&hooks_dir).expect("create empty hooks directory");
            Self {
                unavailable_global_config: parent.join("unavailable-global-config"),
                hooks_dir,
            }
        }

        fn command(&self) -> Command {
            let mut command = Command::new("git");
            command
                .env("LC_ALL", "C")
                .env("LANG", "C")
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .env("GIT_CONFIG_GLOBAL", &self.unavailable_global_config)
                .env("GIT_CONFIG_COUNT", "1")
                .env("GIT_CONFIG_KEY_0", "core.hooksPath")
                .env("GIT_CONFIG_VALUE_0", &self.hooks_dir);
            command
        }
    }

    impl GitCommandRunner for TestGitEnvironment {
        fn run(&self, cwd: &Path, args: &[OsString]) -> Result<GitCommandOutput, io::Error> {
            let output = self.command().args(args).current_dir(cwd).output()?;
            Ok(GitCommandOutput {
                success: output.status.success(),
                exit_code: output.status.code(),
                stdout: output.stdout,
                stderr: output.stderr,
            })
        }
    }

    fn run_git<I, S>(environment: &TestGitEnvironment, cwd: &Path, args: I)
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let output = environment
            .command()
            .args(args)
            .current_dir(cwd)
            .output()
            .expect("git must be installed for the real-git contract tests");
        assert!(
            output.status.success(),
            "git failed in {} (status {:?}): {}",
            cwd.display(),
            output.status.code(),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn initialized_repository() -> TestRepository {
        let temp = TestTempDir::new();
        let root = temp.path().join("repo");
        std::fs::create_dir(&root).expect("create repository directory");
        let git_environment = TestGitEnvironment::new(temp.path());

        run_git(&git_environment, &root, ["init", "--initial-branch=main"]);
        run_git(
            &git_environment,
            &root,
            ["config", "--local", "user.name", "Project Test"],
        );
        run_git(
            &git_environment,
            &root,
            ["config", "--local", "user.email", "project@example.invalid"],
        );
        run_git(
            &git_environment,
            &root,
            ["config", "--local", "commit.gpgsign", "false"],
        );
        run_git(
            &git_environment,
            &root,
            ["config", "--local", "tag.gpgsign", "false"],
        );
        std::fs::write(root.join("seed.txt"), "seed\n").expect("write seed file");
        run_git(&git_environment, &root, ["add", "seed.txt"]);
        run_git(&git_environment, &root, ["commit", "-m", "seed"]);

        TestRepository {
            _temp: temp,
            root,
            git_environment,
        }
    }

    struct ScriptedRunner {
        outputs: Mutex<VecDeque<Result<GitCommandOutput, io::Error>>>,
    }

    impl ScriptedRunner {
        fn new(outputs: impl IntoIterator<Item = Result<GitCommandOutput, io::Error>>) -> Self {
            Self {
                outputs: Mutex::new(outputs.into_iter().collect()),
            }
        }
    }

    impl GitCommandRunner for ScriptedRunner {
        fn run(
            &self,
            _cwd: &Path,
            _args: &[std::ffi::OsString],
        ) -> Result<GitCommandOutput, io::Error> {
            self.outputs
                .lock()
                .unwrap()
                .pop_front()
                .expect("scripted git output")
        }
    }

    fn output(success: bool, code: Option<i32>, stdout: &[u8], stderr: &[u8]) -> GitCommandOutput {
        GitCommandOutput {
            success,
            exit_code: code,
            stdout: stdout.to_vec(),
            stderr: stderr.to_vec(),
        }
    }

    #[test]
    fn real_git_probe_identifies_primary_worktree() {
        let repository = initialized_repository();
        let git = GitCli::with_runner(repository.git_environment);
        let expected_root = repository.root.canonicalize().expect("canonical root");
        let expected_common = repository
            .root
            .join(".git")
            .canonicalize()
            .expect("canonical git directory");

        assert_eq!(
            git.probe_repository(&repository.root),
            Ok(RepositoryProbe::Git {
                canonical_top_level: expected_root,
                canonical_common_dir: expected_common,
                worktree_kind: WorktreeKind::Primary,
            })
        );
        assert_eq!(git.is_linked_worktree(&repository.root), Ok(false));
    }

    #[test]
    fn real_git_probe_identifies_linked_worktree() {
        let repository = initialized_repository();
        let linked = repository._temp.path().join("linked");
        run_git(
            &repository.git_environment,
            &repository.root,
            [
                OsStr::new("worktree"),
                OsStr::new("add"),
                linked.as_os_str(),
                OsStr::new("-b"),
                OsStr::new("linked-probe"),
                OsStr::new("main"),
            ],
        );
        let git = GitCli::with_runner(repository.git_environment);

        let probe = git
            .probe_repository(&linked)
            .expect("probe linked worktree");
        assert!(matches!(
            probe,
            RepositoryProbe::Git {
                worktree_kind: WorktreeKind::Linked,
                ..
            }
        ));
        assert_eq!(git.is_linked_worktree(&linked), Ok(true));
    }

    #[test]
    fn real_git_probe_reports_non_git_directory() {
        let temp = TestTempDir::new();
        let plain = temp.path().join("plain");
        std::fs::create_dir(&plain).expect("create non-git directory");
        let git = GitCli::with_runner(TestGitEnvironment::new(temp.path()));

        assert_eq!(git.probe_repository(&plain), Ok(RepositoryProbe::NonGit));
        assert_eq!(git.is_linked_worktree(&plain), Ok(false));
    }

    #[test]
    fn real_git_show_toplevel_resolves_repository_root_from_subdirectory() {
        let repository = initialized_repository();
        let nested = repository.root.join("one/two");
        std::fs::create_dir_all(&nested).expect("create nested directory");
        let git = GitCli::with_runner(repository.git_environment);

        assert_eq!(
            git.show_toplevel(&nested),
            Ok(repository.root.canonicalize().expect("canonical root"))
        );
    }

    #[test]
    fn real_git_current_branch_returns_main_and_none_when_detached() {
        let repository = initialized_repository();
        let git = GitCli::with_runner(repository.git_environment.clone());
        assert_eq!(
            git.current_branch(&repository.root),
            Ok(Some("main".to_owned()))
        );
        run_git(
            &repository.git_environment,
            &repository.root,
            ["checkout", "--detach", "HEAD"],
        );
        assert_eq!(git.current_branch(&repository.root), Ok(None));
    }

    #[test]
    fn real_git_worktree_add_creates_linked_worktree_on_requested_branch() {
        let repository = initialized_repository();
        let linked = repository._temp.path().join("generated/linked");
        let git = GitCli::with_runner(repository.git_environment);

        git.worktree_add(&repository.root, &linked, "feature/contract", "main")
            .expect("create linked worktree");

        assert_eq!(git.is_linked_worktree(&linked), Ok(true));
        assert_eq!(
            git.current_branch(&linked),
            Ok(Some("feature/contract".to_owned()))
        );
        assert!(matches!(
            git.probe_repository(&linked),
            Ok(RepositoryProbe::Git {
                worktree_kind: WorktreeKind::Linked,
                ..
            })
        ));
    }

    #[test]
    fn probe_maps_missing_git_to_unavailable() {
        let git = GitCli::with_runner(ScriptedRunner::new([Err(io::Error::new(
            ErrorKind::NotFound,
            "missing git",
        ))]));

        assert_eq!(
            git.probe_repository(Path::new("/repo")),
            Err(GitProbeError::GitUnavailable)
        );
    }

    #[test]
    fn operation_maps_permission_denied() {
        let git = GitCli::with_runner(ScriptedRunner::new([Err(io::Error::new(
            ErrorKind::PermissionDenied,
            "denied",
        ))]));

        assert_eq!(
            git.current_branch(Path::new("/repo")),
            Err(GitOperationError::PermissionDenied)
        );
    }

    #[test]
    fn operation_rejects_nonzero_empty_and_invalid_utf8_output() {
        let cases = [
            (
                output(false, Some(17), b"", b"failure"),
                GitOperationError::CommandFailed {
                    exit_code: Some(17),
                },
            ),
            (
                output(true, Some(0), b"  \n", b""),
                GitOperationError::InvalidOutput,
            ),
            (
                output(true, Some(0), &[0xff], b""),
                GitOperationError::InvalidOutput,
            ),
        ];

        for (command_output, expected) in cases {
            let git = GitCli::with_runner(ScriptedRunner::new([Ok(command_output)]));
            assert_eq!(git.current_branch(Path::new("/repo")), Err(expected));
        }
    }

    #[test]
    fn probe_rejects_malformed_success_output() {
        for stdout in [
            b"one-line\n".as_slice(),
            b"a\nb\nc\nd\n".as_slice(),
            &[0xff],
        ] {
            let git = GitCli::with_runner(ScriptedRunner::new([Ok(output(
                true,
                Some(0),
                stdout,
                b"",
            ))]));
            assert_eq!(
                git.probe_repository(Path::new("/repo")),
                Err(GitProbeError::InvalidOutput)
            );
        }
    }
}
