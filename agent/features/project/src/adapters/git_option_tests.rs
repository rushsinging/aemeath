use super::tests::initialized_repository;
use super::*;

#[test]
fn real_git_worktree_add_treats_option_like_base_as_revision() {
    let repository = initialized_repository();
    let linked = repository
        .root
        .parent()
        .expect("repository has temp parent")
        .join("option-like-base-worktree");
    let branch = "feature/option-like-base";
    let git = GitCli::with_runner(repository.git_environment.clone());

    let result = git.worktree_add(&repository.root, &linked, branch, "--force");
    let branch_status = repository
        .git_environment
        .command()
        .args([
            "show-ref",
            "--verify",
            "--quiet",
            &format!("refs/heads/{branch}"),
        ])
        .current_dir(&repository.root)
        .status()
        .expect("git must be installed for the real-git contract test");
    let branch_exists = branch_status.success();

    assert!(
        matches!(result, Err(GitOperationError::CommandFailed { .. }))
            && !linked.exists()
            && !branch_exists,
        "option-like base must fail as a revision without side effects: result={result:?}, \
         worktree_exists={}, branch_exists={branch_exists}",
        linked.exists()
    );
}

fn unique_temp_dir(name: &str) -> std::path::PathBuf {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let id = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!(
        "aemeath_project_git_spawn_{name}_{}_{id}",
        std::process::id()
    ));
    std::fs::create_dir_all(&path).expect("创建临时目录");
    path.canonicalize().expect("规范化临时目录")
}

#[test]
fn probe_repository_when_cwd_deleted_returns_attribution() {
    let removed_cwd = unique_temp_dir("probe_cwd_gone");
    std::fs::remove_dir_all(&removed_cwd).expect("删除工作目录以模拟 worktree 清理");

    let error = GitCli
        .probe_repository(&removed_cwd)
        .expect_err("工作目录缺失时 git 探测必须失败");

    match error {
        GitProbeError::CwdGone(attribution) => {
            assert!(
                attribution.contains("工作目录已不存在"),
                "错误必须包含 cwd 归因，实际：{attribution}"
            );
            assert!(
                attribution.contains(&removed_cwd.display().to_string()),
                "错误必须包含缺失路径，实际：{attribution}"
            );
        }
        other => panic!("期望 GitProbeError::CwdGone，实际：{other:?}"),
    }
}
