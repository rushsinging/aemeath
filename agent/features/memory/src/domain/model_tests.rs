use super::*;
use crate::domain::{
    eviction_candidates, injection_score, is_injection_eligible, jaccard_similarity,
    search_tie_break_score,
};

fn entry(_id: &str) -> MemoryEntry {
    MemoryEntry::new(
        MemoryId::now_v7(),
        1_000_000,
        MemoryLayer::Project,
        MemoryCategory::Pattern,
        "保持 Memory 契约单一",
        MemorySource::User,
    )
    .unwrap()
}

#[test]
fn memory_id_rejects_blank_and_round_trips_as_string() {
    assert!(MemoryId::new("   ").is_err());
    let id = MemoryId::new("01890f3c-7c00-7000-8000-000000000001").unwrap();
    assert_eq!(id.to_string(), "01890f3c-7c00-7000-8000-000000000001");
}

#[test]
fn entry_rejects_blank_content() {
    let result = MemoryEntry::new(
        MemoryId::now_v7(),
        10,
        MemoryLayer::Project,
        MemoryCategory::Fact,
        "  ",
        MemorySource::User,
    );
    assert!(matches!(result, Err(MemoryError::InvalidEntry { .. })));
}

#[test]
fn ttl_expires_only_after_created_at_plus_ttl() {
    let mut memory = entry("memory-ttl");
    memory.ttl = Some(Duration::from_secs(10));
    assert!(!memory.is_ttl_expired(1_000_010));
    assert!(memory.is_ttl_expired(1_000_011));
}

#[test]
fn injection_eligibility_rejects_outdated_and_expired_even_when_pinned() {
    let mut outdated = entry("outdated");
    outdated.pinned = true;
    outdated.outdated = true;
    assert!(!is_injection_eligible(&outdated, 1_000_000));

    let mut expired = entry("expired");
    expired.pinned = true;
    expired.ttl = Some(Duration::from_secs(1));
    assert!(!is_injection_eligible(&expired, 1_000_002));
}

#[test]
fn eligible_pinned_entry_outranks_max_access_unpinned_entry() {
    let mut pinned = entry("pinned");
    pinned.pinned = true;
    let mut frequent = entry("frequent");
    frequent.confirmation_count = u32::MAX;
    assert!(injection_score(&pinned, 1_000_000) > injection_score(&frequent, 1_000_000));
}

#[test]
fn search_tie_break_accepts_ineligible_entries() {
    let mut archived_fact = entry("archived");
    archived_fact.outdated = true;
    archived_fact.ttl = Some(Duration::from_secs(1));
    assert!(search_tie_break_score(&archived_fact, 1_000_002) >= 0);
}

#[test]
fn jaccard_similarity_is_case_insensitive_and_bounded() {
    assert_eq!(jaccard_similarity("Rust Memory", "rust memory"), 1.0);
    assert_eq!(jaccard_similarity("", ""), 0.0);
    assert!((0.0..=1.0).contains(&jaccard_similarity("rust memory", "rust port")));
}

#[test]
fn eviction_candidates_never_include_pinned_entries() {
    let mut pinned = entry("pinned");
    pinned.pinned = true;
    let normal = entry("normal");
    let candidates = eviction_candidates(&[pinned, normal.clone()], 5, 1_000_000);
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].entry, normal);
}
