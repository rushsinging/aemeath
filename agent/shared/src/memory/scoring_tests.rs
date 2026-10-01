use super::*;
use crate::memory::entry::{MemoryCategory, MemoryLayer, MemorySource};

fn entry() -> MemoryEntry {
    let mut entry = MemoryEntry::new(
        "memory-1",
        1_000_000,
        MemoryLayer::Project,
        MemoryCategory::Pattern,
        "测试",
        MemorySource::User,
    );
    entry.accessed_at = 1_000_000;
    entry
}

#[test]
fn test_injection_score_pinned_wins() {
    let mut normal = entry();
    normal.access_count = 20;
    let mut pinned = entry();
    pinned.pinned = true;

    assert!(injection_score(&pinned, 1_000_000) > injection_score(&normal, 1_000_000));
}

#[test]
fn test_injection_score_outdated_penalty() {
    let active = entry();
    let mut outdated = entry();
    outdated.outdated = true;

    assert!(injection_score(&active, 1_000_000) > injection_score(&outdated, 1_000_000));
}

#[test]
fn test_injection_score_old_entry_lower() {
    let recent = entry();
    let mut old = entry();
    old.accessed_at = 1;

    assert!(injection_score(&recent, 1_000_000) > injection_score(&old, 1_000_000));
}

#[test]
fn test_eviction_score_pinned_max() {
    let mut pinned = entry();
    pinned.pinned = true;

    assert_eq!(eviction_score(&pinned, 1_000_000), i64::MAX);
}
