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

pub(super) struct TestRepository {
    _temp: TestTempDir,
    pub(super) root: PathBuf,
    pub(super) git_environment: TestGitEnvironment,
}

#[derive(Clone)]
pub(super) struct TestGitEnvironment {
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

    pub(super) fn command(&self) -> Command {
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

pub(super) fn initialized_repository() -> TestRepository {
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
    // cwd 必须真实存在：NotFound + cwd 缺失会归因为 CwdGone（由
    // probe_repository_when_cwd_deleted_returns_attribution 覆盖），本测试
    // 专注「git 可执行文件缺失」这一 NotFound 来源。
    let temp = TestTempDir::new();
    let git = GitCli::with_runner(ScriptedRunner::new([Err(io::Error::new(
        ErrorKind::NotFound,
        "missing git",
    ))]));

    assert_eq!(
        git.probe_repository(temp.path()),
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
