use super::{WorkspaceIntent, WorkspaceProvider};
use crate::tui::model::conversation::workspace::WorktreeKind;

#[test]
fn set_current_updates_operating_paths_without_bumping_snapshot_revision() {
    let mut workspace = WorkspaceProvider::default();

    workspace.apply(WorkspaceIntent::SetCurrent {
        cwd: "/repo".to_string(),
        worktree: None,
    });

    assert_eq!(workspace.cwd(), Some("/repo"));
    assert_eq!(workspace.worktree(), None);
    assert_eq!(workspace.revision(), 0);
}

#[test]
fn apply_snapshot_updates_fields_and_bumps_revision() {
    let mut workspace = WorkspaceProvider::default();

    workspace.apply(WorkspaceIntent::ApplySnapshot {
        path_base: Some("/repo/.worktrees/feature".to_string()),
        workspace_root: Some("/repo/.worktrees/feature".to_string()),
    });

    assert_eq!(workspace.workspace_root(), Some("/repo/.worktrees/feature"));
    assert_eq!(workspace.branch(), None);
    assert_eq!(workspace.kind(), WorktreeKind::Unknown);
    assert_eq!(workspace.revision(), 1);
}

#[test]
fn matching_metadata_updates_branch_without_bumping_revision() {
    let mut workspace = WorkspaceProvider::default();
    workspace.apply(WorkspaceIntent::ApplySnapshot {
        path_base: Some("/repo".to_string()),
        workspace_root: Some("/repo".to_string()),
    });

    let change = workspace.apply(WorkspaceIntent::ApplyMetadata {
        root: "/repo".to_string(),
        revision: 1,
        branch: Some("main".to_string()),
        kind: WorktreeKind::MainCheckout,
    });

    assert!(matches!(
        change,
        super::WorkspaceChange::MetadataApplied { revision: 1 }
    ));
    assert_eq!(workspace.branch(), Some("main"));
    assert_eq!(workspace.kind(), WorktreeKind::MainCheckout);
    assert_eq!(workspace.revision(), 1);
}

#[test]
fn stale_metadata_does_not_overwrite_newer_snapshot() {
    let mut workspace = WorkspaceProvider::default();
    for root in ["/repo/old", "/repo/new"] {
        workspace.apply(WorkspaceIntent::ApplySnapshot {
            path_base: Some(root.to_string()),
            workspace_root: Some(root.to_string()),
        });
    }

    let change = workspace.apply(WorkspaceIntent::ApplyMetadata {
        root: "/repo/old".to_string(),
        revision: 1,
        branch: Some("old-branch".to_string()),
        kind: WorktreeKind::LinkedWorktree,
    });

    assert!(matches!(
        change,
        super::WorkspaceChange::MetadataDiscarded { revision: 1, .. }
    ));
    assert_eq!(workspace.workspace_root(), Some("/repo/new"));
    assert_eq!(workspace.branch(), None);
    assert_eq!(workspace.kind(), WorktreeKind::Unknown);
    assert_eq!(workspace.revision(), 2);
}
