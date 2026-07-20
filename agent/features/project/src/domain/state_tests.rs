use super::*;
use crate::domain::git::tests::{FakeGit, WorktreeAddCall};

fn st(cwd: &str) -> WorkspaceState {
    WorkspaceState::new(PathBuf::from(cwd))
}

fn unique_temp_dir(name: &str) -> PathBuf {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let id = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!(
        "aemeath_project_state_{name}_{}_{id}",
        std::process::id()
    ));
    std::fs::create_dir_all(&path).unwrap();
    path.canonicalize().unwrap()
}

#[test]
fn init_consistent() {
    let s = st("/repo");
    assert_eq!(s.workspace_root, PathBuf::from("/repo"));
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
    assert_eq!(
        exit(&mut s, &FakeGit::default()),
        Err(WorkspaceError::EmptyStack)
    );
}

#[test]
fn exit_pops_and_restores() {
    let root = unique_temp_dir("exit_restore");
    let common = root.join(".git");
    let mut s = WorkspaceState::from_verified(
        ProjectIdentity {
            initial_cwd: root.display().to_string(),
            git_common_dir: Some(common.display().to_string()),
        },
        root.clone(),
        root.clone(),
        WorktreeKind::Linked,
    );
    s.stack.push(WorkspaceFrame {
        path_base: root.clone(),
        workspace_root: root.clone(),
        worktree_kind: WorktreeKind::Primary,
    });
    let mut git = FakeGit::default();
    git.toplevel.insert(root.clone(), root.clone());
    git.common_dir.insert(root.clone(), common);
    let prev = exit(&mut s, &git).unwrap();
    assert_eq!(prev.path_base, root);
    assert_eq!(s.path_base, prev.path_base);
}

#[test]
fn exit_rejects_noncanonical_frame_path_as_invalid_output_and_keeps_state() {
    let root = unique_temp_dir("exit_noncanonical_frame");
    let nested = root.join("nested");
    std::fs::create_dir_all(&nested).unwrap();
    let noncanonical = nested.join("..");
    let common = root.join(".git");
    let mut state = WorkspaceState::from_verified(
        ProjectIdentity {
            initial_cwd: root.display().to_string(),
            git_common_dir: Some(common.display().to_string()),
        },
        root.clone(),
        root.clone(),
        WorktreeKind::Linked,
    );
    state.stack.push(WorkspaceFrame {
        path_base: noncanonical,
        workspace_root: root.clone(),
        worktree_kind: WorktreeKind::Primary,
    });
    let mut git = FakeGit::default();
    git.toplevel.insert(root.clone(), root.clone());
    git.common_dir.insert(root.clone(), common);
    let before = snapshot(&state);

    let result = exit(&mut state, &git);

    assert_eq!(
        result,
        Err(WorkspaceError::GitProbeFailed(
            crate::GitProbeError::InvalidOutput
        ))
    );
    assert_eq!(snapshot(&state), before);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn exit_rejects_frame_workspace_root_mismatch_as_invalid_output_and_keeps_state() {
    let root = unique_temp_dir("exit_root_mismatch");
    let common = root.join(".git");
    let actual_root = root.join("actual");
    let mut state = WorkspaceState::from_verified(
        ProjectIdentity {
            initial_cwd: root.display().to_string(),
            git_common_dir: Some(common.display().to_string()),
        },
        root.clone(),
        root.clone(),
        WorktreeKind::Linked,
    );
    state.stack.push(WorkspaceFrame {
        path_base: root.clone(),
        workspace_root: root.join("expected"),
        worktree_kind: WorktreeKind::Primary,
    });
    let mut git = FakeGit::default();
    git.toplevel.insert(root.clone(), actual_root.clone());
    git.common_dir.insert(actual_root, common);
    let before = snapshot(&state);

    let result = exit(&mut state, &git);

    assert_eq!(
        result,
        Err(WorkspaceError::GitProbeFailed(
            crate::GitProbeError::InvalidOutput
        ))
    );
    assert_eq!(snapshot(&state), before);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn exit_rejects_frame_worktree_kind_mismatch_as_invalid_output_and_keeps_state() {
    let root = unique_temp_dir("exit_kind_mismatch");
    let common = root.join(".git");
    let mut state = WorkspaceState::from_verified(
        ProjectIdentity {
            initial_cwd: root.display().to_string(),
            git_common_dir: Some(common.display().to_string()),
        },
        root.clone(),
        root.clone(),
        WorktreeKind::Linked,
    );
    state.stack.push(WorkspaceFrame {
        path_base: root.clone(),
        workspace_root: root.clone(),
        worktree_kind: WorktreeKind::Linked,
    });
    let mut git = FakeGit::default();
    git.toplevel.insert(root.clone(), root.clone());
    git.common_dir.insert(root.clone(), common);
    let before = snapshot(&state);

    let result = exit(&mut state, &git);

    assert_eq!(
        result,
        Err(WorkspaceError::GitProbeFailed(
            crate::GitProbeError::InvalidOutput
        ))
    );
    assert_eq!(snapshot(&state), before);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn change_directory_canonicalizes_and_keeps_root() {
    let root = unique_temp_dir("change_directory");
    let sub = root.join("sub");
    std::fs::create_dir_all(&sub).unwrap();
    let mut s = WorkspaceState::from_verified(
        ProjectIdentity::default(),
        root.clone(),
        root.clone(),
        WorktreeKind::NonGit,
    );
    change_directory(&mut s, sub.clone()).unwrap();
    assert_eq!(s.path_base, sub.canonicalize().unwrap());
    assert_eq!(s.workspace_root, root);
}

#[test]
fn snapshot_captures_all_fields() {
    // #894: snapshot MUST 收集 identity + path_base + workspace_root + worktree_kind + stack 全量。
    let mut s = st("/repo");
    s.project_identity = ProjectIdentity {
        initial_cwd: "/repo".into(),
        git_common_dir: Some("/repo/.git".into()),
    };
    s.path_base = "/repo/sub".into();
    s.workspace_root = "/repo".into();
    s.worktree_kind = WorktreeKind::Primary;
    s.stack.push(WorkspaceFrame {
        path_base: "/repo".into(),
        workspace_root: "/repo".into(),
        worktree_kind: WorktreeKind::Primary,
    });

    let dto = snapshot(&s);

    assert_eq!(dto.workspace_id, s.workspace_id());
    assert_eq!(dto.project_identity, s.project_identity);
    assert_eq!(dto.path_base, "/repo/sub");
    assert_eq!(dto.workspace_root, "/repo");
    assert_eq!(dto.worktree_kind, WorktreeKind::Primary);
    assert_eq!(dto.context_stack.len(), 1);
    assert_eq!(dto.context_stack[0].path_base, "/repo");
    assert_eq!(dto.context_stack[0].worktree_kind, WorktreeKind::Primary);
}

/// #894 helper：为 git identity 组装自洽、路径真实存在的合法 DTO。
fn valid_git_dto(root: &Path, path_base: &Path, common: &str) -> PersistedWorkspaceContext {
    let identity = ProjectIdentity {
        initial_cwd: root.display().to_string(),
        git_common_dir: Some(common.to_string()),
    };
    PersistedWorkspaceContext {
        workspace_id: WorkspaceId::derive(&identity, &root.display().to_string()),
        project_identity: identity,
        path_base: path_base.display().to_string(),
        workspace_root: root.display().to_string(),
        worktree_kind: WorktreeKind::Primary,
        context_stack: vec![],
    }
}

fn git_ops_for(root: &Path, common: &str) -> FakeGit {
    let mut git = FakeGit::default();
    git.common_dir
        .insert(root.to_path_buf(), PathBuf::from(common));
    git.toplevel.insert(root.to_path_buf(), root.to_path_buf());
    git
}

#[test]
fn prepare_restore_success_does_not_mutate_live_state() {
    // #894: prepare_restore MUST 在不修改 live state 的前提下完整校验并构造令牌。
    let root = unique_temp_dir("prep_ok_root");
    let sub = root.join("sub");
    std::fs::create_dir_all(&sub).unwrap();
    let sub = sub.canonicalize().unwrap();
    let common = "/repo/.git";
    let dto = valid_git_dto(&root, &sub, common);
    let git = git_ops_for(&root, common);

    let live = st("/repo");
    let before_base = live.path_base.clone();
    let before_root = live.workspace_root.clone();
    let before_identity = live.project_identity.clone();

    let prepared: crate::PreparedWorkspaceRestore =
        prepare_restore(&live, &dto, &git).expect("合法 DTO 应构造令牌");

    // live state（不可变借用）必须原样保留。
    assert_eq!(live.path_base, before_base);
    assert_eq!(live.workspace_root, before_root);
    assert_eq!(live.project_identity, before_identity);
    // 令牌暴露已校验 identity。
    assert_eq!(prepared.project_identity(), &dto.project_identity);
}

#[test]
fn commit_restore_replaces_state_in_one_shot() {
    // #894: commit_restore MUST 无失败地一次全量替换 state（签名无 Result）。
    let root = unique_temp_dir("commit_root");
    let sub = root.join("sub");
    std::fs::create_dir_all(&sub).unwrap();
    let sub = sub.canonicalize().unwrap();
    let common = "/repo/.git";
    let dto = valid_git_dto(&root, &sub, common);
    let git = git_ops_for(&root, common);

    let live = st("/repo");
    let prepared = prepare_restore(&live, &dto, &git).expect("合法 DTO 应构造令牌");

    // 提交进一个与来源不同的 state slot，验证全量替换。
    let mut target = st("/somewhere-else");
    let _: () = commit_restore(&mut target, prepared);

    assert_eq!(target.workspace_root, root);
    assert_eq!(target.path_base, sub);
    assert_eq!(target.project_identity, dto.project_identity);
    assert_eq!(target.worktree_kind, WorktreeKind::Primary);
    assert!(target.stack.is_empty());
}

#[test]
fn prepare_restore_path_not_found_keeps_live_state() {
    // #894: 路径不存在 -> 结构化 PathNotFound，live state 不变。
    let root = unique_temp_dir("prep_missing_root");
    let missing = root.join("missing_sub"); // 位于 root 内但不存在
    let common = "/repo/.git";
    let dto = valid_git_dto(&root, &missing, common);
    let git = git_ops_for(&root, common);

    let live = st("/repo");
    let before = live.path_base.clone();
    let result = prepare_restore(&live, &dto, &git);

    assert!(
        matches!(
            result,
            Err(crate::WorkspaceRestoreError::PathNotFound { .. })
        ),
        "expected PathNotFound, got {result:?}"
    );
    assert_eq!(live.path_base, before);
}

#[test]
fn prepare_restore_path_outside_root_keeps_live_state() {
    // #894: path_base 越出 workspace_root -> PathOutsideWorkspaceRoot，live state 不变。
    let root = unique_temp_dir("prep_outside_root");
    let outside = unique_temp_dir("prep_outside_other"); // 真实存在但不在 root 内
    let common = "/repo/.git";
    let dto = valid_git_dto(&root, &outside, common);
    let git = git_ops_for(&root, common);

    let live = st("/repo");
    let before = live.workspace_root.clone();
    let result = prepare_restore(&live, &dto, &git);

    assert!(
        matches!(
            result,
            Err(crate::WorkspaceRestoreError::PathOutsideWorkspaceRoot { .. })
        ),
        "expected PathOutsideWorkspaceRoot, got {result:?}"
    );
    assert_eq!(live.workspace_root, before);
}

#[test]
fn prepare_restore_workspace_id_mismatch_keeps_live_state() {
    // #894: workspace_id 与 identity/root 不一致 -> WorkspaceIdMismatch，live state 不变。
    let root = unique_temp_dir("prep_wsid_root");
    let sub = root.join("sub");
    std::fs::create_dir_all(&sub).unwrap();
    let sub = sub.canonicalize().unwrap();
    let common = "/repo/.git";
    let mut dto = valid_git_dto(&root, &sub, common);
    dto.workspace_id = WorkspaceId::from("ws-deadbeefdeadbeef"); // 伪造，不匹配派生值
    let git = git_ops_for(&root, common);

    let live = st("/repo");
    let before = live.project_identity.clone();
    let result = prepare_restore(&live, &dto, &git);

    assert!(
        matches!(
            result,
            Err(crate::WorkspaceRestoreError::WorkspaceIdMismatch)
        ),
        "expected WorkspaceIdMismatch, got {result:?}"
    );
    assert_eq!(live.project_identity, before);
}

#[test]
fn prepare_restore_repo_mismatch_keeps_live_state() {
    // #894: root 实际归属另一 git common dir -> RepositoryMismatch，live state 不变。
    let root = unique_temp_dir("prep_repo_root");
    let sub = root.join("sub");
    std::fs::create_dir_all(&sub).unwrap();
    let sub = sub.canonicalize().unwrap();
    // identity 声称 /repo/.git，但实际 probe 返回 /other/.git。
    let dto = valid_git_dto(&root, &sub, "/repo/.git");
    let git = git_ops_for(&root, "/other/.git");

    let live = st("/repo");
    let before = live.workspace_root.clone();
    let result = prepare_restore(&live, &dto, &git);

    assert!(
        matches!(
            result,
            Err(crate::WorkspaceRestoreError::RepositoryMismatch)
        ),
        "expected RepositoryMismatch, got {result:?}"
    );
    assert_eq!(live.workspace_root, before);
}

#[test]
fn prepare_restore_non_git_with_stack_is_invalid_stack_shape() {
    // #894: NonGit identity 下 stack MUST 为空；非空 -> InvalidStackShape，live state 不变。
    let root = unique_temp_dir("prep_nongit_stack_root");
    let identity = ProjectIdentity {
        initial_cwd: root.display().to_string(),
        git_common_dir: None,
    };
    let dto = PersistedWorkspaceContext {
        workspace_id: WorkspaceId::derive(&identity, &root.display().to_string()),
        project_identity: identity,
        path_base: root.display().to_string(),
        workspace_root: root.display().to_string(),
        worktree_kind: WorktreeKind::NonGit,
        context_stack: vec![PersistedWorkspaceFrame {
            path_base: root.display().to_string(),
            workspace_root: root.display().to_string(),
            worktree_kind: WorktreeKind::NonGit,
        }],
    };
    let mut git = FakeGit::default();
    git.non_git.insert(root.clone());

    let live = st("/repo");
    let before = live.stack.clone();
    let result = prepare_restore(&live, &dto, &git);

    assert!(
        matches!(result, Err(crate::WorkspaceRestoreError::InvalidStackShape)),
        "expected InvalidStackShape, got {result:?}"
    );
    assert_eq!(live.stack, before);
}

#[test]
fn prepare_restore_non_git_disguise_over_real_git_keeps_live_state() {
    // #894: identity 声称 NonGit 但实际 probe 为 Git -> 结构化 mismatch，NEVER 以伪 NonGit 恢复。
    let root = unique_temp_dir("prep_nongit_disguise_root");
    let identity = ProjectIdentity {
        initial_cwd: root.display().to_string(),
        git_common_dir: None,
    };
    let dto = PersistedWorkspaceContext {
        workspace_id: WorkspaceId::derive(&identity, &root.display().to_string()),
        project_identity: identity,
        path_base: root.display().to_string(),
        workspace_root: root.display().to_string(),
        worktree_kind: WorktreeKind::NonGit,
        context_stack: vec![],
    };
    // FakeGit 对 root 返回 Git（未加入 non_git），暴露伪装。
    let git = git_ops_for(&root, "/repo/.git");

    let live = st("/repo");
    let before = live.project_identity.clone();
    let result = prepare_restore(&live, &dto, &git);

    assert!(
        matches!(
            result,
            Err(crate::WorkspaceRestoreError::RepositoryMismatch)
                | Err(crate::WorkspaceRestoreError::InvalidProjectIdentity)
        ),
        "expected structured NonGit-disguise mismatch, got {result:?}"
    );
    assert_eq!(live.project_identity, before);
}

#[test]
fn enter_with_stale_stack_clears_only_after_negative_probe() {
    let target = unique_temp_dir("stale_stack_target");
    let mut git = FakeGit::default();
    git.toplevel
        .insert(target.clone(), PathBuf::from("/repo/wt"));
    git.common_dir
        .insert(PathBuf::from("/repo"), PathBuf::from("/repo/.git"));
    git.common_dir
        .insert(PathBuf::from("/repo/wt"), PathBuf::from("/repo/.git"));
    git.worktrees.insert(target.clone());
    let mut state = st("/repo");
    state.stack.push(WorkspaceFrame {
        path_base: "/stale".into(),
        workspace_root: "/stale".into(),
        worktree_kind: WorktreeKind::Primary,
    });

    enter(&mut state, &git, Some(target), None, None).unwrap();

    assert_eq!(state.stack.len(), 1, "残栈清理后只压入当前 frame");
    assert_eq!(state.stack[0].workspace_root, PathBuf::from("/repo"));
}

#[test]
fn enter_when_stale_stack_probe_fails_keeps_state_unchanged() {
    let git = FakeGit {
        worktree_probe_error: Some("probe failed".into()),
        ..FakeGit::default()
    };
    let mut state = st("/repo");
    let frame = WorkspaceFrame {
        path_base: "/stale".into(),
        workspace_root: "/stale".into(),
        worktree_kind: WorktreeKind::Primary,
    };
    state.stack.push(frame.clone());

    let result = enter(&mut state, &git, Some("/target".into()), None, None);

    assert_eq!(
        result,
        Err(WorkspaceError::GitOperationFailed(
            crate::GitOperationError::CommandFailed { exit_code: None }
        ))
    );
    assert_eq!(state.path_base, PathBuf::from("/repo"));
    assert_eq!(state.workspace_root, PathBuf::from("/repo"));
    assert_eq!(state.stack, vec![frame]);
}

#[test]
fn enter_with_stale_stack_and_missing_target_keeps_full_state_unchanged() {
    let git = FakeGit::default();
    let mut state = st("/repo");
    state.stack.push(WorkspaceFrame {
        path_base: "/stale".into(),
        workspace_root: "/stale".into(),
        worktree_kind: WorktreeKind::Primary,
    });
    let before = snapshot(&state);

    let result = enter(&mut state, &git, None, None, None);

    assert_eq!(result, Err(WorkspaceError::MissingPathAndBranch));
    assert_eq!(snapshot(&state), before);
}

#[test]
fn enter_rejects_nested_when_in_worktree() {
    let mut git = FakeGit::default();
    git.worktrees.insert(PathBuf::from("/repo")); // 当前 path_base 在 worktree 中
    let mut s = st("/repo");
    s.stack.push(WorkspaceFrame {
        path_base: "/prev".into(),
        workspace_root: "/prev".into(),
        worktree_kind: WorktreeKind::Primary,
    });
    assert!(matches!(
        enter(&mut s, &git, Some("/other".into()), None, None),
        Err(WorkspaceError::NestedWorktree {
            current_workspace_root,
            current_path_base,
        }) if current_workspace_root == Path::new("/repo")
           && current_path_base == Path::new("/repo")
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
    assert_eq!(s.workspace_root, PathBuf::from("/repo"));
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
    assert_eq!(s.workspace_root, PathBuf::from("/repo"));
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
    git.worktrees.insert(canonical_tmp.clone());

    let mut s = st("/repo");
    switch_to(&mut s, &git, canonical_tmp.clone()).unwrap();

    assert_eq!(s.path_base, canonical_tmp, "path_base should be canonical");
    assert_eq!(
        s.workspace_root, worktree_root,
        "workspace_root should be worktree root"
    );
    assert!(s.stack.is_empty(), "stack must not be pushed");

    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn validate_in_repo_reports_invalid_probe_instead_of_repo_mismatch_for_non_git_target() {
    let target = unique_temp_dir("non_git_target");
    let mut git = FakeGit::default();
    git.toplevel.insert(target.clone(), target.clone());
    git.non_git.insert(target.clone());
    let state = st("/repo");

    let result = validate_in_repo(&state, &git, &target);

    assert_eq!(
        result,
        Err(WorkspaceError::GitProbeFailed(
            crate::GitProbeError::InvalidOutput
        ))
    );
    let _ = std::fs::remove_dir_all(target);
}

#[test]
fn enter_missing_path_and_branch_errors() {
    let git = FakeGit::default();
    let mut s = st("/repo");
    assert_eq!(
        enter(&mut s, &git, None, None, None),
        Err(WorkspaceError::MissingPathAndBranch)
    );
}

#[test]
fn resolve_worktree_path_treats_empty_path_as_missing() {
    let state = st("/repo");

    let resolved =
        resolve_worktree_path(&state, Some(PathBuf::new()), Some("feature/path contract")).unwrap();

    assert_eq!(
        resolved,
        PathBuf::from("/repo/.worktrees/feature-path-contract")
    );
}

#[test]
fn resolve_worktree_base_defaults_for_missing_or_blank_values() {
    assert_eq!(resolve_worktree_base(None), DEFAULT_WORKTREE_BASE);
    assert_eq!(resolve_worktree_base(Some("")), DEFAULT_WORKTREE_BASE);
    assert_eq!(resolve_worktree_base(Some(" \t\n ")), DEFAULT_WORKTREE_BASE);
}

#[test]
fn resolve_worktree_base_preserves_explicit_value() {
    assert_eq!(resolve_worktree_base(Some(" release/v2 ")), " release/v2 ");
}

#[test]
fn enter_with_empty_path_derives_target_and_forwards_default_base() {
    let root = unique_temp_dir("enter_empty_path");
    let expected_target = root.join(".worktrees/feature-empty-path");
    let mut state = WorkspaceState::new(root.clone());
    let git = FakeGit::default();

    enter(
        &mut state,
        &git,
        Some(PathBuf::new()),
        Some("feature/empty path".into()),
        None,
    )
    .unwrap();

    assert_eq!(state.path_base, expected_target);
    assert_eq!(
        git.added.lock().unwrap().as_slice(),
        &[WorktreeAddCall {
            repo_root: root.clone(),
            path: expected_target.clone(),
            branch: "feature/empty path".into(),
            base: DEFAULT_WORKTREE_BASE.into(),
        }]
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn enter_with_blank_base_forwards_default_base() {
    for (case, base) in [("empty", ""), ("whitespace", " \t\n ")] {
        let root = unique_temp_dir(&format!("enter_blank_base_{case}"));
        let expected_target = root.join(format!(".worktrees/feature-{case}"));
        let mut state = WorkspaceState::new(root.clone());
        let git = FakeGit::default();

        enter(
            &mut state,
            &git,
            None,
            Some(format!("feature/{case}")),
            Some(base.into()),
        )
        .unwrap();

        assert_eq!(
            git.added.lock().unwrap().as_slice(),
            &[WorktreeAddCall {
                repo_root: root.clone(),
                path: expected_target,
                branch: format!("feature/{case}"),
                base: DEFAULT_WORKTREE_BASE.into(),
            }]
        );
        let _ = std::fs::remove_dir_all(root);
    }
}

#[test]
fn enter_with_explicit_base_forwards_value_unchanged() {
    let root = unique_temp_dir("enter_explicit_base");
    let expected_target = root.join(".worktrees/feature-explicit");
    let mut state = WorkspaceState::new(root.clone());
    let git = FakeGit::default();

    enter(
        &mut state,
        &git,
        None,
        Some("feature/explicit".into()),
        Some(" release/v2 ".into()),
    )
    .unwrap();

    assert_eq!(
        git.added.lock().unwrap().as_slice(),
        &[WorktreeAddCall {
            repo_root: root.clone(),
            path: expected_target,
            branch: "feature/explicit".into(),
            base: " release/v2 ".into(),
        }]
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn enter_rejects_primary_target_as_not_linked_and_keeps_state_unchanged() {
    let target = unique_temp_dir("primary_target");
    let common = PathBuf::from("/repo/.git");
    let mut git = FakeGit::default();
    git.toplevel.insert(target.clone(), target.clone());
    git.common_dir.insert(target.clone(), common);

    let mut state = st("/repo");
    let before = snapshot(&state);

    let result = enter(&mut state, &git, Some(target.clone()), None, None);

    assert_eq!(
        result,
        Err(WorkspaceError::NotLinkedWorktree {
            path: target.clone()
        })
    );
    assert_eq!(
        result.unwrap_err().to_string(),
        format!(
            "路径 {} 是当前仓库的 primary checkout，不是 linked worktree",
            target.display()
        )
    );
    assert_eq!(snapshot(&state), before);
    let _ = std::fs::remove_dir_all(target);
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
    // show_toplevel: called once inside enter (for canonical_tmp).
    // change_directory does not call show_toplevel.
    git.toplevel
        .insert(canonical_tmp.clone(), worktree_root.clone());
    // git_common_dir: checked for workspace_root ("/repo") and worktree_root.
    git.common_dir
        .insert(PathBuf::from("/repo"), common.clone());
    git.common_dir.insert(worktree_root.clone(), common.clone());
    git.worktrees.insert(canonical_tmp.clone());

    let mut s = st("/repo");
    let saved_path_base = s.path_base.clone();
    let saved_workspace_root = s.workspace_root.clone();

    // Pass the temp dir as an absolute path → resolve_worktree_path returns it directly,
    // target.exists() is true → worktree_add is NOT called.
    let frame = enter(&mut s, &git, Some(canonical_tmp.clone()), None, None).unwrap();

    // Returned frame holds the PRE-change state.
    assert_eq!(frame.path_base, saved_path_base);
    assert_eq!(frame.workspace_root, saved_workspace_root);

    // Stack has exactly one entry (the saved frame).
    assert_eq!(s.stack.len(), 1);

    // State updated to the worktree.
    assert_eq!(s.path_base, canonical_tmp);
    assert_eq!(s.workspace_root, worktree_root);

    // worktree_add was NOT invoked.
    assert!(git.added.lock().unwrap().is_empty());

    let _ = std::fs::remove_dir_all(&tmp);
}

// ---- #894: ProjectIdentity / WorkspaceId / WorktreeKind + NonGit identity ----

use share::session_types::{ProjectIdentity, WorktreeKind};

/// #894: `WorkspaceFrame` 必须携带上一层已验证的 `worktree_kind`（INV-7）。
#[test]
fn frame_carries_worktree_kind() {
    let frame = WorkspaceFrame {
        path_base: "/repo".into(),
        workspace_root: "/repo".into(),
        worktree_kind: WorktreeKind::Primary,
    };
    assert_eq!(frame.worktree_kind, WorktreeKind::Primary);
}

/// #894: `WorkspaceState` 必须暴露完整 `project_identity` 与已验证 `worktree_kind`。
#[test]
fn state_exposes_project_identity_and_worktree_kind() {
    let s = st("/repo");
    let _identity: &ProjectIdentity = &s.project_identity;
    let _kind: WorktreeKind = s.worktree_kind;
}

/// #894: NonGit identity 下 `enter` 必须返回 `UnsupportedForNonGit`，
/// 禁止 worktree transition（INV-3），且不触发任何 git 操作。
#[test]
fn enter_rejects_non_git_identity() {
    let git = FakeGit::default();
    let mut s = st("/repo");
    s.worktree_kind = WorktreeKind::NonGit;
    assert_eq!(
        enter(&mut s, &git, Some("/repo/wt".into()), None, None),
        Err(WorkspaceError::UnsupportedForNonGit)
    );
    assert!(git.added.lock().unwrap().is_empty());
}

/// #894: git `enter` 必须把 `worktree_kind` 升级为 `Linked`，
/// 并把上一层分类压入栈帧（INV-5 / INV-7）。
#[test]
fn enter_promotes_worktree_kind_to_linked_and_captures_previous() {
    use std::time::{SystemTime, UNIX_EPOCH};

    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .subsec_nanos();
    let tmp = std::env::temp_dir().join(format!("aemeath_894_kind_{}", nanos));
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
    git.worktrees.insert(canonical_tmp.clone());

    let mut s = st("/repo");
    s.worktree_kind = WorktreeKind::Primary;

    let frame = enter(&mut s, &git, Some(canonical_tmp.clone()), None, None).unwrap();

    assert_eq!(
        frame.worktree_kind,
        WorktreeKind::Primary,
        "压栈帧必须保留上一层已验证分类"
    );
    assert_eq!(
        s.worktree_kind,
        WorktreeKind::Linked,
        "进入 worktree 后当前分类必须升级为 Linked"
    );

    let _ = std::fs::remove_dir_all(&tmp);
}
