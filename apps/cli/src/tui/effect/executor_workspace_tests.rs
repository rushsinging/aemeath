use super::resolve_workspace_metadata;
use crate::tui::model::conversation::workspace::WorktreeKind;
use tempfile::tempdir;

#[test]
fn resolver_returns_unknown_for_non_git_root() {
    let (branch, kind) = resolve_workspace_metadata("/definitely/not/a/git/workspace");

    assert_eq!(branch, None);
    assert_eq!(kind, WorktreeKind::Unknown);
}

#[test]
fn resolver_returns_branch_and_main_checkout_for_git_repository() {
    let directory = tempdir().expect("temporary repository");
    let run = |args: &[&str]| {
        let status = std::process::Command::new("git")
            .args(args)
            .current_dir(directory.path())
            .status()
            .expect("run git");
        assert!(status.success(), "git command failed: {args:?}");
    };
    run(&["init", "--initial-branch=main"]);

    let (branch, kind) = resolve_workspace_metadata(directory.path().to_str().expect("utf8 root"));

    assert_eq!(branch.as_deref(), Some("main"));
    assert_eq!(kind, WorktreeKind::MainCheckout);
}
