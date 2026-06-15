use super::*;
use crate::business::git_ops::tests::FakeGit;

fn st(cwd: &str) -> WorkspaceState {
    WorkspaceState::new(PathBuf::from(cwd))
}

#[test]
fn init_consistent() {
    let s = st("/repo");
    assert_eq!(s.working_root, PathBuf::from("/repo"));
    assert_eq!(s.path_base, PathBuf::from("/repo"));
    assert!(s.stack.is_empty());
}

#[test]
fn resolve_relative_uses_path_base() {
    let mut s = st("/repo");
    s.path_base = PathBuf::from("/repo/sub");
    assert_eq!(
        s.resolve(Path::new("a/b.rs")),
        PathBuf::from("/repo/sub/a/b.rs")
    );
    assert_eq!(s.resolve(Path::new("/abs/x")), PathBuf::from("/abs/x"));
}

#[test]
fn exit_empty_stack_errors() {
    let mut s = st("/repo");
    assert_eq!(exit(&mut s), Err(WorkspaceError::EmptyStack));
}

#[test]
fn exit_pops_and_restores() {
    let mut s = st("/repo");
    s.stack.push(WorkspaceFrame {
        path_base: "/prev".into(),
        working_root: "/prev".into(),
    });
    s.path_base = "/wt".into();
    s.working_root = "/wt".into();
    let prev = exit(&mut s).unwrap();
    assert_eq!(prev.path_base, PathBuf::from("/prev"));
    assert_eq!(s.path_base, PathBuf::from("/prev"));
}

#[test]
fn set_cwd_detects_root() {
    let mut git = FakeGit::default();
    git.toplevel
        .insert(PathBuf::from("/repo/sub"), PathBuf::from("/repo"));
    let mut s = st("/repo");
    set_cwd(&mut s, &git, PathBuf::from("/repo/sub")).unwrap();
    assert_eq!(s.path_base, PathBuf::from("/repo/sub"));
    assert_eq!(s.working_root, PathBuf::from("/repo"));
}

#[test]
fn snapshot_restore_roundtrip() {
    let mut s = st("/repo");
    s.path_base = "/repo/sub".into();
    s.stack.push(WorkspaceFrame {
        path_base: "/repo".into(),
        working_root: "/repo".into(),
    });
    let dto = snapshot(&s);
    let mut s2 = st("/tmp");
    // restore 校验路径存在：用真实存在的临时目录替换
    let dir = std::env::temp_dir();
    let dto2 = PersistedWorkspaceContext {
        path_base: dir.display().to_string(),
        working_root: dir.display().to_string(),
        context_stack: dto.context_stack.clone(),
    };
    restore(&mut s2, &dto2).unwrap();
    assert_eq!(s2.path_base, dir);
    assert_eq!(s2.stack.len(), 1);
}

#[test]
fn restore_invalid_path_fails_whole() {
    let mut s = st("/repo");
    let bad = PersistedWorkspaceContext {
        path_base: "/definitely/not/here/xyz".into(),
        working_root: "/definitely/not/here/xyz".into(),
        context_stack: vec![],
    };
    assert!(matches!(
        restore(&mut s, &bad),
        Err(WorkspaceError::RestoreInvalidPath(_))
    ));
    // 状态未被部分修改
    assert_eq!(s.path_base, PathBuf::from("/repo"));
}

#[test]
fn enter_rejects_nested_when_in_worktree() {
    let mut git = FakeGit::default();
    git.worktrees.insert(PathBuf::from("/repo")); // 当前 path_base 在 worktree 中
    let mut s = st("/repo");
    s.stack.push(WorkspaceFrame {
        path_base: "/prev".into(),
        working_root: "/prev".into(),
    });
    assert!(matches!(
        enter(&mut s, &git, Some("/other".into()), None),
        Err(WorkspaceError::NestedWorktree {
            current_working_root,
            current_path_base,
        }) if current_working_root == PathBuf::from("/repo")
           && current_path_base == PathBuf::from("/repo")
    ));
}

#[test]
fn switch_to_rejects_nonexistent_path() {
    let git = FakeGit::default();
    let mut s = st("/repo");
    let result = switch_to(&mut s, &git, PathBuf::from("/does/not/exist/xyz"));
    assert!(
        matches!(result, Err(WorkspaceError::PathNotFound(_))),
        "expected PathNotFound, got {:?}",
        result
    );
    // State must remain unchanged.
    assert_eq!(s.path_base, PathBuf::from("/repo"));
    assert_eq!(s.working_root, PathBuf::from("/repo"));
    assert!(s.stack.is_empty());
}

#[test]
fn switch_to_rejects_cross_repo() {
    use std::time::{SystemTime, UNIX_EPOCH};

    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .subsec_nanos();
    let tmp = std::env::temp_dir().join(format!("aemeath_switch_cross_{}", nanos));
    std::fs::create_dir_all(&tmp).unwrap();
    let canonical_tmp = tmp.canonicalize().unwrap();

    let mut git = FakeGit::default();
    // Target: toplevel = /other-repo; common_dir for current root vs other root differ.
    git.toplevel
        .insert(canonical_tmp.clone(), PathBuf::from("/other-repo"));
    git.common_dir
        .insert(PathBuf::from("/repo"), PathBuf::from("/repo/.git"));
    git.common_dir.insert(
        PathBuf::from("/other-repo"),
        PathBuf::from("/other-repo/.git"),
    );

    let mut s = st("/repo");
    let result = switch_to(&mut s, &git, canonical_tmp.clone());
    assert!(
        matches!(result, Err(WorkspaceError::RepoMismatch { .. })),
        "expected RepoMismatch, got {:?}",
        result
    );
    // State must remain unchanged.
    assert_eq!(s.path_base, PathBuf::from("/repo"));
    assert_eq!(s.working_root, PathBuf::from("/repo"));
    assert!(s.stack.is_empty());

    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn switch_to_succeeds_same_repo() {
    use std::time::{SystemTime, UNIX_EPOCH};

    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .subsec_nanos();
    let tmp = std::env::temp_dir().join(format!("aemeath_switch_same_{}", nanos));
    std::fs::create_dir_all(&tmp).unwrap();
    let canonical_tmp = tmp.canonicalize().unwrap();

    let worktree_root = PathBuf::from("/repo/wt");
    let common = PathBuf::from("/repo/.git");

    let mut git = FakeGit::default();
    git.toplevel
        .insert(canonical_tmp.clone(), worktree_root.clone());
    git.common_dir
        .insert(PathBuf::from("/repo"), common.clone());
    git.common_dir.insert(worktree_root.clone(), common.clone());

    let mut s = st("/repo");
    switch_to(&mut s, &git, canonical_tmp.clone()).unwrap();

    assert_eq!(s.path_base, canonical_tmp, "path_base should be canonical");
    assert_eq!(
        s.working_root, worktree_root,
        "working_root should be worktree root"
    );
    assert!(s.stack.is_empty(), "stack must not be pushed");

    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn enter_missing_path_and_branch_errors() {
    let git = FakeGit::default();
    let mut s = st("/repo");
    assert_eq!(
        enter(&mut s, &git, None, None),
        Err(WorkspaceError::MissingPathAndBranch)
    );
}

#[test]
fn enter_happy_path_pushes_frame_and_swaps_cwd() {
    use std::time::{SystemTime, UNIX_EPOCH};

    // Create a unique real temp dir so canonicalize() succeeds.
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .subsec_nanos();
    let tmp = std::env::temp_dir().join(format!("aemeath_test_{}", nanos));
    std::fs::create_dir_all(&tmp).unwrap();
    let canonical_tmp = tmp.canonicalize().unwrap();

    // The worktree root that FakeGit reports for the temp dir.
    let worktree_root = PathBuf::from("/repo/wt");
    // A shared common dir value (same for both initial cwd and worktree root → same repo).
    let common = PathBuf::from("/repo/.git");

    let mut git = FakeGit::default();
    // show_toplevel: called twice — once inside enter (for canonical_tmp), once inside set_cwd.
    git.toplevel
        .insert(canonical_tmp.clone(), worktree_root.clone());
    // git_common_dir: checked for working_root ("/repo") and worktree_root.
    git.common_dir
        .insert(PathBuf::from("/repo"), common.clone());
    git.common_dir.insert(worktree_root.clone(), common.clone());

    let mut s = st("/repo");
    let saved_path_base = s.path_base.clone();
    let saved_working_root = s.working_root.clone();

    // Pass the temp dir as an absolute path → resolve_worktree_path returns it directly,
    // target.exists() is true → worktree_add is NOT called.
    let frame = enter(&mut s, &git, Some(canonical_tmp.clone()), None).unwrap();

    // Returned frame holds the PRE-change state.
    assert_eq!(frame.path_base, saved_path_base);
    assert_eq!(frame.working_root, saved_working_root);

    // Stack has exactly one entry (the saved frame).
    assert_eq!(s.stack.len(), 1);

    // State updated to the worktree.
    assert_eq!(s.path_base, canonical_tmp);
    assert_eq!(s.working_root, worktree_root);

    // worktree_add was NOT invoked.
    assert!(git.added.lock().unwrap().is_empty());

    let _ = std::fs::remove_dir_all(&tmp);
}
