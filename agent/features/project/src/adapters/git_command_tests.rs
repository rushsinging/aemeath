//! `run_git_command` 公开窄面契约测试：真实 git 子进程行为（成功输出、
//! 非零退出、cwd 缺失归因），与消费方（context probe / runtime git 上下文）
//! 依赖的错误映射语义。

use super::tests::initialized_repository;
use super::*;
use crate::domain::types::GitOperationError;

#[test]
fn run_git_command_in_repository_reports_success_and_stdout() {
    let repository = initialized_repository();

    let outcome = run_git_command(&repository.root, &["rev-parse", "--is-inside-work-tree"])
        .expect("git rev-parse in initialized repository must spawn");

    assert!(outcome.is_success());
    assert_eq!(outcome.exit_code(), Some(0));
    assert_eq!(outcome.stdout_bytes(), b"true\n");
    assert!(outcome.stderr_bytes().is_empty());
}

#[test]
fn run_git_command_with_failing_ref_reports_nonzero_exit_without_error() {
    let repository = initialized_repository();

    let outcome = run_git_command(
        &repository.root,
        &["rev-parse", "--verify", "refs/heads/definitely-missing"],
    )
    .expect("git spawn itself must succeed; command failure is carried by the outcome");

    assert!(!outcome.is_success());
    assert_ne!(outcome.exit_code(), Some(0));
    let stderr = String::from_utf8_lossy(outcome.stderr_bytes()).to_ascii_lowercase();
    assert!(
        stderr.contains("needed a single revision")
            || stderr.contains("malformed object name")
            || stderr.contains("unknown revision"),
        "stderr sentinel must be readable and C-locale stable: {stderr}"
    );
}

#[test]
fn run_git_command_with_deleted_cwd_attributes_cwd_gone() {
    let repository = initialized_repository();
    let vanished = repository.root.join("vanished-cwd");
    std::fs::create_dir(&vanished).expect("pre-create cwd");
    std::fs::remove_dir(&vanished).expect("remove cwd before spawn");

    let result = run_git_command(&vanished, &["status"]);

    assert!(
        matches!(result, Err(GitOperationError::CwdGone(_))),
        "spawn NotFound with a deleted cwd must attribute CwdGone, got: {result:?}"
    );
}
