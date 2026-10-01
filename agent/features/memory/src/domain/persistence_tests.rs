use super::*;
use crate::domain::*;

fn entry(id: MemoryId, layer: MemoryLayer, content: &str) -> MemoryEntry {
    MemoryEntry::new(
        id,
        1,
        layer,
        MemoryCategory::Fact,
        content,
        MemorySource::User,
    )
    .unwrap()
}

#[test]
fn project_key_is_versioned_stable_and_does_not_expose_paths() {
    let first = ProjectMemoryKey::derive("/repo", Some("/repo/.git")).unwrap();
    let second = ProjectMemoryKey::derive("/other-worktree", Some("/repo/.git")).unwrap();
    assert_eq!(first, second);
    assert!(first.as_str().starts_with("v2_"));
    assert!(!first.as_str().contains("repo"));
}

#[test]
fn non_git_project_key_uses_initial_cwd_and_rejects_empty_identity() {
    assert_ne!(
        ProjectMemoryKey::derive("/a", None).unwrap(),
        ProjectMemoryKey::derive("/b", None).unwrap()
    );
    assert!(ProjectMemoryKey::derive("", None).is_err());
    assert!(ProjectMemoryKey::derive("/project", Some("")).is_err());
}

#[test]
fn legacy_project_name_matches_predecessor_file_stem() {
    let key = ProjectMemoryKey::derive("/Users/guoyuqi/work/aemeath", None).unwrap();
    assert_eq!(key.legacy_project_name(), "Users-guoyuqi-work-aemeath");

    // Git projects share the key but use their *own* cwd for the legacy stem.
    let main = ProjectMemoryKey::derive("/repo", Some("/repo/.git")).unwrap();
    let worktree = ProjectMemoryKey::derive("/repo/.worktrees/feat", Some("/repo/.git")).unwrap();
    assert_eq!(main, worktree, "keys are equal (shared git identity)");
    assert_eq!(main.legacy_project_name(), "repo");
    assert_eq!(
        worktree.legacy_project_name(),
        "repo-.worktrees-feat",
        "legacy stem uses the *caller* cwd, not the shared git dir"
    );
}

#[test]
fn dataset_rejects_duplicate_ids_across_active_and_archive() {
    let id = MemoryId::now_v7();
    assert!(MemoryDataset::new(
        MemoryLayer::Project,
        vec![entry(id, MemoryLayer::Project, "active")],
        vec![entry(id, MemoryLayer::Project, "archive")]
    )
    .is_err());
}

#[test]
fn dataset_rejects_entries_from_another_layer() {
    assert!(MemoryDataset::new(
        MemoryLayer::Project,
        vec![entry(MemoryId::now_v7(), MemoryLayer::Global, "global")],
        vec![]
    )
    .is_err());
}
