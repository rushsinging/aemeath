use std::path::Path;

#[test]
fn staged_paths_select_only_lightweight_checks() {
    let rust = vec!["apps/cli/src/main.rs".to_owned()];
    let plan = xtask::pre_commit::plan(&rust);
    assert!(plan.format_rust);
    assert!(plan.source_guard);
    assert!(!plan.snapshot_drafts);

    let snapshots = vec!["apps/cli/src/tui/app/scenario_tests/foo.rs".to_owned()];
    let plan = xtask::pre_commit::plan(&snapshots);
    assert!(plan.snapshot_drafts);
}

#[test]
fn draft_snapshot_detection_is_local_and_deterministic() {
    let paths = vec![
        "a.snap".to_owned(),
        "b.snap.new".to_owned(),
        ".pending-snap".to_owned(),
    ];
    assert_eq!(
        xtask::pre_commit::snapshot_drafts(&paths),
        vec![".pending-snap", "b.snap.new"]
    );
    assert!(!xtask::pre_commit::needs_issue_tree_check(Path::new(
        "docs/design.md"
    )));
}
