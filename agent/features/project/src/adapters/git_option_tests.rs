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
